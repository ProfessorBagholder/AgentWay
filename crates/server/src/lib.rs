pub mod publishing;
use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::{
    FromRow, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{convert::Infallible, str::FromStr, time::Duration};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    shutdown: tokio_util::sync::CancellationToken,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub role: String,
    pub status: String,
}
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub instructions: String,
    pub agent_id: String,
    pub status: String,
    pub created_at: String,
    pub revision: i64,
}
#[derive(Serialize)]
struct Snapshot {
    agents: Vec<Agent>,
    tasks: Vec<Task>,
    cursor: i64,
}
#[derive(Deserialize)]
struct NewAgent {
    name: String,
    platform: String,
    role: String,
}
#[derive(Deserialize)]
struct NewTask {
    title: String,
    instructions: String,
    agent_id: String,
    request_id: String,
}

pub struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error":self.1}))).into_response()
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "database operation failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Storage operation failed".into(),
        )
    }
}
fn invalid(message: &str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, message.into())
}
fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "Resource not found".into())
}

pub async fn database(url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;
    Ok(pool)
}

pub fn router(db: SqlitePool, assets: &str) -> Router {
    router_with_shutdown(db, assets, tokio_util::sync::CancellationToken::new())
}
pub fn router_with_shutdown(
    db: SqlitePool,
    assets: &str,
    shutdown: tokio_util::sync::CancellationToken,
) -> Router {
    Router::new()
        .route("/health/ready", get(ready))
        .route("/api/bootstrap", get(snapshot))
        .route("/api/agents", post(create_agent))
        .route("/api/tasks", post(create_task))
        .route("/api/tasks/{id}/cancel", post(cancel_task))
        .route("/api/events", get(events))
        .fallback_service(
            ServeDir::new(assets).not_found_service(ServeFile::new(format!("{assets}/index.html"))),
        )
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn(local_only))
        .layer(TraceLayer::new_for_http())
        .with_state(AppState { db, shutdown })
}

