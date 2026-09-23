use super::http::Error;
use super::*;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
type Api<T> = std::result::Result<Json<T>, Error>;

pub(super) fn routes() -> Router<Publisher> {
    Router::new()
        .route("/api/publishing/connection/access", post(access))
        .route("/api/publishing/connection/disconnect", post(disconnect))
        .route("/api/publishing/connection/enable", post(enable))
        .route("/api/publications/{id}", get(detail))
        .route("/api/publications/{id}/history", get(history))
        .route("/api/publications/{id}/visibility", get(visibility))
}
#[derive(Deserialize)]
struct Access {
    publish_enabled: bool,
}
async fn access(State(p): State<Publisher>, Json(input): Json<Access>) -> Api<Value> {
    let _guard = p.0.mutation.lock().await;
    p.set(
        "publish_enabled",
        if input.publish_enabled {
            "true"
        } else {
            "false"
        },
    )
    .await?;
    changed(&p).await
}
async fn changed(p: &Publisher) -> Api<Value> {
    let value = p.workspace_connection().await?;
    p.emit("agent.connection", value.clone()).await?;
    Ok(Json(value))
}
async fn disconnect(State(p): State<Publisher>) -> Api<Value> {
    let _guard = p.0.mutation.lock().await;
    p.disconnect_connection().await?;
    changed(&p).await
}
async fn enable(State(p): State<Publisher>) -> Api<Value> {
    let _guard = p.0.mutation.lock().await;
    p.set("agent_disconnected", "false").await?;
    changed(&p).await
}
async fn detail(State(p): State<Publisher>, Path(id): Path<String>) -> Api<Value> {
    let (input, channel): (String, String) =
        sqlx::query_as("SELECT input,channel_id FROM publications WHERE id=?")
            .bind(&id)
            .fetch_one(&p.0.db)
            .await?;
    let input = PublishInput::from_saved(&input)?;
    Ok(Json(
        json!({"publication":p.publication(&id).await?,"settings":input,"channel_id":channel}),
    ))
}
#[derive(Deserialize)]
struct HistoryQuery {
    after: Option<i64>,
}
async fn history(
    State(p): State<Publisher>,
    Path(id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Api<Value> {
    p.publication(&id).await?;
    let media_id: String = sqlx::query_scalar("SELECT media_id FROM publications WHERE id=?")
        .bind(&id)
        .fetch_one(&p.0.db)
        .await?;
    let has_transfer: i64 =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media_uploads WHERE media_id=?)")
            .bind(&media_id)
            .fetch_one(&p.0.db)
            .await?;
    let rows: Vec<(i64,String)> = sqlx::query_as("SELECT sequence,payload FROM events WHERE kind='publication.upsert' AND json_extract(payload,'$.id')=? AND sequence>? ORDER BY sequence LIMIT 201")
        .bind(&id).bind(q.after.unwrap_or(0).max(0)).fetch_all(&p.0.db).await?;
    let more = rows.len() > 200;
    let mut items = Vec::new();
    for (sequence, payload) in rows.into_iter().take(200) {
        // Deserialize into the public projection: never expose session URLs, tokens, or raw input.
        let value: Value = serde_json::from_str(&payload)?;
        let event_at = value.get("event_at").and_then(Value::as_str);
        let publication: Publication = serde_json::from_str(&payload)?;
        items.push(json!({"sequence":sequence,"event_at":event_at,"publication":publication}));
    }
    let next = if more {
        items.last().map(|v| v["sequence"].clone())
    } else {
        None
    };
    Ok(Json(
        json!({"items":items,"next":next,"media_id":if has_transfer!=0 {Some(media_id)} else {None},"video_operations":p.video_operations(&id).await?}),
    ))
}
async fn visibility(
    State(p): State<Publisher>,
    Path(id): Path<String>,
) -> Api<PublicationVerification> {
    Ok(Json(p.verified_publication(&id).await?))
}
