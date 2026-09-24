//! Authenticated task notifications. The task inbox remains the durable source.
use super::*;
use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use std::convert::Infallible;
use tokio::time::Instant;

#[derive(FromRow)]
struct StreamRow {
    sequence: i64,
    action: String,
    id: String,
    sender_id: String,
    sender_name: String,
    recipient_id: String,
    title: String,
    status: String,
    result: Option<String>,
    error: Option<String>,
    result_acknowledged_at: Option<String>,
    created_at: String,
}

impl StreamRow {
    fn notification(&self, connection_id: &str) -> Option<(&'static str, Value)> {
        match self.action.as_str() {
            "created" if self.recipient_id == connection_id && self.status == "queued" => Some((
                "task-queued",
                json!({"task_id":self.id,"title":self.title,"sender_name":self.sender_name,"queued_at":self.created_at}),
            )),
            "cancelled" if self.recipient_id == connection_id && self.status == "cancelled" => {
                Some(("task-cancelled", json!({"task_id":self.id})))
            }
            "completed"
                if self.sender_id == connection_id
                    && self.status == "completed"
                    && self.result_acknowledged_at.is_none() =>
            {
                Some((
                    "task-completed",
                    json!({"task_id":self.id,"result":self.result}),
                ))
            }
            "failed" | "timed_out"
                if self.sender_id == connection_id && self.result_acknowledged_at.is_none() =>
            {
                Some((
                    "task-failed",
                    json!({"task_id":self.id,"status":self.status,"error":self.error}),
                ))
            }
            _ => None,
        }
    }
}

async fn read_batch(p: &Publisher, cursor: i64) -> Result<Vec<StreamRow>> {
    sqlx::query_as(
        "SELECT e.sequence,json_extract(e.payload,'$.action') AS action,t.id,t.sender_id,t.sender_name,t.recipient_id,t.title,t.status,t.result,t.error,t.result_acknowledged_at,t.created_at FROM events e JOIN agent_handoffs t ON t.id=json_extract(e.payload,'$.id') WHERE e.kind='agent.handoff' AND e.sequence>? ORDER BY e.sequence LIMIT 100",
    )
    .bind(cursor)
    .fetch_all(&p.0.db)
    .await
    .map_err(Into::into)
}

