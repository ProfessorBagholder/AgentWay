use super::*;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
const CHUNK: usize = 8 * 1024 * 1024;
impl Publisher {
    pub(super) async fn worker(&self) {
        let mut next_cleanup = std::time::Instant::now();
        loop {
            if std::time::Instant::now() >= next_cleanup {
                if let Err(error) = self.expire_uploads().await {
                    tracing::warn!(%error,"Could not expire abandoned media uploads");
                }
                next_cleanup = std::time::Instant::now() + Duration::from_secs(300);
            }
            if self.0.shutdown.is_cancelled() {
                return;
            }
            let next: Result<Option<String>, _> = sqlx::query_scalar("SELECT id FROM publications WHERE status IN ('queued','uploading') ORDER BY created_at LIMIT 1").fetch_optional(&self.0.db).await;
            match next {
                Ok(Some(id)) => {
                    let owner: Result<String, _> =
                        sqlx::query_scalar("SELECT agent_id FROM publications WHERE id=?")
                            .bind(&id)
                            .fetch_one(&self.0.db)
                            .await;
                    let result = match owner {
                        Ok(owner) => self.for_connection(&owner).upload(&id).await,
                        Err(e) => Err(e.into()),
                    };
                    if let Err(error) = result {
                        // Error strings here deliberately exclude URLs, provider responses and credentials.
                        if let Err(storage_error) = self.fail(&id, &error.to_string()).await {
                            tracing::error!(%storage_error, "could not persist upload failure");
                        }
                    }
                }
                Err(error) => {
                    tracing::error!(%error, "publishing queue unavailable");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                Ok(None) => {
                    tokio::select! { _ = self.0.wake.notified() => {}, _ = self.0.shutdown.cancelled() => return, _ = tokio::time::sleep(Duration::from_secs(5)) => {} }
                }
            }
        }
    }
    async fn fail(&self, id: &str, error: &str) -> Result<()> {
        let mut tx = self.0.db.begin().await?;
        let updated = sqlx::query("UPDATE publications SET status='interrupted',error=?,revision=revision+1 WHERE id=? AND status!='uploaded'")
            .bind(error).bind(id).execute(&mut *tx).await?;
        if updated.rows_affected() > 0 {
            Self::publish_event_tx(&mut tx, id).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    async fn upload(&self, id: &str) -> Result<()> {
        self.check_publish_access().await?;
        let (input, channel, session): (String, String, Option<String>) =
            sqlx::query_as("SELECT input,channel_id,session FROM publications WHERE id=?")
                .bind(id)
                .fetch_one(&self.0.db)
                .await?;
        let input = PublishInput::from_saved(&input)?;
        let media: Media = sqlx::query_as("SELECT * FROM media WHERE id=? AND ready=1")
            .bind(&input.media_id)
            .fetch_one(&self.0.db)
            .await?;
        let mut file = tokio::fs::File::open(self.media_file(&media.id).await?)
            .await
            .map_err(|_| {
                anyhow::anyhow!("Stored video is missing. Restore the media file before retrying.")
            })?;
        if file.metadata().await?.len() != media.size as u64 {
            bail!("Stored media size changed; cannot upload");
        }
        let session = if let Some(value) = session {
            self.0.vault.open_secret(&value)?
        } else {
            let _guard = self.0.mutation.lock().await;
            if (input.privacy != "private" || input.settings.publish_at.is_some())
                && self.setting("private_only").await?.as_deref() != Some("false")
            {
                bail!("Private-only mode is enabled. This non-private upload was not started.");
            }
            self.check_publish_access().await?;
            let token = self.access_token(&channel).await?;
            input.settings.validate(&input.privacy, true)?;
            let metadata = input.youtube_metadata()?;
            let parts = metadata
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(",");
            let response = self
                .0
                .client
                .post(&self.0.endpoints.upload)
                .query(&[
                    ("uploadType", "resumable"),
                    ("part", parts.as_str()),
                    (
                        "notifySubscribers",
                        if input.notify_subscribers {
                            "true"
                        } else {
                            "false"
                        },
                    ),
                ])
                .bearer_auth(token)
                .header("X-Upload-Content-Length", media.size)
                .header("X-Upload-Content-Type", &media.mime)
                .json(&metadata)
                .send()
                .await
                .map_err(|_| {
                    anyhow::anyhow!("Could not start YouTube upload. Check your network and retry.")
                })?;
            if !response.status().is_success() {
                if response.status().as_u16() == 401 {
                    *self.0.access_token.lock().await = None;
                }
                return Err(provider_error(response.status().as_u16()));
            }
            let session = response
                .headers()
                .get("location")
                .and_then(|h| h.to_str().ok())
                .ok_or_else(|| anyhow::anyhow!("YouTube did not provide a resumable upload URL"))?
                .to_owned();
            validate_session(&session, &self.0.endpoints.upload)?;
            // Persist before sending any media, so a lost response never creates a second video.
            let mut tx = self.0.db.begin().await?;
            sqlx::query("UPDATE publications SET session=?,status='uploading',error=NULL,revision=revision+1 WHERE id=?")
                .bind(self.0.vault.seal(&session)?).bind(id).execute(&mut *tx).await?;
            Self::publish_event_tx(&mut tx, id).await?;
            tx.commit().await?;
            session
        };
        validate_session(&session, &self.0.endpoints.upload)?;
        let mut offset;
        {
            let _guard = self.0.mutation.lock().await;
            self.check_publish_access().await?;
            let token = self.access_token(&channel).await?;
            let response = self
                .0
                .client
                .put(&session)
                .bearer_auth(token)
                .header("Content-Length", 0)
                .header("Content-Range", format!("bytes */{}", media.size))
                .send()
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "Could not check upload progress. Retry to resume the same upload."
                    )
                })?;
            if response.status().is_success() {
                return self.complete(id, response).await;
            }
            if response.status().as_u16() != 308 {
                if response.status().as_u16() == 401 {
                    *self.0.access_token.lock().await = None;
                }
                return Err(provider_error(response.status().as_u16()));
            }
            offset = next_offset(response.headers(), media.size)?;
        }
        while offset < media.size {
            if self.0.shutdown.is_cancelled() {
                return Ok(());
            }
            let _guard = self.0.mutation.lock().await;
            // Recheck account/policy before every chunk; disconnect does not leave a cached token running.
            if (input.privacy != "private" || input.settings.publish_at.is_some())
                && self.setting("private_only").await?.as_deref() != Some("false")
            {
                bail!("Private-only mode is enabled; this upload is paused");
            }
            self.check_publish_access().await?;
            let token = self.access_token(&channel).await?;
            let length = CHUNK.min((media.size - offset) as usize);
            let mut bytes = vec![0; length];
            file.seek(std::io::SeekFrom::Start(offset as u64)).await?;
            file.read_exact(&mut bytes).await?;
            let response = self.0.client.put(&session).bearer_auth(token).header("Content-Type", &media.mime)
                .header("Content-Range",format!("bytes {}-{}/{}",offset,offset+length as i64-1,media.size)).body(bytes).send().await
                .map_err(|_|anyhow::anyhow!("Upload interrupted. Retry to ask YouTube for the saved offset and resume; do not create a new upload."))?;
            if response.status().is_success() {
                return self.complete(id, response).await;
            }
            if response.status().as_u16() != 308 {
                if response.status().as_u16() == 401 {
                    *self.0.access_token.lock().await = None;
                }
                return Err(provider_error(response.status().as_u16()));
            }
            let next = next_offset(response.headers(), media.size)?;
            if next <= offset || next > offset + length as i64 {
                bail!(
                    "YouTube returned an unexpected upload offset. Retry to check the saved session."
                );
            }
            offset = next;
            let mut tx = self.0.db.begin().await?;
            sqlx::query("UPDATE publications SET uploaded_bytes=?,status='uploading',revision=revision+1 WHERE id=?").bind(offset).bind(id).execute(&mut *tx).await?;
            Self::publish_event_tx(&mut tx, id).await?;
            tx.commit().await?;
        }
        bail!(
            "YouTube has all bytes but has not confirmed completion. Retry to check the same session."
        )
    }
    async fn complete(&self, id: &str, response: reqwest::Response) -> Result<()> {
        let result: Value = response.json().await.map_err(|_| {
            anyhow::anyhow!(
                "YouTube's completion response was unreadable. Retry to check the same session."
            )
        })?;
        let video = result["id"]
            .as_str()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 64
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "YouTube did not confirm a video ID. Retry to check the same session."
                )
            })?;
        let mut tx = self.0.db.begin().await?;
        sqlx::query("UPDATE publications SET status='uploaded',uploaded_bytes=total_bytes,video_id=?,video_url=?,error=NULL,revision=revision+1 WHERE id=?")
            .bind(video).bind(format!("https://www.youtube.com/watch?v={video}")).bind(id).execute(&mut *tx).await?;
        Self::publish_event_tx(&mut tx, id).await?;
        tx.commit().await?;
        Ok(())
    }
}
fn provider_error(status: u16) -> anyhow::Error {
    anyhow::anyhow!(match status {
        401 => "YouTube authorization expired. Reconnect the original account, then retry.",
        403 =>
            "YouTube denied the upload. Check the project's enabled API, quota, channel permissions and account restrictions, then retry.",
        404 | 410 =>
            "YouTube's upload session expired. Automatic recreation is disabled to avoid duplicates. Check YouTube Studio before starting a new request.",
        429 => "YouTube rate limit reached. Wait before retrying this upload.",
        500..=599 => "YouTube is temporarily unavailable. Retry to resume the same upload.",
        _ => "YouTube rejected the upload. Check video format and metadata before retrying.",
    })
}
pub(super) fn validate_session(value: &str, allowed_endpoint: &str) -> Result<()> {
    let allowed = reqwest::Url::parse(allowed_endpoint)?;
    let url = reqwest::Url::parse(value)?;
    if url.origin() != allowed.origin()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != allowed.path()
    {
        bail!("Unexpected YouTube upload endpoint");
    }
    Ok(())
}
pub(super) fn next_offset(headers: &reqwest::header::HeaderMap, size: i64) -> Result<i64> {
    let Some(range) = headers.get("range") else {
        return Ok(0);
    };
    let end = range
        .to_str()?
        .strip_prefix("bytes=0-")
        .ok_or_else(|| anyhow::anyhow!("Invalid YouTube upload range"))?
        .parse::<i64>()?;
    if end < 0 || end >= size {
        bail!("YouTube upload range is out of bounds");
    }
    Ok(end + 1)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offsets_and_session_urls_are_validated() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(next_offset(&headers, 100).unwrap(), 0);
        headers.insert("range", "bytes=0-24".parse().unwrap());
        assert_eq!(next_offset(&headers, 100).unwrap(), 25);
        headers.insert("range", "bytes=0-100".parse().unwrap());
        assert!(next_offset(&headers, 100).is_err());
        assert!(
            validate_session(
                "https://www.googleapis.com/upload/youtube/v3/videos?upload_id=x",
                "https://www.googleapis.com/upload/youtube/v3/videos"
            )
            .is_ok()
        );
        for bad in [
            "http://www.googleapis.com/upload/youtube/",
            "https://evil.test/upload/youtube/",
            "https://www.googleapis.com.evil.test/upload/youtube/",
        ] {
            assert!(
                validate_session(bad, "https://www.googleapis.com/upload/youtube/v3/videos")
                    .is_err()
            );
        }
    }
}
