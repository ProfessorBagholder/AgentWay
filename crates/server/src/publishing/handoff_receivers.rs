//! Receiver transport identity and admission. Transport receipt never claims work.
use super::*;
use axum::http::StatusCode;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type Api<T> = std::result::Result<Json<T>, http::Error>;

#[derive(Clone)]
pub(super) struct ReceiverIdentity {
    pub(super) connection_id: String,
    pub(super) generation: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnrollInput {
    native_target: String,
    #[serde(default)]
    grok_webhook_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmitInput {
    generation: i64,
    native_reference: String,
}

#[derive(Deserialize)]
struct DeliveryQuery {
    before: Option<i64>,
}

#[derive(FromRow)]
struct BindingStatus {
    connection_id: String,
    product: String,
    generation: i64,
    enabled: bool,
    verified_at: Option<i64>,
    proof_expires_at: Option<i64>,
}

pub(super) fn admin_routes() -> Router<Publisher> {
    Router::new().route(
        "/api/handoff-receivers/{connection_id}",
        get(binding_status).post(enroll).delete(disable),
    )
}

pub(super) fn transport_routes() -> Router<Publisher> {
    Router::new()
        .route("/v1/handoff-receiver/deliveries", get(deliveries))
        .route(
            "/v1/handoff-receiver/deliveries/stream",
            get(handoff_receiver_stream::stream),
        )
        .route("/v1/handoff-receiver/deliveries/{id}/admit", post(admit))
}

fn invalid(message: &'static str) -> anyhow::Error {
    handoffs::HandoffError {
        status: StatusCode::BAD_REQUEST,
        message,
    }
    .into()
}

fn unavailable(message: &'static str) -> anyhow::Error {
    handoffs::HandoffError {
        status: StatusCode::CONFLICT,
        message,
    }
    .into()
}

impl Publisher {
    pub(super) async fn authenticate_receiver(
        &self,
        credential: &str,
    ) -> Result<Option<ReceiverIdentity>> {
        let hash = format!("{:x}", Sha256::digest(credential.as_bytes()));
        let records: Vec<(String, i64, String)> = sqlx::query_as(
            "SELECT b.connection_id,b.generation,b.receiver_secret_hash FROM handoff_receiver_bindings b JOIN agent_connections c ON c.id=b.connection_id WHERE b.enabled=1 AND c.disconnected=0",
        )
        .fetch_all(&self.0.db).await?;
        let mut found = None;
        for (connection_id, generation, expected) in records {
            if bool::from(hash.as_bytes().ct_eq(expected.as_bytes())) {
                found = Some(ReceiverIdentity {
                    connection_id,
                    generation,
                });
            }
        }
        Ok(found)
    }

    pub(super) async fn receiver_binding_status(&self, connection_id: &str) -> Result<Value> {
        let row: Option<BindingStatus> = sqlx::query_as(
            "SELECT b.connection_id,b.product,b.generation,b.enabled,b.verified_at,b.proof_expires_at FROM handoff_receiver_bindings b WHERE b.connection_id=?",
        )
        .bind(connection_id).fetch_optional(&self.0.db).await?;
        Ok(match row {
            Some(row) => {
                let grok_webhook_configured: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM handoff_grok_webhooks WHERE connection_id=? AND binding_generation=?)",
                )
                .bind(&row.connection_id).bind(row.generation).fetch_one(&self.0.db).await?;
                json!({
                "connection_id":row.connection_id,"product":row.product,"generation":row.generation,"enabled":row.enabled,
                "verified_at":row.verified_at,"proof_expires_at":row.proof_expires_at,
                "grok_webhook_configured":grok_webhook_configured,
                "ready":row.enabled && (row.product != "Grok Bot" || grok_webhook_configured) && row.verified_at.is_some() && row.proof_expires_at.is_some_and(|at| at > chrono::Utc::now().timestamp())
                })
            }
            None => {
                json!({"connection_id":connection_id,"enabled":false,"ready":false,"grok_webhook_configured":false})
            }
        })
    }

