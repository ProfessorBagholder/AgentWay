//! Receiver-scoped hints for the dormant automatic-delivery outbox.
//! A stream proves transport connectivity, never native wake or acceptance.
use super::*;
use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use std::convert::Infallible;
use tokio::time::Instant;

#[derive(FromRow)]
struct Envelope {
    cursor: i64,
    id: String,
    task_id: String,
    kind: String,
    binding_generation: i64,
    expires_at: i64,
}

async fn read_batch(
    p: &Publisher,
    receiver: &handoff_receivers::ReceiverIdentity,
    cursor: i64,
) -> Result<Vec<Envelope>> {
    sqlx::query_as(
        "SELECT o.rowid AS cursor,o.id,o.task_id,o.kind,o.binding_generation,o.expires_at FROM handoff_delivery_outbox o JOIN handoff_receiver_bindings b ON b.connection_id=o.connection_id JOIN agent_handoffs t ON t.id=o.task_id WHERE o.connection_id=? AND o.binding_generation=? AND b.generation=? AND b.enabled=1 AND o.state IN ('pending','admitted') AND o.expires_at>unixepoch() AND o.rowid>? AND (o.kind='result_available' OR (t.status IN ('queued','claimed') AND EXISTS(SELECT 1 FROM agent_handoff_grants g WHERE g.sender_id=t.sender_id AND g.recipient_id=t.recipient_id))) ORDER BY o.rowid LIMIT 100",
    )
    .bind(&receiver.connection_id).bind(receiver.generation).bind(receiver.generation).bind(cursor)
    .fetch_all(&p.0.db).await.map_err(Into::into)
}

