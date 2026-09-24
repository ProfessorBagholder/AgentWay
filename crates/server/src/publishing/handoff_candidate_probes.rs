//! One-shot owner probes of an unverified receiver. A provider response is
//! transport evidence only; this route never writes `verified_at`.
use super::*;
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use handoff_dispatch::{DeliveryEnvelope, TransportOutcome};

const TITLE: &str = "AgentWay receiver probe";
const INSTRUCTIONS: &str = "Confirm receipt. Do not publish anything.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeInput {
    task_id: String,
}

struct ReservedProbe {
    envelope: DeliveryEnvelope,
    url_ciphertext: String,
    key_ciphertext: String,
}

#[derive(FromRow)]
struct ProbeStatus {
    id: String,
    task_id: String,
    connection_id: String,
    binding_generation: i64,
    state: String,
    started_at: i64,
    finished_at: Option<i64>,
    task_status: String,
    result: Option<String>,
    result_acknowledged_at: Option<String>,
}

pub(super) fn admin_routes() -> Router<Publisher> {
    Router::new()
        .route("/api/handoff-receivers/{connection_id}/probe", post(start))
        .route("/api/handoff-probes/{id}", get(status))
}

fn conflict(message: &'static str) -> anyhow::Error {
    handoffs::HandoffError {
        status: axum::http::StatusCode::CONFLICT,
        message,
    }
    .into()
}

fn missing() -> anyhow::Error {
    handoffs::HandoffError {
        status: axum::http::StatusCode::NOT_FOUND,
        message: "Probe not found",
    }
    .into()
}

impl Publisher {
    async fn reserve_candidate_probe(
        &self,
        connection_id: &str,
        task_id: &str,
    ) -> Result<ReservedProbe> {
        Uuid::parse_str(task_id)?;
        let _guard = self.0.mutation.lock().await;
        let mut tx = self.0.db.begin().await?;
        let candidate: Option<(i64, String, String, i64)> = sqlx::query_as(
            "SELECT b.generation,w.url_ciphertext,w.key_ciphertext,t.expires_at FROM agent_handoffs t JOIN agent_connections c ON c.id=t.recipient_id JOIN handoff_receiver_bindings b ON b.connection_id=c.id JOIN handoff_grok_webhooks w ON w.connection_id=b.connection_id AND w.binding_generation=b.generation WHERE t.id=? AND t.recipient_id=? AND t.delivery_mode='pull' AND t.status='queued' AND t.title=? AND t.instructions=? AND t.expires_at>unixepoch()+60 AND c.product='Grok' AND c.disconnected=0 AND b.product='Grok' AND b.enabled=1 AND b.verified_at IS NULL AND EXISTS(SELECT 1 FROM agent_handoff_grants g WHERE g.sender_id=t.sender_id AND g.recipient_id=t.recipient_id)",
        )
        .bind(task_id).bind(connection_id).bind(TITLE).bind(INSTRUCTIONS)
        .fetch_optional(&mut *tx).await?;
        let (generation, url_ciphertext, key_ciphertext, expires_at) =
            candidate.ok_or_else(|| conflict("Probe task or candidate receiver is unavailable"))?;
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO handoff_candidate_probes(id,task_id,connection_id,binding_generation,state) VALUES(?,?,?,?,'started')")
            .bind(&id).bind(task_id).bind(connection_id).bind(generation)
            .execute(&mut *tx).await.map_err(|error| match &error {
                sqlx::Error::Database(database) if database.is_unique_violation() =>
                    conflict("This task already has a receiver probe"),
                _ => error.into(),
            })?;
        tx.commit().await?;
        Ok(ReservedProbe {
            envelope: DeliveryEnvelope {
                id,
                task_id: task_id.to_owned(),
                kind: "task_offered".into(),
                binding_generation: generation,
                expires_at,
            },
            url_ciphertext,
            key_ciphertext,
        })
    }

    async fn finish_candidate_probe(&self, id: &str, outcome: TransportOutcome) -> Result<Value> {
        let state = match outcome {
            TransportOutcome::Admitted { .. } => "admitted",
            TransportOutcome::NotSent => "not_sent",
            TransportOutcome::Unknown => "unknown",
        };
        sqlx::query("UPDATE handoff_candidate_probes SET state=?,finished_at=unixepoch() WHERE id=? AND state='started'")
            .bind(state).bind(id).execute(&self.0.db).await?;
        self.candidate_probe_status(id).await
    }

    async fn candidate_probe_status(&self, id: &str) -> Result<Value> {
        Uuid::parse_str(id)?;
        let row: Option<ProbeStatus> = sqlx::query_as(
            "SELECT p.id,p.task_id,p.connection_id,p.binding_generation,p.state,p.started_at,p.finished_at,t.status AS task_status,t.result,t.result_acknowledged_at FROM handoff_candidate_probes p JOIN agent_handoffs t ON t.id=p.task_id WHERE p.id=?",
        ).bind(id).fetch_optional(&self.0.db).await?;
        let row = row.ok_or_else(missing)?;
        Ok(json!({
            "id":row.id,"task_id":row.task_id,"connection_id":row.connection_id,"binding_generation":row.binding_generation,
            "transport_state": if row.state == "started" && row.started_at < chrono::Utc::now().timestamp()-30 { "unknown" } else { &row.state },
            "started_at":row.started_at,"finished_at":row.finished_at,
            "task_status":row.task_status,"result":row.result,"result_acknowledged_at":row.result_acknowledged_at,
            "receiver_ready":false
        }))
    }

