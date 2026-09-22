use super::*;
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post, put},
};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::io::AsyncWriteExt;

pub struct Error(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let message = if self.0.is::<sqlx::Error>() || self.0.is::<std::io::Error>() {
            "Storage operation failed".to_string()
        } else {
            self.0.to_string()
        };
        (StatusCode::BAD_REQUEST, Json(json!({"error":message}))).into_response()
    }
}
type Api<T> = std::result::Result<Json<T>, Error>;
#[derive(Deserialize)]
pub struct Config {
    client_id: String,
    client_secret: String,
}
#[derive(Deserialize)]
pub struct Policy {
    private_only: bool,
}
#[derive(Deserialize)]
pub struct BridgeUrl {
    url: String,
}
#[derive(Deserialize)]
pub struct Callback {
    state: String,
    code: Option<String>,
}
impl Publisher {
    pub fn admin_router(&self) -> Router {
        Router::new()
            .merge(super::workspace::routes())
            .route("/api/youtube", get(status))
            .route("/api/youtube/config", post(config))
            .route("/api/youtube/connect", post(connect))
            .route("/api/youtube/callback", get(callback))
            .route("/api/youtube/disconnect", post(disconnect))
            .route("/api/youtube/policy", post(policy))
            .route("/api/publishing/bridge", post(bridge_url))
            .route("/api/publishing/token", post(token))
            .route("/api/publishing/token/rotate", post(rotate))
            .route("/api/publications", get(list))
            .route(
                "/api/publishing/connection",
                get(connection).post(name_connection),
            )
            .route("/api/publications/{id}/retry", post(retry))
            .layer(DefaultBodyLimit::max(64 * 1024))
            .with_state(self.clone())
            .layer(middleware::from_fn(crate::local_only))
    }
    /// Separate listener: no UI, OAuth callbacks, settings, credentials or arbitrary filesystem reads.
    pub fn bridge_router(&self) -> Router {
        Router::new()
            .route("/v1/status", get(status))
            .route("/v1/media", post(create_media))
            .route(
                "/v1/media/{id}",
                put(upload_media)
                    .delete(remove_media)
                    .layer(DefaultBodyLimit::disable()),
            )
            .route("/v1/youtube/publish", post(publish))
            .route("/v1/publications", get(list))
            .route("/v1/publications/{id}", get(publication))
            .route("/v1/publications/{id}/youtube", get(video_status))
            .route("/v1/publications/{id}/visibility", post(set_visibility))
            .route("/v1/publications/{id}/delete", post(delete_replaced_video))
            .route("/v1/publications/{id}/operations", get(video_operations))
            .route("/v1/video-operations/{id}", get(video_operation))
            .route("/v1/publications/{id}/retry", post(retry))
            .nest_service("/mcp", self.mcp_service())
            .layer(DefaultBodyLimit::max(64 * 1024))
            .layer(middleware::from_fn_with_state(self.clone(), authenticate))
            .with_state(self.clone())
    }
}
/// HTTP authentication schemes are case-insensitive (RFC 9110 §11.1).
fn bearer_credential(headers: &HeaderMap) -> Result<&str, &'static str> {
    let mut values = headers.get_all("authorization").iter();
    let value = values.next().ok_or("authorization_missing")?;
    if values.next().is_some() {
        return Err("authorization_multiple");
    }
    let value = value.to_str().map_err(|_| "authorization_malformed")?;
    let (scheme, token) = value.split_once(' ').ok_or("authorization_malformed")?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return Err("authorization_wrong_scheme");
    }
    let token = token.trim_start_matches(' ');
    if token.is_empty() || token.bytes().any(|b| b.is_ascii_whitespace() || b == b',') {
        return Err("authorization_malformed");
    }
    Ok(token)
}
async fn authenticate(State(p): State<Publisher>, req: Request, next: Next) -> Response {
    // Browser cross-origin use is unsupported; this API is for agent HTTP clients.
    if req.headers().contains_key("origin") {
        return StatusCode::FORBIDDEN.into_response();
    }
    if p.setting("agent_disconnected")
        .await
        .ok()
        .flatten()
        .as_deref()
        == Some("true")
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let credential = bearer_credential(req.headers());
    let expected = match p.secret("agent_token").await {
        Ok(s) => s,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let failure = match credential {
        Err(reason) => Some(reason),
        Ok(actual)
            if !bool::from(
                Sha256::digest(actual.as_bytes()).ct_eq(&Sha256::digest(expected.as_bytes())),
            ) =>
        {
            Some("token_mismatch")
        }
        Ok(_) => None,
    };
    if let Some(reason) = failure {
        // Bounded diagnostics: never record header values, credentials or request URLs.
        let mut last = p.0.auth_failure_log.lock().await;
        if last.is_none_or(|at| at.elapsed() >= Duration::from_secs(5)) {
            tracing::warn!(
                reason,
                "Agent authentication rejected (at most once per 5 seconds)"
            );
            *last = Some(std::time::Instant::now());
        }
        drop(last);
        return (
            StatusCode::UNAUTHORIZED,
            [("www-authenticate", "Bearer realm=\"AgentWay\"")],
            Json(json!({"error":"AgentWay bearer token required"})),
        )
            .into_response();
    }
    // Record only known authenticated bridge operations, never tokens, query strings,
    // media bytes or client-supplied names. A shared token cannot identify an agent.
    let path = req.uri().path();
    let operation = match (req.method().as_str(), path) {
        ("GET", "/v1/status") => Some("Checked connection"),
        ("POST", "/v1/media") => Some("Reserved video upload"),
        ("PUT", p) if p.starts_with("/v1/media/") => Some("Transferred video"),
        ("DELETE", p) if p.starts_with("/v1/media/") => Some("Removed staged video"),
        ("POST", "/v1/youtube/publish") => Some("Requested YouTube upload"),
        ("GET", p) if p.starts_with("/v1/publications/") => Some("Checked upload status"),
        ("POST", p) if p.starts_with("/v1/publications/") && p.ends_with("/retry") => {
            Some("Requested upload retry")
        }
        ("POST", p) if p.starts_with("/v1/publications/") && p.ends_with("/visibility") => {
            Some("Requested visibility change")
        }
        (_, "/mcp") => Some("MCP request"),
        _ => None,
    };
    if let Some(operation) = operation
        && p.record_activity(operation).await.is_err()
    {
        tracing::warn!("Could not record authenticated bridge activity");
    }
    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}
async fn connection(State(p): State<Publisher>) -> Api<Value> {
    Ok(Json(p.workspace_connection().await?))
}
#[derive(Deserialize)]
struct ConnectionName {
    name: String,
}
async fn name_connection(
    State(p): State<Publisher>,
    Json(input): Json<ConnectionName>,
) -> Api<Value> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(anyhow::anyhow!("Connection name must contain 1–80 characters").into());
    }
    p.set("agent_connection_name", name).await?;
    p.emit("bridge.name", json!({"name":name})).await?;
    connection(State(p)).await
}
async fn status(State(p): State<Publisher>) -> Api<Value> {
    Ok(Json(p.status().await?))
}
async fn config(State(p): State<Publisher>, Json(c): Json<Config>) -> Api<Value> {
    if c.client_id.len() > 512
        || !c.client_id.ends_with(".apps.googleusercontent.com")
        || c.client_secret.is_empty()
        || c.client_secret.len() > 512
    {
        return Err(anyhow::anyhow!("Enter a Google OAuth web client ID and client secret").into());
    }
    let _guard = p.0.mutation.lock().await;
    let connected: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM youtube_account")
        .fetch_one(&p.0.db)
        .await?;
    if connected > 0 {
        return Err(anyhow::anyhow!("Disconnect YouTube before changing OAuth credentials").into());
    }
    let mut tx = p.0.db.begin().await?;
    for (key, value) in [
        ("client_id", c.client_id),
        ("client_secret", c.client_secret),
    ] {
        sqlx::query("INSERT INTO publishing_settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(key).bind(p.0.vault.seal(&value)?).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM oauth_attempts")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let status = p.status().await?;
    p.emit("youtube.status", status.clone()).await?;
    Ok(Json(status))
}
async fn policy(State(p): State<Publisher>, Json(c): Json<Policy>) -> Api<Value> {
    let _guard = p.0.mutation.lock().await;
    p.set(
        "private_only",
        if c.private_only { "true" } else { "false" },
    )
    .await?;
    let status = p.status().await?;
    p.emit("youtube.status", status.clone()).await?;
    Ok(Json(status))
}
async fn bridge_url(State(p): State<Publisher>, Json(c): Json<BridgeUrl>) -> Api<Value> {
    if !c.url.is_empty() {
        let u = reqwest::Url::parse(&c.url).map_err(|_| {
            anyhow::anyhow!("Enter an HTTPS origin, such as https://agentway.example.com")
        })?;
        if u.scheme() != "https"
            || u.host_str().is_none()
            || u.path() != "/"
            || u.query().is_some()
            || u.fragment().is_some()
            || !u.username().is_empty()
            || u.password().is_some()
        {
            return Err(anyhow::anyhow!(
                "Enter an HTTPS origin without a path, query or credentials"
            )
            .into());
        }
    }
    p.set("bridge_url", c.url.trim_end_matches('/')).await?;
    let status = p.status().await?;
    p.emit("youtube.status", status.clone()).await?;
    Ok(Json(status))
}
async fn token(State(p): State<Publisher>) -> Api<Value> {
    Ok(Json(json!({"token":p.secret("agent_token").await?})))
}
async fn rotate(State(p): State<Publisher>) -> Api<Value> {
    p.set_secret(
        "agent_token",
        &format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
    )
    .await?;
    Ok(Json(json!({"token":p.secret("agent_token").await?})))
}
async fn connect(
    State(p): State<Publisher>,
    headers: HeaderMap,
) -> std::result::Result<Response, Error> {
    let host = headers
        .get("host")
        .and_then(|s| s.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("Missing host"))?;
    let (url, state) = p
        .authorize(format!("http://{host}/api/youtube/callback"))
        .await?;
    Ok(([("set-cookie",format!("agentway_oauth={state}; HttpOnly; SameSite=Lax; Path=/api/youtube/callback; Max-Age=600"))],Json(json!({"url":url}))).into_response())
}
async fn callback(
    State(p): State<Publisher>,
    headers: HeaderMap,
    Query(q): Query<Callback>,
) -> Response {
    match p
        .oauth_callback(&q.state, q.code.as_deref(), &headers)
        .await
    {
        Ok(()) => (
            [(
                "set-cookie",
                "agentway_oauth=; HttpOnly; SameSite=Lax; Path=/api/youtube/callback; Max-Age=0",
            )],
            Redirect::to("/#/platforms/youtube"),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            format!("YouTube connection failed: {e}. Return to AgentWay and try again."),
        )
            .into_response(),
    }
}
async fn disconnect(State(p): State<Publisher>) -> Api<Value> {
    p.disconnect().await?;
    Ok(Json(p.status().await?))
}
async fn list(State(p): State<Publisher>) -> Api<Vec<Publication>> {
    Ok(Json(p.list().await?))
}
async fn retry(State(p): State<Publisher>, Path(id): Path<String>) -> Api<Publication> {
    Ok(Json(p.retry(&id).await?))
}
async fn publication(
    State(p): State<Publisher>,
    Path(id): Path<String>,
) -> Api<PublicationVerification> {
    Ok(Json(p.verified_publication(&id).await?))
}
async fn create_media(State(p): State<Publisher>, Json(input): Json<MediaInput>) -> Api<Value> {
    Ok(Json(p.create_media(input).await?))
}
async fn publish(State(p): State<Publisher>, Json(input): Json<PublishInput>) -> Api<Publication> {
    Ok(Json(p.enqueue(input).await?))
}
async fn remove_media(State(p): State<Publisher>, Path(id): Path<String>) -> Api<Value> {
    let _guard = p.0.mutation.lock().await;
    if Uuid::parse_str(&id).is_err() {
        return Err(anyhow::anyhow!("Invalid media ID").into());
    }
    let used: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM publications WHERE media_id=?")
        .bind(&id)
        .fetch_one(&p.0.db)
        .await?;
    if used > 0 {
        return Err(anyhow::anyhow!(
            "Media is referenced by an upload and cannot be removed in this version"
        )
        .into());
    }
    sqlx::query("DELETE FROM media WHERE id=?")
        .bind(&id)
        .execute(&p.0.db)
        .await?;
    let _ = tokio::fs::remove_file(p.0.dir.join("media").join(id)).await;
    Ok(Json(json!({"removed":true})))
}
async fn upload_media(
    State(p): State<Publisher>,
    Path(id): Path<String>,
    body: Body,
) -> Api<Value> {
    if Uuid::parse_str(&id).is_err() {
        return Err(anyhow::anyhow!("Invalid media ID").into());
    }
    let media: Media = sqlx::query_as("SELECT * FROM media WHERE id=?")
        .bind(&id)
        .fetch_optional(&p.0.db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Unknown media ID"))?;
    if media.ready == 1 {
        return Ok(Json(json!({"media_id":id,"ready":true})));
    }
    let _transfer_permit =
        p.0.transfers
            .try_acquire()
            .map_err(|_| anyhow::anyhow!("Two media transfers are already active. Retry later."))?;
    // Unique temporary files prevent concurrent PUTs from overwriting each other's bytes.
    let temp =
        p.0.dir
            .join("media")
            .join(format!("{}.part", Uuid::new_v4()));
    let transfer = async {
        let mut file = tokio::fs::File::create(&temp).await?;
        let mut stream = body.into_data_stream();
        let mut received = 0i64;
        while let Some(chunk) = tokio::time::timeout(Duration::from_secs(60), stream.next())
            .await
            .map_err(|_| anyhow::anyhow!("Media transfer timed out"))?
        {
            let chunk = chunk.map_err(|_| anyhow::anyhow!("Media transfer interrupted"))?;
            received += chunk.len() as i64;
            if received > media.size {
                bail!("Received more bytes than the declared media size");
            }
            file.write_all(&chunk).await?;
        }
        if received != media.size {
            bail!("Media transfer incomplete. Retry PUT with the entire file.");
        }
        file.sync_all().await?;
        drop(file);
        let _guard = p.0.mutation.lock().await;
        let ready: Option<i64> = sqlx::query_scalar("SELECT ready FROM media WHERE id=?")
            .bind(&id)
            .fetch_optional(&p.0.db)
            .await?;
        if ready == Some(0) {
            tokio::fs::rename(&temp, p.0.dir.join("media").join(&id)).await?;
            sqlx::query("UPDATE media SET ready=1 WHERE id=?")
                .bind(&id)
                .execute(&p.0.db)
                .await?;
        } else if ready.is_none() {
            bail!("Media was removed during transfer");
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    let _ = tokio::fs::remove_file(&temp).await;
    transfer?;
    Ok(Json(json!({"media_id":id,"ready":true})))
}

async fn video_status(State(p): State<Publisher>, Path(id): Path<String>) -> Api<Value> {
    Ok(Json(p.youtube_video_status(&id).await?))
}
async fn set_visibility(
    State(p): State<Publisher>,
    Path(id): Path<String>,
    Json(input): Json<VisibilityInput>,
) -> Api<VideoOperation> {
    Ok(Json(p.set_video_visibility(&id, input).await?))
}
async fn video_operations(
    State(p): State<Publisher>,
    Path(id): Path<String>,
) -> Api<Vec<VideoOperation>> {
    Ok(Json(p.video_operations(&id).await?))
}
async fn video_operation(
    State(p): State<Publisher>,
    Path(id): Path<String>,
) -> Api<VideoOperation> {
    Ok(Json(p.video_operation(&id).await?))
}

async fn delete_replaced_video(
    State(p): State<Publisher>,
    Path(id): Path<String>,
    Json(input): Json<DeleteInput>,
) -> Api<VideoOperation> {
    Ok(Json(p.delete_replaced_video(&id, input).await?))
}
