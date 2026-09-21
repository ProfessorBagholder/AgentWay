use agentway_server::{database, router};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn call(app: axum::Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "127.0.0.1:8787")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[tokio::test]
async fn task_idempotency_cancel_and_restart_are_durable() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}", dir.path().join("test.db").display());
    let db = database(&url).await.unwrap();
    let app = router(db.clone(), "missing-assets");
    let (_, agent) = call(
        app.clone(),
        "POST",
        "/api/agents",
        json!({"name":"Coordinator","platform":"grok_bot","role":"manager"}),
    )
    .await;
    assert_eq!(agent["status"], "unconfigured");
    let input = json!({"title":"Captions","instructions":"Keep it brief","agent_id":agent["id"],"request_id":uuid::Uuid::new_v4().to_string()});
    let (a, b) = tokio::join!(
        call(app.clone(), "POST", "/api/tasks", input.clone()),
        call(app.clone(), "POST", "/api/tasks", input.clone())
    );
    assert_eq!(a.0, StatusCode::CREATED);
    assert_eq!(a.1["id"], b.1["id"]);
    let mut different = input;
    different["title"] = json!("Different");
    assert_eq!(
        call(app.clone(), "POST", "/api/tasks", different).await.0,
        StatusCode::CONFLICT
    );
    let path = format!("/api/tasks/{}/cancel", a.1["id"].as_str().unwrap());
    let (_, cancelled) = call(app.clone(), "POST", &path, json!({})).await;
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(cancelled["revision"], 2);
    call(app.clone(), "POST", &path, json!({})).await;
    drop(app);
    db.close().await;
    let reopened = database(&url).await.unwrap();
    let (_, state) = call(
        router(reopened.clone(), "missing-assets"),
        "GET",
        "/api/bootstrap",
        json!(null),
    )
    .await;
    assert_eq!(state["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(state["tasks"][0]["status"], "cancelled");
    assert_eq!(state["cursor"], 3); // one agent, one task, one cancellation
    reopened.close().await;
}
#[tokio::test]
async fn reject_invalid_platforms_and_cross_origin_writes() {
    let dir = tempfile::tempdir().unwrap();
    let db = database(&format!(
        "sqlite://{}",
        dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    let app = router(db.clone(), "missing-assets");
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/agents",
            json!({"name":"Test","platform":"codex","role":"manager"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    for (host, origin) in [
        ("evil.example", "http://evil.example"),
        ("127.0.0.1:8787", "https://evil.example"),
    ] {
        let result = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/agents")
                    .header("host", host)
                    .header("origin", origin)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::FORBIDDEN);
    }
    db.close().await;
}

#[tokio::test]
async fn events_replay_after_cursor_and_close_on_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let db = database(&format!(
        "sqlite://{}",
        dir.path().join("events.db").display()
    ))
    .await
    .unwrap();
    let shutdown = tokio_util::sync::CancellationToken::new();
    let app = agentway_server::router_with_shutdown(db.clone(), "missing-assets", shutdown.clone());
    for name in ["First", "Second"] {
        call(
            app.clone(),
            "POST",
            "/api/agents",
            json!({"name":name,"platform":"muse","role":"worker"}),
        )
        .await;
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/events?after=0")
                .header("host", "127.0.0.1:8787")
                .header("last-event-id", "1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut body = response.into_body();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    assert!(text.contains("id: 2"));
    assert!(text.contains("Second"));
    assert!(!text.contains("First"));
    shutdown.cancel();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), body.frame())
            .await
            .unwrap()
            .is_none()
    );
    db.close().await;
}

#[tokio::test]
async fn oauth_return_allows_document_navigation_but_not_cross_site_api_access() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "AgentWay app").unwrap();
    let db = database(&format!(
        "sqlite://{}",
        dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    let app = router(db.clone(), dir.path().to_str().unwrap());
    for (method, path, mode, dest, expected) in [
        ("GET", "/", "navigate", "document", StatusCode::OK),
        (
            "POST",
            "/api/agents",
            "navigate",
            "document",
            StatusCode::FORBIDDEN,
        ),
        (
            "GET",
            "/api/bootstrap",
            "navigate",
            "document",
            StatusCode::FORBIDDEN,
        ),
        ("GET", "/", "cors", "empty", StatusCode::FORBIDDEN),
        ("GET", "/", "navigate", "iframe", StatusCode::FORBIDDEN),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("host", "127.0.0.1:8787")
                    .header("sec-fetch-site", "cross-site")
                    .header("sec-fetch-mode", mode)
                    .header("sec-fetch-dest", dest)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{method} {path} {mode} {dest}");
    }
    db.close().await;
}