    async fn send_candidate_probe(&self, connection_id: &str, task_id: &str) -> Result<Value> {
        let reserved = self.reserve_candidate_probe(connection_id, task_id).await?;
        let outcome = match (
            self.0.vault.open_secret(&reserved.url_ciphertext),
            self.0.vault.open_secret(&reserved.key_ciphertext),
        ) {
            (Ok(url), Ok(key)) if grok_webhook_adapter::valid_url(&url) => {
                let current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM handoff_receiver_bindings b JOIN handoff_grok_webhooks w ON w.connection_id=b.connection_id AND w.binding_generation=b.generation JOIN agent_handoffs t ON t.id=? WHERE b.connection_id=? AND b.generation=? AND b.enabled=1 AND t.status='queued' AND t.expires_at>unixepoch() AND EXISTS(SELECT 1 FROM agent_handoff_grants g WHERE g.sender_id=t.sender_id AND g.recipient_id=t.recipient_id))")
                    .bind(task_id).bind(connection_id).bind(reserved.envelope.binding_generation)
                    .fetch_one(&self.0.db).await?;
                if current {
                    grok_webhook_adapter::send_http(&self.0.client, &url, &key, &reserved.envelope)
                        .await
                } else {
                    TransportOutcome::NotSent
                }
            }
            _ => TransportOutcome::NotSent,
        };
        self.finish_candidate_probe(&reserved.envelope.id, outcome)
            .await
    }
}

async fn start(
    State(p): State<Publisher>,
    Path(connection_id): Path<String>,
    Json(input): Json<ProbeInput>,
) -> Result<Json<Value>, http::Error> {
    Ok(Json(
        p.send_candidate_probe(&connection_id, &input.task_id)
            .await?,
    ))
}

async fn status(
    State(p): State<Publisher>,
    Path(id): Path<String>,
) -> Result<Json<Value>, http::Error> {
    Ok(Json(p.candidate_probe_status(&id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn candidate_probe_is_single_send_and_never_verifies_a_binding() {
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
        for (id, product) in [("sender", "Muse"), ("receiver", "Grok")] {
            sqlx::query("INSERT INTO agent_connections(id,name,product,token,publish_enabled) VALUES(?,?,?,?,0)")
                .bind(id).bind(product).bind(product).bind("encrypted-test-token")
                .execute(&p.0.db).await.unwrap();
        }
        sqlx::query(
            "INSERT INTO agent_handoff_grants(sender_id,recipient_id) VALUES('sender','receiver')",
        )
        .execute(&p.0.db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO handoff_receiver_bindings(connection_id,product,target_ciphertext,receiver_secret_hash,generation,enabled) VALUES('receiver','Grok','encrypted-target','hash',1,1)")
            .execute(&p.0.db).await.unwrap();
        sqlx::query("INSERT INTO handoff_grok_webhooks(connection_id,binding_generation,url_ciphertext,key_ciphertext) VALUES('receiver',1,?,?)")
            .bind(p.0.vault.seal("https://api2.cursor.sh/automations/webhook/test").unwrap())
            .bind(p.0.vault.seal("test-key").unwrap())
            .execute(&p.0.db).await.unwrap();
        let task_id = Uuid::new_v4().to_string();
        let wrong_task_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO agent_handoffs(id,request_id,sender_id,sender_name,recipient_id,recipient_name,title,instructions,expires_at) VALUES(?,?,'sender','Muse','receiver','Grok','Publish video',?,unixepoch()+1800)")
            .bind(&wrong_task_id).bind(Uuid::new_v4().to_string()).bind(INSTRUCTIONS)
            .execute(&p.0.db).await.unwrap();
        assert!(
            p.reserve_candidate_probe("receiver", &wrong_task_id)
                .await
                .is_err()
        );
        sqlx::query("INSERT INTO agent_handoffs(id,request_id,sender_id,sender_name,recipient_id,recipient_name,title,instructions,expires_at) VALUES(?,?,'sender','Muse','receiver','Grok',?,?,unixepoch()+1800)")
            .bind(&task_id).bind(Uuid::new_v4().to_string()).bind(TITLE).bind(INSTRUCTIONS)
            .execute(&p.0.db).await.unwrap();
        let reserved = p
            .reserve_candidate_probe("receiver", &task_id)
            .await
            .unwrap();
        assert_eq!(reserved.envelope.task_id, task_id);
        assert!(
            p.reserve_candidate_probe("receiver", &task_id)
                .await
                .is_err()
        );
        let result = p
            .finish_candidate_probe(
                &reserved.envelope.id,
                TransportOutcome::Admitted {
                    native_reference: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(result["transport_state"], "admitted");
        assert_eq!(result["task_status"], "queued");
        assert_eq!(result["receiver_ready"], false);
        assert_eq!(
            p.receiver_binding_status("receiver").await.unwrap()["ready"],
            false
        );
        sqlx::query("UPDATE handoff_candidate_probes SET state='started',started_at=unixepoch()-60,finished_at=NULL WHERE id=?")
            .bind(&reserved.envelope.id).execute(&p.0.db).await.unwrap();
        assert_eq!(
            p.candidate_probe_status(&reserved.envelope.id)
                .await
                .unwrap()["transport_state"],
            "unknown"
        );
    }
}
