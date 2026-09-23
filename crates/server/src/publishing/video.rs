use super::*;

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VisibilityInput {
    /// Stable UUID for this visibility operation. Reuse exactly on retries.
    pub request_id: String,
    /// private, unlisted, or public.
    pub privacy: String,
    /// When retiring an original, the corrected publication ID. Requires privacy=private;
    /// the replacement must be processed and public before the original can be hidden.
    pub replacement_id: Option<String>,
}
#[derive(Debug, Serialize, FromRow, schemars::JsonSchema)]
pub struct VideoOperation {
    pub action: String,
    pub provider_attempted: bool,
    pub request_id: String,
    pub publication_id: String,
    pub privacy: String,
    pub replacement_id: Option<String>,
    pub status: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
}
impl Publisher {
    async fn video_target(&self, id: &str) -> Result<(String, String)> {
        let (channel, video): (String, Option<String>) = sqlx::query_as(
            "SELECT channel_id,video_id FROM publications WHERE id=? AND status='uploaded' AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.0.db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Publication is not a completed upload"))?;
        Ok((
            channel,
            video.ok_or_else(|| anyhow::anyhow!("Publication has no YouTube video ID"))?,
        ))
    }
    pub(super) async fn owned_video(&self, id: &str) -> Result<(String, Value)> {
        let (token, item) = self.lookup_owned_video(id).await?;
        Ok((
            token,
            item.ok_or_else(|| anyhow::anyhow!("YouTube video not found or inaccessible"))?,
        ))
    }
    async fn lookup_owned_video(&self, id: &str) -> Result<(String, Option<Value>)> {
        let (channel, video) = self.video_target(id).await?;
        let token = self.access_token(&channel).await?;
        let response = self
            .0
            .client
            .get(&self.0.endpoints.videos)
            .timeout(Duration::from_secs(20))
            .query(&[
                ("part", "snippet,status,processingDetails,contentDetails,localizations,recordingDetails,paidProductPlacementDetails"),
                ("id", video.as_str()),
            ])
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("Could not read YouTube video; retry the status check"))?;
        if response.status().as_u16() == 401 {
            *self.0.access_token.lock().await = None;
        }
        if !response.status().is_success() {
            bail!("YouTube video lookup failed; check channel authorization and retry");
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| anyhow::anyhow!("Invalid YouTube status response"))?;
        let items = body["items"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid YouTube video list"))?;
        let Some(item) = items.iter().find(|v| v["id"] == video) else {
            if items.is_empty() {
                return Ok((token, None));
            }
            bail!("YouTube returned an unexpected video; absence cannot be verified");
        };
        if item["snippet"]["channelId"] != channel {
            bail!("Video does not belong to the publication's channel");
        }
        Ok((token, Some(item.clone())))
    }
    pub async fn youtube_video_status(&self, id: &str) -> Result<Value> {
        let publication = self.publication(id).await?;
        if let Some(deleted_at) = publication.deleted_at {
            return Ok(
                json!({"publication_id":id,"deleted":true,"deleted_at":deleted_at,"ready":false}),
            );
        }
        let (_, item) = self.owned_video(id).await?;
        Ok(video_projection(id, &item))
    }
    pub async fn video_operations(&self, id: &str) -> Result<Vec<VideoOperation>> {
        self.publication(id).await?;
        Ok(sqlx::query_as(
            "SELECT * FROM video_operations WHERE publication_id=? ORDER BY rowid DESC LIMIT 100",
        )
        .bind(id)
        .fetch_all(&self.0.db)
        .await?)
    }
    pub async fn video_operation(&self, id: &str) -> Result<VideoOperation> {
        Ok(
            sqlx::query_as("SELECT * FROM video_operations WHERE request_id=?")
                .bind(id)
                .fetch_one(&self.0.db)
                .await?,
        )
    }
    pub async fn set_video_visibility(
        &self,
        id: &str,
        input: VisibilityInput,
    ) -> Result<VideoOperation> {
        if Uuid::parse_str(&input.request_id).is_err()
            || !["private", "unlisted", "public"].contains(&input.privacy.as_str())
        {
            bail!("Provide a UUID request_id and private, unlisted, or public privacy");
        }
        if let Some(replacement) = &input.replacement_id
            && (replacement == id
                || input.privacy != "private"
                || Uuid::parse_str(replacement).is_err())
        {
            bail!(
                "A replacement must be a different publication; retiring an original requires private visibility"
            );
        }
        let _guard = self.0.mutation.lock().await;
        self.check_publish_access().await?;
        if input.privacy != "private"
            && self.setting("private_only").await?.as_deref() != Some("false")
        {
            bail!("The owner's private-only policy prevents this visibility change");
        }
        self.check_pending_settings(id).await?;
        let (channel, _) = self.video_target(id).await?;
        if self.setting("youtube_manage_channel").await?.as_deref() != Some(channel.as_str()) {
            bail!(
                "Video-management permission required: authorize YouTube again in AgentWay, then retry. Existing upload access and agent token are unchanged"
            );
        }
        let existing: Option<VideoOperation> =
            sqlx::query_as("SELECT * FROM video_operations WHERE request_id=?")
                .bind(&input.request_id)
                .fetch_optional(&self.0.db)
                .await?;
        if let Some(existing) = existing {
            if existing.action != "visibility"
                || existing.publication_id != id
                || existing.privacy != input.privacy
                || existing.replacement_id != input.replacement_id
            {
                bail!("request_id belongs to a different visibility operation");
            }
            if existing.status == "completed" {
                return Ok(existing);
            }
            let newer: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM video_operations WHERE publication_id=? AND rowid>(SELECT rowid FROM video_operations WHERE request_id=?))")
                .bind(id).bind(&input.request_id).fetch_one(&self.0.db).await?;
            if newer {
                bail!(
                    "A newer visibility operation superseded this request; inspect current video status"
                );
            }
        } else {
            if let Some(replacement) = &input.replacement_id {
                self.video_target(replacement).await?;
            }
            sqlx::query("INSERT INTO video_operations(request_id,publication_id,privacy,replacement_id) VALUES(?,?,?,?)")
                .bind(&input.request_id).bind(id).bind(&input.privacy).bind(&input.replacement_id).execute(&self.0.db).await?;
        }
        // Intent is durable before any provider mutation. Retrying reads YouTube first,
        // reconciling a successful update whose response was lost rather than uploading again.
        let outcome = self.apply_visibility(id, &input).await;
        let (status, result, error) = match outcome {
            Ok(value) => ("completed", Some(value.to_string()), None),
            Err(error) => ("interrupted", None, Some(error.to_string())),
        };
        let mut tx = self.0.db.begin().await?;
        sqlx::query("UPDATE video_operations SET status=?,result=?,error=? WHERE request_id=?")
            .bind(status)
            .bind(result)
            .bind(error)
            .bind(&input.request_id)
            .execute(&mut *tx)
            .await?;
        let op: VideoOperation =
            sqlx::query_as("SELECT * FROM video_operations WHERE request_id=?")
                .bind(&input.request_id)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query("INSERT INTO events(kind,payload) VALUES('video.operation',?)")
            .bind(serde_json::to_string(&op)?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(op)
    }
    async fn apply_visibility(&self, id: &str, input: &VisibilityInput) -> Result<Value> {
        if let Some(replacement) = &input.replacement_id {
            let (_, item) = self.owned_video(replacement).await?;
            if !processed(&item) || item["status"]["privacyStatus"] != "public" {
                bail!("Original kept unchanged: replacement must be processed and verified public");
            }
        }
        let (token, item) = self.owned_video(id).await?;
        if input.privacy != "private" && !processed(&item) {
            bail!("Video is not verified processed; retry after processing succeeds");
        }
        if item["status"]["privacyStatus"] == input.privacy {
            return Ok(video_projection(id, &item));
        }
        if item["status"].get("publishAt").is_some() {
            bail!(
                "Scheduled video visibility changes are not supported; schedule would need explicit handling"
            );
        }
        // YouTube updates whole parts. Preserve every documented writable status field.
        let mut status = serde_json::Map::new();
        for key in [
            "embeddable",
            "license",
            "publicStatsViewable",
            "selfDeclaredMadeForKids",
            "containsSyntheticMedia",
        ] {
            if let Some(value) = item["status"].get(key) {
                status.insert(key.into(), value.clone());
            }
        }
        status.insert("privacyStatus".into(), json!(input.privacy));
        let response = self
            .0
            .client
            .put(&self.0.endpoints.videos)
            .timeout(Duration::from_secs(20))
            .query(&[("part", "status")])
            .bearer_auth(token)
            .json(&json!({"id":item["id"],"status":status}))
            .send()
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "YouTube update outcome is uncertain; retry this same request_id to reconcile"
                )
            })?;
        if response.status().as_u16() == 401 {
            *self.0.access_token.lock().await = None;
        }
        if response.status().as_u16() == 403 {
            bail!(
                "YouTube denied editing this video. Authorize video-management permission in AgentWay's YouTube connection; then retry this request_id. Quota or channel restrictions can also cause this response"
            );
        }
        if !response.status().is_success() {
            bail!("YouTube visibility update failed; retry the same request_id");
        }
        let (_, current) = self.owned_video(id).await?;
        if current["status"]["privacyStatus"] != input.privacy {
            bail!("YouTube visibility is not yet verified; retry this same request_id");
        }
        Ok(video_projection(id, &current))
    }
}
fn processed(item: &Value) -> bool {
    item["processingDetails"]["processingStatus"] == "succeeded"
        && item["status"]["uploadStatus"] == "processed"
}
fn video_projection(id: &str, item: &Value) -> Value {
    json!({"etag":item["etag"],"snippet":item["snippet"],"status":item["status"],"localizations":item["localizations"],"recordingDetails":item["recordingDetails"],"paidProductPlacementDetails":item["paidProductPlacementDetails"],"publication_id":id,"video_id":item["id"],"actual_privacy":item["status"]["privacyStatus"],
        "upload_status":item["status"]["uploadStatus"],"processing_status":item["processingDetails"]["processingStatus"],
        "processing_failure_reason":item["processingDetails"]["processingFailureReason"],
        "rejection_reason":item["status"]["rejectionReason"],"failure_reason":item["status"]["failureReason"],
        "duration":item["contentDetails"]["duration"],"ready":processed(item)})
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteInput {
    /// Stable UUID for this deletion. Reuse with identical arguments on retries.
    pub request_id: String,
    /// Corrected AgentWay publication. Must be distinct, processed, public, and in the same channel.
    pub replacement_id: String,
    /// Must be true only when the user has authorized permanent deletion of this original.
    pub confirm_delete: bool,
}
impl Publisher {
    pub async fn delete_replaced_video(
        &self,
        id: &str,
        input: DeleteInput,
    ) -> Result<VideoOperation> {
        if !input.confirm_delete
            || Uuid::parse_str(&input.request_id).is_err()
            || Uuid::parse_str(&input.replacement_id).is_err()
            || input.replacement_id == id
        {
            bail!(
                "Provide a UUID request_id, a different replacement_id, and confirm_delete=true for user-authorized permanent cleanup"
            );
        }
        let _guard = self.0.mutation.lock().await;
        self.check_publish_access().await?;
        let existing: Option<VideoOperation> =
            sqlx::query_as("SELECT * FROM video_operations WHERE request_id=?")
                .bind(&input.request_id)
                .fetch_optional(&self.0.db)
                .await?;
        if let Some(existing) = &existing {
            if existing.action != "delete"
                || existing.publication_id != id
                || existing.replacement_id.as_deref() != Some(input.replacement_id.as_str())
            {
                bail!("request_id belongs to a different operation");
            }
            if existing.status == "completed" {
                return self.video_operation(&input.request_id).await;
            }
            let newer: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM video_operations WHERE publication_id=? AND rowid>(SELECT rowid FROM video_operations WHERE request_id=?))")
                .bind(id).bind(&input.request_id).fetch_one(&self.0.db).await?;
            if newer {
                bail!(
                    "A newer operation superseded this deletion; inspect current status before creating a new request"
                );
            }
        }
        self.check_pending_settings(id).await?;
        let (channel, _) = self.video_target(id).await?;
        if self.setting("youtube_manage_channel").await?.as_deref() != Some(channel.as_str()) {
            bail!("Video-management permission is required; authorize YouTube in AgentWay");
        }
        let (replacement_channel, replacement_video) =
            self.video_target(&input.replacement_id).await?;
        let (_, original_video) = self.video_target(id).await?;
        if replacement_channel != channel || replacement_video == original_video {
            bail!("Replacement must be a different video in the same channel");
        }
        if existing.is_none() {
            sqlx::query("INSERT INTO video_operations(request_id,publication_id,privacy,replacement_id,action) VALUES(?,?,'private',?,'delete')")
                .bind(&input.request_id).bind(id).bind(&input.replacement_id).execute(&self.0.db).await?;
        }
        let outcome = self.apply_deletion(id, &input).await;
        let (status, result, error) = match outcome {
            Ok(result) => ("completed", Some(result.to_string()), None),
            Err(error) => ("interrupted", None, Some(error.to_string())),
        };
        let mut tx = self.0.db.begin().await?;
        if status == "completed" {
            sqlx::query("UPDATE publications SET deleted_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),revision=revision+1 WHERE id=?")
                .bind(id).execute(&mut *tx).await?;
            let publication: Publication = sqlx::query_as("SELECT * FROM publications WHERE id=?")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO events(kind,payload) VALUES('publication.upsert',?)")
                .bind(serde_json::to_string(&publication)?)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("UPDATE video_operations SET status=?,result=?,error=? WHERE request_id=?")
            .bind(status)
            .bind(result)
            .bind(error)
            .bind(&input.request_id)
            .execute(&mut *tx)
            .await?;
        let operation: VideoOperation =
            sqlx::query_as("SELECT * FROM video_operations WHERE request_id=?")
                .bind(&input.request_id)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query("INSERT INTO events(kind,payload) VALUES('video.operation',?)")
            .bind(serde_json::to_string(&operation)?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(operation)
    }
    async fn apply_deletion(&self, id: &str, input: &DeleteInput) -> Result<Value> {
        let (_, replacement) = self.owned_video(&input.replacement_id).await?;
        if !processed(&replacement) || replacement["status"]["privacyStatus"] != "public" {
            bail!("Original not deleted: replacement must be processed and public");
        }
        let (token, original) = self.lookup_owned_video(id).await?;
        let attempted = self
            .video_operation(&input.request_id)
            .await?
            .provider_attempted;
        let Some(original) = original else {
            if attempted {
                return Ok(
                    json!({"publication_id":id,"replacement_id":input.replacement_id,"deleted":true,"verification":"absent_after_delete_attempt"}),
                );
            }
            bail!(
                "Original is missing or inaccessible; no deletion was attempted and success cannot be claimed"
            );
        };
        if original["status"]["privacyStatus"] != "private" {
            bail!("Make the original private with its replacement_id before permanent deletion");
        }
        // Persist the authorized, ownership-checked attempt before sending the irreversible request.
        sqlx::query("UPDATE video_operations SET provider_attempted=1 WHERE request_id=?")
            .bind(&input.request_id)
            .execute(&self.0.db)
            .await?;
        let response = self
            .0
            .client
            .delete(&self.0.endpoints.videos)
            .timeout(Duration::from_secs(20))
            .query(&[("id", original["id"].as_str().unwrap_or_default())])
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| {
                anyhow::anyhow!("Delete outcome uncertain; retry this same request_id to reconcile")
            })?;
        if response.status().as_u16() == 401 {
            *self.0.access_token.lock().await = None;
        }
        if response.status().as_u16() != 204 {
            bail!(
                "YouTube did not confirm deletion; retry the same request_id. Check authorization or quota if this persists"
            );
        }
        Ok(
            json!({"publication_id":id,"replacement_id":input.replacement_id,"deleted":true,"verification":"youtube_204"}),
        )
    }
}
