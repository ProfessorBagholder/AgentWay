//! Connection identity is assigned by authentication, never by tool arguments.
use super::*;
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
type Api<T> = std::result::Result<Json<T>, http::Error>;

#[derive(FromRow)]
struct Connection {
    id: String,
    name: String,
    product: String,
    token: String,
    publish_enabled: bool,
    disconnected: bool,
    last_seen: Option<String>,
    operation: Option<String>,
    revision: i64,
}
impl Connection {
    fn public(&self) -> Value {
        json!({"id":self.id,"name":self.name,"product":self.product,
            "state":if self.disconnected {"Disconnected"} else if self.last_seen.is_some() {"Connected"} else {"Setup incomplete"},
            "publish_enabled":self.publish_enabled,"revision":self.revision,
            "activity":self.last_seen.as_ref().map(|time| json!({"last_seen":time,"operation":self.operation,"revision":self.revision}))})
    }
}
impl Publisher {
    pub(super) fn connection_id(&self) -> &str {
        self.1.as_deref().unwrap_or("publishing")
    }
    pub(super) fn for_connection(&self, id: &str) -> Self {
        Self(self.0.clone(), Some(id.into()))
    }
    async fn connection_record(&self) -> Result<Connection> {
        Ok(sqlx::query_as("SELECT * FROM agent_connections WHERE id=?")
            .bind(self.connection_id())
            .fetch_one(&self.0.db)
            .await?)
    }
    pub(super) async fn initialize_connections(&self) -> Result<()> {
        let token = self.0.vault.seal(&format!(
            "{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        ))?;
        sqlx::query("INSERT OR IGNORE INTO agent_connections(id,name,product,token,publish_enabled) VALUES('publishing','Agent','Agent',?,1)")
            .bind(token).execute(&self.0.db).await?;
        Ok(())
    }
    // Compatibility with the original publishing connection API and saved settings.
    // All connection values have one canonical source: agent_connections.
    pub(super) async fn connection_setting(&self, key: &str) -> Result<Option<Option<String>>> {
        if !matches!(
            key,
            "agent_token" | "agent_connection_name" | "publish_enabled" | "agent_disconnected"
        ) {
            return Ok(None);
        }
        let row = self.connection_record().await?;
        Ok(Some(Some(match key {
            "agent_token" => row.token,
            "agent_connection_name" => row.name,
            "publish_enabled" => row.publish_enabled.to_string(),
            _ => row.disconnected.to_string(),
        })))
    }
    pub(super) async fn set_connection_setting(&self, key: &str, value: &str) -> Result<bool> {
        let column = match key {
            "agent_token" => "token",
            "agent_connection_name" => "name",
            "publish_enabled" => "publish_enabled",
            "agent_disconnected" => "disconnected",
            _ => return Ok(false),
        };
        let query =
            format!("UPDATE agent_connections SET {column}=?,revision=revision+1 WHERE id=?");
        let value = if matches!(key, "publish_enabled" | "agent_disconnected") {
            if value == "true" { "1" } else { "0" }
        } else {
            value
        };
        sqlx::query(&query)
            .bind(value)
            .bind(self.connection_id())
            .execute(&self.0.db)
            .await?;
        Ok(true)
    }
    pub(super) async fn authenticate_connection(&self, credential: &str) -> Result<Option<Self>> {
        let records: Vec<Connection> =
            sqlx::query_as("SELECT * FROM agent_connections WHERE disconnected=0")
                .fetch_all(&self.0.db)
                .await?;
        let actual = Sha256::digest(credential.as_bytes());
        let mut found = None;
        for record in records {
            let expected = self.0.vault.open_secret(&record.token)?;
            if bool::from(actual.ct_eq(&Sha256::digest(expected.as_bytes()))) {
                found = Some(self.for_connection(&record.id));
            }
        }
        Ok(found)
    }
    pub(super) async fn workspace_connection(&self) -> Result<Value> {
        Ok(self.connection_record().await?.public())
    }
    pub(super) async fn check_publish_access(&self) -> Result<()> {
        let c = self.connection_record().await?;
        if c.disconnected || !c.publish_enabled {
            bail!(
                "Publishing permission has been removed. Check the agent's platform permissions."
            );
        }
        Ok(())
    }
    pub(super) async fn check_media_owner(&self, id: &str) -> Result<()> {
        let owner: Option<String> = sqlx::query_scalar("SELECT agent_id FROM media WHERE id=?")
            .bind(id)
            .fetch_optional(&self.0.db)
            .await?;
        if owner.as_deref() != Some(self.connection_id()) {
            bail!("Media is not available to this connection");
        }
        Ok(())
    }
    pub(super) async fn check_publication_owner(&self, id: &str) -> Result<()> {
        let owner: Option<String> =
            sqlx::query_scalar("SELECT agent_id FROM publications WHERE id=?")
                .bind(id)
                .fetch_optional(&self.0.db)
                .await?;
        if owner.as_deref() != Some(self.connection_id()) {
            bail!("Upload retry belongs to another connection");
        }
        Ok(())
    }
    pub(super) async fn disconnect_connection(&self) -> Result<()> {
        let replacement = self.0.vault.seal(&format!(
            "{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        ))?;
        let mut tx = self.0.db.begin().await?;
        sqlx::query("UPDATE agent_connections SET token=?,disconnected=1,last_seen=NULL,operation=NULL,revision=revision+1 WHERE id=?")
            .bind(replacement).bind(self.connection_id()).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM agent_handoff_grants WHERE sender_id=? OR recipient_id=?")
            .bind(self.connection_id())
            .bind(self.connection_id())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    pub(super) async fn connection_changed(&self) -> Api<Value> {
        let value = self.workspace_connection().await?;
        self.emit("agent.connection", value.clone()).await?;
        Ok(Json(value))
    }
}

pub(super) fn routes() -> Router<Publisher> {
    Router::new()
        .route("/api/agent-connections", get(list).post(create))
        .route("/api/agent-connections/{id}", get(detail))
        .route("/api/agent-connections/{id}/token", post(token))
        .route("/api/agent-connections/{id}/access", post(access))
        .route("/api/agent-connections/{id}/disconnect", post(disconnect))
        .route("/api/agent-connections/{id}/enable", post(enable))
}
async fn list(State(p): State<Publisher>) -> Api<Value> {
    let rows: Vec<Connection> = sqlx::query_as("SELECT * FROM agent_connections ORDER BY rowid")
        .fetch_all(&p.0.db)
        .await?;
    Ok(Json(json!(
        rows.iter().map(Connection::public).collect::<Vec<_>>()
    )))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    product: String,
    publish_enabled: bool,
}
async fn create(State(p): State<Publisher>, Json(input): Json<Create>) -> Api<Value> {
    let name = match input.product.as_str() {
        "Grok Bot" => "Grok",
        "Muse" => "Muse",
        "Claude" => "Claude",
        "ChatGPT" => "ChatGPT",
        "Codex" => "Codex",
        _ => return Err(anyhow::anyhow!("Choose an agent platform").into()),
    };
    let _guard = p.0.mutation.lock().await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_connections")
        .fetch_one(&p.0.db)
        .await?;
    if count >= 100 {
        return Err(anyhow::anyhow!("Connection limit reached").into());
    }
    let id = Uuid::new_v4().to_string();
    let token = p.0.vault.seal(&format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    ))?;
    sqlx::query(
        "INSERT INTO agent_connections(id,name,product,token,publish_enabled) VALUES(?,?,?,?,?)",
    )
    .bind(&id)
    .bind(name)
    .bind(input.product)
    .bind(token)
    .bind(input.publish_enabled)
    .execute(&p.0.db)
    .await?;
    p.for_connection(&id).connection_changed().await
}
async fn detail(State(p): State<Publisher>, Path(id): Path<String>) -> Api<Value> {
    Ok(Json(p.for_connection(&id).workspace_connection().await?))
}
async fn token(State(p): State<Publisher>, Path(id): Path<String>) -> Api<Value> {
    let p = p.for_connection(&id);
    let c = p.connection_record().await?;
    if c.disconnected {
        return Err(anyhow::anyhow!("Set up the connection before requesting its token").into());
    }
    Ok(Json(json!({"token":p.0.vault.open_secret(&c.token)?})))
}
#[derive(Deserialize)]
struct Access {
    publish_enabled: bool,
}
async fn access(
    State(p): State<Publisher>,
    Path(id): Path<String>,
    Json(input): Json<Access>,
) -> Api<Value> {
    let p = p.for_connection(&id);
    let _guard = p.0.mutation.lock().await;
    p.connection_record().await?;
    p.set(
        "publish_enabled",
        if input.publish_enabled {
            "true"
        } else {
            "false"
        },
    )
    .await?;
    p.connection_changed().await
}
async fn disconnect(State(p): State<Publisher>, Path(id): Path<String>) -> Api<Value> {
    let p = p.for_connection(&id);
    let _guard = p.0.mutation.lock().await;
    p.disconnect_connection().await?;
    p.connection_changed().await
}
async fn enable(State(p): State<Publisher>, Path(id): Path<String>) -> Api<Value> {
    let p = p.for_connection(&id);
    let _guard = p.0.mutation.lock().await;
    p.set("agent_disconnected", "false").await?;
    p.connection_changed().await
}