pub(super) async fn stream(
    State(p): State<Publisher>,
    axum::Extension(receiver): axum::Extension<handoff_receivers::ReceiverIdentity>,
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
    let permit =
        p.0.handoff_streams
            .clone()
            .try_acquire_owned()
            .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    let max_cursor: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(rowid),0) FROM handoff_delivery_outbox")
            .fetch_one(&p.0.db)
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if cursor > max_cursor {
        cursor = 0;
    }
    let stream = async_stream::stream! {
        let _permit = permit;
        let mut next_auth_check = Instant::now();
        loop {
            let mut checked_auth = false;
            if Instant::now() >= next_auth_check {
                match p.authenticate_receiver(&credential).await {
                    Ok(Some(current)) if current.connection_id == receiver.connection_id && current.generation == receiver.generation => {}
                    Ok(_) => break,
                    Err(error) => {
                        tracing::error!(%error,"Could not recheck receiver stream authorization");
                        break;
                    }
                }
                checked_auth = true;
                next_auth_check = Instant::now() + Duration::from_secs(5);
            }
            let rows = match read_batch(&p, &receiver, cursor).await {
                Ok(rows) => rows,
                Err(error) => {
                    tracing::error!(%error,"Could not read receiver stream outbox");
                    break;
                }
            };
            if !rows.is_empty() && !checked_auth {
                match p.authenticate_receiver(&credential).await {
                    Ok(Some(current)) if current.connection_id == receiver.connection_id && current.generation == receiver.generation => {}
                    Ok(_) => break,
                    Err(error) => {
                        tracing::error!(%error,"Could not recheck receiver stream authorization");
                        break;
                    }
                }
                next_auth_check = Instant::now() + Duration::from_secs(5);
            }
            let full_batch = rows.len() == 100;
            for row in rows {
                cursor = row.cursor;
                let data = json!({"protocol_version":1,"delivery_id":row.id,"task_id":row.task_id,"kind":row.kind,"binding_generation":row.binding_generation,"expires_at":row.expires_at});
                yield Ok::<Event, Infallible>(
                    Event::default().id(cursor.to_string()).event("delivery-available").data(data.to_string())
                );
            }
            if full_batch { continue; }
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
    use sha2::{Digest, Sha256};
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
            sqlx::query("INSERT INTO agent_connections(id,name,product,token,publish_enabled) VALUES(?,?,?,?,0)")
                .bind(id).bind(id).bind("test").bind(p.0.vault.seal(&format!("agent-{id}")).unwrap())
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
            let hash = format!("{:x}", Sha256::digest(format!("receiver-{id}").as_bytes()));
            sqlx::query("INSERT INTO handoff_receiver_bindings(connection_id,product,target_ciphertext,receiver_secret_hash,generation,enabled) VALUES(?,'test','cipher',?,1,1)")
                .bind(id).bind(hash).execute(&p.0.db).await.unwrap();
        }
        (dir, p)
    }

    async fn offer(p: &Publisher, recipient: &str) -> (String, String, i64) {
        let task_id = Uuid::new_v4().to_string();
        let delivery_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO agent_handoffs(id,request_id,sender_id,sender_name,recipient_id,recipient_name,title,instructions,delivery_mode) VALUES(?,?,'sender','sender',?,?,'Secret title','Secret instructions','automatic')")
            .bind(&task_id).bind(Uuid::new_v4().to_string()).bind(recipient).bind(recipient)
            .execute(&p.0.db).await.unwrap();
        sqlx::query("INSERT INTO handoff_delivery_outbox(id,task_id,kind,connection_id,binding_generation,expires_at) VALUES(?,?,'task_offered',?,1,unixepoch()+600)")
            .bind(&delivery_id).bind(&task_id).bind(recipient).execute(&p.0.db).await.unwrap();
        let cursor: i64 =
            sqlx::query_scalar("SELECT rowid FROM handoff_delivery_outbox WHERE id=?")
                .bind(&delivery_id)
                .fetch_one(&p.0.db)
                .await
                .unwrap();
        (task_id, delivery_id, cursor)
    }

    fn identity() -> handoff_receivers::ReceiverIdentity {
        handoff_receivers::ReceiverIdentity {
            connection_id: "receiver".into(),
            generation: 1,
        }
    }

    fn headers(last: Option<i64>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer receiver-receiver".parse().unwrap(),
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
        String::from_utf8(frame.into_data().expect("expected SSE data").to_vec()).unwrap()
    }

    #[tokio::test]
    async fn stream_filters_by_receiver_and_replays_after_cursor() {
        let (_dir, p) = fixture().await;
        let other = offer(&p, "other").await;
        let first = offer(&p, "receiver").await;
        let second = offer(&p, "receiver").await;
        let response = stream(
            State(p.clone()),
            axum::Extension(identity()),
            headers(Some(first.2)),
        )
        .await
        .unwrap();
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        assert_eq!(response.headers()["x-accel-buffering"], "no");
        let event = next_frame(&mut response.into_body()).await;
        assert!(event.contains("event: delivery-available"), "{event}");
        assert!(event.contains(&second.1), "{event}");
        assert!(!event.contains(&first.1));
        assert!(!event.contains(&other.1));
        assert!(!event.contains("Secret title"));
        assert!(!event.contains("Secret instructions"));
        assert_eq!(
            stream(State(p), axum::Extension(identity()), {
                let mut invalid = headers(None);
                invalid.insert("last-event-id", "bad".parse().unwrap());
                invalid
            })
            .await
            .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn rotated_receiver_token_closes_stream_and_agent_token_cannot_open_it() {
        use axum::{body::Body, http::Request};
        let (_dir, p) = fixture().await;
        let app = p.bridge_router();
        let request = |token: &str| {
            Request::builder()
                .uri("/v1/handoff-receiver/deliveries/stream")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(
            app.clone()
                .oneshot(request("agent-receiver"))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.clone()
                .oneshot(
                    Request::builder()
                        .uri("/v1/handoff-receiver/deliveries/stream")
                        .body(Body::empty())
                        .unwrap()
                )
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let mut body = stream(State(p.clone()), axum::Extension(identity()), headers(None))
            .await
            .unwrap()
            .into_body();
        let first = offer(&p, "receiver").await;
        assert!(next_frame(&mut body).await.contains(&first.1));
        let hash = format!("{:x}", Sha256::digest("rotated-token".as_bytes()));
        sqlx::query("UPDATE handoff_receiver_bindings SET receiver_secret_hash=?,generation=2 WHERE connection_id='receiver'")
            .bind(hash).execute(&p.0.db).await.unwrap();
        offer(&p, "receiver").await;
        let closed = tokio::time::timeout(Duration::from_secs(8), body.frame())
            .await
            .unwrap();
        assert!(
            closed.is_none(),
            "rotated token kept receiving delivery hints"
        );
    }

    #[tokio::test]
    async fn pending_envelope_replays_after_server_restart() {
        let (dir, p) = fixture().await;
        let delivery = offer(&p, "receiver").await;
        let db = p.0.db.clone();
        drop(p);
        let restarted = Publisher::new(db, dir.path().join("publishing"), CancellationToken::new())
            .await
            .unwrap();
        let event = next_frame(
            &mut stream(State(restarted), axum::Extension(identity()), headers(None))
                .await
                .unwrap()
                .into_body(),
        )
        .await;
        assert!(event.contains(&delivery.1), "{event}");
    }
}
