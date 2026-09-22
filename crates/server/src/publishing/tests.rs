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
        notify_subscribers: true,
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
    assert_eq!(response["result"]["instructions"], guidance::INSTRUCTIONS);
    let status: Value = client
        .get(url.replace("/mcp", "/v1/status"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["agent_guidance"], guidance::payload());
    let required = status["agent_guidance"]["publish_schema"]["required"]
        .as_array()
        .unwrap();
    assert!(required.contains(&json!("made_for_kids")));
    assert!(required.contains(&json!("contains_synthetic_media")));
    assert!(!required.contains(&json!("notify_subscribers")));
    assert_eq!(
        status["agent_guidance"]["publish_schema"]["properties"]["notify_subscribers"]["default"],
        true
    );
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
    for name in [
        "get_youtube_video",
        "set_youtube_visibility",
        "get_video_operation",
        "list_video_operations",
    ] {
        assert!(
            listed["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"] == name)
        );
    }
    let args: super::mcp::VisibilityRequest = serde_json::from_value(
        json!({"id":"publication","request_id":Uuid::new_v4().to_string(),"privacy":"private"}),
    )
    .unwrap();
    assert_eq!(args.input.privacy, "private");
    let called=mcp_json(client.post(&url).bearer_auth(&token).header("accept","application/json, text/event-stream").header("mcp-session-id",&session)
        .json(&json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"youtube_status","arguments":{}}})).send().await.unwrap()).await;
    assert_eq!(called["result"]["structuredContent"]["private_only"], true);
    assert!(called["result"]["structuredContent"]["account"].is_null());
    assert_eq!(
        called["result"]["structuredContent"]["agent_guidance"],
        status["agent_guidance"]
    );
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

#[tokio::test]
async fn activity_tracks_authenticated_requests_and_persists_events() {
    let (_dir, p) = fixture().await;
    assert!(p.activity().await.unwrap().is_none());
    call(&p, "GET", "/v1/status", vec![], false).await;
    assert!(p.activity().await.unwrap().is_none());
    call(&p, "GET", "/v1/status", vec![], true).await;
    let first = p.activity().await.unwrap().unwrap();
    assert_eq!(first.operation, "Checked connection");
    assert!(!first.last_seen.is_empty());
    call(&p, "GET", "/v1/status", vec![], true).await;
    let next = p.activity().await.unwrap().unwrap();
    assert_eq!(next.revision, first.revision + 1);
    let payload: String = sqlx::query_scalar(
        "SELECT payload FROM events WHERE kind='bridge.activity' ORDER BY sequence DESC LIMIT 1",
    )
    .fetch_one(&p.0.db)
    .await
    .unwrap();
    let event: BridgeActivity = serde_json::from_str(&payload).unwrap();
    assert_eq!(event.revision, next.revision);
    assert!(!payload.contains(&p.secret("agent_token").await.unwrap()));
}

