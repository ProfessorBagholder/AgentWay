//! Artifact lifecycle belongs to AgentWay; tusd owns bytes, offsets and transport locks.
use super::*;
use axum::body::Body;
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

pub const MAX_CHUNK: usize = 8 * 1024 * 1024;
#[derive(Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateUpload {
    /// Stable UUID. Identical retries by this connection return the same media ID.
    pub request_id: String,
    /// Exact file bytes; video at most 2 GiB, artwork/captions at most 2 MiB.
    pub size: i64,
    /// video/mp4, video/quicktime, video/webm, image/png, image/jpeg, text/vtt or application/x-subrip.
    pub mime: String,
    /// SHA-256 of the entire source file, 64 lowercase hexadecimal characters.
    pub sha256: String,
}
#[derive(FromRow)]
struct Upload {
    media_id: String,
    agent_id: String,
    request_id: String,
    size: i64,
    mime: String,
    sha256: String,
    #[sqlx(skip)]
    offset: i64,
    transport_id: Option<String>,
    status: String,
    expires_at: i64,
}
#[derive(Debug)]
pub struct TransferError {
    pub status: u16,
    pub code: &'static str,
    pub offset: Option<i64>,
}
impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)
    }
}
impl std::error::Error for TransferError {}
pub(super) fn error(status: u16, code: &'static str, offset: Option<i64>) -> anyhow::Error {
    TransferError {
        status,
        code,
        offset,
    }
    .into()
}
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
impl Upload {
    fn value(&self) -> Value {
        json!({"media_id":self.media_id,"request_id":self.request_id,"size":self.size,"mime":self.mime,"sha256":self.sha256,"offset":self.offset,"status":self.status,"ready":self.status=="ready","expires_at":if self.status=="ready" {None} else {Some(self.expires_at)},"max_chunk_bytes":MAX_CHUNK,"upload_path":format!("/v1/media/uploads/{}/bytes",self.media_id),"status_path":format!("/v1/media/uploads/{}",self.media_id),"complete_path":format!("/v1/media/uploads/{}/complete",self.media_id),"protocol":"tus","tus_version":"1.0.0"})
    }
    fn active(&self) -> Result<()> {
        if ["cancelled", "expired", "cancelling", "expiring"].contains(&self.status.as_str())
            || (self.status != "ready" && self.expires_at <= now())
        {
            return Err(error(410, "upload_gone", None));
        }
        if self.status == "checksum_mismatch" {
            return Err(error(422, "file_checksum_mismatch", Some(self.offset)));
        }
        Ok(())
    }
}
impl Publisher {
    pub async fn transport_ready(&self) -> Result<()> {
        let base = self
            .0
            .tus_url
            .as_deref()
            .ok_or_else(|| error(503, "media_transport_unavailable", None))?;
        let response = self
            .0
            .client
            .get(format!("{base}/health"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map_err(|_| error(503, "media_transport_unavailable", None))?;
        if !response.status().is_success() {
            return Err(error(503, "media_transport_unavailable", None));
        }
        Ok(())
    }
    async fn check_storage_capacity(&self, requested: i64) -> Result<()> {
        let path = self.0.tus_dir.clone();
        let available = tokio::task::spawn_blocking(move || -> Result<u64> {
            let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())?;
            let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
            // libc requires a valid NUL-terminated path and writable stat storage.
            if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let stat = unsafe { stat.assume_init() };
            // statvfs counter widths differ between Darwin and Linux.
            #[allow(clippy::unnecessary_cast)]
            let bytes = (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64);
            Ok(bytes)
        })
        .await??;
        // Conservatively account for the full size of unfinished reservations;
        // partial bytes may be counted twice, never omitted from admission control.
        let outstanding:i64=sqlx::query_scalar("SELECT COALESCE(SUM(size),0) FROM media_uploads WHERE status NOT IN ('ready','cancelled','expired')")
            .fetch_one(&self.0.db).await?;
        if available < (requested + outstanding) as u64 + 256 * 1024 * 1024 {
            return Err(error(507, "media_disk_headroom_exhausted", None));
        }
        Ok(())
    }
    async fn cleanup_transport_attempts(&self) -> Result<()> {
        // A lost creation response can leave an unselected empty transport. Its
        // durable attempt ID permits cleanup even if creation finishes late.
        let attempts:Vec<(String,String)>=sqlx::query_as("SELECT a.id,a.media_id FROM media_transfer_attempts a JOIN media_uploads u ON u.media_id=a.media_id WHERE a.created_at<unixepoch()-300 AND a.last_cleanup<unixepoch()-300 AND ((u.transport_id IS NOT NULL AND u.transport_id!=a.id) OR u.status IN ('cancelled','expired')) ORDER BY a.last_cleanup,a.id LIMIT 100")
            .fetch_all(&self.0.db).await?;
        for (attempt, media_id) in attempts {
            let _guard = self.media_lock(&media_id).await?;
            let current: Upload = sqlx::query_as("SELECT * FROM media_uploads WHERE media_id=?")
                .bind(&media_id)
                .fetch_one(&self.0.db)
                .await?;
            if (current.transport_id.is_none() || current.transport_id.as_deref() == Some(&attempt))
                && !["cancelled", "expired"].contains(&current.status.as_str())
            {
                continue;
            }
            let response = self
                .transport_request(reqwest::Method::DELETE, &attempt)
                .await?;
            if !response.status().is_success()
                && response.status() != reqwest::StatusCode::NOT_FOUND
            {
                return Err(error(503, "media_transport_unavailable", None));
            }
            sqlx::query("UPDATE media_transfer_attempts SET last_cleanup=unixepoch() WHERE id=?")
                .bind(&attempt)
                .execute(&self.0.db)
                .await?;
        }
        Ok(())
    }
    pub(super) async fn media_lock(&self, id: &str) -> Result<tokio::sync::MutexGuard<'_, ()>> {
        let id = Uuid::parse_str(id).map_err(|_| error(400, "invalid_media_id", None))?;
        Ok(self.0.media_locks[(id.as_u128() % 64) as usize]
            .lock()
            .await)
    }
    async fn upload_record(&self, id: &str) -> Result<Upload> {
        Uuid::parse_str(id).map_err(|_| error(400, "invalid_media_id", None))?;
        let upload: Upload = sqlx::query_as("SELECT * FROM media_uploads WHERE media_id=?")
            .bind(id)
            .fetch_optional(&self.0.db)
            .await?
            .ok_or_else(|| error(404, "upload_not_found", None))?;
        if self.1.is_some() && upload.agent_id != self.connection_id() {
            return Err(error(404, "upload_not_found", None));
        }
        Ok(upload)
    }
    pub async fn create_resumable_upload(&self, input: CreateUpload) -> Result<Value> {
        self.check_publish_access().await?;
        if Uuid::parse_str(&input.request_id).is_err()
            || input.sha256.len() != 64
            || !input
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(error(400, "invalid_request_id_or_sha256", None));
        }
        if input.size > self.0.max_video_bytes && input.mime.starts_with("video/") {
            return Err(error(413, "media_size_limit_exceeded", None));
        }
        validate_media(&MediaInput {
            size: input.size,
            mime: input.mime.clone(),
        })?;
        self.expire_uploads().await?;
        self.transport_ready().await?;
        let guard = self.0.mutation.lock().await;
        if let Some(saved) = sqlx::query_as::<_, Upload>(
            "SELECT * FROM media_uploads WHERE agent_id=? AND request_id=?",
        )
        .bind(self.connection_id())
        .bind(&input.request_id)
        .fetch_optional(&self.0.db)
        .await?
        {
            if saved.size != input.size || saved.mime != input.mime || saved.sha256 != input.sha256
            {
                return Err(error(409, "request_id_conflict", None));
            }
            saved.active()?;
            drop(guard);
            return self.media_upload_status(&saved.media_id).await;
        }
        self.check_storage_capacity(input.size).await?;
        let id = Uuid::new_v4().to_string();
        let mut tx = self.0.db.begin().await?;
        let inserted=sqlx::query("INSERT INTO media(id,size,mime,agent_id) SELECT ?,?,?,? WHERE (SELECT COALESCE(SUM(size),0) FROM media)+?<=? AND (SELECT COUNT(*) FROM media_uploads WHERE status NOT IN ('cancelled','expired'))<1024")
            .bind(&id).bind(input.size).bind(&input.mime).bind(self.connection_id()).bind(input.size).bind(self.0.media_budget).execute(&mut *tx).await?;
        if inserted.rows_affected() == 0 {
            return Err(error(507, "media_storage_quota_exceeded", None));
        }
        sqlx::query("INSERT INTO media_uploads(media_id,agent_id,request_id,size,mime,sha256) VALUES(?,?,?,?,?,?)")
            .bind(&id).bind(self.connection_id()).bind(input.request_id).bind(input.size).bind(input.mime).bind(input.sha256).execute(&mut *tx).await?;
        transfer_event(&mut tx, &id, self.connection_id(), "reserved").await?;
        tx.commit().await?;
        drop(guard);
        self.media_upload_status(&id).await
    }
    pub async fn media_upload_status(&self, id: &str) -> Result<Value> {
        let p = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            let _guard = p.media_lock(&id).await?;
            let mut upload = p.upload_record(&id).await?;
            upload.active()?;
            p.ensure_transport(&mut upload).await?;
            upload.offset = p.transport_offset(&upload).await?;
            Ok(upload.value())
        })
        .await?
    }
    pub(super) async fn media_file(&self, id: &str) -> Result<PathBuf> {
        let transport: Option<Option<String>> =
            sqlx::query_scalar("SELECT transport_id FROM media_uploads WHERE media_id=?")
                .bind(id)
                .fetch_optional(&self.0.db)
                .await?;
        match transport {
            Some(Some(id)) => Ok(self.0.tus_dir.join(id)),
            Some(None) => Err(error(409, "upload_incomplete", None)),
            None => Ok(self.0.dir.join("media").join(id)),
        }
    }
    fn transport_url(&self, id: &str) -> Result<String> {
        // Only server-generated UUIDs ever select a private transport resource.
        Uuid::parse_str(id).map_err(|_| error(500, "invalid_transport_id", None))?;
        Ok(format!(
            "{}/files/{id}",
            self.0.tus_url.as_deref().ok_or_else(|| error(
                503,
                "media_transport_unavailable",
                None
            ))?
        ))
    }
    async fn transport_request(
        &self,
        method: reqwest::Method,
        id: &str,
    ) -> Result<reqwest::Response> {
        self.0
            .client
            .request(method, self.transport_url(id)?)
            .header("Tus-Resumable", "1.0.0")
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|_| error(503, "media_transport_unavailable", None))
    }
    async fn transport_offset(&self, upload: &Upload) -> Result<i64> {
        let response = self
            .transport_request(
                reqwest::Method::HEAD,
                upload
                    .transport_id
                    .as_deref()
                    .ok_or_else(|| error(503, "media_transport_pending", None))?,
            )
            .await?;
        if !response.status().is_success() {
            return Err(error(503, "media_transport_unavailable", None));
        }
        let number = |name| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok())
        };
        let offset = number("upload-offset")
            .ok_or_else(|| error(502, "invalid_transport_response", None))?;
        if number("upload-length") != Some(upload.size) || !(0..=upload.size).contains(&offset) {
            return Err(error(502, "invalid_transport_response", None));
        }
        Ok(offset)
    }
    async fn ensure_transport(&self, upload: &mut Upload) -> Result<()> {
        if upload.transport_id.is_some() {
            return Ok(());
        }
        let base = self
            .0
            .tus_url
            .as_deref()
            .ok_or_else(|| error(503, "media_transport_unavailable", None))?;
        // Persist each unique creation attempt BEFORE sending. Never POST that ID
        // twice: tusd's custom-ID hook does not itself prevent truncation on collision.
        let attempts: Vec<(String,i64)> = sqlx::query_as("SELECT id,created_at FROM media_transfer_attempts WHERE media_id=? ORDER BY created_at DESC,id")
            .bind(&upload.media_id).fetch_all(&self.0.db).await?;
        for (id, created) in &attempts {
            let response = self.transport_request(reqwest::Method::HEAD, id).await?;
            if response.status().is_success() {
                sqlx::query("UPDATE media_uploads SET transport_id=? WHERE media_id=? AND transport_id IS NULL").bind(id).bind(&upload.media_id).execute(&self.0.db).await?;
                upload.transport_id = Some(id.clone());
                return Ok(());
            }
            if response.status() != reqwest::StatusCode::NOT_FOUND {
                return Err(error(503, "media_transport_unavailable", None));
            }
            if now() - created < 30 {
                return Err(error(503, "media_transport_pending", None));
            }
        }
        // Bound empty uncertain creations as well as reserved data bytes.
        if attempts.len() >= 3 {
            return Err(error(503, "media_transport_creation_unresolved", None));
        }
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO media_transfer_attempts(id,media_id) VALUES(?,?)")
            .bind(&id)
            .bind(&upload.media_id)
            .execute(&self.0.db)
            .await?;
        let response = self
            .0
            .client
            .post(format!("{base}/files/"))
            .header("Tus-Resumable", "1.0.0")
            .header("Upload-Length", upload.size)
            .header(
                "Upload-Metadata",
                format!("agentway_attempt {}", STANDARD.encode(&id)),
            )
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|_| error(503, "media_transport_pending", None))?;
        if response.status() != reqwest::StatusCode::CREATED {
            return Err(error(503, "media_transport_pending", None));
        }
        // The pinned pre-create hook selects this ID; do not follow an internal Location.
        upload.transport_id = Some(id.clone());
        self.transport_offset(upload).await?;
        tokio::fs::File::open(self.0.tus_dir.join(format!("{id}.info")))
            .await?
            .sync_all()
            .await?;
        self.sync_transfer_directory().await?;
        sqlx::query("UPDATE media_uploads SET transport_id=? WHERE media_id=?")
            .bind(&id)
            .bind(&upload.media_id)
            .execute(&self.0.db)
            .await?;
        Ok(())
    }
    pub(super) async fn receive_chunk(
        &self,
        id: &str,
        offset: i64,
        checksum: Option<&str>,
        length: Option<usize>,
        body: Body,
    ) -> Result<i64> {
        let p = self.clone();
        let id = id.to_owned();
        let checksum = checksum.map(str::to_owned);
        // Client cancellation must not release serialization while a filesystem
        // write or SQLite commit is still in flight. The bounded task owns both.
        tokio::spawn(async move {
            p.receive_chunk_inner(&id, offset, checksum.as_deref(), length, body)
                .await
        })
        .await?
    }
    async fn receive_chunk_inner(
        &self,
        id: &str,
        offset: i64,
        checksum: Option<&str>,
        length: Option<usize>,
        body: Body,
    ) -> Result<i64> {
        self.check_publish_access().await?;
        let _permit = self
            .0
            .transfers
            .try_acquire()
            .map_err(|_| error(429, "transfers_busy", None))?;
        // Serialize validation and forwarding with finalization/deletion. After
        // an uncertain transport response, recover progress through tusd HEAD.
        let _guard = self.media_lock(id).await?;
        let mut upload = self.upload_record(id).await?;
        upload.active()?;
        self.ensure_transport(&mut upload).await?;
        upload.offset = self.transport_offset(&upload).await?;
        if offset != upload.offset {
            return Err(error(409, "offset_conflict", Some(upload.offset)));
        }
        if upload.status == "ready" {
            return Err(error(409, "media_already_finalized", Some(upload.offset)));
        }
        if length.is_some_and(|v| v > MAX_CHUNK || v as i64 > upload.size - offset) {
            return Err(error(413, "chunk_too_large", Some(offset)));
        }
        let expected = if let Some(value) = checksum {
            let (algorithm, value) = value
                .split_once(' ')
                .ok_or_else(|| error(400, "invalid_chunk_checksum", None))?;
            let digest_len = match algorithm {
                "sha256" => 32,
                "sha1" => 20,
                _ => return Err(error(400, "unsupported_checksum_algorithm", None)),
            };
            let bytes = STANDARD
                .decode(value)
                .map_err(|_| error(400, "invalid_chunk_checksum", None))?;
            if bytes.len() != digest_len {
                return Err(error(400, "invalid_chunk_checksum", None));
            }
            Some((algorithm, bytes))
        } else {
            None
        };
        let mut bytes = Vec::new();
        let mut stream = body.into_data_stream();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        while let Some(chunk) = tokio::time::timeout_at(deadline, stream.next())
            .await
            .map_err(|_| error(408, "chunk_timeout", Some(offset)))?
        {
            let chunk = chunk.map_err(|_| error(400, "chunk_interrupted", Some(offset)))?;
            if bytes.len() + chunk.len() > MAX_CHUNK
                || (bytes.len() + chunk.len()) as i64 > upload.size - offset
            {
                return Err(error(413, "chunk_too_large", Some(offset)));
            }
            bytes.extend_from_slice(&chunk);
        }
        if length.is_some_and(|v| v != bytes.len()) {
            return Err(error(400, "chunk_length_mismatch", Some(offset)));
        }
        if expected.is_some_and(|(algorithm, expected)| {
            let actual = if algorithm == "sha1" {
                sha1::Sha1::digest(&bytes).to_vec()
            } else {
                Sha256::digest(&bytes).to_vec()
            };
            expected != actual
        }) {
            return Err(error(460, "chunk_checksum_mismatch", Some(offset)));
        }
        self.check_publish_access().await?;
        if self.upload_record(id).await?.expires_at <= now() {
            return Err(error(410, "upload_gone", None));
        }
        let transport_id = upload.transport_id.as_deref().unwrap();
        let response = self
            .0
            .client
            .patch(self.transport_url(transport_id)?)
            .header("Tus-Resumable", "1.0.0")
            .header("Upload-Offset", offset)
            .header("Content-Type", "application/offset+octet-stream")
            .body(bytes)
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .map_err(|_| error(503, "media_transport_uncertain", None))?;
        if response.status() == reqwest::StatusCode::CONFLICT {
            return Err(error(
                409,
                "offset_conflict",
                Some(self.transport_offset(&upload).await?),
            ));
        }
        if response.status() != reqwest::StatusCode::NO_CONTENT {
            return Err(error(503, "media_transport_uncertain", None));
        }
        let next = self.transport_offset(&upload).await?;
        // tusd owns the offset; AgentWay does not maintain a second byte ledger.
        // Sync the shared data before acknowledging at the public boundary.
        tokio::fs::File::open(self.0.tus_dir.join(transport_id))
            .await?
            .sync_all()
            .await?;
        self.sync_transfer_directory().await?;
        sqlx::query("UPDATE media_uploads SET expires_at=unixepoch()+604800 WHERE media_id=?")
            .bind(id)
            .execute(&self.0.db)
            .await?;
        Ok(next)
    }
    async fn sync_transfer_directory(&self) -> Result<()> {
        let path = self.0.tus_dir.clone();
        tokio::task::spawn_blocking(move || std::fs::File::open(path)?.sync_all()).await??;
        Ok(())
    }
    pub async fn complete_media_upload(&self, id: &str) -> Result<Value> {
        let p = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move { p.complete_media_upload_inner(&id).await }).await?
    }
    async fn complete_media_upload_inner(&self, id: &str) -> Result<Value> {
        self.check_publish_access().await?;
        let _permit = self
            .0
            .transfers
            .try_acquire()
            .map_err(|_| error(429, "transfers_busy", None))?;
        let _guard = self.media_lock(id).await?;
        let mut upload = self.upload_record(id).await?;
        upload.active()?;
        self.ensure_transport(&mut upload).await?;
        upload.offset = self.transport_offset(&upload).await?;
        if upload.status == "ready" {
            return Ok(upload.value());
        }
        if upload.offset != upload.size {
            return Err(error(409, "upload_incomplete", Some(upload.offset)));
        }
        let path = self.media_file(id).await?;
        let mut file = tokio::fs::File::open(&path).await?;
        if file.metadata().await?.len() != upload.size as u64 {
            return Err(error(500, "file_length_mismatch", None));
        }
        let mut hash = Sha256::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        if format!("{:x}", hash.finalize()) != upload.sha256 {
            let mut tx = self.0.db.begin().await?;
            sqlx::query("UPDATE media_uploads SET status='checksum_mismatch' WHERE media_id=?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            transfer_event(&mut tx, id, &upload.agent_id, "checksum_mismatch").await?;
            tx.commit().await?;
            return Err(error(422, "file_checksum_mismatch", Some(upload.offset)));
        }
        file.sync_all().await?;
        drop(file);
        let _mutation = self.0.mutation.lock().await;
        self.check_publish_access().await?;
        tokio::fs::File::open(
            self.0
                .tus_dir
                .join(format!("{}.info", upload.transport_id.as_deref().unwrap())),
        )
        .await?
        .sync_all()
        .await?;
        // No multi-gigabyte copy: ready artifacts remain in the private tus volume.
        self.sync_transfer_directory().await?;
        let mut tx = self.0.db.begin().await?;
        sqlx::query("UPDATE media SET ready=1 WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE media_uploads SET status='ready' WHERE media_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        transfer_event(&mut tx, id, &upload.agent_id, "ready").await?;
        tx.commit().await?;
        upload.status = "ready".into();
        Ok(upload.value())
    }
    pub async fn cancel_media_upload(&self, id: &str) -> Result<Value> {
        let p = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move { p.cancel_media_upload_inner(&id).await }).await?
    }
    async fn cancel_media_upload_inner(&self, id: &str) -> Result<Value> {
        self.check_publish_access().await?;
        let _guard = self.media_lock(id).await?;
        let upload = self.upload_record(id).await?;
        if ["cancelled", "expired"].contains(&upload.status.as_str()) {
            return Ok(upload.value());
        }
        self.remove_media_locked(id, "cancelled").await?;
        Ok(self.upload_record(id).await?.value())
    }
    pub(super) async fn remove_media_locked(&self, id: &str, status: &str) -> Result<()> {
        let mutation = self.0.mutation.lock().await;
        self.check_media_owner(id).await?;
        let used:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM publications WHERE media_id=?) OR EXISTS(SELECT 1 FROM youtube_asset_operations WHERE json_extract(input,'$.media_id')=? AND json_extract(result,'$.status') IN ('upload_pending','outcome_unknown')) OR EXISTS(SELECT 1 FROM podcast_operations WHERE json_extract(input,'$.media_id')=? AND json_extract(result,'$.status') IN ('upload_pending','outcome_unknown'))")
            .bind(id).bind(id).bind(id).fetch_one(&self.0.db).await?;
        if used {
            return Err(error(409, "media_in_use", None));
        }
        // Invalidate ready media durably before unlinking it. Recovery completes
        // interrupted cleanup without exposing a ready row whose file is gone.
        let mut tx = self.0.db.begin().await?;
        sqlx::query("UPDATE media SET ready=0 WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE media_uploads SET status=? WHERE media_id=?")
            .bind(if status == "expired" {
                "expiring"
            } else {
                "cancelling"
            })
            .bind(id)
            .execute(&mut *tx)
            .await?;
        transfer_event(
            &mut tx,
            id,
            self.connection_id(),
            if status == "expired" {
                "expiring"
            } else {
                "cancelling"
            },
        )
        .await?;
        tx.commit().await?;
        // Readiness is durably invalidated. Retain only the media-specific lock
        // while waiting for storage; unrelated permission changes must remain available.
        drop(mutation);
        let attempts: Vec<String> =
            sqlx::query_scalar("SELECT id FROM media_transfer_attempts WHERE media_id=?")
                .bind(id)
                .fetch_all(&self.0.db)
                .await?;
        for attempt in attempts {
            let response = self
                .transport_request(reqwest::Method::DELETE, &attempt)
                .await?;
            if !response.status().is_success()
                && response.status() != reqwest::StatusCode::NOT_FOUND
            {
                return Err(error(503, "media_transport_unavailable", None));
            }
        }
        match tokio::fs::remove_file(self.0.dir.join("media").join(id)).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let mut tx = self.0.db.begin().await?;
        sqlx::query("UPDATE media_uploads SET status=? WHERE media_id=?")
            .bind(status)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM media WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        transfer_event(&mut tx, id, self.connection_id(), status).await?;
        tx.commit().await?;
        Ok(())
    }
    pub(super) async fn expire_uploads(&self) -> Result<()> {
        self.cleanup_transport_attempts().await?;
        let ids:Vec<String>=sqlx::query_scalar("SELECT media_id FROM media_uploads WHERE (status IN ('receiving','checksum_mismatch') AND expires_at<=unixepoch()) OR status IN ('cancelling','expiring') LIMIT 100").fetch_all(&self.0.db).await?;
        for id in ids {
            let _guard = self.media_lock(&id).await?;
            let root = Self(self.0.clone(), None);
            let upload = root.upload_record(&id).await?;
            if (["receiving", "checksum_mismatch"].contains(&upload.status.as_str())
                && upload.expires_at <= now())
                || ["cancelling", "expiring"].contains(&upload.status.as_str())
            {
                let status = if upload.status == "cancelling" {
                    "cancelled"
                } else {
                    "expired"
                };
                root.for_connection(&upload.agent_id)
                    .remove_media_locked(&id, status)
                    .await?;
            }
        }
        Ok(())
    }
}

