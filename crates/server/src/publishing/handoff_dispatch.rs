//! Dormant, provider-neutral dispatch accounting. No production adapter is wired yet.
//! A persisted `started` attempt is uncertain after a crash and is never resent blindly.
use super::*;
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use std::future::Future;

const MAX_ATTEMPTS: i64 = 3;

#[derive(Debug, Clone)]
pub(super) struct DeliveryEnvelope {
    pub id: String,
    pub task_id: String,
    pub kind: String,
    pub binding_generation: i64,
    pub expires_at: i64,
}

/// `NotSent` is only valid when the adapter can prove no remote admission
/// occurred. A timeout or lost response must be `Unknown`.
#[derive(Clone)]
pub(super) enum TransportOutcome {
    Admitted { native_reference: String },
    NotSent,
    Unknown,
}

pub(super) trait DeliveryAdapter {
    fn send(&self, envelope: &DeliveryEnvelope) -> impl Future<Output = TransportOutcome> + Send;
}

struct ReservedAttempt {
    id: String,
    envelope: DeliveryEnvelope,
    sequence: i64,
}

#[derive(Serialize, FromRow)]
struct AttemptHistoryRow {
    id: String,
    sequence: i64,
    state: String,
    safe_error_code: Option<String>,
    started_at: i64,
    finished_at: Option<i64>,
}

pub(super) fn admin_routes() -> Router<Publisher> {
    Router::new().route(
        "/api/handoff-deliveries/{id}/attempts",
        get(attempt_history),
    )
}

async fn attempt_history(
    State(p): State<Publisher>,
    Path(id): Path<String>,
) -> Result<Json<Value>, http::Error> {
    Ok(Json(p.handoff_attempt_history(&id).await?))
}

impl Publisher {
    async fn handoff_attempt_history(&self, id: &str) -> Result<Value> {
        Uuid::parse_str(id)?;
        let row: Option<(String, String, String, i64, Option<String>)> = sqlx::query_as(
            "SELECT task_id,kind,state,attempts,safe_error_code FROM handoff_delivery_outbox WHERE id=?",
        ).bind(id).fetch_optional(&self.0.db).await?;
        let (task_id, kind, state, attempts, safe_error_code) =
            row.ok_or(handoffs::HandoffError {
                status: axum::http::StatusCode::NOT_FOUND,
                message: "Delivery not found",
            })?;
        let entries: Vec<AttemptHistoryRow> = sqlx::query_as(
            "SELECT id,sequence,state,safe_error_code,started_at,finished_at FROM handoff_delivery_attempts WHERE delivery_id=? ORDER BY sequence",
        ).bind(id).fetch_all(&self.0.db).await?;
        Ok(
            json!({"delivery_id":id,"task_id":task_id,"kind":kind,"state":state,"attempts":attempts,"safe_error_code":safe_error_code,"items":entries}),
        )
    }