pub(super) async fn stream(
    http::AgentState(p): http::AgentState,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    let mut cursor = match headers.get("last-event-id") {
        Some(value) => value
            .to_str()
            .ok()
            .and_then(|text| text.parse::<i64>().ok())
            .filter(|cursor| *cursor >= 0)
            .ok_or(StatusCode::BAD_REQUEST)?,
        None => 0,
    };
    let credential = http::bearer_credential(&headers)
        .map_err(|_| StatusCode::UNAUTHORIZED)?
        .to_owned();
    let connection_id = p.connection_id().to_owned();
    let permit =
        p.0.handoff_streams
            .clone()
            .try_acquire_owned()
            .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    let max_sequence: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sequence),0) FROM events")
        .fetch_one(&p.0.db)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if cursor > max_sequence {
        // A client can outlive a restored database. Replay actionable records
        // instead of waiting forever for the old sequence to reappear.
        cursor = 0;
    }
    let stream = async_stream::stream! {
        let _permit = permit;
        let mut next_auth_check = Instant::now();
        loop {
            let mut checked_auth = false;
            if Instant::now() >= next_auth_check {
                match p.authenticate_connection(&credential).await {
                    Ok(Some(agent)) if agent.connection_id() == connection_id => {}
                    Ok(_) => break,
                    Err(error) => {
                        tracing::error!(%error, "Could not recheck handoff stream authorization");
                        break;
                    }
                }
                checked_auth = true;
                next_auth_check = Instant::now() + Duration::from_secs(5);
            }
            let rows = match read_batch(&p, cursor).await {
                Ok(rows) => rows,
                Err(error) => {
                    tracing::error!(%error, "Could not read handoff stream journal");
                    break;
                }
            };
            if !rows.is_empty() && !checked_auth {
                // A rotated or disconnected credential must not receive a new
                // event merely because its existing socket is still open.
                match p.authenticate_connection(&credential).await {
                    Ok(Some(agent)) if agent.connection_id() == connection_id => {}
                    Ok(_) => break,
                    Err(error) => {
                        tracing::error!(%error, "Could not recheck handoff stream authorization");
                        break;
                    }
                }
                next_auth_check = Instant::now() + Duration::from_secs(5);
            }
            let full_batch = rows.len() == 100;
            for row in rows {
                cursor = row.sequence;
                if let Some((kind, data)) = row.notification(&connection_id) {
                    yield Ok::<Event, Infallible>(
                        Event::default().id(row.sequence.to_string()).event(kind).data(data.to_string())
                    );
                }
            }
            if full_batch {
                continue;
            }
            tokio::select! {
                _ = p.0.shutdown.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
        }
    };
    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("heartbeat"),
        )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

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
        for id in ["sender", "receiver", "other"] {
            let token = p.0.vault.seal(&format!("stream-token-{id}")).unwrap();
            sqlx::query("INSERT INTO agent_connections(id,name,product,token,publish_enabled) VALUES(?,?,?,?,0)")
                .bind(id).bind(id).bind("Codex").bind(token)
                .execute(&p.0.db).await.unwrap();
        }
        for id in ["receiver", "other"] {
            sqlx::query(
                "INSERT INTO agent_handoff_grants(sender_id,recipient_id) VALUES('sender',?)",
            )
            .bind(id)
            .execute(&p.0.db)
            .await
            .unwrap();
        }
        (dir, p)
    }

    fn headers(id: &str, last: Option<i64>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer stream-token-{id}").parse().unwrap(),
        );
        if let Some(last) = last {
            headers.insert("last-event-id", last.to_string().parse().unwrap());
        }
        headers
    }

    async fn next_frame(body: &mut axum::body::Body) -> String {
        let frame = tokio::time::timeout(Duration::from_secs(3), body.frame())
            .await
            .expect("stream timed out")
            .expect("stream closed")
            .expect("stream frame failed");
        String::from_utf8(frame.into_data().expect("expected SSE data frame").to_vec()).unwrap()
    }

    #[tokio::test]
    async fn stream_replays_queued_work_and_routes_terminal_events_by_principal() {
        let (_dir, p) = fixture().await;
        let sender = p.for_connection("sender");
        let unrelated = sender
            .create_handoff(handoffs::CreateHandoff {
                request_id: Uuid::new_v4().to_string(),
                recipient_id: "other".into(),
                title: "Other agent task".into(),
                instructions: "Private other instructions".into(),
                timeout_seconds: Some(600),
                automatic_delivery: false,
            })
            .await
            .unwrap();
        let task = sender
            .create_handoff(handoffs::CreateHandoff {
                request_id: Uuid::new_v4().to_string(),
                recipient_id: "receiver".into(),
                title: "Receiver task".into(),
                instructions: "Private receiver instructions".into(),
                timeout_seconds: Some(600),
                automatic_delivery: false,
            })
            .await
            .unwrap();
        let response = stream(
            http::AgentState(p.for_connection("receiver")),
            headers("receiver", None),
        )
        .await
        .unwrap();
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        assert_eq!(response.headers()["x-accel-buffering"], "no");
        let mut body = response.into_body();
        let queued = next_frame(&mut body).await;
        assert!(queued.contains("event: task-queued"), "{queued}");
        assert!(queued.contains(task["id"].as_str().unwrap()), "{queued}");
        assert!(!queued.contains(unrelated["id"].as_str().unwrap()));
        assert!(!queued.contains("Private receiver instructions"));

        sender
            .cancel_handoff(task["id"].as_str().unwrap())
            .await
            .unwrap();
        let cancelled = next_frame(&mut body).await;
        assert!(cancelled.contains("event: task-cancelled"), "{cancelled}");
        assert!(cancelled.contains(task["id"].as_str().unwrap()));
        let task2 = sender
            .create_handoff(handoffs::CreateHandoff {
                request_id: Uuid::new_v4().to_string(),
                recipient_id: "receiver".into(),
                title: "Result task".into(),
                instructions: "Return a harmless result".into(),
                timeout_seconds: Some(600),
                automatic_delivery: false,
            })
            .await
            .unwrap();
        let recipient = p.for_connection("receiver");
        let token = Uuid::new_v4().to_string();
        recipient
            .claim_handoff(task2["id"].as_str().unwrap(), &token, None)
            .await
            .unwrap();
        recipient
            .finish_handoff(
                task2["id"].as_str().unwrap(),
                handoffs::FinishInput {
                    claim_token: token,
                    message: "Done".into(),
                },
                false,
            )
            .await
            .unwrap();
        let sender_response = stream(http::AgentState(sender), headers("sender", None))
            .await
            .unwrap();
        let completed = next_frame(&mut sender_response.into_body()).await;
        assert!(completed.contains("event: task-completed"), "{completed}");
        assert!(completed.contains(task2["id"].as_str().unwrap()));
        assert!(completed.contains("Done"));
    }

    #[tokio::test]
    async fn last_event_id_skips_old_events_and_rotated_token_closes_stream() {
        let (_dir, p) = fixture().await;
        let sender = p.for_connection("sender");
        let first = sender
            .create_handoff(handoffs::CreateHandoff {
                request_id: Uuid::new_v4().to_string(),
                recipient_id: "receiver".into(),
                title: "First".into(),
                instructions: "No action".into(),
                timeout_seconds: Some(600),
                automatic_delivery: false,
            })
            .await
            .unwrap();
        let cursor: i64 = sqlx::query_scalar("SELECT MAX(sequence) FROM events")
            .fetch_one(&p.0.db)
            .await
            .unwrap();
        let second = sender
            .create_handoff(handoffs::CreateHandoff {
                request_id: Uuid::new_v4().to_string(),
                recipient_id: "receiver".into(),
                title: "Second".into(),
                instructions: "No action".into(),
                timeout_seconds: Some(600),
                automatic_delivery: false,
            })
            .await
            .unwrap();
        let mut body = stream(
            http::AgentState(p.for_connection("receiver")),
            headers("receiver", Some(cursor)),
        )
        .await
        .unwrap()
        .into_body();
        let event = next_frame(&mut body).await;
        assert!(event.contains(second["id"].as_str().unwrap()));
        assert!(!event.contains(first["id"].as_str().unwrap()));
        let replacement = p.0.vault.seal("rotated-stream-token").unwrap();
        sqlx::query("UPDATE agent_connections SET token=? WHERE id='receiver'")
            .bind(replacement)
            .execute(&p.0.db)
            .await
            .unwrap();
        sender
            .create_handoff(handoffs::CreateHandoff {
                request_id: Uuid::new_v4().to_string(),
                recipient_id: "receiver".into(),
                title: "After rotation".into(),
                instructions: "No action".into(),
                timeout_seconds: Some(600),
                automatic_delivery: false,
            })
            .await
            .unwrap();
        let closed = tokio::time::timeout(Duration::from_secs(3), body.frame())
            .await
            .unwrap();
        assert!(
            closed.is_none(),
            "rotated credential kept receiving stream frames"
        );
        assert_eq!(
            stream(http::AgentState(p.for_connection("receiver")), {
                let mut invalid = headers("receiver", None);
                invalid.insert("last-event-id", "not-an-integer".parse().unwrap());
                invalid
            })
            .await
            .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn bridge_requires_the_agent_credential_for_the_stream() {
        use axum::{body::Body, http::Request};
        let (_dir, p) = fixture().await;
        let app = p.bridge_router();
        let request = |token: Option<&str>| {
            let mut builder = Request::builder()
                .uri("/v1/agent-tasks/stream")
                .header(header::ACCEPT, "text/event-stream");
            if let Some(token) = token {
                builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
            }
            builder.body(Body::empty()).unwrap()
        };
        assert_eq!(
            app.clone().oneshot(request(None)).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.clone()
                .oneshot(request(Some("wrong-token")))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let response = app
            .oneshot(request(Some("stream-token-receiver")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
    }
}