async fn transfer_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    agent: &str,
    status: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO events(kind,payload) VALUES('media.transfer',?)")
        .bind(json!({"media_id":id,"agent_id":agent,"status":status}).to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

use super::http::{AgentState, Error};
use axum::{
    Json,
    extract::{Path, Request},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
pub(super) async fn create(
    AgentState(p): AgentState,
    Json(input): Json<CreateUpload>,
) -> Result<Json<Value>, Error> {
    Ok(Json(p.create_resumable_upload(input).await?))
}
pub(super) async fn status(
    AgentState(p): AgentState,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    Ok(Json(p.media_upload_status(&id).await?))
}
pub(super) async fn complete(
    AgentState(p): AgentState,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    Ok(Json(p.complete_media_upload(&id).await?))
}
pub(super) async fn cancel(
    AgentState(p): AgentState,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    Ok(Json(p.cancel_media_upload(&id).await?))
}
fn header<'a>(headers: &'a HeaderMap, key: &str) -> Result<Option<&'a str>> {
    let mut values = headers.get_all(key).iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(error(400, "duplicate_upload_header", None));
    }
    value
        .map(|v| {
            v.to_str()
                .map_err(|_| error(400, "invalid_upload_header", None))
        })
        .transpose()
}
pub(super) async fn tus_headers(request: Request, next: Next) -> Response {
    let tus = request.uri().path().starts_with("/v1/media/uploads/")
        && request.uri().path().ends_with("/bytes");
    let mut response = next.run(request).await;
    if tus {
        response
            .headers_mut()
            .insert("tus-resumable", "1.0.0".parse().unwrap());
        response
            .headers_mut()
            .insert("cache-control", "no-store".parse().unwrap());
        if response.status() == StatusCode::PRECONDITION_FAILED {
            response
                .headers_mut()
                .insert("tus-version", "1.0.0".parse().unwrap());
        }
    }
    response
}
pub(super) async fn tus(
    AgentState(p): AgentState,
    Path(id): Path<String>,
    request: Request,
) -> Result<Response, Error> {
    let (parts, body) = request.into_parts();
    let method = header(&parts.headers, "x-http-method-override")?.unwrap_or(parts.method.as_str());
    if method == "OPTIONS" {
        return Ok((
            StatusCode::NO_CONTENT,
            [
                ("tus-version", "1.0.0"),
                ("tus-extension", "checksum,termination,expiration"),
                ("tus-checksum-algorithm", "sha256,sha1"),
                ("tus-max-size", "2147483648"),
            ],
        )
            .into_response());
    }
    if header(&parts.headers, "tus-resumable")? != Some("1.0.0") {
        return Err(error(412, "unsupported_tus_version", None).into());
    }
    match method {
        "HEAD" => {
            let value = p.media_upload_status(&id).await?;
            Ok((
                StatusCode::OK,
                [
                    ("upload-offset", value["offset"].to_string()),
                    ("upload-length", value["size"].to_string()),
                ],
            )
                .into_response())
        }
        "PATCH" => {
            if header(&parts.headers, "content-type")? != Some("application/offset+octet-stream") {
                return Err(error(415, "invalid_chunk_content_type", None).into());
            }
            if header(&parts.headers, "content-encoding")?.is_some() {
                return Err(error(415, "encoded_chunks_unsupported", None).into());
            }
            let offset = header(&parts.headers, "upload-offset")?
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|v| *v <= i64::MAX as u64)
                .ok_or_else(|| error(400, "invalid_upload_offset", None))?
                as i64;
            let length = header(&parts.headers, "content-length")?
                .map(|s| {
                    s.parse::<usize>()
                        .map_err(|_| error(400, "invalid_content_length", None))
                })
                .transpose()?;
            let offset = p
                .receive_chunk(
                    &id,
                    offset,
                    header(&parts.headers, "upload-checksum")?,
                    length,
                    body,
                )
                .await?;
            Ok((
                StatusCode::NO_CONTENT,
                [
                    ("upload-offset", offset.to_string()),
                    (
                        "upload-expires",
                        (chrono::Utc::now() + chrono::Duration::days(7))
                            .format("%a, %d %b %Y %H:%M:%S GMT")
                            .to_string(),
                    ),
                ],
            )
                .into_response())
        }
        "DELETE" => {
            p.cancel_media_upload(&id).await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        _ => Ok((
            StatusCode::METHOD_NOT_ALLOWED,
            [("allow", "HEAD, PATCH, DELETE, OPTIONS")],
        )
            .into_response()),
    }
}