    /// Reconcile records left in flight by a previous process before starting
    /// dispatch. The provider may have acted, so a new attempt is forbidden.
    pub(super) async fn recover_handoff_attempts(&self) -> Result<u64> {
        let _guard = self.0.mutation.lock().await;
        let mut tx = self.0.db.begin().await?;
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT a.delivery_id,o.state FROM handoff_delivery_attempts a JOIN handoff_delivery_outbox o ON o.id=a.delivery_id WHERE a.state='started'",
        )
        .fetch_all(&mut *tx)
        .await?;
        for (id, state) in &rows {
            let admitted = state == "admitted" || state == "delivered";
            if !admitted {
                sqlx::query("UPDATE handoff_delivery_outbox SET state='blocked',safe_error_code='native_outcome_unknown',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=? AND state='pending'")
                    .bind(id).execute(&mut *tx).await?;
            }
            sqlx::query("UPDATE handoff_delivery_attempts SET state=?,safe_error_code=?,finished_at=unixepoch() WHERE delivery_id=? AND state='started'")
                .bind(if admitted { "admitted" } else { "uncertain" })
                .bind(if admitted { None } else { Some("native_outcome_unknown") })
                .bind(id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(rows.len() as u64)
    }

    async fn reserve_handoff_attempt(&self) -> Result<Option<ReservedAttempt>> {
        let _guard = self.0.mutation.lock().await;
        let mut tx = self.0.db.begin().await?;
        let row: Option<(String, String, String, i64, i64, i64)> = sqlx::query_as(
            "SELECT o.id,o.task_id,o.kind,o.binding_generation,o.expires_at,o.attempts FROM handoff_delivery_outbox o JOIN handoff_receiver_bindings b ON b.connection_id=o.connection_id AND b.generation=o.binding_generation JOIN agent_connections c ON c.id=o.connection_id JOIN agent_handoffs t ON t.id=o.task_id WHERE o.state='pending' AND o.due_at<=unixepoch() AND o.expires_at>unixepoch() AND o.attempts<? AND b.enabled=1 AND b.verified_at IS NOT NULL AND b.proof_expires_at>unixepoch() AND c.disconnected=0 AND NOT EXISTS(SELECT 1 FROM handoff_delivery_attempts a WHERE a.delivery_id=o.id AND a.state='started') AND (o.kind='result_available' OR (t.status='queued' AND EXISTS(SELECT 1 FROM agent_handoff_grants g WHERE g.sender_id=t.sender_id AND g.recipient_id=t.recipient_id))) ORDER BY o.due_at,o.rowid LIMIT 1",
        )
        .bind(MAX_ATTEMPTS)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((id, task_id, kind, binding_generation, expires_at, attempts)) = row else {
            return Ok(None);
        };
        let changed = sqlx::query("UPDATE handoff_delivery_outbox SET attempts=attempts+1,due_at=unixepoch()+30,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=? AND state='pending' AND attempts=?")
            .bind(&id).bind(attempts).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Ok(None);
        }
        let attempt_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO handoff_delivery_attempts(id,delivery_id,sequence,state) VALUES(?,?,?,'started')")
            .bind(&attempt_id).bind(&id).bind(attempts + 1).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(ReservedAttempt {
            id: attempt_id,
            envelope: DeliveryEnvelope {
                id,
                task_id,
                kind,
                binding_generation,
                expires_at,
            },
            sequence: attempts + 1,
        }))
    }

    async fn finish_handoff_attempt(
        &self,
        attempt: &ReservedAttempt,
        outcome: TransportOutcome,
    ) -> Result<()> {
        let _guard = self.0.mutation.lock().await;
        let mut tx = self.0.db.begin().await?;
        let (state, attempt_state, code, due_at, reference) = match outcome {
            TransportOutcome::Admitted { native_reference }
                if !native_reference.trim().is_empty()
                    && native_reference.len() <= 255
                    && !native_reference.chars().any(char::is_control) =>
            {
                (
                    "admitted",
                    "admitted",
                    None,
                    None,
                    Some(self.0.vault.seal(&native_reference)?),
                )
            }
            TransportOutcome::Admitted { .. } | TransportOutcome::Unknown => (
                "blocked",
                "uncertain",
                Some("native_outcome_unknown"),
                None,
                None,
            ),
            TransportOutcome::NotSent if attempt.sequence < MAX_ATTEMPTS => (
                "pending",
                "retryable",
                Some("receiver_unavailable"),
                Some(chrono::Utc::now().timestamp() + 5 * (1 << (attempt.sequence - 1))),
                None,
            ),
            TransportOutcome::NotSent => (
                "blocked",
                "blocked",
                Some("receiver_unavailable"),
                None,
                None,
            ),
        };
        let updated = sqlx::query("UPDATE handoff_delivery_outbox SET state=?,safe_error_code=?,due_at=COALESCE(?,due_at),native_reference_ciphertext=COALESCE(?,native_reference_ciphertext),admitted_at=CASE WHEN ?='admitted' THEN unixepoch() ELSE admitted_at END,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=? AND state='pending' AND expires_at>unixepoch()")
            .bind(state).bind(code).bind(due_at).bind(reference).bind(state).bind(&attempt.envelope.id)
            .execute(&mut *tx).await?;
        let (attempt_state, code) = if updated.rows_affected() == 1 {
            (attempt_state, code)
        } else {
            let current: String =
                sqlx::query_scalar("SELECT state FROM handoff_delivery_outbox WHERE id=?")
                    .bind(&attempt.envelope.id)
                    .fetch_one(&mut *tx)
                    .await?;
            if current == "admitted" || current == "delivered" {
                ("admitted", None)
            } else {
                sqlx::query("UPDATE handoff_delivery_outbox SET state='blocked',safe_error_code='native_outcome_unknown',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=? AND state='pending'")
                    .bind(&attempt.envelope.id).execute(&mut *tx).await?;
                ("uncertain", Some("native_outcome_unknown"))
            }
        };
        let changed = sqlx::query("UPDATE handoff_delivery_attempts SET state=?,safe_error_code=?,finished_at=unixepoch() WHERE id=? AND state='started'")
            .bind(attempt_state).bind(code).bind(&attempt.id).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            anyhow::bail!("Delivery attempt is no longer active");
        }
        tx.commit().await?;
        Ok(())
    }

    /// Executes at most one eligible delivery. The call site controls
    /// concurrency; this foundation deliberately has no live adapter loop.
    pub(super) async fn dispatch_one_handoff<A: DeliveryAdapter>(
        &self,
        adapter: &A,
    ) -> Result<bool> {
        let Some(attempt) = self.reserve_handoff_attempt().await? else {
            return Ok(false);
        };
        let outcome = adapter.send(&attempt.envelope).await;
        self.finish_handoff_attempt(&attempt, outcome).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeAdapter(TransportOutcome);

    impl DeliveryAdapter for FakeAdapter {
        async fn send(&self, envelope: &DeliveryEnvelope) -> TransportOutcome {
            assert_eq!(envelope.kind, "task_offered");
            assert_eq!(envelope.binding_generation, 1);
            assert!(envelope.expires_at > chrono::Utc::now().timestamp());
            self.0.clone()
        }
    }

    async fn fixture() -> (tempfile::TempDir, Publisher, String) {
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
        for id in ["sender", "receiver"] {
            sqlx::query("INSERT INTO agent_connections(id,name,product,token,publish_enabled) VALUES(?,?,?,?,0)")
                .bind(id).bind(id).bind(id).bind("test-ciphertext")
                .execute(&p.0.db).await.unwrap();
        }
        sqlx::query(
            "INSERT INTO agent_handoff_grants(sender_id,recipient_id) VALUES('sender','receiver')",
        )
        .execute(&p.0.db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO handoff_receiver_bindings(connection_id,product,target_ciphertext,receiver_secret_hash,generation,enabled,verified_at,proof_expires_at) VALUES('receiver','test','cipher','hash',1,1,unixepoch(),unixepoch()+3600)")
            .execute(&p.0.db).await.unwrap();
        let task_id = Uuid::new_v4().to_string();
        let delivery_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO agent_handoffs(id,request_id,sender_id,sender_name,recipient_id,recipient_name,title,instructions,delivery_mode) VALUES(?,?,'sender','sender','receiver','receiver','Probe','Test','automatic')")
            .bind(&task_id).bind(Uuid::new_v4().to_string()).execute(&p.0.db).await.unwrap();
        sqlx::query("INSERT INTO handoff_delivery_outbox(id,task_id,kind,connection_id,binding_generation,expires_at) VALUES(?,?,'task_offered','receiver',1,unixepoch()+3600)")
            .bind(&delivery_id).bind(&task_id).execute(&p.0.db).await.unwrap();
        (dir, p, delivery_id)
    }

    #[tokio::test]
    async fn admission_records_one_attempt_and_opaque_reference() {
        let (_dir, p, id) = fixture().await;
        let adapter = FakeAdapter(TransportOutcome::Admitted {
            native_reference: "run-secret-123".into(),
        });
        assert!(p.dispatch_one_handoff(&adapter).await.unwrap());
        assert!(!p.dispatch_one_handoff(&adapter).await.unwrap());
        let row: (String, i64, String) = sqlx::query_as("SELECT state,attempts,native_reference_ciphertext FROM handoff_delivery_outbox WHERE id=?")
            .bind(&id).fetch_one(&p.0.db).await.unwrap();
        assert_eq!((row.0.as_str(), row.1), ("admitted", 1));
        assert!(!row.2.contains("run-secret-123"));
        let attempt: (i64, String) = sqlx::query_as(
            "SELECT sequence,state FROM handoff_delivery_attempts WHERE delivery_id=?",
        )
        .bind(&id)
        .fetch_one(&p.0.db)
        .await
        .unwrap();
        assert_eq!(attempt, (1, "admitted".into()));
        let history = p.handoff_attempt_history(&id).await.unwrap();
        assert_eq!(history["attempts"], 1);
        assert_eq!(history["items"][0]["state"], "admitted");
        assert!(!history.to_string().contains("run-secret-123"));
    }

    #[tokio::test]
    async fn proven_not_sent_retries_are_bounded() {
        let (_dir, p, id) = fixture().await;
        let adapter = FakeAdapter(TransportOutcome::NotSent);
        for sequence in 1..=MAX_ATTEMPTS {
            assert!(p.dispatch_one_handoff(&adapter).await.unwrap());
            let state: String =
                sqlx::query_scalar("SELECT state FROM handoff_delivery_outbox WHERE id=?")
                    .bind(&id)
                    .fetch_one(&p.0.db)
                    .await
                    .unwrap();
            assert_eq!(
                state,
                if sequence == MAX_ATTEMPTS {
                    "blocked"
                } else {
                    "pending"
                }
            );
            sqlx::query("UPDATE handoff_delivery_outbox SET due_at=unixepoch()-1 WHERE id=?")
                .bind(&id)
                .execute(&p.0.db)
                .await
                .unwrap();
        }
        assert!(!p.dispatch_one_handoff(&adapter).await.unwrap());
        let attempts: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM handoff_delivery_attempts WHERE delivery_id=?",
        )
        .bind(&id)
        .fetch_one(&p.0.db)
        .await
        .unwrap();
        assert_eq!(attempts, MAX_ATTEMPTS);
    }

    #[tokio::test]
    async fn restart_marks_unfinished_send_uncertain_without_repeating_it() {
        let (dir, p, id) = fixture().await;
        let reserved = p.reserve_handoff_attempt().await.unwrap().unwrap();
        assert_eq!(reserved.envelope.id, id);
        let db = p.0.db.clone();
        drop(p);
        let restarted = Publisher::new(db, dir.path().join("publishing"), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(restarted.recover_handoff_attempts().await.unwrap(), 1);
        assert_eq!(restarted.recover_handoff_attempts().await.unwrap(), 0);
        assert!(
            !restarted
                .dispatch_one_handoff(&FakeAdapter(TransportOutcome::NotSent))
                .await
                .unwrap()
        );
        let row: (String, String) =
            sqlx::query_as("SELECT state,safe_error_code FROM handoff_delivery_outbox WHERE id=?")
                .bind(&id)
                .fetch_one(&restarted.0.db)
                .await
                .unwrap();
        assert_eq!(row, ("blocked".into(), "native_outcome_unknown".into()));
    }

    #[tokio::test]
    async fn unknown_transport_outcome_never_retries() {
        let (_dir, p, id) = fixture().await;
        assert!(
            p.dispatch_one_handoff(&FakeAdapter(TransportOutcome::Unknown))
                .await
                .unwrap()
        );
        assert!(
            !p.dispatch_one_handoff(&FakeAdapter(TransportOutcome::NotSent))
                .await
                .unwrap()
        );
        let row: (String, String) =
            sqlx::query_as("SELECT state,safe_error_code FROM handoff_delivery_outbox WHERE id=?")
                .bind(&id)
                .fetch_one(&p.0.db)
                .await
                .unwrap();
        assert_eq!(row, ("blocked".into(), "native_outcome_unknown".into()));
    }

    #[tokio::test]
    async fn external_admission_wins_a_lost_transport_response() {
        let (_dir, p, id) = fixture().await;
        let attempt = p.reserve_handoff_attempt().await.unwrap().unwrap();
        sqlx::query("UPDATE handoff_delivery_outbox SET state='admitted',admitted_at=unixepoch() WHERE id=?")
            .bind(&id).execute(&p.0.db).await.unwrap();
        p.finish_handoff_attempt(&attempt, TransportOutcome::Unknown)
            .await
            .unwrap();
        let row: (String, Option<String>) = sqlx::query_as(
            "SELECT state,safe_error_code FROM handoff_delivery_attempts WHERE id=?",
        )
        .bind(&attempt.id)
        .fetch_one(&p.0.db)
        .await
        .unwrap();
        assert_eq!(row, ("admitted".into(), None));
        assert!(
            !p.dispatch_one_handoff(&FakeAdapter(TransportOutcome::NotSent))
                .await
                .unwrap()
        );
    }
}