#[tokio::test]
async fn documented_request_requires_explicit_declarations_and_rejects_unknown_settings() {
    let (_dir, p) = fixture().await;
    account(&p).await;
    let mut example: Value = serde_json::from_str(include_str!(
        "../../../../docs/examples/youtube-publish.json"
    ))
    .unwrap();
    example["media_id"] = json!(media(&p).await);
    let decoded: PublishInput = serde_json::from_value(example.clone()).unwrap();
    assert!(decoded.notify_subscribers);
    let mut invalid = example.clone();
    invalid["notify_subscribers"] = json!("false");
    assert_eq!(
        call(
            &p,
            "POST",
            "/v1/youtube/publish",
            serde_json::to_vec(&invalid).unwrap(),
            true
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for field in ["made_for_kids", "contains_synthetic_media"] {
        let mut missing = example.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert_eq!(
            call(
                &p,
                "POST",
                "/v1/youtube/publish",
                serde_json::to_vec(&missing).unwrap(),
                true
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let mut wrong_type = example.clone();
        wrong_type[field] = json!("false");
        assert_eq!(
            call(
                &p,
                "POST",
                "/v1/youtube/publish",
                serde_json::to_vec(&wrong_type).unwrap(),
                true
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let mut unknown = example.clone();
    unknown["tags"] = json!(["unsupported"]);
    assert_eq!(
        call(
            &p,
            "POST",
            "/v1/youtube/publish",
            serde_json::to_vec(&unknown).unwrap(),
            true
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(p.list().await.unwrap().is_empty());
    let (status, first) = call(
        &p,
        "POST",
        "/v1/youtube/publish",
        serde_json::to_vec(&example).unwrap(),
        true,
    )
    .await;
    assert!(status.is_success());
    let (_, retried) = call(
        &p,
        "POST",
        "/v1/youtube/publish",
        serde_json::to_vec(&example).unwrap(),
        true,
    )
    .await;
    assert_eq!(first["id"], retried["id"]);
    // Old records omitted the notification field. The same old request still deduplicates.
    sqlx::query(
        "UPDATE publications SET input=json_remove(input,'$.notify_subscribers') WHERE id=?",
    )
    .bind(first["id"].as_str().unwrap())
    .execute(&p.0.db)
    .await
    .unwrap();
    let (status, legacy_retry) = call(
        &p,
        "POST",
        "/v1/youtube/publish",
        serde_json::to_vec(&example).unwrap(),
        true,
    )
    .await;
    assert!(status.is_success());
    assert_eq!(first["id"], legacy_retry["id"]);
    example["contains_synthetic_media"] = json!(false);
    assert_eq!(
        call(
            &p,
            "POST",
            "/v1/youtube/publish",
            serde_json::to_vec(&example).unwrap(),
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(p.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn worker_sends_visibility_and_explicit_disclosures_unchanged() {
    let (_dir, mut p) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let captured = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let mock = Router::new()
        .route(
            "/token",
            post(|| async {
                Json(json!({"access_token":"access","token_type":"Bearer","expires_in":3600}))
            }),
        )
        .route(
            "/upload/youtube/v3/videos",
            post({
                let captured = captured.clone();
                move |axum::extract::Query(query): axum::extract::Query<
                    std::collections::HashMap<String, String>,
                >,
                      Json(body): Json<Value>| {
                    let captured = captured.clone();
                    async move {
                        assert_eq!(query["part"], "snippet,status");
                        captured
                            .lock()
                            .unwrap()
                            .push(json!({"body":body,"notify":query["notifySubscribers"]}));
                        // Stop before transferring bytes; only metadata submission is under test.
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                }
            }),
        );
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    Arc::get_mut(&mut p.0).unwrap().endpoints.token = format!("{base}/token");
    Arc::get_mut(&mut p.0).unwrap().endpoints.upload = format!("{base}/upload/youtube/v3/videos");
    account(&p).await;
    p.set("private_only", "false").await.unwrap();
    let media_id = media(&p).await;
    let legacy = p.enqueue(input(media_id.clone())).await.unwrap();
    sqlx::query(
        "UPDATE publications SET input=json_remove(input,'$.notify_subscribers') WHERE id=?",
    )
    .bind(&legacy.id)
    .execute(&p.0.db)
    .await
    .unwrap();
    let worker = p.start_worker();
    tokio::time::timeout(Duration::from_secs(5), async {
        while p.publication(&legacy.id).await.unwrap().status != "interrupted" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(captured.lock().unwrap()[0]["notify"], "false");
    captured.lock().unwrap().clear();
    for privacy in ["private", "unlisted", "public"] {
        for (kids, synthetic) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut request = input(media_id.clone());
            request.privacy = privacy.into();
            request.made_for_kids = kids;
            request.contains_synthetic_media = synthetic;
            request.notify_subscribers = !kids;
            let job = p.enqueue(request).await.unwrap();
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
            let captured_request = captured.lock().unwrap().last().unwrap().clone();
            assert_eq!(
                captured_request["notify"],
                if !kids { "true" } else { "false" }
            );
            let body = &captured_request["body"];
            assert_eq!(
                body["snippet"],
                json!({"title":"Test video","description":"","categoryId":"24"})
            );
            assert_eq!(
                body["status"],
                json!({"privacyStatus":privacy,"selfDeclaredMadeForKids":kids,"containsSyntheticMedia":synthetic})
            );
        }
    }
    assert_eq!(captured.lock().unwrap().len(), 12);
    p.0.shutdown.cancel();
    worker.await.unwrap();
    server.abort();
}

async fn admin(p: &Publisher, method: &str, path: &str, value: Value) -> (StatusCode, Value) {
    let response = p
        .admin_router()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "127.0.0.1:8787")
                .header("content-type", "application/json")
                .body(Body::from(value.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}
#[tokio::test]
async fn workspace_permissions_and_disconnect_preserve_existing_publications() {
    let (_dir, p) = fixture().await;
    account(&p).await;
    let media = media(&p).await;
    let request = input(media);
    let job = p.enqueue(request.clone()).await.unwrap();
    let original = p.secret("agent_token").await.unwrap();
    let (status, connection) = admin(
        &p,
        "POST",
        "/api/publishing/connection/access",
        json!({"publish_enabled":false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(connection["publish_enabled"], false);
    assert!(
        p.enqueue(request.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("permission")
    );
    assert!(p.retry(&job.id).await.is_err());
    assert_eq!(
        call(&p, "GET", "/v1/status", vec![], true).await.0,
        StatusCode::OK
    );
    admin(
        &p,
        "POST",
        "/api/publishing/connection/access",
        json!({"publish_enabled":true}),
    )
    .await;
    assert_eq!(p.enqueue(request).await.unwrap().id, job.id);
    assert_eq!(p.secret("agent_token").await.unwrap(), original);
    let (_, connection) = admin(
        &p,
        "POST",
        "/api/publishing/connection/disconnect",
        json!({}),
    )
    .await;
    assert_eq!(connection["state"], "Disconnected");
    assert_eq!(
        call(&p, "GET", "/v1/status", vec![], true).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_ne!(p.secret("agent_token").await.unwrap(), original);
    let (_, connection) = admin(&p, "POST", "/api/publishing/connection/enable", json!({})).await;
    assert_eq!(connection["state"], "Setup incomplete");
    let old = p
        .bridge_router()
        .oneshot(
            Request::builder()
                .uri("/v1/status")
                .header("authorization", format!("Bearer {original}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(old.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(p.list().await.unwrap().len(), 1);
    assert_eq!(
        call(&p, "GET", "/v1/status", vec![], true).await.0,
        StatusCode::OK
    );
}
#[tokio::test]
async fn workspace_history_and_settings_are_real_and_management_only() {
    let (_dir, p) = fixture().await;
    account(&p).await;
    let job = p.enqueue(input(media(&p).await)).await.unwrap();
    let path = format!("/api/publications/{}", job.id);
    let (status, detail) = admin(&p, "GET", &path, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["settings"]["made_for_kids"], false);
    assert_eq!(detail["publication"]["id"], job.id);
    let (_, history) = admin(&p, "GET", &format!("{path}/history"), Value::Null).await;
    assert_eq!(history["items"].as_array().unwrap().len(), 1);
    assert!(history["items"][0]["event_at"].is_string());
    assert_eq!(history["items"][0]["publication"]["status"], "queued");
    assert!(!history.to_string().contains("refresh-secret"));
    assert!(history["items"][0]["publication"].get("session").is_none());
    assert_eq!(
        call(&p, "GET", &path, vec![], true).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn bearer_scheme_interoperability_and_rejection() {
    let (_dir, p) = fixture().await;
    let token = p.secret("agent_token").await.unwrap();
    for scheme in ["Bearer", "bearer", "BEARER", "bEaReR", "Bearer   "] {
        let response = p
            .bridge_router()
            .oneshot(
                Request::builder()
                    .uri("/v1/status")
                    .header("Authorization", format!("{scheme} {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    for headers in [
        vec![],
        vec!["Bearer wrong".to_string()],
        vec!["Bearer".to_string()],
        vec![format!("Basic {token}")],
        vec![format!("Bearer {token}, Bearer {token}")],
        vec![format!("Bearer {token}"), format!("Bearer {token}")],
        vec![format!("Bearer {}", token.to_uppercase())],
    ] {
        let mut request = Request::builder().uri("/v1/status");
        for header in headers {
            request = request.header("authorization", header);
        }
        let response = p
            .bridge_router()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers()["www-authenticate"],
            "Bearer realm=\"AgentWay\""
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(!String::from_utf8_lossy(&body).contains(&token));
    }
    assert_eq!(p.secret("agent_token").await.unwrap(), token);
}

#[tokio::test]
async fn video_correction_reconciles_failures_preserves_settings_and_guards_retirement() {
    use axum::routing::get;
    let (_dir, mut p) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let videos = Arc::new(std::sync::Mutex::new(std::collections::HashMap::<
        String,
        Value,
    >::new()));
    let writes = Arc::new(AtomicUsize::new(0));
    let lose_response = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let get_videos = videos.clone();
    let put_videos = videos.clone();
    let put_writes = writes.clone();
    let lose = lose_response.clone();
    let mock = Router::new().route("/token", post(|| async {
        Json(json!({"access_token":"access","token_type":"Bearer","expires_in":3600}))
    })).route("/videos", get(move |axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>| {
        let videos = get_videos.clone(); async move {
            assert_eq!(q["part"], "snippet,status,processingDetails,contentDetails");
            Json(json!({"items":[videos.lock().unwrap().get(&q["id"]).unwrap().clone()]}))
        }
    }).put(move |axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String,String>>, Json(body): Json<Value>| {
        let videos = put_videos.clone(); let writes = put_writes.clone(); let lose=lose.clone();
        async move {
            assert_eq!(q["part"], "status");
            assert!(body.get("snippet").is_none());
            assert_eq!(body["status"]["containsSyntheticMedia"], true);
            assert_eq!(body["status"]["selfDeclaredMadeForKids"], false);
            assert_eq!(body["status"]["embeddable"], false);
            assert_eq!(body["status"]["license"], "creativeCommon");
            assert_eq!(body["status"]["publicStatsViewable"], false);
            assert!(body["status"].get("uploadStatus").is_none());
            videos.lock().unwrap().get_mut(body["id"].as_str().unwrap()).unwrap()["status"]["privacyStatus"] = body["status"]["privacyStatus"].clone();
            writes.fetch_add(1, Ordering::SeqCst);
            if lose.swap(false, Ordering::SeqCst) { StatusCode::SERVICE_UNAVAILABLE.into_response() }
            else { Json(body).into_response() }
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    Arc::get_mut(&mut p.0).unwrap().endpoints.token = format!("{base}/token");
    Arc::get_mut(&mut p.0).unwrap().endpoints.videos = format!("{base}/videos");
    account(&p).await;
    let media_id = media(&p).await;
    let original = p.enqueue(input(media_id.clone())).await.unwrap();
    let corrected = p.enqueue(input(media_id)).await.unwrap();
    for (job, video, privacy) in [
        (&original, "original", "public"),
        (&corrected, "corrected", "private"),
    ] {
        sqlx::query("UPDATE publications SET status='uploaded',video_id=? WHERE id=?")
            .bind(video)
            .bind(&job.id)
            .execute(&p.0.db)
            .await
            .unwrap();
        videos.lock().unwrap().insert(video.into(), json!({"id":video,"snippet":{"channelId":"channel"},
            "status":{"privacyStatus":privacy,"uploadStatus":"processed","selfDeclaredMadeForKids":false,"containsSyntheticMedia":true,"embeddable":false,"license":"creativeCommon","publicStatsViewable":false},
            "processingDetails":{"processingStatus":"succeeded"},"contentDetails":{"duration":"PT5M"}}));
    }
    let path = format!("/v1/publications/{}/visibility", corrected.id);
    let request = json!({"request_id":Uuid::new_v4().to_string(),"privacy":"public"});
    assert_eq!(
        call(&p, "POST", &path, request.to_string().into_bytes(), false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&p, "POST", &path, request.to_string().into_bytes(), true)
            .await
            .0,
        StatusCode::BAD_REQUEST
    ); // private-only
    p.set("private_only", "false").await.unwrap();
    let (_, denied) = call(&p, "POST", &path, request.to_string().into_bytes(), true).await;
    assert!(
        denied["error"]
            .as_str()
            .unwrap()
            .contains("permission required")
    );
    p.set("youtube_manage_channel", "channel").await.unwrap();
    let retire = VisibilityInput {
        request_id: Uuid::new_v4().to_string(),
        privacy: "private".into(),
        replacement_id: Some(corrected.id.clone()),
    };
    let not_ready = p
        .set_video_visibility(&original.id, retire.clone())
        .await
        .unwrap();
    assert_eq!(not_ready.status, "interrupted");
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    videos.lock().unwrap().get_mut("corrected").unwrap()["processingDetails"]["processingStatus"] =
        json!("processing");
    assert_eq!(
        p.youtube_video_status(&corrected.id).await.unwrap()["ready"],
        false
    );
    let (_, processing) = call(&p, "POST", &path, request.to_string().into_bytes(), true).await;
    assert_eq!(processing["status"], "interrupted");
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    videos.lock().unwrap().get_mut("corrected").unwrap()["processingDetails"]["processingStatus"] =
        json!("succeeded");
    let (_, uncertain) = call(&p, "POST", &path, request.to_string().into_bytes(), true).await;
    assert_eq!(uncertain["status"], "interrupted");
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    let (_, recovered) = call(&p, "POST", &path, request.to_string().into_bytes(), true).await;
    assert_eq!(recovered["status"], "completed");
    assert_eq!(writes.load(Ordering::SeqCst), 1); // provider succeeded but response was lost
    let completed = p
        .set_video_visibility(&original.id, retire.clone())
        .await
        .unwrap();
    assert_eq!(completed.status, "completed");
    assert_eq!(completed.replacement_id, Some(corrected.id.clone()));
    assert_eq!(
        p.youtube_video_status(&original.id).await.unwrap()["actual_privacy"],
        "private"
    );
    let prior_writes = writes.load(Ordering::SeqCst);
    assert_eq!(
        p.set_video_visibility(&original.id, retire.clone())
            .await
            .unwrap()
            .status,
        "completed"
    );
    assert_eq!(writes.load(Ordering::SeqCst), prior_writes);
    let mut conflict = retire.clone();
    conflict.privacy = "public".into();
    conflict.replacement_id = None;
    assert!(
        p.set_video_visibility(&original.id, conflict)
            .await
            .is_err()
    );
    assert_eq!(p.video_operations(&original.id).await.unwrap().len(), 1);
    assert_eq!(
        call(
            &p,
            "GET",
            &format!("/v1/video-operations/{}", retire.request_id),
            vec![],
            true
        )
        .await
        .1["status"],
        "completed"
    );
    videos.lock().unwrap().get_mut("original").unwrap()["snippet"]["channelId"] = json!("other");
    assert!(p.youtube_video_status(&original.id).await.is_err());
    p.set("publish_enabled", "false").await.unwrap();
    assert!(
        p.set_video_visibility(
            &original.id,
            VisibilityInput {
                request_id: Uuid::new_v4().to_string(),
                privacy: "private".into(),
                replacement_id: None
            }
        )
        .await
        .is_err()
    );
    let token_before = p.secret("agent_token").await.unwrap();
    p.0.db.close().await;
    drop(p);
    let db = crate::database(&format!(
        "sqlite://{}",
        _dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    let restarted = Publisher::new(db, _dir.path().join("publishing"), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        restarted
            .video_operation(&retire.request_id)
            .await
            .unwrap()
            .status,
        "completed"
    );
    assert_eq!(restarted.secret("agent_token").await.unwrap(), token_before);
    server.abort();
}