// This milestone is loopback-only. Remote/LAN access stays disabled until scoped auth exists.
async fn local_only(request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let hostname = host.split(':').next().unwrap_or("");
    if !matches!(hostname, "127.0.0.1" | "localhost") {
        return (StatusCode::FORBIDDEN, "Local access only").into_response();
    }
    if headers.get("sec-fetch-site").and_then(|h| h.to_str().ok()) == Some("cross-site")
        && !(request.method() == axum::http::Method::GET
            && request.uri().path() == "/api/youtube/callback")
    {
        return (StatusCode::FORBIDDEN, "Cross-site access denied").into_response();
    }
    if let Some(origin) = headers.get("origin") {
        let expected = format!("http://{host}");
        if origin.to_str().ok() != Some(expected.as_str()) {
            return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
        }
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response
        .headers_mut()
        .insert("referrer-policy", "no-referrer".parse().unwrap());
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response.headers_mut().insert("content-security-policy", "default-src 'self'; connect-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'".parse().unwrap());
    response
}
async fn ready(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    sqlx::query("SELECT 1").execute(&state.db).await?;
    Ok(Json(
        serde_json::json!({"status":"ready","service":"agentway","version":env!("CARGO_PKG_VERSION")}),
    ))
}
async fn snapshot(State(state): State<AppState>) -> Result<Json<Snapshot>, ApiError> {
    let mut tx = state.db.begin().await?;
    // Establish a consistent SQLite read snapshot before reading resource state.
    let cursor = sqlx::query_scalar("SELECT COALESCE(MAX(sequence),0) FROM events")
        .fetch_one(&mut *tx)
        .await?;
    let agents = sqlx::query_as("SELECT * FROM agents ORDER BY name, id")
        .fetch_all(&mut *tx)
        .await?;
    let tasks = sqlx::query_as("SELECT * FROM tasks ORDER BY created_at DESC, id DESC")
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(Snapshot {
        agents,
        tasks,
        cursor,
    }))
}
async fn create_agent(
    State(state): State<AppState>,
    Json(input): Json<NewAgent>,
) -> Result<(StatusCode, Json<Agent>), ApiError> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(invalid("Name must contain 1–80 characters"));
    }
    if !["grok_bot", "muse", "chatgpt", "claude"].contains(&input.platform.as_str()) {
        return Err(invalid("Unknown platform"));
    }
    if !["manager", "worker"].contains(&input.role.as_str()) {
        return Err(invalid("Unknown role"));
    }
    let mut tx = state.db.begin().await?;
    let agent: Agent =
        sqlx::query_as("INSERT INTO agents(id,name,platform,role) VALUES(?,?,?,?) RETURNING *")
            .bind(Uuid::new_v4().to_string())
            .bind(name)
            .bind(input.platform)
            .bind(input.role)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("INSERT INTO events(kind,payload) VALUES('agent.upsert',?)")
        .bind(serde_json::to_string(&agent).expect("serializable agent"))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(agent)))
}
async fn create_task(
    State(state): State<AppState>,
    Json(input): Json<NewTask>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    if input.title.trim().is_empty() || input.title.chars().count() > 160 {
        return Err(invalid("Title must contain 1–160 characters"));
    }
    if input.instructions.len() > 32_000 {
        return Err(invalid("Instructions are too long"));
    }
    if Uuid::parse_str(&input.request_id).is_err() {
        return Err(invalid("A UUID request_id is required"));
    }
    let mut tx = state.db.begin().await?;
    // Write first to serialize admission and make concurrent idempotent retries safe.
    sqlx::query("INSERT INTO tasks(id,title,instructions,agent_id,request_id) SELECT ?,?,?,?,? WHERE EXISTS(SELECT 1 FROM agents WHERE id=?) ON CONFLICT(request_id) DO NOTHING")
        .bind(Uuid::new_v4().to_string()).bind(input.title.trim()).bind(&input.instructions).bind(&input.agent_id).bind(&input.request_id).bind(&input.agent_id)
        .execute(&mut *tx).await?;
    let task: Task = sqlx::query_as("SELECT * FROM tasks WHERE request_id=?")
        .bind(&input.request_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(not_found)?;
    if task.title != input.title.trim()
        || task.instructions != input.instructions
        || task.agent_id != input.agent_id
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Request ID was used with different contents".into(),
        ));
    }
    // No duplicate event for a retried request.
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events WHERE kind='task.upsert' AND json_extract(payload,'$.id')=?",
    )
    .bind(&task.id)
    .fetch_one(&mut *tx)
    .await?;
    if exists == 0 {
        sqlx::query("INSERT INTO events(kind,payload) VALUES('task.upsert',?)")
            .bind(serde_json::to_string(&task).expect("serializable task"))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(task)))
}
async fn cancel_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Task>, ApiError> {
    let mut tx = state.db.begin().await?;
    let changed = sqlx::query(
        "UPDATE tasks SET status='cancelled',revision=revision+1 WHERE id=? AND status='queued'",
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let task: Task = sqlx::query_as("SELECT * FROM tasks WHERE id=?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(not_found)?;
    if changed > 0 {
        sqlx::query("INSERT INTO events(kind,payload) VALUES('task.upsert',?)")
            .bind(serde_json::to_string(&task).expect("serializable task"))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(task))
}
#[derive(Deserialize)]
struct EventQuery {
    after: Option<i64>,
}
async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventQuery>,
) -> impl IntoResponse {
    let mut cursor = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .or(query.after)
        .unwrap_or(0)
        .max(0);
    let stream = async_stream::stream! {
        loop {
            let rows = sqlx::query_as::<_, (i64, String, String)>("SELECT sequence,kind,payload FROM events WHERE sequence>? ORDER BY sequence LIMIT 100")
                .bind(cursor).fetch_all(&state.db).await;
            match rows {
                Ok(rows) => {
                    let full_batch = rows.len() == 100;
                    for (sequence,kind,payload) in rows {
                        cursor = sequence;
                        yield Ok::<_, Infallible>(Event::default().id(sequence.to_string()).event(kind).data(payload));
                    }
                    if full_batch { continue; }
                }
                Err(error) => { tracing::error!(%error, "event replay failed"); break; }
            }
            tokio::select! {
                _ = state.shutdown.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_millis(500)) => {}
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
