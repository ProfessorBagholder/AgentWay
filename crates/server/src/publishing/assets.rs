use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetAction {
    SetThumbnail,
    SetCaption,
    DeleteCaption,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VideoAssetInput {
    pub request_id: String,
    pub publication_id: String,
    pub action: AssetAction,
    /// Ready media owned by this connection. PNG/JPEG thumbnail or UTF-8 timed SRT/WebVTT captions, at most 2 MiB.
    pub media_id: Option<String>,
    /// Existing caption ID for replacement/deletion. Omit only when creating a new track.
    pub caption_id: Option<String>,
    /// BCP-47 language, required when creating a caption track.
    pub language: Option<String>,
    /// Caption track name, at most 150 characters; required on creation (empty allowed).
    pub name: Option<String>,
    /// Explicit draft/public choice for a caption track.
    pub is_draft: Option<bool>,
}
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CategoryQuery {
    /// Two-letter ISO country code, e.g. CA or US.
    pub region_code: String,
}
impl Publisher {
    pub async fn youtube_categories(&self, input: CategoryQuery) -> Result<Value> {
        if input.region_code.len() != 2
            || !input.region_code.bytes().all(|b| b.is_ascii_uppercase())
        {
            bail!("region_code must be two uppercase ISO country letters");
        }
        let channel = self.connected_channel_id().await?;
        let token = self.access_token(&channel).await?;
        let response = self
            .0
            .client
            .get(&self.0.endpoints.categories)
            .bearer_auth(token)
            .query(&[
                ("part", "snippet"),
                ("regionCode", input.region_code.as_str()),
            ])
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("YouTube category lookup timed out"))?;
        if !response.status().is_success() {
            bail!(
                "YouTube category lookup returned HTTP {}",
                response.status().as_u16()
            );
        }
        let value: Value = response
            .json()
            .await
            .map_err(|_| anyhow::anyhow!("Invalid YouTube category response"))?;
        let items = value["items"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing YouTube categories"))?;
        Ok(
            json!({"categories":items.iter().filter(|i|i["snippet"]["assignable"]==true).map(|i|json!({"id":i["id"],"title":i["snippet"]["title"]})).collect::<Vec<_>>()}),
        )
    }
    pub async fn youtube_captions(&self, id: &str) -> Result<Value> {
        let (token, item) = self.owned_video(id).await?;
        self.caption_resources(&token, item["id"].as_str().unwrap_or(""))
            .await
    }
    async fn caption_resources(&self, token: &str, video: &str) -> Result<Value> {
        let response = self
            .0
            .client
            .get(&self.0.endpoints.captions)
            .bearer_auth(token)
            .query(&[("part", "snippet"), ("videoId", video)])
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("YouTube caption lookup failed"))?;
        if !response.status().is_success() {
            bail!(
                "YouTube caption lookup returned HTTP {}",
                response.status().as_u16()
            );
        }
        let mut body: Value = response
            .json()
            .await
            .map_err(|_| anyhow::anyhow!("Invalid caption response"))?;
        match body.get("items") {
            None if body.is_object() => body["items"] = json!([]),
            Some(Value::Array(_)) => {}
            _ => bail!("Invalid YouTube caption list; no mutation is safe"),
        }
        Ok(body)
    }
    pub async fn video_asset_operation(&self, id: &str) -> Result<Value> {
        let (owner, result): (String, String) = sqlx::query_as(
            "SELECT agent_id,result FROM youtube_asset_operations WHERE request_id=?",
        )
        .bind(id)
        .fetch_one(&self.0.db)
        .await?;
        if self.1.is_some() && owner != self.connection_id() {
            bail!("Operation belongs to another connection");
        }
        Ok(serde_json::from_str(&result)?)
    }
    pub async fn manage_video_asset(&self, input: VideoAssetInput) -> Result<Value> {
        if Uuid::parse_str(&input.request_id).is_err() {
            bail!("request_id must be a UUID");
        }
        let _guard = self.0.mutation.lock().await;
        self.check_publish_access().await?;
        let encoded = serde_json::to_string(&input)?;
        let saved = sqlx::query_scalar::<_, String>(
            "SELECT input FROM youtube_asset_operations WHERE request_id=?",
        )
        .bind(&input.request_id)
        .fetch_optional(&self.0.db)
        .await?;
        let continuing = saved.is_some();
        if let Some(saved) = saved {
            if saved != encoded {
                bail!("request_id belongs to another operation");
            }
            let result = self.video_asset_operation(&input.request_id).await?;
            if result["status"] != "upload_pending" {
                return Ok(result);
            }
        }
        let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM youtube_asset_operations WHERE publication_id=? AND request_id<>? AND json_extract(result,'$.status') IN ('outcome_unknown','upload_pending'))").bind(&input.publication_id).bind(&input.request_id).fetch_one(&self.0.db).await?;
        if pending {
            bail!(
                "An earlier asset write is unresolved; inspect its request_id and YouTube state before another asset mutation"
            );
        }
        let (token, item) = self.owned_video(&input.publication_id).await?;
        let channel = item["snippet"]["channelId"].as_str().unwrap_or("");
        if self.setting("youtube_manage_channel").await?.as_deref() != Some(channel) {
            bail!("YouTube management permission required");
        }
        let video = item["id"].as_str().unwrap_or("");
        let mut bytes = Vec::new();
        let mut mime = String::new();
        if !matches!(input.action, AssetAction::DeleteCaption) {
            let id = input
                .media_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("media_id required"))?;
            self.check_media_owner(id).await?;
            let media: Media = sqlx::query_as("SELECT * FROM media WHERE id=? AND ready=1")
                .bind(id)
                .fetch_one(&self.0.db)
                .await?;
            if media.size > 2 * 1024 * 1024 {
                bail!("Thumbnail/caption maximum is 2 MiB");
            }
            mime = media.mime;
            bytes = tokio::fs::read(self.media_file(id).await?).await?;
            if bytes.len() != media.size as usize {
                bail!("Media length changed");
            }
        }
        let mut request = match input.action {
            AssetAction::SetThumbnail => {
                if input.caption_id.is_some()
                    || input.language.is_some()
                    || input.name.is_some()
                    || input.is_draft.is_some()
                {
                    bail!("Caption fields are invalid for thumbnails");
                }
                if !["image/png", "image/jpeg"].contains(&mime.as_str()) {
                    bail!("Thumbnail requires PNG or JPEG");
                }
                let check = bytes.clone();
                tokio::task::spawn_blocking(move || {
                    image::ImageReader::new(std::io::Cursor::new(check))
                        .with_guessed_format()?
                        .into_dimensions()
                })
                .await??;
                self.0
                    .client
                    .post(&self.0.endpoints.thumbnails)
                    .query(&[("videoId", video), ("uploadType", "resumable")])
                    // No metadata body: Google still requires an explicit length.
                    .header(reqwest::header::CONTENT_LENGTH, 0)
                    .body(Vec::<u8>::new())
            }
            AssetAction::SetCaption => {
                if !["text/vtt", "application/x-subrip"].contains(&mime.as_str()) {
                    bail!("Caption requires timed UTF-8 SRT or WebVTT");
                }
                let content = std::str::from_utf8(&bytes)
                    .map_err(|_| anyhow::anyhow!("Captions must be UTF-8"))?;
                if !content.contains("-->") {
                    bail!(
                        "Captions must include time codes; automatic transcript synchronization is unsupported"
                    );
                }
                let draft = input
                    .is_draft
                    .ok_or_else(|| anyhow::anyhow!("is_draft must be explicit"))?;
                let metadata = if let Some(id) = &input.caption_id {
                    if input.language.is_some() || input.name.is_some() {
                        bail!("Caption replacement preserves language/name; omit those fields");
                    }
                    let tracks = self.caption_resources(&token, video).await?;
                    if !continuing
                        && !tracks["items"]
                            .as_array()
                            .is_some_and(|items| items.iter().any(|i| i["id"] == *id))
                    {
                        bail!("Caption does not belong to this video");
                    }
                    json!({"id":id,"snippet":{"isDraft":draft}})
                } else {
                    let lang = input
                        .language
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("Caption language required"))?;
                    lang.parse::<language_tags::LanguageTag>()
                        .map_err(|_| anyhow::anyhow!("Invalid caption language"))?;
                    let name = input
                        .name
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("Caption name required; empty allowed"))?;
                    if name.chars().count() > 150 {
                        bail!("Caption name maximum is 150 characters");
                    }
                    let tracks = self.caption_resources(&token, video).await?;
                    if !continuing
                        && tracks["items"].as_array().is_some_and(|items| {
                            items.iter().any(|i| {
                                i["snippet"]["language"] == lang && i["snippet"]["name"] == name
                            })
                        })
                    {
                        bail!("Caption track already exists; use its caption_id to replace it");
                    }
                    json!({"snippet":{"videoId":video,"language":lang,"name":name,"isDraft":draft}})
                };
                let method = if input.caption_id.is_some() {
                    reqwest::Method::PUT
                } else {
                    reqwest::Method::POST
                };
                self.0
                    .client
                    .request(method, &self.0.endpoints.caption_upload)
                    .query(&[("part", "snippet"), ("uploadType", "resumable")])
                    .json(&metadata)
            }
            AssetAction::DeleteCaption => {
                if input.media_id.is_some()
                    || input.language.is_some()
                    || input.name.is_some()
                    || input.is_draft.is_some()
                {
                    bail!("Caption deletion accepts only caption_id");
                }
                let id = input
                    .caption_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("caption_id required"))?;
                let tracks = self.caption_resources(&token, video).await?;
                if !tracks["items"]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|i| i["id"] == id))
                {
                    bail!("Caption not found in this video; deletion not attempted");
                }
                self.0
                    .client
                    .delete(&self.0.endpoints.captions)
                    .query(&[("id", id)])
            }
        };
        let mut unknown = json!({"request_id":input.request_id,"publication_id":input.publication_id,"status":"outcome_unknown","next_action":"Do not submit another write. Read the video's thumbnail or caption tracks to inspect the provider outcome."});
        if !matches!(input.action, AssetAction::DeleteCaption) {
            unknown["status"] = json!("upload_pending");
        }
        if !continuing {
            sqlx::query("INSERT INTO youtube_asset_operations(request_id,publication_id,agent_id,input,result) VALUES(?,?,?,?,?)").bind(&input.request_id).bind(&input.publication_id).bind(self.connection_id()).bind(encoded).bind(unknown.to_string()).execute(&self.0.db).await?;
        }
        if !matches!(input.action, AssetAction::DeleteCaption) {
            let endpoint = if matches!(input.action, AssetAction::SetThumbnail) {
                &self.0.endpoints.thumbnails
            } else {
                &self.0.endpoints.caption_upload
            };
            return self
                .upload_video_asset(
                    &input,
                    AssetUpload {
                        initialize: request,
                        endpoint: endpoint.clone(),
                        bytes,
                        mime,
                    },
                    &token,
                )
                .await;
        }
        request = request.bearer_auth(token).timeout(Duration::from_secs(30));
        let result = match request.send().await {
            Ok(response) => {
                let status = response.status();
                if status.as_u16() == 401 {
                    *self.0.access_token.lock().await = None;
                }
                if status.is_success() {
                    let resource = if status.as_u16() == 204 {
                        json!({"deleted":true})
                    } else {
                        response.json::<Value>().await.unwrap_or(Value::Null)
                    };
                    if resource.is_null() {
                        unknown
                    } else {
                        json!({"request_id":input.request_id,"publication_id":input.publication_id,"status":"accepted","resource":resource,"next_action":"For captions, list tracks until snippet.status is serving. Accepted is not a content-quality check."})
                    }
                } else {
                    json!({"request_id":input.request_id,"publication_id":input.publication_id,"status":if status.is_client_error(){"rejected"}else{"outcome_unknown"},"error":settings::provider_error("asset operation", response).await})
                }
            }
            Err(_) => unknown,
        };
        sqlx::query("UPDATE youtube_asset_operations SET result=? WHERE request_id=?")
            .bind(result.to_string())
            .bind(&input.request_id)
            .execute(&self.0.db)
            .await?;
        Ok(result)
    }
}

