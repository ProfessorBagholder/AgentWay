//! Owner-only projection of media transfer work and its redacted event chain.
use super::http::Error;
use super::*;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};

#[derive(Serialize, FromRow)]
pub(super) struct TransferActivity {
    cursor: i64,
    id: String,
    agent_name: Option<String>,
    mime: String,
    size: i64,
    offset: i64,
    status: String,
    last_error: Option<String>,
    created_at: Option<String>,
    has_publication: bool,
}

pub(super) fn routes() -> Router<Publisher> {
    Router::new()
        .route("/api/media-transfers", get(list))
        .route("/api/media-transfers/{id}", get(detail))
        .route("/api/media-transfers/{id}/history", get(history))
}

const PROJECTION: &str = "SELECT u.rowid AS cursor,u.media_id AS id,u.agent_name,u.mime,u.size,
    CASE WHEN u.status='ready' THEN u.size ELSE u.observed_offset END AS offset,
    u.status,u.last_error,u.created_at,
    EXISTS(SELECT 1 FROM publications p WHERE p.media_id=u.media_id) AS has_publication
    FROM media_uploads u";

#[derive(Deserialize)]
struct ListQuery {
    before: Option<i64>,
}

#[derive(Serialize)]
struct TransferPage {
    items: Vec<TransferActivity>,
    next: Option<i64>,
}

async fn list(
    State(p): State<Publisher>,
    Query(q): Query<ListQuery>,
) -> Result<Json<TransferPage>, Error> {
    let rows = sqlx::query_as::<_, TransferActivity>(&format!(
        "{PROJECTION} WHERE u.rowid<? ORDER BY u.rowid DESC LIMIT 101"
    ))
    .bind(q.before.unwrap_or(i64::MAX))
    .fetch_all(&p.0.db)
    .await?;
    let more = rows.len() > 100;
    let items: Vec<_> = rows.into_iter().take(100).collect();
    let next = if more {
        items.last().map(|row| row.cursor)
    } else {
        None
    };
    Ok(Json(TransferPage { items, next }))
}

async fn detail(
    State(p): State<Publisher>,
    Path(id): Path<String>,
) -> Result<Json<TransferActivity>, Error> {
    Uuid::parse_str(&id)?;
    let row = sqlx::query_as::<_, TransferActivity>(&format!("{PROJECTION} WHERE u.media_id=?"))
        .bind(id)
        .fetch_one(&p.0.db)
        .await?;
    Ok(Json(row))
}

#[derive(Deserialize)]
struct HistoryQuery {
    after: Option<i64>,
}

async fn history(
    State(p): State<Publisher>,
    Path(id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Value>, Error> {
    Uuid::parse_str(&id)?;
    let exists: i64 =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media_uploads WHERE media_id=?)")
            .bind(&id)
            .fetch_one(&p.0.db)
            .await?;
    if exists == 0 {
        return Err(anyhow::anyhow!("Transfer not found").into());
    }
    let rows: Vec<(i64,String)> = sqlx::query_as("SELECT sequence,payload FROM events WHERE kind='media.transfer' AND json_extract(payload,'$.media_id')=? AND sequence>? ORDER BY sequence LIMIT 201")
        .bind(&id).bind(q.after.unwrap_or(0).max(0)).fetch_all(&p.0.db).await?;
    let more = rows.len() > 200;
    let items: Vec<Value> = rows
        .into_iter()
        .take(200)
        .map(|(sequence, payload)| {
            let mut value: Value = serde_json::from_str(&payload)?;
            value["sequence"] = json!(sequence);
            Ok::<Value, serde_json::Error>(value)
        })
        .collect::<Result<_, _>>()?;
    let next = if more {
        items.last().map(|v| v["sequence"].clone())
    } else {
        None
    };
    Ok(Json(json!({"items":items,"next":next})))
}
