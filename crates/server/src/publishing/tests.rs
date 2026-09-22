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
        ..Endpoints::default()
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
        ..Endpoints::default()
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
        "list_youtube_playlists",
        "manage_youtube_podcast",
        "get_youtube_podcast_operation",
        "get_youtube_channel",
        "set_youtube_channel_description",
        "delete_replaced_youtube_video",
        "list_publications",
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

#[tokio::test]
async fn deletion_requires_authorization_and_replacement_and_recovers_lost_response() {
    use axum::routing::get;
    use std::sync::atomic::AtomicBool;
    let (_dir, mut p) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let present = Arc::new(AtomicBool::new(true));
    let ready = Arc::new(AtomicBool::new(false));
    let delete_count = Arc::new(AtomicUsize::new(0));
    let get_present = present.clone();
    let get_ready = ready.clone();
    let del_present = present.clone();
    let del_count = delete_count.clone();
    let mock=Router::new().route("/token",post(||async {Json(json!({"access_token":"access","token_type":"Bearer","expires_in":3600}))}))
        .route("/videos",get(move |axum::extract::Query(q):axum::extract::Query<std::collections::HashMap<String,String>>| {
            let present=get_present.clone();let ready=get_ready.clone();async move {
                let video=&q["id"];
                if video.starts_with("old") && !present.load(Ordering::SeqCst) { return Json(json!({"items":[]})); }
                Json(json!({"items":[{"id":video,"snippet":{"channelId":"channel"},"status":{"privacyStatus":if video.starts_with("old") {"private"} else {"public"},"uploadStatus":"processed"},"processingDetails":{"processingStatus":if ready.load(Ordering::SeqCst){"succeeded"}else{"processing"}}}]}))
            }
        }).delete(move |axum::extract::Query(q):axum::extract::Query<std::collections::HashMap<String,String>>| {
            let present=del_present.clone();let count=del_count.clone();async move {
                assert!(q["id"].starts_with("old"));
                present.store(false,Ordering::SeqCst);
                let n=count.fetch_add(1,Ordering::SeqCst);
                if n==0 {StatusCode::SERVICE_UNAVAILABLE} else {StatusCode::NO_CONTENT}
            }
        }));
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    Arc::get_mut(&mut p.0).unwrap().endpoints.token = format!("{base}/token");
    Arc::get_mut(&mut p.0).unwrap().endpoints.videos = format!("{base}/videos");
    account(&p).await;
    let m = media(&p).await;
    let old = p.enqueue(input(m.clone())).await.unwrap();
    let new = p.enqueue(input(m)).await.unwrap();
    for (id, video) in [(&old.id, "old"), (&new.id, "new")] {
        sqlx::query("UPDATE publications SET status='uploaded',video_id=? WHERE id=?")
            .bind(video)
            .bind(id)
            .execute(&p.0.db)
            .await
            .unwrap();
    }
    let mut request = DeleteInput {
        request_id: Uuid::new_v4().to_string(),
        replacement_id: new.id.clone(),
        confirm_delete: false,
    };
    assert!(
        p.delete_replaced_video(&old.id, request.clone())
            .await
            .is_err()
    );
    request.confirm_delete = true;
    assert!(
        p.delete_replaced_video(&old.id, request.clone())
            .await
            .is_err()
    ); // no management consent
    p.set("youtube_manage_channel", "channel").await.unwrap();
    let path = format!("/v1/publications/{}/delete", old.id);
    assert_eq!(
        call(
            &p,
            "POST",
            &path,
            serde_json::to_vec(&request).unwrap(),
            false
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let mut same = request.clone();
    same.replacement_id = old.id.clone();
    assert!(p.delete_replaced_video(&old.id, same).await.is_err());
    p.set("publish_enabled", "false").await.unwrap();
    assert!(
        p.delete_replaced_video(&old.id, request.clone())
            .await
            .is_err()
    );
    p.set("publish_enabled", "true").await.unwrap();
    let blocked = p
        .delete_replaced_video(&old.id, request.clone())
        .await
        .unwrap();
    assert_eq!(blocked.status, "interrupted");
    assert!(!blocked.provider_attempted);
    assert_eq!(delete_count.load(Ordering::SeqCst), 0);
    ready.store(true, Ordering::SeqCst);
    present.store(false, Ordering::SeqCst);
    assert_eq!(
        p.delete_replaced_video(&old.id, request.clone())
            .await
            .unwrap()
            .status,
        "interrupted"
    ); // missing before attempt is not success
    assert_eq!(delete_count.load(Ordering::SeqCst), 0);
    present.store(true, Ordering::SeqCst);
    let uncertain = p
        .delete_replaced_video(&old.id, request.clone())
        .await
        .unwrap();
    assert_eq!(uncertain.status, "interrupted");
    assert!(uncertain.provider_attempted);
    assert!(p.publication(&old.id).await.unwrap().deleted_at.is_none());
    let (_, recovered) = call(
        &p,
        "POST",
        &path,
        serde_json::to_vec(&request).unwrap(),
        true,
    )
    .await;
    assert_eq!(recovered["status"], "completed");
    assert_eq!(recovered["action"], "delete");
    assert_eq!(delete_count.load(Ordering::SeqCst), 1);
    assert!(p.publication(&old.id).await.unwrap().deleted_at.is_some());
    assert_eq!(
        p.youtube_video_status(&old.id).await.unwrap()["deleted"],
        true
    );
    assert!(p.publication(&new.id).await.unwrap().deleted_at.is_none());
    assert_eq!(
        p.delete_replaced_video(&old.id, request.clone())
            .await
            .unwrap()
            .status,
        "completed"
    );
    let mut collision = request.clone();
    collision.replacement_id = old.id.clone();
    assert!(p.delete_replaced_video(&new.id, collision).await.is_err());
    // A separate cleanup exercises YouTube's normal 204 success path.
    let fresh_media = media(&p).await;
    let fresh = p.enqueue(input(fresh_media)).await.unwrap();
    sqlx::query("UPDATE publications SET status='uploaded',video_id='old2' WHERE id=?")
        .bind(&fresh.id)
        .execute(&p.0.db)
        .await
        .unwrap();
    present.store(true, Ordering::SeqCst);
    let direct = p
        .delete_replaced_video(
            &fresh.id,
            DeleteInput {
                request_id: Uuid::new_v4().to_string(),
                replacement_id: new.id.clone(),
                confirm_delete: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(direct.status, "completed");
    assert!(direct.result.unwrap().contains("youtube_204"));
    assert_eq!(delete_count.load(Ordering::SeqCst), 2);
    let saved_token = p.secret("agent_token").await.unwrap();
    p.0.db.close().await;
    drop(p);
    let db = crate::database(&format!(
        "sqlite://{}",
        _dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    let p = Publisher::new(db, _dir.path().join("publishing"), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        p.delete_replaced_video(&old.id, request)
            .await
            .unwrap()
            .status,
        "completed"
    );
    assert_eq!(p.secret("agent_token").await.unwrap(), saved_token);
    server.abort();
}

#[tokio::test]
async fn channel_description_preserves_settings_and_reconciles_uncertain_writes() {
    use axum::routing::get;
    let (_dir, mut p) = fixture().await;
    let saved = Arc::new(Mutex::new(
        json!({"id":"channel","snippet":{"title":"Test channel"},"brandingSettings":{"channel":{"title":"Test channel","description":"Old blurb","country":"CA","keywords":"podcast comedy","defaultLanguage":"en","unsubscribedTrailer":"trailer"},"image":{"bannerImageUrl":"deprecated"}}}),
    ));
    let writes = Arc::new(AtomicUsize::new(0));
    let read = saved.clone();
    let write = saved.clone();
    let count = writes.clone();
    let app = Router::new()
        .route(
            "/token",
            post(|| async {
                Json(json!({"access_token":"access","token_type":"Bearer","expires_in":3600}))
            }),
        )
        .route(
            "/channels",
            get(move || {
                let read = read.clone();
                async move { Json(json!({"items":[read.lock().await.clone()]})) }
            })
            .put(move |Json(body): Json<Value>| {
                let write = write.clone();
                let count = count.clone();
                async move {
                    assert_eq!(body["id"], "channel");
                    assert_eq!(body["brandingSettings"]["channel"]["country"], "CA");
                    assert_eq!(
                        body["brandingSettings"]["channel"]["keywords"],
                        "podcast comedy"
                    );
                    assert_eq!(
                        body["brandingSettings"]["channel"]["unsubscribedTrailer"],
                        "trailer"
                    );
                    assert!(body["brandingSettings"].get("image").is_none());
                    let n = count.fetch_add(1, Ordering::SeqCst);
                    // First write applies but the response fails. Third write lies about success.
                    if n == 1 {
                        // Realistic propagation: first read is stale, later reads converge.
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            write.lock().await["brandingSettings"] =
                                body["brandingSettings"].clone();
                        });
                    } else if n != 2 {
                        write.lock().await["brandingSettings"] = body["brandingSettings"].clone();
                    }
                    if n == 0 {
                        StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        StatusCode::OK
                    }
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Arc::get_mut(&mut p.0).unwrap().endpoints.token = format!("{base}/token");
    Arc::get_mut(&mut p.0).unwrap().endpoints.channels = format!("{base}/channels");
    account(&p).await;
    assert_eq!(
        call(&p, "GET", "/v1/youtube/channel", vec![], false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let read = call(&p, "GET", "/v1/youtube/channel", vec![], true).await;
    assert_eq!(read.0, StatusCode::OK);
    assert_eq!(read.1["description"], "Old blurb");
    let mut input = ChannelDescriptionInput {
        channel_id: "channel".into(),
        expected_description: "Old blurb".into(),
        description: "New blurb 🦍".into(),
    };
    assert!(p.set_channel_description(input.clone()).await.is_err()); // consent required
    p.set("youtube_manage_channel", "channel").await.unwrap();
    input.channel_id = "other".into();
    assert!(p.set_channel_description(input.clone()).await.is_err());
    input.channel_id = "channel".into();
    input.expected_description = "stale".into();
    assert!(p.set_channel_description(input.clone()).await.is_err());
    input.expected_description = "Old blurb".into();
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert!(p.set_channel_description(input.clone()).await.is_err()); // uncertain response
    let retried = call(
        &p,
        "POST",
        "/v1/youtube/channel/description",
        serde_json::to_vec(&input).unwrap(),
        true,
    )
    .await;
    assert_eq!(retried.0, StatusCode::OK);
    assert_eq!(retried.1["verified"], true);
    assert_eq!(writes.load(Ordering::SeqCst), 1); // retry reconciles without another write
    input.expected_description = input.description.clone();
    input.description = String::new(); // explicit clearing allowed
    assert_eq!(
        p.set_channel_description(input.clone()).await.unwrap()["channel"]["description"],
        ""
    );
    input.expected_description = String::new();
    input.description = "not saved".into();
    let pending = call(
        &p,
        "POST",
        "/v1/youtube/channel/description",
        serde_json::to_vec(&input).unwrap(),
        true,
    )
    .await;
    assert_eq!(pending.0, StatusCode::ACCEPTED);
    assert_eq!(pending.1["status"], "verification_pending");
    assert_eq!(pending.1["verified"], false);
    assert_eq!(pending.1["write_accepted"], true);
    assert!(
        p.setting("channel_description_pending")
            .await
            .unwrap()
            .is_some()
    );
    let db = p.0.db.clone();
    let dir = p.0.dir.clone();
    drop(p);
    let mut p = Publisher::new(db, dir, CancellationToken::new())
        .await
        .unwrap();
    Arc::get_mut(&mut p.0).unwrap().endpoints.token = format!("{base}/token");
    Arc::get_mut(&mut p.0).unwrap().endpoints.channels = format!("{base}/channels");
    assert_eq!(
        p.set_channel_description(input.clone()).await.unwrap()["status"],
        "verification_pending"
    );
    assert_eq!(writes.load(Ordering::SeqCst), 3); // restart + stale read never repeats the PUT
    // A later read observes the accepted value; identical retry verifies without a PUT.
    saved.lock().await["brandingSettings"]["channel"]["description"] = json!(input.description);
    assert_eq!(
        p.set_channel_description(input.clone()).await.unwrap()["verified"],
        true
    );
    assert_eq!(writes.load(Ordering::SeqCst), 3);
    input.description = "🦍".repeat(1001);
    assert!(p.set_channel_description(input.clone()).await.is_err());
    assert_eq!(writes.load(Ordering::SeqCst), 3);
    p.set("publish_enabled", "false").await.unwrap();
    input.description = "blocked".into();
    assert!(p.set_channel_description(input.clone()).await.is_err());
    p.set("publish_enabled", "true").await.unwrap();
    saved.lock().await["id"] = json!("another-channel");
    assert!(p.set_channel_description(input).await.is_err());
    assert_eq!(writes.load(Ordering::SeqCst), 3);
    server.abort();
}

#[tokio::test]
async fn publication_keeps_submitting_connection_name() {
    let (_dir, p) = fixture().await;
    account(&p).await;
    p.set("agent_connection_name", "Muse").await.unwrap();
    let m = media(&p).await;
    let publication = p.enqueue(input(m)).await.unwrap();
    assert_eq!(publication.agent_name.as_deref(), Some("Muse"));
    p.set("agent_connection_name", "Claude").await.unwrap();
    assert_eq!(
        p.publication(&publication.id)
            .await
            .unwrap()
            .agent_name
            .as_deref(),
        Some("Muse")
    );
}

#[tokio::test]
async fn podcasts_preserve_settings_and_never_repeat_ambiguous_inserts() {
    use axum::{extract::Query, routing::get};
    use std::collections::HashMap;
    let (dir, mut p) = fixture().await;
    let writes = Arc::new(AtomicUsize::new(0));
    let counter = writes.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mock = Router::new()
        .route("/token", post(|| async { Json(json!({"access_token":"access","token_type":"Bearer","expires_in":3600})) }))
        .route("/playlists", get(|Query(q): Query<HashMap<String,String>>| async move {
            let id = q.get("id").map(String::as_str).unwrap_or("show");
            Json(json!({"items":[{"id":id,"snippet":{"channelId":if id=="foreign" {"other"} else {"channel"},"title":"Existing show"},"status":{"privacyStatus":"unlisted","podcastStatus":if id=="enabled" {"enabled"} else {"unspecified"}}}],"nextPageToken":"next-page"}))
        }).post(move || { let c=counter.clone(); async move {c.fetch_add(1,Ordering::SeqCst); StatusCode::BAD_GATEWAY} })
          .put(|Json(body):Json<Value>| async move {
              assert!(body.get("snippet").is_none());
              assert_eq!(body["status"]["privacyStatus"],"unlisted");
              assert_eq!(body["status"]["podcastStatus"],"enabled");
              Json(body)
          }))
        .route("/videos",get(|| async {Json(json!({"items":[{"id":"video","snippet":{"channelId":"channel"}}]}))}))
        .route("/items",get(|| async {Json(json!({"items":[{"id":"membership","snippet":{"resourceId":{"videoId":"video"}}}]}))}));
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    fn endpoints(base: &str) -> Endpoints {
        Endpoints {
            token: format!("{base}/token"),
            playlists: format!("{base}/playlists"),
            playlist_items: format!("{base}/items"),
            videos: format!("{base}/videos"),
            ..Endpoints::default()
        }
    }
    Arc::get_mut(&mut p.0).unwrap().endpoints = endpoints(&base);
    account(&p).await;
    p.set("youtube_manage_channel", "channel").await.unwrap();
    let create = json!({"request_id":Uuid::new_v4().to_string(),"channel_id":"channel","action":"create_playlist","title":"Show","description":"Episodes","privacy":"private"});
    // Exercise flattened request parsing and HTTP 202 for uncertain writes.
    let (status, result) = call(
        &p,
        "POST",
        "/v1/youtube/podcasts",
        serde_json::to_vec(&create).unwrap(),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(result["status"], "outcome_unknown");
    assert_eq!(writes.load(Ordering::SeqCst), 1);
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
    Arc::get_mut(&mut p.0).unwrap().endpoints = endpoints(&base);
    let (_, replayed) = call(
        &p,
        "POST",
        "/v1/youtube/podcasts",
        serde_json::to_vec(&create).unwrap(),
        true,
    )
    .await;
    assert_eq!(replayed, result);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    let mut conflict = create.clone();
    conflict["title"] = json!("Different");
    assert_eq!(
        call(
            &p,
            "POST",
            "/v1/youtube/podcasts",
            serde_json::to_vec(&conflict).unwrap(),
            true
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let input = |action: &str, playlist: &str| json!({"request_id":Uuid::new_v4().to_string(),"channel_id":"channel","action":action,"playlist_id":playlist});
    let enabled = p
        .manage_podcast(serde_json::from_value(input("enable_podcast", "show")).unwrap())
        .await
        .unwrap();
    assert_eq!(enabled["status"], "completed");
    assert!(
        p.manage_podcast(serde_json::from_value(input("enable_podcast", "foreign")).unwrap())
            .await
            .is_err()
    );
    let mut add = input("add_episode", "show");
    add["video_id"] = json!("video");
    add["full_episode"] = json!(true);
    let added = p
        .manage_podcast(serde_json::from_value(add.clone()).unwrap())
        .await
        .unwrap();
    assert_eq!(added["already_present"], true);
    add["request_id"] = json!(Uuid::new_v4().to_string());
    add["full_episode"] = json!(false);
    assert!(
        p.manage_podcast(serde_json::from_value(add).unwrap())
            .await
            .is_err()
    );
    let listed = p
        .youtube_playlists(podcast::PlaylistQuery {
            playlist_id: None,
            page_token: Some("page".into()),
        })
        .await
        .unwrap();
    assert_eq!(listed["nextPageToken"], "next-page");
    assert_eq!(
        call(&p, "GET", "/v1/youtube/playlists", vec![], false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    server.abort();
}

#[tokio::test]
async fn podcast_cover_is_validated_and_cannot_be_published_as_video() {
    use axum::routing::get;
    let (dir, mut p) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let received = Arc::new(AtomicUsize::new(0));
    let uploaded = received.clone();
    let sessions = Arc::new(AtomicUsize::new(0));
    let initiated = sessions.clone();
    let checked = sessions.clone();
    let session_url = format!("{base}/upload?upload_id=secret-session");
    let mock = Router::new()
        .route(
            "/token",
            post(|| async {
                Json(json!({"access_token":"access","token_type":"Bearer","expires_in":3600}))
            }),
        )
        .route(
            "/playlists",
            get(|| async {
                Json(json!({"items":[{"id":"show","snippet":{"channelId":"channel"}}]}))
            }),
        )
        .route(
            "/images",
            get(|| async { Json(json!({"kind":"youtube#playlistImageListResponse"})) }),
        )
        .route(
            "/upload",
            post(
                move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                    let url = session_url.clone();
                    initiated.fetch_add(1, Ordering::SeqCst);
                    async move {
                        assert_eq!(body["snippet"]["type"], "hero");
                        assert!(body["snippet"].get("width").is_none());
                        assert!(body["snippet"].get("height").is_none());
                        assert_eq!(body["snippet"]["playlistId"], "show");
                        assert_eq!(headers["x-upload-content-type"], "image/png");
                        (StatusCode::OK, [("location", url)])
                    }
                },
            )
            .put(
                move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                    let received = uploaded.clone();
                    let sessions = checked.clone();
                    async move {
                        if headers["content-range"]
                            .to_str()
                            .unwrap()
                            .starts_with("bytes */")
                        {
                            if sessions.load(Ordering::SeqCst) == 1 {
                                return StatusCode::GONE.into_response();
                            }
                            if received.load(Ordering::SeqCst) == 0 {
                                return StatusCode::PERMANENT_REDIRECT.into_response();
                            }
                            return Json(
                                json!({"id":"cover","snippet":{"playlistId":"show","type":"hero"}}),
                            )
                            .into_response();
                        }
                        assert!(!body.is_empty());
                        received.fetch_add(1, Ordering::SeqCst);
                        // Simulate an applied upload whose completion response was lost.
                        StatusCode::BAD_GATEWAY.into_response()
                    }
                },
            ),
        );
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    Arc::get_mut(&mut p.0).unwrap().endpoints = Endpoints {
        token: format!("{base}/token"),
        playlists: format!("{base}/playlists"),
        playlist_images: format!("{base}/images"),
        playlist_images_upload: format!("{base}/upload"),
        ..Endpoints::default()
    };
    account(&p).await;
    p.set("youtube_manage_channel", "channel").await.unwrap();
    assert!(
        p.create_media(MediaInput {
            size: 2 * 1024 * 1024 + 1,
            mime: "image/png".into()
        })
        .await
        .is_err()
    );
    for (width, height) in [(2, 1), (2, 2)] {
        let mut cursor = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(width, height)
            .write_to(&mut cursor, image::ImageFormat::Png)
            .unwrap();
        let bytes = cursor.into_inner();
        let reserved = p
            .create_media(MediaInput {
                size: bytes.len() as i64,
                mime: "image/png".into(),
            })
            .await
            .unwrap();
        let media_id = reserved["media_id"].as_str().unwrap();
        assert_eq!(
            call(
                &p,
                "PUT",
                reserved["upload_path"].as_str().unwrap(),
                bytes,
                true
            )
            .await
            .0,
            StatusCode::OK
        );
        assert!(p.enqueue(input(media_id.into())).await.is_err());
        let request: podcast::PodcastInput=serde_json::from_value(json!({"request_id":Uuid::new_v4().to_string(),"channel_id":"channel","action":"set_cover","playlist_id":"show","media_id":media_id})).unwrap();
        if width == height {
            // Upgrade a legacy multipart operation that has no saved upload session.
            sqlx::query("INSERT INTO podcast_operations(request_id,channel_id,input,result) VALUES(?,?,?,?)")
                .bind(&request.request_id).bind("channel").bind(serde_json::to_string(&request).unwrap())
                .bind(r#"{"status":"outcome_unknown"}"#).execute(&p.0.db).await.unwrap();
        }
        let result = p.manage_podcast(request.clone()).await;
        if width == height {
            let expired = result.unwrap();
            assert_eq!(expired["status"], "upload_pending");
            assert_eq!(expired["http_status"], 410);
            assert_eq!(expired["error"], "upload_session_expired");
            assert_eq!(received.load(Ordering::SeqCst), 0);
            let pending = p.manage_podcast(request.clone()).await.unwrap();
            assert_eq!(pending["status"], "upload_pending");
            assert_eq!(pending["http_status"], 502);
            assert!(!pending.to_string().contains("secret-session"));
            let endpoints = p.0.endpoints.clone();
            p.0.db.close().await;
            drop(p);
            let db = crate::database(&format!(
                "sqlite://{}",
                dir.path().join("test.db").display()
            ))
            .await
            .unwrap();
            p = Publisher::new(db, dir.path().join("publishing"), CancellationToken::new())
                .await
                .unwrap();
            Arc::get_mut(&mut p.0).unwrap().endpoints = endpoints;
            assert_eq!(
                p.manage_podcast(request).await.unwrap()["status"],
                "completed"
            );
            assert_eq!(received.load(Ordering::SeqCst), 1);
            assert_eq!(sessions.load(Ordering::SeqCst), 2);
        } else {
            assert!(result.unwrap_err().to_string().contains("square"));
        }
    }
    server.abort();
}

#[tokio::test]
async fn full_episode_can_be_inserted_into_an_ordinary_empty_playlist() {
    use axum::routing::get;
    let (_dir, mut p) = fixture().await;
    let writes = Arc::new(AtomicUsize::new(0));
    let counter = writes.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mock = Router::new()
        .route("/token", post(|| async { Json(json!({"access_token":"access","token_type":"Bearer","expires_in":3600})) }))
        .route("/playlists", get(|| async { Json(json!({"items":[{"id":"show","snippet":{"channelId":"channel"},"status":{"podcastStatus":"unspecified"}}]})) }))
        .route("/videos", get(|| async { Json(json!({"items":[{"id":"episode","snippet":{"channelId":"channel"}}]})) }))
        .route("/items", get(|| async { Json(json!({"kind":"youtube#playlistItemListResponse"})) })
            .post(move |Json(body): Json<Value>| { let counter = counter.clone(); async move {
                assert_eq!(body["snippet"]["playlistId"], "show");
                assert_eq!(body["snippet"]["resourceId"]["videoId"], "episode");
                counter.fetch_add(1, Ordering::SeqCst);
                Json(json!({"id":"membership","snippet":body["snippet"]}))
            }}));
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    Arc::get_mut(&mut p.0).unwrap().endpoints = Endpoints {
        token: format!("{base}/token"),
        playlists: format!("{base}/playlists"),
        videos: format!("{base}/videos"),
        playlist_items: format!("{base}/items"),
        ..Endpoints::default()
    };
    account(&p).await;
    p.set("youtube_manage_channel", "channel").await.unwrap();
    let input: podcast::PodcastInput = serde_json::from_value(json!({"request_id":Uuid::new_v4().to_string(),"channel_id":"channel","action":"add_episode","playlist_id":"show","video_id":"episode","full_episode":true})).unwrap();
    let result = p.manage_podcast(input.clone()).await.unwrap();
    assert_eq!(result["status"], "completed");
    assert_eq!(p.manage_podcast(input).await.unwrap(), result);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    server.abort();
}
