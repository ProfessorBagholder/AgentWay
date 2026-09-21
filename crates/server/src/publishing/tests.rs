use super::*;
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::post,
};
use http_body_util::BodyExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;

async fn fixture() -> (tempfile::TempDir, Publisher) {
    let dir = tempfile::tempdir().unwrap();
    let db = crate::database(&format!(
        "sqlite://{}",
        dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    let publisher = Publisher::new(db, dir.path().join("publishing"), CancellationToken::new())
        .await
        .unwrap();
    (dir, publisher)
}
async fn call(
    p: &Publisher,
    method: &str,
    path: &str,
    body: Vec<u8>,
    auth: bool,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if auth {
        request = request.header(
            "authorization",
            format!("Bearer {}", p.secret("agent_token").await.unwrap()),
        );
    }
    let res = p
        .bridge_router()
        .oneshot(request.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn media(p: &Publisher) -> String {
    let value = p
        .create_media(MediaInput {
            size: 4,
            mime: "video/mp4".into(),
        })
        .await
        .unwrap();
    let id = value["media_id"].as_str().unwrap().to_owned();
    assert_eq!(
        call(p, "PUT", &format!("/v1/media/{id}"), vec![1, 2, 3, 4], true)
            .await
            .0,
        StatusCode::OK
    );
    id
}
fn input(media_id: String) -> PublishInput {
    PublishInput {
        request_id: Uuid::new_v4().to_string(),
        media_id,
        title: "Test video".into(),
        description: String::new(),
        privacy: "private".into(),
        made_for_kids: false,
        contains_synthetic_media: false,
    }
}
async fn account(p: &Publisher) {
    p.set_secret("client_id", "test.apps.googleusercontent.com")
        .await
        .unwrap();
    p.set_secret("client_secret", "secret").await.unwrap();
    sqlx::query("INSERT INTO youtube_account VALUES(1,'channel','Test channel',?)")
        .bind(p.0.vault.seal("refresh-secret").unwrap())
        .execute(&p.0.db)
        .await
        .unwrap();
}
#[tokio::test]
async fn authentication_media_limits_and_idempotency() {
    let (_dir, p) = fixture().await;
    assert_eq!(
        call(&p, "GET", "/v1/status", vec![], false).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&p, "GET", "/api/publishing/token", vec![], true)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert!(
        p.create_media(MediaInput {
            size: MAX_MEDIA + 1,
            mime: "video/mp4".into()
        })
        .await
        .is_err()
    );
    let id = p
        .create_media(MediaInput {
            size: 4,
            mime: "video/mp4".into(),
        })
        .await
        .unwrap()["media_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        call(&p, "PUT", &format!("/v1/media/{id}"), vec![1, 2], true)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    account(&p).await;
    assert!(p.enqueue(input(id)).await.is_err());
    let i = input(media(&p).await);
    let (a, b) = tokio::join!(p.enqueue(i.clone()), p.enqueue(i.clone()));
    assert_eq!(a.unwrap().id, b.unwrap().id);
    let mut changed = i.clone();
    changed.title = "Changed".into();
    assert!(p.enqueue(changed).await.is_err());
    let mut public = input(i.media_id);
    public.privacy = "public".into();
    assert!(p.enqueue(public).await.is_err());
    assert_eq!(p.list().await.unwrap().len(), 1);
    let stored = p.setting("client_secret").await.unwrap().unwrap();
    assert!(!stored.contains("secret"));
}
#[tokio::test]
async fn oauth_state_requires_matching_cookie_expires_and_is_single_use() {
    let (_dir, p) = fixture().await;
    account(&p).await;
    let (url, state) = p
        .authorize("http://127.0.0.1:8787/api/youtube/callback".into())
        .await
        .unwrap();
    assert!(url.contains("code_challenge_method=S256"));
    assert!(
        p.oauth_callback(&state, None, &Default::default())
            .await
            .unwrap_err()
            .to_string()
            .contains("does not match")
    );
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("cookie", format!("agentway_oauth={state}").parse().unwrap());
    assert!(
        p.oauth_callback(&state, None, &headers)
            .await
            .unwrap_err()
            .to_string()
            .contains("declined")
    );
    assert!(
        p.oauth_callback(&state, None, &headers)
            .await
            .unwrap_err()
            .to_string()
            .contains("already used")
    );
    let (_, state) = p
        .authorize("http://127.0.0.1:8787/api/youtube/callback".into())
        .await
        .unwrap();
    sqlx::query("UPDATE oauth_attempts SET expires_at=0")
        .execute(&p.0.db)
        .await
        .unwrap();
    headers.insert("cookie", format!("agentway_oauth={state}").parse().unwrap());
    assert!(p.oauth_callback(&state, None, &headers).await.is_err());
}
#[derive(Clone)]
struct Mock {
    base: String,
    initiated: Arc<AtomicUsize>,
    received: Arc<AtomicUsize>,
}
async fn initiate(State(s): State<Mock>) -> impl IntoResponse {
    s.initiated.fetch_add(1, Ordering::SeqCst);
    (
        StatusCode::OK,
        [(
            "location",
            format!("{}/upload/youtube/v3/videos?session=one", s.base),
        )],
        Json(json!({})),
    )
}
async fn receive(State(s): State<Mock>, body: axum::body::Bytes) -> axum::response::Response {
    if body.is_empty() {
        if s.received.load(Ordering::SeqCst) > 0 {
            return Json(json!({"id":"test_video"})).into_response();
        }
        return StatusCode::PERMANENT_REDIRECT.into_response();
    }
    assert_eq!(&body[..], &[1, 2, 3, 4]);
    s.received.fetch_add(1, Ordering::SeqCst);
    // Simulate accepted video plus a lost final response.
    StatusCode::SERVICE_UNAVAILABLE.into_response()
}
#[tokio::test]
async fn restart_recovers_lost_final_response_without_duplicate_upload() {
    let (dir, mut p) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = Mock {
        base: base.clone(),
        initiated: Arc::new(AtomicUsize::new(0)),
        received: Arc::new(AtomicUsize::new(0)),
    };
    let mock = Router::new()
        .route(
            "/token",
            post(|| async {
                Json(json!({"access_token":"access","token_type":"Bearer","expires_in":3600}))
            }),
        )
        .route("/upload/youtube/v3/videos", post(initiate).put(receive))
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    Arc::get_mut(&mut p.0).unwrap().endpoints = Endpoints {
        token: format!("{base}/token"),
        channels: format!("{base}/channels"),
        upload: format!("{base}/upload/youtube/v3/videos"),
        videos: format!("{base}/videos"),
    };
    account(&p).await;
    let job = p.enqueue(input(media(&p).await)).await.unwrap();
    let worker = p.start_worker();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if p.publication(&job.id).await.unwrap().status == "interrupted" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    p.0.shutdown.cancel();
    worker.await.unwrap();
    p.0.db.close().await;
    drop(p);
    let db = crate::database(&format!(
        "sqlite://{}",
        dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    let mut p = Publisher::new(db, dir.path().join("publishing"), CancellationToken::new())
        .await
        .unwrap();
    Arc::get_mut(&mut p.0).unwrap().endpoints = Endpoints {
        token: format!("{base}/token"),
        channels: format!("{base}/channels"),
        upload: format!("{base}/upload/youtube/v3/videos"),
        videos: format!("{base}/videos"),
    };
    p.retry(&job.id).await.unwrap();
    let worker = p.start_worker();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if p.publication(&job.id).await.unwrap().status == "uploaded" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        p.publication(&job.id).await.unwrap().video_url.as_deref(),
        Some("https://www.youtube.com/watch?v=test_video")
    );
    assert_eq!(state.initiated.load(Ordering::SeqCst), 1);
    assert_eq!(state.received.load(Ordering::SeqCst), 1);
    p.0.shutdown.cancel();
    worker.await.unwrap();
    server.abort();
}

#[tokio::test]
async fn mcp_negotiates_lists_tools_and_calls_the_publisher() {
    let (_dir, p) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let app = p.bridge_router();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let token = p.secret("agent_token").await.unwrap();
    let init=client.post(&url).bearer_auth(&token).header("accept","application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"integration-test","version":"1"}}})).send().await.unwrap();
    assert_eq!(init.status(), StatusCode::OK);
    let session = init
        .headers()
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let response = mcp_json(init).await;
    assert!(response["result"]["capabilities"]["tools"].is_object());
    client
        .post(&url)
        .bearer_auth(&token)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-session-id", &session)
        .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .send()
        .await
        .unwrap();
    let listed = mcp_json(
        client
            .post(&url)
            .bearer_auth(&token)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-session-id", &session)
            .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert!(
        listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "publish_youtube")
    );
    let called=mcp_json(client.post(&url).bearer_auth(&token).header("accept","application/json, text/event-stream").header("mcp-session-id",&session)
        .json(&json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"youtube_status","arguments":{}}})).send().await.unwrap()).await;
    assert_eq!(called["result"]["structuredContent"]["private_only"], true);
    assert!(called["result"]["structuredContent"]["account"].is_null());
    server.abort();
}

async fn mcp_json(response: reqwest::Response) -> Value {
    use futures_util::StreamExt;
    if response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("application/json")
    {
        return response.json().await.unwrap();
    }
    let mut stream = response.bytes_stream();
    let mut buffered = String::new();
    while let Some(bytes) = stream.next().await {
        buffered.push_str(std::str::from_utf8(&bytes.unwrap()).unwrap());
        for line in buffered.lines() {
            if let Some(data) = line.strip_prefix("data: ")
                && let Ok(value) = serde_json::from_str::<Value>(data)
                && value.get("id").is_some()
            {
                return value;
            }
        }
    }
    panic!("No MCP response: {buffered}");
}

#[tokio::test]
async fn agent_status_verifies_visibility_without_reuploading() {
    let (_dir, mut p) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mode = Arc::new(AtomicUsize::new(0));
    let mock = Router::new()
        .route("/token", post(|| async { Json(json!({"access_token":"access","token_type":"Bearer","expires_in":3600})) }))
        .route("/videos", axum::routing::get({
            let mode = mode.clone();
            move |headers: axum::http::HeaderMap, axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String,String>>| {
                let mode = mode.load(Ordering::SeqCst);
                async move {
                    assert_eq!(headers["authorization"], "Bearer access");
                    assert_eq!(query["part"], "status");
                    assert_eq!(query["id"], "test_video");
                    match mode {
                        0 => Json(json!({"items":[{"id":"test_video","status":{"privacyStatus":"private"}}]})).into_response(),
                        1 => Json(json!({"items":[{"id":"test_video","status":{"privacyStatus":"public"}}]})).into_response(),
                        2 => Json(json!({"items":[]})).into_response(),
                        _ => StatusCode::SERVICE_UNAVAILABLE.into_response(),
                    }
                }
            }
        }));
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    Arc::get_mut(&mut p.0).unwrap().endpoints.token = format!("{base}/token");
    Arc::get_mut(&mut p.0).unwrap().endpoints.videos = format!("{base}/videos");
    account(&p).await;
    p.set("private_only", "false").await.unwrap();
    let mut request = input(media(&p).await);
    request.privacy = "public".into();
    let job = p.enqueue(request).await.unwrap();
    assert!(
        p.verified_publication(&job.id)
            .await
            .unwrap()
            .actual_privacy
            .is_none()
    );
    sqlx::query("UPDATE publications SET status='uploaded',video_id='test_video',video_url='https://www.youtube.com/watch?v=test_video' WHERE id=?")
        .bind(&job.id).execute(&p.0.db).await.unwrap();
    for (state, expected) in [
        (0, Some("private")),
        (1, Some("public")),
        (2, None),
        (3, None),
    ] {
        mode.store(state, Ordering::SeqCst);
        let (status, result) = call(
            &p,
            "GET",
            &format!("/v1/publications/{}", job.id),
            vec![],
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["requested_privacy"], "public");
        assert_eq!(result["actual_privacy"].as_str(), expected);
        assert_eq!(result["status"], "uploaded");
        assert_eq!(result["visibility_error"].is_null(), expected.is_some());
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM publications")
        .fetch_one(&p.0.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
    server.abort();
}
