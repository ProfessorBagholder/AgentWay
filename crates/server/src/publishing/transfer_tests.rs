use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
// This fake exercises AgentWay's adapter and failure states; the real tusd
// container is exercised separately by tests/architecture and the ingress probe.
async fn fixture() -> (tempfile::TempDir, Publisher) {
    let (dir, mut p) = super::fixture().await;
    let data = dir.path().join("tus");
    tokio::fs::create_dir(&data).await.unwrap();
    let inner = Arc::get_mut(&mut p.0).unwrap();
    inner.tus_dir = data.clone();
    async fn transport(
        State(dir): State<PathBuf>,
        request: Request<Body>,
    ) -> axum::response::Response {
        let (parts, body) = request.into_parts();
        let id = parts.uri.path().trim_start_matches("/files/");
        if parts.uri.path() == "/health" {
            return StatusCode::OK.into_response();
        }
        if parts.method == "POST" {
            let id = String::from_utf8(
                STANDARD
                    .decode(
                        parts.headers["upload-metadata"]
                            .to_str()
                            .unwrap()
                            .split_once(' ')
                            .unwrap()
                            .1,
                    )
                    .unwrap(),
            )
            .unwrap();
            let size = parts.headers["upload-length"].to_str().unwrap();
            tokio::fs::write(dir.join(&id), []).await.unwrap();
            tokio::fs::write(dir.join(format!("{id}.info")), size)
                .await
                .unwrap();
            return StatusCode::CREATED.into_response();
        }
        let Ok(size) = tokio::fs::read_to_string(dir.join(format!("{id}.info"))).await else {
            return StatusCode::NOT_FOUND.into_response();
        };
        if parts.method == "DELETE" {
            let _ = tokio::fs::remove_file(dir.join(id)).await;
            let _ = tokio::fs::remove_file(dir.join(format!("{id}.info"))).await;
            return StatusCode::NO_CONTENT.into_response();
        }
        let offset = tokio::fs::metadata(dir.join(id)).await.unwrap().len();
        if parts.method == "HEAD" {
            return (
                StatusCode::OK,
                [
                    ("upload-offset", offset.to_string()),
                    ("upload-length", size),
                ],
            )
                .into_response();
        }
        if parts.headers["upload-offset"].to_str().unwrap() != offset.to_string() {
            return StatusCode::CONFLICT.into_response();
        }
        let bytes = body.collect().await.unwrap().to_bytes();
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .open(dir.join(id))
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut file, &bytes)
            .await
            .unwrap();
        (
            StatusCode::NO_CONTENT,
            [("upload-offset", (offset + bytes.len() as u64).to_string())],
        )
            .into_response()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    inner.tus_url = Some(format!("http://{}", listener.local_addr().unwrap()));
    let app = Router::new().fallback(transport).with_state(data);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (dir, p)
}
async fn restart(p: Publisher, dir: &std::path::Path) -> Publisher {
    let url = p.0.tus_url.clone();
    let data = p.0.tus_dir.clone();
    let db = p.0.db.clone();
    drop(p);
    let mut p = Publisher::new(db, dir.join("publishing"), CancellationToken::new())
        .await
        .unwrap();
    let inner = Arc::get_mut(&mut p.0).unwrap();
    inner.tus_url = url;
    inner.tus_dir = data;
    p
}
fn reservation(data: &[u8]) -> transfers::CreateUpload {
    transfers::CreateUpload {
        request_id: Uuid::new_v4().to_string(),
        size: data.len() as i64,
        mime: "video/mp4".into(),
        sha256: format!("{:x}", Sha256::digest(data)),
    }
}
async fn tus_request(
    p: &Publisher,
    id: &str,
    method: &str,
    offset: i64,
    data: Vec<u8>,
    checksum: Option<String>,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(format!("/v1/media/uploads/{id}/bytes"))
        .header(
            "authorization",
            format!("Bearer {}", p.secret("agent_token").await.unwrap()),
        )
        .header("tus-resumable", "1.0.0")
        .header("upload-offset", offset)
        .header("content-type", "application/offset+octet-stream")
        .header("content-length", data.len());
    if let Some(sum) = checksum {
        builder = builder.header("upload-checksum", sum);
    }
    p.bridge_router()
        .oneshot(builder.body(Body::from(data)).unwrap())
        .await
        .unwrap()
}
fn checksum(data: &[u8]) -> String {
    format!("sha256 {}", STANDARD.encode(Sha256::digest(data)))
}

