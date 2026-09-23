mod channel;
mod connections;
use channel::ChannelDescriptionInput;
mod assets;
mod guidance;
mod http;
mod mcp;
mod oauth;
mod podcast;
mod settings;
#[cfg(test)]
mod tests;
mod vault;
mod video;
use video::{DeleteInput, VideoOperation, VisibilityInput};
mod worker;
mod workspace;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, SqlitePool};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, Notify, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub struct Publisher(Arc<Inner>, Option<String>);
struct Inner {
    db: SqlitePool,
    _lock: std::fs::File,
    dir: PathBuf,
    vault: vault::Vault,
    client: reqwest::Client,
    endpoints: Endpoints,
    wake: Notify,
    transfers: Semaphore,
    access_token: Mutex<Option<CachedToken>>,
    mutation: Mutex<()>,
    auth_failure_log: Mutex<Option<std::time::Instant>>,
    shutdown: CancellationToken,
}
struct CachedToken {
    channel: String,
    value: String,
    expires: std::time::Instant,
}
#[derive(Clone)]
struct Endpoints {
    categories: String,
    captions: String,
    caption_upload: String,
    thumbnails: String,
    token: String,
    channels: String,
    upload: String,
    videos: String,
    playlists: String,
    playlist_items: String,
    playlist_images: String,
    playlist_images_upload: String,
}
impl Default for Endpoints {
    fn default() -> Self {
        Self {
            categories: "https://www.googleapis.com/youtube/v3/videoCategories".into(),
            captions: "https://www.googleapis.com/youtube/v3/captions".into(),
            caption_upload: "https://www.googleapis.com/upload/youtube/v3/captions".into(),
            thumbnails: "https://www.googleapis.com/upload/youtube/v3/thumbnails/set".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            channels: "https://www.googleapis.com/youtube/v3/channels".into(),
            upload: "https://www.googleapis.com/upload/youtube/v3/videos".into(),
            videos: "https://www.googleapis.com/youtube/v3/videos".into(),
            playlists: "https://www.googleapis.com/youtube/v3/playlists".into(),
            playlist_items: "https://www.googleapis.com/youtube/v3/playlistItems".into(),
            playlist_images: "https://www.googleapis.com/youtube/v3/playlistImages".into(),
            playlist_images_upload: "https://www.googleapis.com/upload/youtube/v3/playlistImages"
                .into(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Publication {
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub deleted_at: Option<String>,
    pub id: String,
    pub title: String,
    pub status: String,
    pub uploaded_bytes: i64,
    pub total_bytes: i64,
    pub video_url: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub revision: i64,
}
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct BridgeActivity {
    pub last_seen: String,
    pub operation: String,
    pub revision: i64,
}
#[derive(Debug, Serialize)]
pub struct PublicationVerification {
    #[serde(flatten)]
    publication: Publication,
    requested_privacy: String,
    actual_privacy: Option<String>,
    visibility_error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishInput {
    /// Stable UUID. Reuse on retries; never generate a new ID for the same upload.
    pub request_id: String,
    pub media_id: String,
    /// 1–100 characters; no < or >.
    pub title: String,
    #[serde(default)]
    /// At most 5000 UTF-8 bytes; no < or >. Defaults to empty.
    pub description: String,
    /// private, unlisted, or public. Subject to the owner's private-only setting.
    #[serde(default = "private")]
    pub privacy: String,
    /// Required audience declaration. True if child-directed; do not default from agent identity.
    pub made_for_kids: bool,
    /// Required realistic altered/synthetic content declaration. AI script assistance alone is not sufficient.
    /// See https://support.google.com/youtube/answer/14328491.
    pub contains_synthetic_media: bool,
    /// Notify subscribers about this upload. Defaults to true; set false to disable.
    #[serde(default = "notifications_enabled")]
    pub notify_subscribers: bool,
    /// Optional YouTube settings. Omitted fields retain legacy upload defaults.
    #[serde(default)]
    pub settings: settings::VideoSettings,
}
impl PublishInput {
    fn from_saved(input: &str) -> Result<Self> {
        // Previously queued jobs retain their original notification behavior.
        let mut saved: Value = serde_json::from_str(input)?;
        saved
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("Invalid stored publication"))?
            .entry("notify_subscribers")
            .or_insert(json!(false));
        Ok(serde_json::from_value(saved)?)
    }
}
fn notifications_enabled() -> bool {
    true
}
fn private() -> String {
    "private".into()
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaInput {
    /// Exact byte length; maximum 2 GiB in this version.
    pub size: i64,
    /// Video: video/mp4, video/quicktime, video/webm (2 GiB). Artwork: image/png or image/jpeg. Timed captions: text/vtt or application/x-subrip. Artwork/captions: 2 MiB.
    pub mime: String,
}
#[derive(FromRow)]
struct Media {
    id: String,
    size: i64,
    mime: String,
    ready: i64,
}
const MAX_MEDIA: i64 = 2 * 1024 * 1024 * 1024;
impl Publisher {
    pub async fn new(db: SqlitePool, dir: PathBuf, shutdown: CancellationToken) -> Result<Self> {
        let vault = vault::Vault::open(&dir)?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("publisher.lock"))?;
        lock.try_lock().map_err(|_| {
            anyhow::anyhow!("Another AgentWay process is using this publishing directory")
        })?;
        tokio::fs::create_dir_all(dir.join("media")).await?;
        let mut files = tokio::fs::read_dir(dir.join("media")).await?;
        while let Some(file) = files.next_entry().await? {
            if file.path().extension().is_some_and(|e| e == "part") {
                tokio::fs::remove_file(file.path()).await?;
            }
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .build()?;
        let this = Self(
            Arc::new(Inner {
                db,
                _lock: lock,
                dir,
                vault,
                client,
                endpoints: Endpoints::default(),
                wake: Notify::new(),
                transfers: Semaphore::new(2),
                access_token: Mutex::new(None),
                mutation: Mutex::new(()),
                auth_failure_log: Mutex::new(None),
                shutdown,
            }),
            None,
        );
        this.initialize_connections().await?;
        Ok(this)
    }
    pub fn start_worker(&self) -> tokio::task::JoinHandle<()> {
        let this = self.clone();
        tokio::spawn(async move { this.worker().await })
    }
    async fn setting(&self, key: &str) -> Result<Option<String>> {
        if let Some(value) = self.connection_setting(key).await? {
            return Ok(value);
        }
        Ok(
            sqlx::query_scalar("SELECT value FROM publishing_settings WHERE key=?")
                .bind(key)
                .fetch_optional(&self.0.db)
                .await?,
        )
    }
    async fn set(&self, key: &str, value: &str) -> Result<()> {
        if self.set_connection_setting(key, value).await? {
            return Ok(());
        }
        sqlx::query("INSERT INTO publishing_settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(key).bind(value).execute(&self.0.db).await?;
        Ok(())
    }
    async fn set_secret(&self, key: &str, value: &str) -> Result<()> {
        self.set(key, &self.0.vault.seal(value)?).await
    }
    async fn secret(&self, key: &str) -> Result<String> {
        self.0.vault.open_secret(
            &self
                .setting(key)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Configure YouTube in Publishing first"))?,
        )
    }
    pub async fn status(&self) -> Result<Value> {
        let account: Option<(String, String)> =
            sqlx::query_as("SELECT channel_id,channel_name FROM youtube_account WHERE id=1")
                .fetch_optional(&self.0.db)
                .await?;
        let manage_channel = self.setting("youtube_manage_channel").await?;
        Ok(
            json!({"connection":self.workspace_connection().await?, "video_management_authorized": account.as_ref().is_some_and(|(id,_)| Some(id.as_str()) == manage_channel.as_deref()), "configured": self.setting("client_id").await?.is_some(), "account": account.map(|(id,name)| json!({"id":id,"name":name})), "private_only": self.setting("private_only").await?.as_deref() != Some("false"), "bridge_url":self.setting("bridge_url").await?.unwrap_or_default(), "agent_guidance":guidance::payload()}),
        )
    }
    pub async fn activity(&self) -> Result<Option<BridgeActivity>> {
        Ok(sqlx::query_as("SELECT last_seen,operation,revision FROM agent_connections WHERE id=? AND last_seen IS NOT NULL")
            .bind(self.connection_id()).fetch_optional(&self.0.db).await?)
    }
    async fn record_activity(&self, operation: &str) -> Result<()> {
        sqlx::query("UPDATE agent_connections SET last_seen=strftime('%Y-%m-%dT%H:%M:%fZ','now'),operation=?,revision=revision+1 WHERE id=? AND disconnected=0")
            .bind(operation).bind(self.connection_id()).execute(&self.0.db).await?;
        self.emit("agent.connection", self.workspace_connection().await?)
            .await
    }
    pub async fn list(&self) -> Result<Vec<Publication>> {
        Ok(
            sqlx::query_as("SELECT * FROM publications ORDER BY created_at DESC LIMIT 100")
                .fetch_all(&self.0.db)
                .await?,
        )
    }
    pub async fn publication(&self, id: &str) -> Result<Publication> {
        Ok(sqlx::query_as("SELECT * FROM publications WHERE id=?")
            .bind(id)
            .fetch_one(&self.0.db)
            .await?)
    }
    pub async fn verified_publication(&self, id: &str) -> Result<PublicationVerification> {
        let publication = self.publication(id).await?;
        let (input, channel, video): (String, String, Option<String>) =
            sqlx::query_as("SELECT input,channel_id,video_id FROM publications WHERE id=?")
                .bind(id)
                .fetch_one(&self.0.db)
                .await?;
        let input = PublishInput::from_saved(&input)?;
        let mut result = PublicationVerification {
            publication,
            requested_privacy: input.privacy,
            actual_privacy: None,
            visibility_error: None,
        };
        if result.publication.deleted_at.is_some() {
            result.visibility_error = Some("Video was deleted; upload history is retained".into());
            return Ok(result);
        }
        if let Some(video) = video {
            match self.video_privacy(&channel, &video).await {
                Ok(privacy) => result.actual_privacy = Some(privacy),
                Err(_) => result.visibility_error = Some(
                    "Could not verify current YouTube visibility. Upload remains complete; retry this status check, not the upload.".into()),
            }
        }
        Ok(result)
    }
    async fn video_privacy(&self, channel: &str, video: &str) -> Result<String> {
        let token = self.access_token(channel).await?;
        let response = self
            .0
            .client
            .get(&self.0.endpoints.videos)
            .query(&[("part", "status"), ("id", video)])
            .bearer_auth(token)
            .send()
            .await?;
        if response.status().as_u16() == 401 {
            *self.0.access_token.lock().await = None;
        }
        let body: Value = response.error_for_status()?.json().await?;
        body["items"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["id"].as_str() == Some(video)))
            .and_then(|item| item["status"]["privacyStatus"].as_str())
            .filter(|privacy| ["private", "unlisted", "public"].contains(privacy))
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("YouTube did not return visibility"))
    }
    async fn emit(&self, kind: &str, value: Value) -> Result<()> {
        sqlx::query("INSERT INTO events(kind,payload) VALUES(?,?)")
            .bind(kind)
            .bind(value.to_string())
            .execute(&self.0.db)
            .await?;
        Ok(())
    }
    async fn publish_event(&self, id: &str) -> Result<()> {
        let mut value = serde_json::to_value(self.publication(id).await?)?;
        let timestamp: String = sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')")
            .fetch_one(&self.0.db)
            .await?;
        value["event_at"] = json!(timestamp);
        self.emit("publication.upsert", value).await
    }
    pub async fn create_media(&self, input: MediaInput) -> Result<Value> {
        if !(1..=MAX_MEDIA).contains(&input.size)
            || ![
                "video/mp4",
                "video/quicktime",
                "video/webm",
                "image/png",
                "image/jpeg",
                "text/vtt",
                "application/x-subrip",
            ]
            .contains(&input.mime.as_str())
            || (!input.mime.starts_with("video/") && input.size > 2 * 1024 * 1024)
        {
            bail!(
                "Provide video up to 2 GiB or PNG/JPEG artwork or timed UTF-8 SRT/WebVTT captions up to 2 MiB"
            );
        }
        self.check_publish_access().await?;
        let id = Uuid::new_v4().to_string();
        // Bound disk reservations, including unfinished transfers.
        let mut tx = self.0.db.begin().await?;
        let result = sqlx::query("INSERT INTO media(id,size,mime,agent_id) SELECT ?,?,?,? WHERE (SELECT COALESCE(SUM(size),0) FROM media)+?<=?")
            .bind(&id).bind(input.size).bind(input.mime).bind(self.connection_id()).bind(input.size).bind(MAX_MEDIA * 5).execute(&mut *tx).await?;
        if result.rows_affected() == 0 {
            bail!(
                "Media storage limit reached (10 GiB). Remove unused media before uploading more."
            );
        }
        tx.commit().await?;
        Ok(
            json!({"media_id": id, "upload_path":format!("/v1/media/{id}"), "method":"PUT", "instructions":"Send raw file bytes to upload_path using the same Authorization bearer token. Do not send a local path or base64. Retry the complete PUT if interrupted. Then call publish_youtube for video, or manage_youtube_podcast action=set_cover for an image, with media_id."}),
        )
    }
    pub async fn enqueue(&self, input: PublishInput) -> Result<Publication> {
        self.check_publish_access().await?;
        if Uuid::parse_str(&input.request_id).is_err() || Uuid::parse_str(&input.media_id).is_err()
        {
            bail!("request_id and media_id must be UUIDs");
        }
        if input.title.trim().is_empty()
            || input.title.chars().count() > 100
            || input.title.contains(['<', '>'])
            || input.description.len() > 5000
            || input.description.contains(['<', '>'])
        {
            bail!(
                "Title must contain 1–100 characters; description at most 5000 bytes; neither may contain < or >"
            );
        }
        if !["private", "unlisted", "public"].contains(&input.privacy.as_str()) {
            bail!("privacy must be private, unlisted or public");
        }
        let _guard = self.0.mutation.lock().await;
        let encoded = serde_json::to_string(&input)?;
        let existing: Option<(String, String)> =
            sqlx::query_as("SELECT id,input FROM publications WHERE request_id=?")
                .bind(&input.request_id)
                .fetch_optional(&self.0.db)
                .await?;
        if let Some((id, original)) = existing {
            self.check_publication_owner(&id).await?;
            // Normalize old requests that predate this optional field, preserving retry IDs.
            let original: PublishInput = serde_json::from_str(&original)?;
            if serde_json::to_string(&original)? != encoded {
                bail!(
                    "request_id already belongs to a different upload; reuse the original arguments"
                );
            }
            return self.publication(&id).await;
        }
        input.settings.validate(&input.privacy, true)?;
        if (input.privacy != "private" || input.settings.publish_at.is_some())
            && self.setting("private_only").await?.as_deref() != Some("false")
        {
            bail!("The owner has enabled private-only uploads in Publishing");
        }
        let channel: String =
            sqlx::query_scalar("SELECT channel_id FROM youtube_account WHERE id=1")
                .fetch_optional(&self.0.db)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Connect a YouTube account in Publishing first"))?;
        self.check_media_owner(&input.media_id).await?;
        let media: Media = sqlx::query_as("SELECT * FROM media WHERE id=? AND ready=1")
            .bind(&input.media_id)
            .fetch_optional(&self.0.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Upload the complete media file first"))?;
        if !media.mime.starts_with("video/") {
            bail!("Publishing requires video media, not a podcast cover");
        }
        let id = Uuid::new_v4().to_string();
        let agent_name = self.setting("agent_connection_name").await?;
        sqlx::query("INSERT INTO publications(id,request_id,input,channel_id,media_id,title,total_bytes,agent_name,agent_id) VALUES(?,?,?,?,?,?,?,?,?)")
            .bind(&id).bind(&input.request_id).bind(encoded).bind(channel).bind(&media.id).bind(&input.title).bind(media.size).bind(agent_name).bind(self.connection_id()).execute(&self.0.db).await?;
        self.publish_event(&id).await?;
        self.0.wake.notify_one();
        self.publication(&id).await
    }
    pub async fn retry(&self, id: &str) -> Result<Publication> {
        let owner: String = sqlx::query_scalar("SELECT agent_id FROM publications WHERE id=?")
            .bind(id)
            .fetch_one(&self.0.db)
            .await?;
        if self.1.is_some() {
            self.check_publication_owner(id).await?;
        }
        self.for_connection(&owner).check_publish_access().await?;
        sqlx::query("UPDATE publications SET status='queued',error=NULL,revision=revision+1 WHERE id=? AND status='interrupted'").bind(id).execute(&self.0.db).await?;
        self.publish_event(id).await?;
        self.0.wake.notify_one();
        self.publication(id).await
    }
}