struct AssetUpload {
    initialize: reqwest::RequestBuilder,
    endpoint: String,
    bytes: Vec<u8>,
    mime: String,
}
impl Publisher {
    async fn save_asset_result(&self, input: &VideoAssetInput, mut result: Value) -> Result<Value> {
        result["request_id"] = json!(input.request_id);
        result["publication_id"] = json!(input.publication_id);
        sqlx::query("UPDATE youtube_asset_operations SET result=? WHERE request_id=?")
            .bind(result.to_string())
            .bind(&input.request_id)
            .execute(&self.0.db)
            .await?;
        Ok(result)
    }
    async fn upload_video_asset(
        &self,
        input: &VideoAssetInput,
        upload: AssetUpload,
        token: &str,
    ) -> Result<Value> {
        let mut pending = json!({"status":"upload_pending","retry_after_seconds":5,"next_action":"Retry this SAME request_id and identical arguments to query/resume its saved upload session."});
        let size = upload.bytes.len() as i64;
        let saved: Option<String> = sqlx::query_scalar(
            "SELECT upload_session FROM youtube_asset_operations WHERE request_id=?",
        )
        .bind(&input.request_id)
        .fetch_one(&self.0.db)
        .await?;
        let session = if let Some(saved) = saved {
            self.0.vault.open_secret(&saved)?
        } else {
            pending["phase"] = json!("initialize");
            let response = upload
                .initialize
                .bearer_auth(token)
                .header("X-Upload-Content-Length", size)
                .header("X-Upload-Content-Type", &upload.mime)
                .timeout(Duration::from_secs(30))
                .send()
                .await;
            let response = match response {
                Ok(v) => v,
                Err(_) => {
                    pending["error"] = json!("provider_transport_error");
                    return self.save_asset_result(input, pending).await;
                }
            };
            if !response.status().is_success() {
                return self.asset_upload_response(input, pending, response).await;
            }
            let Some(session) = response
                .headers()
                .get("location")
                .and_then(|h| h.to_str().ok())
            else {
                pending["error"] = json!("upload_session_missing");
                return self.save_asset_result(input, pending).await;
            };
            super::worker::validate_session(session, &upload.endpoint)?;
            sqlx::query("UPDATE youtube_asset_operations SET upload_session=? WHERE request_id=?")
                .bind(self.0.vault.seal(session)?)
                .bind(&input.request_id)
                .execute(&self.0.db)
                .await?;
            session.to_string()
        };
        super::worker::validate_session(&session, &upload.endpoint)?;
        pending["phase"] = json!("check_session");
        let response = self
            .0
            .client
            .put(&session)
            .bearer_auth(token)
            .header("Content-Length", 0)
            .header("Content-Range", format!("bytes */{size}"))
            .timeout(Duration::from_secs(30))
            .send()
            .await;
        let response = match response {
            Ok(v) => v,
            Err(_) => {
                pending["error"] = json!("provider_transport_error");
                return self.save_asset_result(input, pending).await;
            }
        };
        if response.status().as_u16() != 308 {
            return self.asset_upload_response(input, pending, response).await;
        }
        let offset = super::worker::next_offset(response.headers(), size)?;
        if offset == size {
            return self.save_asset_result(input, pending).await;
        }
        pending["phase"] = json!("transfer");
        let response = self
            .0
            .client
            .put(&session)
            .bearer_auth(token)
            .header("Content-Type", upload.mime)
            .header(
                "Content-Range",
                format!("bytes {offset}-{}/{size}", size - 1),
            )
            .body(upload.bytes[offset as usize..].to_vec())
            .timeout(Duration::from_secs(30))
            .send()
            .await;
        match response {
            Ok(response) => self.asset_upload_response(input, pending, response).await,
            Err(_) => {
                pending["error"] = json!("provider_transport_error");
                self.save_asset_result(input, pending).await
            }
        }
    }
    async fn asset_upload_response(
        &self,
        input: &VideoAssetInput,
        mut pending: Value,
        response: reqwest::Response,
    ) -> Result<Value> {
        let status = response.status();
        if status.as_u16() == 401 {
            *self.0.access_token.lock().await = None;
        }
        if status.is_success() {
            let body = response.json::<Value>().await.unwrap_or(Value::Null);
            let confirmed = match input.action {
                AssetAction::SetCaption => body["id"].is_string(),
                AssetAction::SetThumbnail => {
                    body["items"].as_array().is_some_and(|v| !v.is_empty())
                }
                AssetAction::DeleteCaption => false,
            };
            if confirmed {
                return self.save_asset_result(input,json!({"status":"accepted","resource":body,"next_action":"For captions, poll list_youtube_captions until serving or failed. Accepted is not a content-quality check."})).await;
            }
            pending["error"] = json!("provider_response_unconfirmed");
        } else if status.as_u16() != 308 {
            pending["error"] = json!(settings::provider_error("asset upload", response).await);
            if [404, 410].contains(&status.as_u16()) && pending["phase"] != "initialize" {
                // Expired insert sessions must not be recreated: the resource may already exist.
                pending["status"] = json!("outcome_unknown");
                pending["next_action"] = json!(
                    "Upload session expired. Inspect YouTube state; do not create a duplicate caption or replace artwork blindly."
                );
            } else if status.is_client_error() && ![408, 429, 401].contains(&status.as_u16()) {
                pending["status"] = json!("rejected");
            }
        }
        self.save_asset_result(input, pending).await
    }
}