    async fn enroll_receiver(
        &self,
        connection_id: &str,
        target: &str,
        grok_key: Option<&str>,
    ) -> Result<Value> {
        if target.trim().is_empty() || target.len() > 1024 || target.chars().any(char::is_control) {
            return Err(invalid("Invalid native target reference"));
        }
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let secret_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
        let target_ciphertext = self.0.vault.seal(target)?;
        let _guard = self.0.mutation.lock().await;
        let mut tx = self.0.db.begin().await?;
        let product: Option<String> = sqlx::query_scalar(
            "SELECT product FROM agent_connections WHERE id=? AND disconnected=0",
        )
        .bind(connection_id)
        .fetch_optional(&mut *tx)
        .await?;
        let product = product.ok_or_else(|| unavailable("Connection is unavailable"))?;
        if let Some(key) = grok_key
            && (product != "Grok Bot"
                || !super::grok_webhook_adapter::valid_url(target)
                || key.is_empty()
                || key.len() > 4096
                || key.chars().any(char::is_whitespace))
        {
            return Err(invalid("Invalid Grok webhook configuration"));
        }
        let generation: i64 = sqlx::query_scalar(
            "SELECT COALESCE((SELECT generation+1 FROM handoff_receiver_bindings WHERE connection_id=?),1)",
        )
        .bind(connection_id).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO handoff_receiver_bindings(connection_id,product,target_ciphertext,receiver_secret_hash,generation,enabled) VALUES(?,?,?,?,?,1) ON CONFLICT(connection_id) DO UPDATE SET product=excluded.product,target_ciphertext=excluded.target_ciphertext,receiver_secret_hash=excluded.receiver_secret_hash,generation=excluded.generation,enabled=1,verified_at=NULL,proof_expires_at=NULL,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')")
            .bind(connection_id).bind(product).bind(target_ciphertext).bind(secret_hash).bind(generation)
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE handoff_delivery_outbox SET state='blocked',safe_error_code=CASE WHEN state='admitted' THEN 'native_outcome_unknown' ELSE 'binding_stale' END,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE connection_id=? AND binding_generation<>? AND state IN ('pending','admitted')")
            .bind(connection_id).bind(generation).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM handoff_grok_webhooks WHERE connection_id=?")
            .bind(connection_id)
            .execute(&mut *tx)
            .await?;
        if let Some(key) = grok_key {
            sqlx::query("INSERT INTO handoff_grok_webhooks(connection_id,binding_generation,url_ciphertext,key_ciphertext) VALUES(?,?,?,?)")
                .bind(connection_id).bind(generation).bind(self.0.vault.seal(target)?).bind(self.0.vault.seal(key)?)
                .execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(
            json!({"connection_id":connection_id,"generation":generation,"receiver_token":token,"enabled":true,"ready":false}),
        )
    }

    async fn disable_receiver(&self, connection_id: &str) -> Result<Value> {
        let _guard = self.0.mutation.lock().await;
        let mut tx = self.0.db.begin().await?;
        let changed = sqlx::query("UPDATE handoff_receiver_bindings SET enabled=0,verified_at=NULL,proof_expires_at=NULL,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE connection_id=?")
            .bind(connection_id).execute(&mut *tx).await?;
        if changed.rows_affected() == 0 {
            return Err(unavailable("Receiver binding is unavailable"));
        }
        sqlx::query("UPDATE handoff_delivery_outbox SET state='blocked',safe_error_code=CASE WHEN state='admitted' THEN 'native_outcome_unknown' ELSE 'receiver_unavailable' END,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE connection_id=? AND state IN ('pending','admitted')")
            .bind(connection_id).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM handoff_grok_webhooks WHERE connection_id=?")
            .bind(connection_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.receiver_binding_status(connection_id).await
    }

    async fn receiver_deliveries(
        &self,
        receiver: &ReceiverIdentity,
        before: Option<i64>,
    ) -> Result<Value> {
        let before = before.unwrap_or(i64::MAX);
        if before < 1 {
            return Err(invalid("Invalid delivery cursor"));
        }
        let rows: Vec<(i64, String, String, String, i64, i64)> = sqlx::query_as(
            "SELECT o.rowid,o.id,o.kind,o.task_id,o.binding_generation,o.expires_at FROM handoff_delivery_outbox o JOIN handoff_receiver_bindings b ON b.connection_id=o.connection_id JOIN agent_handoffs t ON t.id=o.task_id WHERE o.connection_id=? AND o.binding_generation=? AND b.generation=? AND b.enabled=1 AND o.state IN ('pending','admitted') AND o.expires_at>unixepoch() AND (o.kind='result_available' OR (t.status IN ('queued','claimed') AND EXISTS(SELECT 1 FROM agent_handoff_grants g WHERE g.sender_id=t.sender_id AND g.recipient_id=t.recipient_id))) AND o.rowid<? ORDER BY o.rowid DESC LIMIT 101",
        )
        .bind(&receiver.connection_id).bind(receiver.generation).bind(receiver.generation)
        .bind(before).fetch_all(&self.0.db).await?;
        let next = if rows.len() > 100 {
            rows.get(99).map(|row| row.0)
        } else {
            None
        };
        let items: Vec<Value> = rows
            .into_iter()
            .take(100)
            .map(|(_, id, kind, task_id, binding_generation, expires_at)| {
                json!({
                    "protocol_version":1,"delivery_id":id,"kind":kind,"task_id":task_id,
                    "binding_generation":binding_generation,"expires_at":expires_at
                })
            })
            .collect();
        Ok(json!({"items":items,"next":next}))
    }

    async fn admit_delivery(
        &self,
        receiver: &ReceiverIdentity,
        id: &str,
        input: AdmitInput,
    ) -> Result<Value> {
        Uuid::parse_str(id)?;
        if input.generation != receiver.generation
            || input.native_reference.trim().is_empty()
            || input.native_reference.len() > 255
            || input.native_reference.chars().any(char::is_control)
        {
            return Err(invalid("Invalid receiver admission"));
        }
        let _guard = self.0.mutation.lock().await;
        let mut tx = self.0.db.begin().await?;
        let active: Option<i64> = sqlx::query_scalar("SELECT 1 FROM handoff_receiver_bindings WHERE connection_id=? AND generation=? AND enabled=1")
            .bind(&receiver.connection_id).bind(receiver.generation).fetch_optional(&mut *tx).await?;
        if active.is_none() {
            return Err(unavailable("Receiver binding is unavailable"));
        }
        let prior: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT state,native_reference_ciphertext FROM handoff_delivery_outbox WHERE id=? AND connection_id=? AND binding_generation=?",
        ).bind(id).bind(&receiver.connection_id).bind(receiver.generation)
            .fetch_optional(&mut *tx).await?;
        if let Some((state, Some(ciphertext))) = &prior
            && state == "admitted"
            && self.0.vault.open_secret(ciphertext)? == input.native_reference
        {
            return Ok(
                json!({"delivery_id":id,"state":"admitted","binding_generation":receiver.generation}),
            );
        }
        let ciphertext = self.0.vault.seal(&input.native_reference)?;
        let changed = sqlx::query("UPDATE handoff_delivery_outbox SET state='admitted',native_reference_ciphertext=?,admitted_at=unixepoch(),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=? AND connection_id=? AND binding_generation=? AND state='pending' AND expires_at>unixepoch() AND EXISTS(SELECT 1 FROM handoff_receiver_bindings WHERE connection_id=? AND generation=? AND enabled=1) AND EXISTS(SELECT 1 FROM agent_handoffs t WHERE t.id=task_id AND (kind='result_available' OR (t.status IN ('queued','claimed') AND EXISTS(SELECT 1 FROM agent_handoff_grants g WHERE g.sender_id=t.sender_id AND g.recipient_id=t.recipient_id))))")
            .bind(ciphertext).bind(id).bind(&receiver.connection_id).bind(receiver.generation)
            .bind(&receiver.connection_id).bind(receiver.generation).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(unavailable("Delivery is no longer available"));
        }
        tx.commit().await?;
        Ok(json!({"delivery_id":id,"state":"admitted","binding_generation":receiver.generation}))
    }
}