#[tokio::test]
async fn owner_activity_tracks_progress_failure_and_recovery_without_secrets() {
    let (_dir, mut p) = fixture().await;
    let upload = p
        .create_resumable_upload(reservation(b"abcdefgh"))
        .await
        .unwrap();
    let id = upload["media_id"].as_str().unwrap().to_owned();
    let (status, list) = super::admin(&p, "GET", "/api/media-transfers", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"][0]["id"], id);
    assert_eq!(list["items"][0]["offset"], 0);
    assert!(list["next"].is_null());
    let original = p.0.tus_url.clone();
    Arc::get_mut(&mut p.0).unwrap().tus_url = Some("http://127.0.0.1:1".into());
    let failure = tus_request(
        &p,
        &id,
        "PATCH",
        0,
        b"abcd".to_vec(),
        Some(checksum(b"abcd")),
    )
    .await;
    assert_eq!(failure.status(), StatusCode::SERVICE_UNAVAILABLE);
    let (_, detail) =
        super::admin(&p, "GET", &format!("/api/media-transfers/{id}"), json!({})).await;
    assert_eq!(detail["status"], "interrupted");
    assert_eq!(detail["offset"], 0);
    assert_eq!(detail["last_error"], "media_transport_unavailable");
    Arc::get_mut(&mut p.0).unwrap().tus_url = original;
    let restored = tus_request(
        &p,
        &id,
        "PATCH",
        0,
        b"abcd".to_vec(),
        Some(checksum(b"abcd")),
    )
    .await;
    assert_eq!(restored.status(), StatusCode::NO_CONTENT);
    let (_, detail) =
        super::admin(&p, "GET", &format!("/api/media-transfers/{id}"), json!({})).await;
    assert_eq!(detail["status"], "receiving");
    assert_eq!(detail["offset"], 4);
    assert!(detail["last_error"].is_null());
    let (_, history) = super::admin(
        &p,
        "GET",
        &format!("/api/media-transfers/{id}/history"),
        json!({}),
    )
    .await;
    let steps = history["items"].as_array().unwrap();
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0]["status"], "reserved");
    assert_eq!(steps[1]["error"], "media_transport_unavailable");
    assert_eq!(steps[2]["offset"], 4);
    assert!(steps.iter().all(|step| step["event_at"].is_string()));
    assert!(!history.to_string().contains("Bearer"));
}

#[tokio::test]
async fn owner_transfer_activity_can_reach_older_records() {
    let (_dir, p) = fixture().await;
    for _ in 0..101 {
        sqlx::query("INSERT INTO media_uploads(media_id,agent_id,request_id,size,mime,sha256,created_at) VALUES(?,?,?,?,?,?,'2026-09-23T00:00:00Z')")
            .bind(Uuid::new_v4().to_string())
            .bind(p.connection_id())
            .bind(Uuid::new_v4().to_string())
            .bind(1_i64)
            .bind("video/mp4")
            .bind("0".repeat(64))
            .execute(&p.0.db)
            .await
            .unwrap();
    }
    let (_, first) = super::admin(&p, "GET", "/api/media-transfers", json!({})).await;
    assert_eq!(first["items"].as_array().unwrap().len(), 100);
    let cursor = first["next"].as_i64().unwrap();
    let (_, second) = super::admin(
        &p,
        "GET",
        &format!("/api/media-transfers?before={cursor}"),
        json!({}),
    )
    .await;
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert!(second["next"].is_null());
}

