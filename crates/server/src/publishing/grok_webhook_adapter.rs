//! Product-hosted Grok Bot routine ingress. HTTP 200 starts a run; only the
//! Bot's separate authenticated AgentWay claim and result complete the task.
use super::*;
use axum::http::StatusCode;
use handoff_dispatch::{DeliveryAdapter, DeliveryEnvelope, TransportOutcome};

pub(super) struct GrokWebhookAdapter(pub(super) Publisher);

pub(super) fn valid_url(value: &str) -> bool {
    if value.len() > 2048 || value.chars().any(char::is_control) {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    url.scheme() == "https"
        && (host == "api2.cursor.sh" || host == "cursor.com" || host.ends_with(".cursor.com"))
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

pub(super) async fn send_http(
    client: &reqwest::Client,
    url: &str,
    key: &str,
    envelope: &DeliveryEnvelope,
) -> TransportOutcome {
    let body = json!({
        "protocol_version": 1,
        "delivery_id": envelope.id,
        "task_id": envelope.task_id,
        "kind": envelope.kind,
        "binding_generation": envelope.binding_generation,
        "expires_at": envelope.expires_at,
    });
    match client
        .post(url)
        .bearer_auth(key)
        .timeout(Duration::from_secs(15))
        .json(&body)
        .send()
        .await
    {
        // The provider does not document a native run ID in this response.
        Ok(response) if response.status() == StatusCode::OK => TransportOutcome::Admitted {
            native_reference: None,
        },
        // Cursor documents that every non-200 response means no run started.
        Ok(_) => TransportOutcome::NotSent,
        // A request can reach Cursor even when AgentWay loses the response.
        Err(_) => TransportOutcome::Unknown,
    }
}

impl DeliveryAdapter for GrokWebhookAdapter {
    fn product(&self) -> Option<&'static str> {
        Some("Grok Bot")
    }

    async fn send(&self, envelope: &DeliveryEnvelope) -> TransportOutcome {
        let row: Option<(String, String)> = match sqlx::query_as(
            "SELECT w.url_ciphertext,w.key_ciphertext FROM handoff_grok_webhooks w JOIN handoff_delivery_outbox o ON o.connection_id=w.connection_id AND o.binding_generation=w.binding_generation JOIN handoff_receiver_bindings b ON b.connection_id=o.connection_id AND b.generation=o.binding_generation JOIN agent_connections c ON c.id=o.connection_id JOIN agent_handoffs t ON t.id=o.task_id WHERE o.id=? AND o.state='pending' AND o.expires_at>unixepoch() AND b.enabled=1 AND b.verified_at IS NOT NULL AND b.proof_expires_at>unixepoch() AND c.disconnected=0 AND (o.kind='result_available' OR (t.status='queued' AND EXISTS(SELECT 1 FROM agent_handoff_grants g WHERE g.sender_id=t.sender_id AND g.recipient_id=t.recipient_id)))",
        )
        .bind(&envelope.id)
        .fetch_optional(&self.0.0.db)
        .await
        {
            Ok(row) => row,
            Err(_) => return TransportOutcome::NotSent,
        };
        let Some((url_ciphertext, key_ciphertext)) = row else {
            return TransportOutcome::NotSent;
        };
        let (Ok(url), Ok(key)) = (
            self.0.0.vault.open_secret(&url_ciphertext),
            self.0.0.vault.open_secret(&key_ciphertext),
        ) else {
            return TransportOutcome::NotSent;
        };
        if !valid_url(&url) {
            return TransportOutcome::NotSent;
        }
        send_http(&self.0.0.client, &url, &key, envelope).await
    }
}

impl Publisher {
    pub(super) async fn grok_handoff_dispatch_worker(&self) {
        if let Err(error) = self.recover_handoff_attempts().await {
            tracing::error!(%error, "Could not reconcile handoff sends after restart");
            return;
        }
        let adapter = GrokWebhookAdapter(self.clone());
        loop {
            if self.0.shutdown.is_cancelled() {
                return;
            }
            match self.dispatch_one_handoff(&adapter).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => tracing::error!(%error, "Could not persist a Grok handoff send"),
            }
            tokio::select! {
                _ = self.0.shutdown.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(5)) => {},
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_cursor_https_webhook_hosts() {
        assert!(valid_url(
            "https://api2.cursor.sh/automations/webhook/example"
        ));
        assert!(valid_url("https://hooks.cursor.com/routine"));
        for url in [
            "http://api2.cursor.sh/x",
            "https://cursor.com.evil.test/x",
            "https://user:key@cursor.com/x",
            "https://127.0.0.1/x",
            "https://cursor.com:8443/x",
            "https://cursor.com/x#secret",
        ] {
            assert!(!valid_url(url), "accepted {url}");
        }
    }

    #[tokio::test]
    async fn http_200_is_only_transport_admission_and_non_200_is_not_sent() {
        use axum::{Router, http::HeaderMap, routing::post};
        let app = Router::new()
            .route(
                "/",
                post(
                    |headers: HeaderMap, axum::Json(body): axum::Json<Value>| async move {
                        assert_eq!(headers["authorization"], "Bearer test-key");
                        assert_eq!(body["kind"], "task_offered");
                        assert!(body.get("instructions").is_none());
                        StatusCode::OK
                    },
                ),
            )
            .route("/rejected", post(|| async { StatusCode::FORBIDDEN }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let envelope = DeliveryEnvelope {
            id: Uuid::new_v4().to_string(),
            task_id: Uuid::new_v4().to_string(),
            kind: "task_offered".into(),
            binding_generation: 1,
            expires_at: chrono::Utc::now().timestamp() + 60,
        };
        let outcome = send_http(
            &reqwest::Client::new(),
            &format!("http://{addr}/"),
            "test-key",
            &envelope,
        )
        .await;
        assert!(matches!(
            outcome,
            TransportOutcome::Admitted {
                native_reference: None
            }
        ));
        let rejected = send_http(
            &reqwest::Client::new(),
            &format!("http://{addr}/rejected"),
            "test-key",
            &envelope,
        )
        .await;
        assert!(matches!(rejected, TransportOutcome::NotSent));
    }
}