async fn binding_status(State(p): State<Publisher>, Path(id): Path<String>) -> Api<Value> {
    Ok(Json(p.receiver_binding_status(&id).await?))
}
async fn enroll(
    State(p): State<Publisher>,
    Path(id): Path<String>,
    Json(input): Json<EnrollInput>,
) -> Api<Value> {
    Ok(Json(
        p.enroll_receiver(&id, &input.native_target, input.grok_webhook_key.as_deref())
            .await?,
    ))
}
async fn disable(State(p): State<Publisher>, Path(id): Path<String>) -> Api<Value> {
    Ok(Json(p.disable_receiver(&id).await?))
}
async fn deliveries(
    State(p): State<Publisher>,
    axum::Extension(receiver): axum::Extension<ReceiverIdentity>,
    Query(q): Query<DeliveryQuery>,
) -> Api<Value> {
    Ok(Json(p.receiver_deliveries(&receiver, q.before).await?))
}
async fn admit(
    State(p): State<Publisher>,
    axum::Extension(receiver): axum::Extension<ReceiverIdentity>,
    Path(id): Path<String>,
    Json(input): Json<AdmitInput>,
) -> Api<Value> {
    Ok(Json(p.admit_delivery(&receiver, &id, input).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fixture() -> (tempfile::TempDir, Publisher) {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::database(&format!(
            "sqlite://{}",
            dir.path().join("test.db").display()
        ))
        .await
        .unwrap();
        let p = Publisher::new(db, dir.path().join("publishing"), CancellationToken::new())
            .await
            .unwrap();
        for (id, product) in [
            ("sender", "Muse"),
            ("receiver", "Grok"),
            ("other", "Claude"),
        ] {
            let token = p.0.vault.seal(&format!("agent-token-{id}")).unwrap();
            sqlx::query("INSERT INTO agent_connections(id,name,product,token,publish_enabled) VALUES(?,?,?,?,0)")
                .bind(id).bind(product).bind(product).bind(token)
                .execute(&p.0.db).await.unwrap();
        }
        (dir, p)
    }

    #[tokio::test]
    async fn grok_webhook_enrollment_encrypts_credentials_and_rotation_removes_them() {
        let (_dir, p) = fixture().await;
        sqlx::query("UPDATE agent_connections SET product='Grok Bot' WHERE id='receiver'")
            .execute(&p.0.db)
            .await
            .unwrap();
        let url = "https://api2.cursor.sh/automations/webhook/probe";
        let key = "private-routine-key";
        assert!(p.enroll_receiver("other", url, Some(key)).await.is_err());
        assert!(
            p.enroll_receiver("receiver", "https://127.0.0.1/probe", Some(key))
                .await
                .is_err()
        );
        let enrolled = p.enroll_receiver("receiver", url, Some(key)).await.unwrap();
        assert_eq!(enrolled["ready"], false);
        assert_eq!(
            p.receiver_binding_status("receiver").await.unwrap()["grok_webhook_configured"],
            true
        );
        sqlx::query("UPDATE handoff_receiver_bindings SET verified_at=unixepoch(),proof_expires_at=unixepoch()+3600 WHERE connection_id='receiver'")
            .execute(&p.0.db).await.unwrap();
        assert_eq!(
            p.receiver_binding_status("receiver").await.unwrap()["ready"],
            true
        );
        let stored: (i64, String, String) = sqlx::query_as("SELECT binding_generation,url_ciphertext,key_ciphertext FROM handoff_grok_webhooks WHERE connection_id='receiver'")
            .fetch_one(&p.0.db).await.unwrap();
        assert_eq!(stored.0, 1);
        assert!(!stored.1.contains(url));
        assert!(!stored.2.contains(key));
        assert_eq!(p.0.vault.open_secret(&stored.1).unwrap(), url);
        assert_eq!(p.0.vault.open_secret(&stored.2).unwrap(), key);
        sqlx::query("DELETE FROM handoff_grok_webhooks WHERE connection_id='receiver'")
            .execute(&p.0.db)
            .await
            .unwrap();
        assert_eq!(
            p.receiver_binding_status("receiver").await.unwrap()["ready"],
            false
        );
        sqlx::query("INSERT INTO handoff_grok_webhooks(connection_id,binding_generation,url_ciphertext,key_ciphertext) VALUES('receiver',1,?,?)")
            .bind(&stored.1).bind(&stored.2).execute(&p.0.db).await.unwrap();
        assert!(!enrolled.to_string().contains(key));
        p.enroll_receiver("receiver", "new-target", None)
            .await
            .unwrap();
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM handoff_grok_webhooks WHERE connection_id='receiver'",
        )
        .fetch_one(&p.0.db)
        .await
        .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn enrollment_separates_receiver_secret_and_rotation_fences_old_delivery() {
        let (_dir, p) = fixture().await;
        let first = p
            .enroll_receiver("receiver", "native-private-target", None)
            .await
            .unwrap();
        let first_token = first["receiver_token"].as_str().unwrap();
        assert_eq!(first["generation"], 1);
        assert_eq!(first["ready"], false);
        assert!(
            p.authenticate_receiver(first_token)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            p.authenticate_receiver("wrong-token")
                .await
                .unwrap()
                .is_none()
        );
        let stored: (String, String) = sqlx::query_as("SELECT target_ciphertext,receiver_secret_hash FROM handoff_receiver_bindings WHERE connection_id='receiver'")
            .fetch_one(&p.0.db).await.unwrap();
        assert!(!stored.0.contains("native-private-target"));
        assert!(!stored.1.contains(first_token));
        assert_eq!(
            p.receiver_binding_status("receiver").await.unwrap()["ready"],
            false
        );

        let task_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO agent_handoffs(id,request_id,sender_id,sender_name,recipient_id,recipient_name,title,instructions,delivery_mode) VALUES(?,?, 'sender','Muse','receiver','Grok','Probe','Only test', 'automatic')")
            .bind(&task_id).bind(Uuid::new_v4().to_string()).execute(&p.0.db).await.unwrap();
        sqlx::query("INSERT INTO handoff_delivery_outbox(id,task_id,kind,connection_id,binding_generation,expires_at) VALUES(?,?,'task_offered','receiver',1,unixepoch()+3600)")
            .bind(Uuid::new_v4().to_string()).bind(&task_id).execute(&p.0.db).await.unwrap();
        let second = p
            .enroll_receiver("receiver", "different-target", None)
            .await
            .unwrap();
        assert_eq!(second["generation"], 2);
        assert!(
            p.authenticate_receiver(first_token)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            p.authenticate_receiver(second["receiver_token"].as_str().unwrap())
                .await
                .unwrap()
                .is_some()
        );
        let state: (String, String) = sqlx::query_as(
            "SELECT state,safe_error_code FROM handoff_delivery_outbox WHERE task_id=?",
        )
        .bind(task_id)
        .fetch_one(&p.0.db)
        .await
        .unwrap();
        assert_eq!(state, ("blocked".into(), "binding_stale".into()));
    }

    #[tokio::test]
    async fn transport_admission_is_scoped_and_does_not_accept_task() {
        let (_dir, p) = fixture().await;
        let enrolled = p.enroll_receiver("receiver", "target", None).await.unwrap();
        let receiver = p
            .authenticate_receiver(enrolled["receiver_token"].as_str().unwrap())
            .await
            .unwrap()
            .unwrap();
        let other = p
            .enroll_receiver("other", "another-target", None)
            .await
            .unwrap();
        let other = p
            .authenticate_receiver(other["receiver_token"].as_str().unwrap())
            .await
            .unwrap()
            .unwrap();
        sqlx::query(
            "INSERT INTO agent_handoff_grants(sender_id,recipient_id) VALUES('sender','receiver')",
        )
        .execute(&p.0.db)
        .await
        .unwrap();
        let task_id = Uuid::new_v4().to_string();
        let delivery_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO agent_handoffs(id,request_id,sender_id,sender_name,recipient_id,recipient_name,title,instructions,delivery_mode) VALUES(?,?,'sender','Muse','receiver','Grok','Probe','Secret instructions', 'automatic')")
            .bind(&task_id).bind(Uuid::new_v4().to_string()).execute(&p.0.db).await.unwrap();
        sqlx::query("INSERT INTO handoff_delivery_outbox(id,task_id,kind,connection_id,binding_generation,expires_at) VALUES(?,?,'task_offered','receiver',1,unixepoch()+3600)")
            .bind(&delivery_id).bind(&task_id).execute(&p.0.db).await.unwrap();
        assert_eq!(
            p.receiver_deliveries(&other, None).await.unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        let feed = p.receiver_deliveries(&receiver, None).await.unwrap();
        assert_eq!(feed["items"][0]["delivery_id"], delivery_id);
        assert!(!feed.to_string().contains("Secret instructions"));
        assert!(
            p.admit_delivery(
                &other,
                &delivery_id,
                AdmitInput {
                    generation: 1,
                    native_reference: "run".into()
                }
            )
            .await
            .is_err()
        );
        let admit = AdmitInput {
            generation: 1,
            native_reference: "native-run".into(),
        };
        assert_eq!(
            p.admit_delivery(&receiver, &delivery_id, admit)
                .await
                .unwrap()["state"],
            "admitted"
        );
        let task_status: String =
            sqlx::query_scalar("SELECT status FROM agent_handoffs WHERE id=?")
                .bind(&task_id)
                .fetch_one(&p.0.db)
                .await
                .unwrap();
        assert_eq!(task_status, "queued");
        assert_eq!(
            p.admit_delivery(
                &receiver,
                &delivery_id,
                AdmitInput {
                    generation: 1,
                    native_reference: "native-run".into()
                }
            )
            .await
            .unwrap()["state"],
            "admitted"
        );
        assert!(
            p.admit_delivery(
                &receiver,
                &delivery_id,
                AdmitInput {
                    generation: 1,
                    native_reference: "different-run".into()
                }
            )
            .await
            .is_err()
        );
        sqlx::query(
            "DELETE FROM agent_handoff_grants WHERE sender_id='sender' AND recipient_id='receiver'",
        )
        .execute(&p.0.db)
        .await
        .unwrap();
        assert_eq!(
            p.receiver_deliveries(&receiver, None).await.unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        p.disable_receiver("receiver").await.unwrap();
        assert!(
            p.authenticate_receiver(enrolled["receiver_token"].as_str().unwrap())
                .await
                .unwrap()
                .is_none()
        );
        let state: (String, String) =
            sqlx::query_as("SELECT state,safe_error_code FROM handoff_delivery_outbox WHERE id=?")
                .bind(delivery_id)
                .fetch_one(&p.0.db)
                .await
                .unwrap();
        assert_eq!(state, ("blocked".into(), "native_outcome_unknown".into()));
    }

    #[tokio::test]
    async fn bridge_routes_keep_receiver_and_agent_credentials_separate() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        let (_dir, p) = fixture().await;
        let enrolled = p.enroll_receiver("receiver", "target", None).await.unwrap();
        let receiver_token = enrolled["receiver_token"].as_str().unwrap();
        let call = |path: &str, token: &str| {
            Request::builder()
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap()
        };
        let app = p.bridge_router();
        assert_eq!(
            app.clone()
                .oneshot(call("/v1/handoff-receiver/deliveries", receiver_token))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            app.clone()
                .oneshot(call("/v1/status", receiver_token))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.clone()
                .oneshot(call(
                    "/v1/handoff-receiver/deliveries",
                    "agent-token-receiver"
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.oneshot(call("/v1/status", "agent-token-receiver"))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn expired_transport_records_stop_without_retries() {
        let (_dir, p) = fixture().await;
        let task_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO agent_handoffs(id,request_id,sender_id,sender_name,recipient_id,recipient_name,title,instructions,delivery_mode) VALUES(?,?,'sender','Muse','receiver','Grok','Probe','Check','automatic')")
            .bind(&task_id).bind(Uuid::new_v4().to_string()).execute(&p.0.db).await.unwrap();
        for (kind, state) in [
            ("task_offered", "pending"),
            ("result_available", "admitted"),
        ] {
            let connection_id = if kind == "task_offered" {
                "receiver"
            } else {
                "sender"
            };
            sqlx::query("INSERT INTO handoff_delivery_outbox(id,task_id,kind,connection_id,binding_generation,state,expires_at) VALUES(?,?,?,?,1,?,unixepoch()-1)")
                .bind(Uuid::new_v4().to_string()).bind(&task_id).bind(kind).bind(connection_id).bind(state)
                .execute(&p.0.db).await.unwrap();
        }
        assert_eq!(p.expire_handoff_deliveries().await.unwrap(), 2);
        assert_eq!(p.expire_handoff_deliveries().await.unwrap(), 0);
        let states: Vec<(String, String)> = sqlx::query_as("SELECT state,safe_error_code FROM handoff_delivery_outbox WHERE task_id=? ORDER BY kind")
            .bind(task_id).fetch_all(&p.0.db).await.unwrap();
        assert_eq!(
            states,
            vec![
                ("blocked".into(), "native_outcome_unknown".into()),
                ("expired".into(), "delivery_expired".into())
            ]
        );
    }
}