#[tokio::test]
async fn resumable_retry_restart_and_finalization_are_durable() {
    let (dir, p) = fixture().await;
    let args = reservation(b"abcdefgh");
    let upload = p.create_resumable_upload(args.clone()).await.unwrap();
    let id = upload["media_id"].as_str().unwrap().to_owned();
    assert_eq!(
        p.create_resumable_upload(args.clone()).await.unwrap()["media_id"],
        id
    );
    let mut changed = args.clone();
    changed.sha256 = "0".repeat(64);
    assert!(p.create_resumable_upload(changed).await.is_err());
    assert_eq!(
        call(
            &p,
            "PUT",
            &format!("/v1/media/{id}"),
            b"abcdefgh".to_vec(),
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let response = tus_request(
        &p,
        &id,
        "PATCH",
        0,
        b"abcd".to_vec(),
        Some(checksum(b"abcd")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(response.headers()["upload-offset"], "4");
    assert!(response.headers().contains_key("upload-expires"));
    let retry = tus_request(&p, &id, "PATCH", 0, b"abcd".to_vec(), None).await;
    assert_eq!(retry.status(), StatusCode::CONFLICT);
    assert_eq!(retry.headers()["upload-offset"], "4");
    // A client lost the first acknowledgement, then the application restarted.
    let p = restart(p, dir.path()).await;
    let head = tus_request(&p, &id, "HEAD", 0, vec![], None).await;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()["upload-offset"], "4");
    assert_eq!(head.headers()["cache-control"], "no-store");
    assert!(p.complete_media_upload(&id).await.is_err());
    let sha1 = format!("sha1 {}", STANDARD.encode(sha1::Sha1::digest(b"efgh")));
    assert_eq!(
        tus_request(&p, &id, "PATCH", 4, b"efgh".to_vec(), Some(sha1))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(p.media_upload_status(&id).await.unwrap()["ready"], false);
    let p = restart(p, dir.path()).await;
    assert_eq!(p.complete_media_upload(&id).await.unwrap()["ready"], true);
    assert_eq!(p.complete_media_upload(&id).await.unwrap()["ready"], true);
    assert_eq!(
        tokio::fs::read(p.media_file(&id).await.unwrap())
            .await
            .unwrap(),
        b"abcdefgh"
    );
    account(&p).await;
    assert!(p.enqueue(input(id.clone())).await.is_ok());
    assert!(p.cancel_media_upload(&id).await.is_err());
}

#[tokio::test]
async fn resumable_rejects_bad_checksums_offsets_lengths_and_concurrent_overlap() {
    let (_dir, p) = fixture().await;
    let u = p
        .create_resumable_upload(reservation(b"abcdefgh"))
        .await
        .unwrap();
    let id = u["media_id"].as_str().unwrap();
    assert_eq!(
        tus_request(
            &p,
            id,
            "PATCH",
            0,
            b"abcd".to_vec(),
            Some(checksum(b"wrong"))
        )
        .await
        .status()
        .as_u16(),
        460
    );
    assert_eq!(p.media_upload_status(id).await.unwrap()["offset"], 0);
    assert_eq!(
        tus_request(&p, id, "PATCH", 9, vec![], None).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        tus_request(&p, id, "PATCH", 0, vec![0; 9], None)
            .await
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let (a, b) = tokio::join!(
        tus_request(&p, id, "PATCH", 0, b"abcd".to_vec(), None),
        tus_request(&p, id, "PATCH", 0, b"xxxx".to_vec(), None)
    );
    assert!((a.status() == 204 && b.status() == 409) || (a.status() == 409 && b.status() == 204));
    assert_eq!(p.media_upload_status(id).await.unwrap()["offset"], 4);
    assert_eq!(
        tus_request(&p, id, "PATCH", 4, b"WRNG".to_vec(), None)
            .await
            .status(),
        204
    );
    assert!(
        p.complete_media_upload(id)
            .await
            .unwrap_err()
            .to_string()
            .contains("checksum")
    );
    let ready: i64 = sqlx::query_scalar("SELECT ready FROM media WHERE id=?")
        .bind(id)
        .fetch_one(&p.0.db)
        .await
        .unwrap();
    assert_eq!(ready, 0);
    assert!(p.complete_media_upload(id).await.is_err());
    assert_eq!(
        p.cancel_media_upload(id).await.unwrap()["status"],
        "cancelled"
    );
    assert_eq!(
        p.cancel_media_upload(id).await.unwrap()["status"],
        "cancelled"
    );
    assert_eq!(
        tus_request(&p, id, "HEAD", 0, vec![], None).await.status(),
        410
    );
}

#[tokio::test]
async fn resumable_agent_isolation_revocation_expiry_and_tombstones() {
    let (_dir, p) = fixture().await;
    sqlx::query("INSERT INTO agent_connections(id,name,product,token,publish_enabled) VALUES('other','Grok','Grok Bot','unused',1)").execute(&p.0.db).await.unwrap();
    let other = p.for_connection("other");
    let args = reservation(b"abcd");
    let u = other.create_resumable_upload(args.clone()).await.unwrap();
    let id = u["media_id"].as_str().unwrap();
    assert!(
        p.for_connection("publishing")
            .media_upload_status(id)
            .await
            .is_err()
    );
    assert!(
        p.for_connection("publishing")
            .complete_media_upload(id)
            .await
            .is_err()
    );
    assert!(
        p.for_connection("publishing")
            .cancel_media_upload(id)
            .await
            .is_err()
    );
    other
        .receive_chunk(id, 0, None, Some(2), Body::from("ab"))
        .await
        .unwrap();
    other.set("publish_enabled", "false").await.unwrap();
    assert!(
        other
            .receive_chunk(id, 2, None, Some(2), Body::from("cd"))
            .await
            .is_err()
    );
    other.set("publish_enabled", "true").await.unwrap();
    sqlx::query("UPDATE media_uploads SET expires_at=0 WHERE media_id=?")
        .bind(id)
        .execute(&p.0.db)
        .await
        .unwrap();
    p.expire_uploads().await.unwrap();
    assert_eq!(
        other.cancel_media_upload(id).await.unwrap()["status"],
        "expired"
    );
    assert!(
        other
            .create_resumable_upload(args)
            .await
            .unwrap_err()
            .to_string()
            .contains("gone")
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media WHERE id=?")
        .bind(id)
        .fetch_one(&p.0.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn resumable_interrupted_body_does_not_advance_and_protocol_headers_are_correct() {
    let (_dir, p) = fixture().await;
    let u = p
        .create_resumable_upload(reservation(b"abcdefgh"))
        .await
        .unwrap();
    let id = u["media_id"].as_str().unwrap();
    let stream = futures_util::stream::iter(vec![
        Ok(axum::body::Bytes::from_static(b"ab")),
        Err(std::io::Error::other("interrupted")),
    ]);
    assert!(
        p.receive_chunk(id, 0, None, None, Body::from_stream(stream))
            .await
            .is_err()
    );
    assert_eq!(p.media_upload_status(id).await.unwrap()["offset"], 0);
    let token = p.secret("agent_token").await.unwrap();
    let response = p
        .bridge_router()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/media/uploads/{id}/bytes"))
                .header("authorization", format!("Bearer {token}"))
                .header("tus-resumable", "0.0.0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 412);
    assert_eq!(response.headers()["tus-version"], "1.0.0");
    assert_eq!(response.headers()["tus-resumable"], "1.0.0");
    let response = tus_request(&p, id, "OPTIONS", 0, vec![], None).await;
    assert_eq!(response.status(), 204);
    assert!(
        response.headers()["tus-extension"]
            .to_str()
            .unwrap()
            .contains("expiration")
    );
    // A core client can use POST with method override when PATCH isn't available.
    let response = p
        .bridge_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/media/uploads/{id}/bytes"))
                .header("authorization", format!("Bearer {token}"))
                .header("tus-resumable", "1.0.0")
                .header("x-http-method-override", "HEAD")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["upload-offset"], "0");
}

#[tokio::test]
async fn resumable_client_cancellation_keeps_disk_commit_serialized() {
    let (_dir, p) = fixture().await;
    let u = p
        .create_resumable_upload(reservation(b"abcd"))
        .await
        .unwrap();
    let id = u["media_id"].as_str().unwrap().to_owned();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let started_body = started.clone();
    let release_body = release.clone();
    let stream = async_stream::stream! {
        yield Ok::<_,std::io::Error>(axum::body::Bytes::from_static(b"ab"));
        started_body.notify_one();
        release_body.notified().await;
        yield Ok(axum::body::Bytes::from_static(b"cd"));
    };
    let clone = p.clone();
    let task_id = id.clone();
    let request = tokio::spawn(async move {
        clone
            .receive_chunk(&task_id, 0, None, Some(4), Body::from_stream(stream))
            .await
    });
    started.notified().await;
    request.abort();
    release.notify_one();
    let state = tokio::time::timeout(Duration::from_secs(3), p.media_upload_status(&id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state["offset"], 4);
    assert_eq!(p.complete_media_upload(&id).await.unwrap()["ready"], true);
}

#[tokio::test]
async fn resumable_quota_chunk_bounds_and_interrupted_cleanup_are_enforced() {
    let (_dir, p) = fixture().await;
    let mut ids = Vec::new();
    for _ in 0..5 {
        let mut args = reservation(b"a");
        args.size = MAX_MEDIA;
        ids.push(
            p.create_resumable_upload(args).await.unwrap()["media_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    assert!(
        p.create_resumable_upload(reservation(b"a"))
            .await
            .unwrap_err()
            .to_string()
            .contains("quota")
    );
    assert!(
        p.receive_chunk(
            &ids[0],
            0,
            None,
            Some(transfers::MAX_CHUNK + 1),
            Body::empty()
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("too_large")
    );
    assert_eq!(p.media_upload_status(&ids[0]).await.unwrap()["offset"], 0);
    // Crash after cancellation invalidates readiness but before unlink/DB deletion.
    sqlx::query("UPDATE media_uploads SET status='cancelling' WHERE media_id=?")
        .bind(&ids[0])
        .execute(&p.0.db)
        .await
        .unwrap();
    p.expire_uploads().await.unwrap();
    assert!(p.create_resumable_upload(reservation(b"a")).await.is_ok());
    // The authoritative offset comes from transport, never a stale DB copy.
    p.receive_chunk(&ids[1], 0, None, Some(1), Body::from("a"))
        .await
        .unwrap();
    assert_eq!(p.media_upload_status(&ids[1]).await.unwrap()["offset"], 1);
}

#[tokio::test]
async fn resumable_recovers_creation_response_loss_and_reports_unavailable_transport() {
    let (_dir, p) = fixture().await;
    let args = reservation(b"abcd");
    let u = p.create_resumable_upload(args.clone()).await.unwrap();
    let id = u["media_id"].as_str().unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media_transfer_attempts")
        .fetch_one(&p.0.db)
        .await
        .unwrap();
    // Crash after tusd created the file but before the mapping committed.
    sqlx::query("UPDATE media_uploads SET transport_id=NULL WHERE media_id=?")
        .bind(id)
        .execute(&p.0.db)
        .await
        .unwrap();
    assert_eq!(
        p.create_resumable_upload(args).await.unwrap()["media_id"],
        id
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM media_transfer_attempts")
            .fetch_one(&p.0.db)
            .await
            .unwrap(),
        count
    );
    // Unknown outcome retains its durable ID and does not immediately POST again.
    let transport = p.media_file(id).await.unwrap();
    tokio::fs::remove_file(transport.with_extension("info"))
        .await
        .unwrap();
    sqlx::query("UPDATE media_uploads SET transport_id=NULL WHERE media_id=?")
        .bind(id)
        .execute(&p.0.db)
        .await
        .unwrap();
    assert!(
        p.media_upload_status(id)
            .await
            .unwrap_err()
            .to_string()
            .contains("pending")
    );
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE kind='media.transfer'")
        .fetch_one(&p.0.db)
        .await
        .unwrap();
    assert!(events > 0);
}

#[tokio::test]
async fn resumable_slow_deletion_does_not_block_unrelated_permission_changes() {
    let (_dir, mut p) = fixture().await;
    let u = p
        .create_resumable_upload(reservation(b"abcd"))
        .await
        .unwrap();
    let id = u["media_id"].as_str().unwrap().to_owned();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let state = (started.clone(), release.clone());
    async fn delayed(State((started, release)): State<(Arc<Notify>, Arc<Notify>)>) -> StatusCode {
        started.notify_one();
        release.notified().await;
        StatusCode::NO_CONTENT
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    Arc::get_mut(&mut p.0).unwrap().tus_url =
        Some(format!("http://{}", listener.local_addr().unwrap()));
    tokio::spawn(async move {
        axum::serve(listener, Router::new().fallback(delayed).with_state(state))
            .await
            .unwrap();
    });
    let clone = p.clone();
    let task = tokio::spawn(async move { clone.cancel_media_upload(&id).await });
    started.notified().await;
    let guard = tokio::time::timeout(Duration::from_secs(1), p.0.mutation.lock())
        .await
        .expect("storage deletion held the global lock");
    p.set("publish_enabled", "false").await.unwrap();
    drop(guard);
    release.notify_one();
    assert_eq!(task.await.unwrap().unwrap()["status"], "cancelled");
}
