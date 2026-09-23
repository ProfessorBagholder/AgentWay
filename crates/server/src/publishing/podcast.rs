use super::*;
use reqwest::Method;

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaylistQuery {
    /// Omit to list this channel's playlists (including podcasts). Supply to list its episodes.
    pub playlist_id: Option<String>,
    /// nextPageToken from the previous response. Each page has at most 50 entries.
    pub page_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct PodcastInput {
    /// Stable UUID for this operation. Reuse identical inputs after interruption.
    pub request_id: String,
    /// Intended connected YouTube channel ID, from youtube_status.
    pub channel_id: String,
    #[serde(flatten)]
    pub operation: PodcastAction,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PodcastAction {
    /// Create a playlist, add full episodes, upload its square cover, then enable podcast status.
    CreatePlaylist {
        title: String,
        description: String,
        /// private, unlisted or public. Explicit user choice.
        privacy: String,
    },
    /// Upload a square PNG/JPEG via create_media_upload and its authenticated PUT first.
    SetCover {
        playlist_id: String,
        media_id: String,
    },
    /// Enable podcast features on a playlist with a cover, preserving its other settings.
    EnablePodcast { playlist_id: String },
    /// Add a full episode to an ordinary or podcast playlist owned by this channel. Does not reupload video bytes.
    AddEpisode {
        playlist_id: String,
        video_id: String,
        /// Must be true: the user selected a full episode, not a promotional short or clip.
        full_episode: bool,
        /// Optional zero-based playlist position. Omit to append; does not reorder existing entries.
        position: Option<u32>,
    },
}

impl Publisher {
    async fn podcast_read(
        &self,
        endpoint: &str,
        query: &[(&str, &str)],
        token: &str,
    ) -> Result<Value> {
        let response = self
            .0
            .client
            .get(endpoint)
            .query(query)
            .bearer_auth(token)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("YouTube playlist read failed; retry the read"))?;
        if response.status().as_u16() == 401 {
            *self.0.access_token.lock().await = None;
        }
        if !response.status().is_success() {
            bail!(
                "YouTube playlist read returned HTTP {}",
                response.status().as_u16()
            );
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| anyhow::anyhow!("Invalid YouTube playlist response"))?;
        normalize_list(body)
    }

    async fn owned_playlist(&self, id: &str, channel: &str, token: &str) -> Result<Value> {
        valid_id(id)?;
        let body = self
            .podcast_read(
                &self.0.endpoints.playlists,
                &[("part", "snippet,status,contentDetails"), ("id", id)],
                token,
            )
            .await?;
        let item = body["items"]
            .as_array()
            .and_then(|v| v.iter().find(|v| v["id"] == id))
            .ok_or_else(|| anyhow::anyhow!("Playlist not found"))?;
        if item["snippet"]["channelId"] != channel {
            bail!("Playlist is not owned by the connected channel");
        }
        Ok(item.clone())
    }

    pub async fn youtube_playlists(&self, input: PlaylistQuery) -> Result<Value> {
        self.check_publish_access().await?;
        let channel = self.connected_channel_id().await?;
        let token = self.access_token(&channel).await?;
        let page = input.page_token.as_deref().unwrap_or("");
        if page.len() > 2048 {
            bail!("Invalid page token");
        }
        if let Some(id) = input.playlist_id {
            let playlist = self.owned_playlist(&id, &channel, &token).await?;
            let mut result = self
                .podcast_read(
                    &self.0.endpoints.playlist_items,
                    &[
                        ("part", "snippet,contentDetails"),
                        ("playlistId", &id),
                        ("maxResults", "50"),
                        ("pageToken", page),
                    ],
                    &token,
                )
                .await?;
            result["cover_images"] = self
                .podcast_read(
                    &self.0.endpoints.playlist_images,
                    &[("part", "snippet"), ("parent", &id), ("maxResults", "50")],
                    &token,
                )
                .await?;
            result["playlist"] = playlist;
            Ok(result)
        } else {
            self.podcast_read(
                &self.0.endpoints.playlists,
                &[
                    ("part", "snippet,status,contentDetails"),
                    ("mine", "true"),
                    ("maxResults", "50"),
                    ("pageToken", page),
                ],
                &token,
            )
            .await
        }
    }

    pub async fn podcast_operation(&self, id: &str) -> Result<Value> {
        self.check_publish_access().await?;
        let channel = self.connected_channel_id().await?;
        let result: String = sqlx::query_scalar(
            "SELECT result FROM podcast_operations WHERE request_id=? AND channel_id=?",
        )
        .bind(id)
        .bind(channel)
        .fetch_optional(&self.0.db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Podcast operation not found"))?;
        Ok(serde_json::from_str(&result)?)
    }

    pub async fn manage_podcast(&self, input: PodcastInput) -> Result<Value> {
        Uuid::parse_str(&input.request_id)
            .map_err(|_| anyhow::anyhow!("request_id must be a UUID"))?;
        let _guard = self.0.mutation.lock().await;
        self.check_publish_access().await?;
        let channel = self.connected_channel_id().await?;
        if channel != input.channel_id {
            bail!("Connected channel changed; check the intended account");
        }
        if self.setting("youtube_manage_channel").await?.as_deref() != Some(&channel) {
            bail!("YouTube management permission required; authorize management in AgentWay");
        }
        let encoded = serde_json::to_string(&input)?;
        let saved: Option<(String, String)> =
            sqlx::query_as("SELECT input,result FROM podcast_operations WHERE request_id=?")
                .bind(&input.request_id)
                .fetch_optional(&self.0.db)
                .await?;
        if let Some((original, result)) = saved {
            if original != encoded {
                bail!("request_id already belongs to another operation; reuse original inputs");
            }
            let result: Value = serde_json::from_str(&result)?;
            // Cover is a desired-state update: resume its saved upload rather than
            // replaying playlist/episode insertions. Old multipart attempts can be
            // recovered by setting the same cover, updating any existing hero image.
            if !matches!(input.operation, PodcastAction::SetCover { .. })
                || !["outcome_unknown", "upload_pending"]
                    .contains(&result["status"].as_str().unwrap_or(""))
            {
                return Ok(result);
            }
        }
        let token = self.access_token(&channel).await?;
        let mut request = match &input.operation {
            PodcastAction::CreatePlaylist {
                title,
                description,
                privacy,
            } => {
                if title.trim().is_empty()
                    || title.chars().count() > 150
                    || description.chars().count() > 5000
                    || title.contains(['<', '>'])
                    || description.contains(['<', '>'])
                {
                    bail!(
                        "Playlist title must be 1–150 characters and description at most 5000, without < or >"
                    );
                }
                if !["private", "unlisted", "public"].contains(&privacy.as_str()) {
                    bail!("Invalid playlist privacy");
                }
                if privacy != "private"
                    && self.setting("private_only").await?.as_deref() != Some("false")
                {
                    bail!(
                        "Owner private-only policy prevents creating a public or unlisted playlist"
                    );
                }
                self.0.client.post(&self.0.endpoints.playlists).query(&[("part", "snippet,status")])
                    .json(&json!({"snippet":{"title":title,"description":description},"status":{"privacyStatus":privacy}}))
            }
            PodcastAction::EnablePodcast { playlist_id } => {
                let item = self.owned_playlist(playlist_id, &channel, &token).await?;
                if item["status"]["podcastStatus"] == "enabled" {
                    return self
                        .save_podcast_result(
                            &input,
                            &encoded,
                            json!({"status":"completed","resource":item,"already_present":true}),
                        )
                        .await;
                }
                let privacy = item["status"]["privacyStatus"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing playlist privacy"))?;
                // playlists.update requires snippet.title even for podcast designation.
                // Echo only the mutable snippet fields, preserving the current values.
                let title = item["snippet"]["title"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing playlist title; update is unsafe"))?;
                let mut snippet = json!({"title":title,"description":item["snippet"]["description"].as_str().unwrap_or("")});
                if let Some(language) = item["snippet"]["defaultLanguage"].as_str() {
                    snippet["defaultLanguage"] = json!(language);
                }
                self.0.client.put(&self.0.endpoints.playlists).query(&[("part", "snippet,status")])
                    .json(&json!({"id":playlist_id,"snippet":snippet,"status":{"privacyStatus":privacy,"podcastStatus":"enabled"}}))
            }
            PodcastAction::AddEpisode {
                playlist_id,
                video_id,
                full_episode,
                position,
            } => {
                if !full_episode {
                    bail!(
                        "Podcast playlists contain full episodes; keep clips and shorts separate"
                    );
                }
                valid_id(video_id)?;
                self.owned_playlist(playlist_id, &channel, &token).await?;
                let videos = self
                    .podcast_read(
                        &self.0.endpoints.videos,
                        &[("part", "snippet"), ("id", video_id)],
                        &token,
                    )
                    .await?;
                if !videos["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v["id"] == *video_id && v["snippet"]["channelId"] == channel)
                {
                    bail!("Episode is not a video owned by the connected channel");
                }
                let existing = self
                    .podcast_read(
                        &self.0.endpoints.playlist_items,
                        &[
                            ("part", "snippet"),
                            ("playlistId", playlist_id),
                            ("videoId", video_id),
                        ],
                        &token,
                    )
                    .await?;
                if let Some(item) = existing["items"].as_array().unwrap().first() {
                    return self
                        .save_podcast_result(
                            &input,
                            &encoded,
                            json!({"status":"completed","resource":item,"already_present":true}),
                        )
                        .await;
                }
                let mut snippet = json!({"playlistId":playlist_id,"resourceId":{"kind":"youtube#video","videoId":video_id}});
                if let Some(position) = position {
                    snippet["position"] = json!(position);
                }
                self.0
                    .client
                    .post(&self.0.endpoints.playlist_items)
                    .query(&[("part", "snippet")])
                    .json(&json!({"snippet":snippet}))
            }
            PodcastAction::SetCover {
                playlist_id,
                media_id,
            } => {
                self.owned_playlist(playlist_id, &channel, &token).await?;
                Uuid::parse_str(media_id)
                    .map_err(|_| anyhow::anyhow!("media_id must be a UUID"))?;
                self.check_media_owner(media_id).await?;
                let media: Media = sqlx::query_as("SELECT * FROM media WHERE id=? AND ready=1")
                    .bind(media_id)
                    .fetch_optional(&self.0.db)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Upload the complete cover image first"))?;
                if !["image/png", "image/jpeg"].contains(&media.mime.as_str())
                    || media.size > 2 * 1024 * 1024
                {
                    bail!("Cover must be a PNG/JPEG of at most 2 MiB");
                }
                let bytes = tokio::fs::read(self.0.dir.join("media").join(media_id)).await?;
                let format = image::guess_format(&bytes)
                    .map_err(|_| anyhow::anyhow!("Invalid cover image"))?;
                if format.to_mime_type() != media.mime {
                    bail!("Cover bytes do not match the declared MIME type");
                }
                let (width, height) =
                    image::ImageReader::with_format(std::io::Cursor::new(&bytes), format)
                        .into_dimensions()
                        .map_err(|_| anyhow::anyhow!("Invalid cover image dimensions"))?;
                if width == 0 || width != height {
                    bail!("Podcast cover must be square");
                }

                let existing = self
                    .podcast_read(
                        &self.0.endpoints.playlist_images,
                        &[
                            ("part", "snippet"),
                            ("parent", playlist_id),
                            ("maxResults", "50"),
                        ],
                        &token,
                    )
                    .await?;
                let mut metadata = json!({"snippet":{"playlistId":playlist_id,"type":"hero"}});
                let method = if let Some(id) = existing["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|v| v["snippet"]["type"] == "hero")
                    .and_then(|v| v["id"].as_str())
                {
                    metadata["id"] = json!(id);
                    Method::PUT
                } else {
                    Method::POST
                };
                return self
                    .upload_podcast_cover(
                        &input,
                        &encoded,
                        CoverUpload {
                            method,
                            metadata,
                            bytes,
                            mime: media.mime,
                        },
                        &token,
                    )
                    .await;
            }
        };
        request = request.bearer_auth(token).timeout(Duration::from_secs(30));
        // Persist before contacting YouTube. A crash cannot turn a retry into a second insert.
        let pending = json!({"status":"outcome_unknown","request_id":input.request_id,
            "next_action":"Do not create a new request to repeat this write. List playlists or episodes to reconcile the remote outcome. This request ID will never resend a mutation."});
        self.save_podcast_result(&input, &encoded, pending.clone())
            .await?;
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                let mut pending = pending;
                pending["error"] = json!(if error.is_timeout() {
                    "provider_timeout"
                } else if error.is_connect() {
                    "provider_connection_failed"
                } else {
                    "provider_transport_error"
                });
                return self.save_podcast_result(&input, &encoded, pending).await;
            }
        };
        let status = response.status();
        if status.as_u16() == 401 {
            *self.0.access_token.lock().await = None;
        }
        let result = if status.is_success() {
            match response.json::<Value>().await {
                Ok(resource) if resource["id"].is_string() => {
                    json!({"status":"completed","resource":resource})
                }
                _ => pending,
            }
        } else if status.is_client_error() && status.as_u16() != 408 {
            // A definite rejection can be corrected using a new operation ID.
            let body = response.json::<Value>().await.unwrap_or_default();
            let reason = body
                .pointer("/error/errors/0/reason")
                .and_then(Value::as_str)
                .unwrap_or("provider_rejected");
            json!({"status":"rejected","http_status":status.as_u16(),"reason":reason,
                "next_action":"Correct the reported issue and use a new request_id. For podcastNotAllowed, upload a square playlist cover first."})
        } else {
            let mut pending = pending;
            pending["http_status"] = json!(status.as_u16());
            pending
        };
        self.save_podcast_result(&input, &encoded, result).await
    }

    async fn save_podcast_result(
        &self,
        input: &PodcastInput,
        encoded: &str,
        mut result: Value,
    ) -> Result<Value> {
        result["request_id"] = json!(input.request_id);
        let mut tx = self.0.db.begin().await?;
        sqlx::query("INSERT INTO podcast_operations(request_id,channel_id,input,result) VALUES(?,?,?,?) ON CONFLICT(request_id) DO UPDATE SET result=excluded.result")
            .bind(&input.request_id).bind(&input.channel_id).bind(encoded).bind(serde_json::to_string(&result)?).execute(&mut *tx).await?;
        let event = json!({"operation_id":input.request_id,"platform":"youtube","action":serde_json::to_value(&input.operation)?["action"],"result":result});
        sqlx::query("INSERT INTO events(kind,payload) VALUES('podcast.operation',?)")
            .bind(serde_json::to_string(&event)?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result)
    }
}

fn valid_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 150
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        bail!("Invalid YouTube resource ID");
    }
    Ok(())
}

// Google list responses can omit an empty repeated field. Preserve pagination and
// resource metadata, while still rejecting malformed item values and error bodies.
fn normalize_list(mut body: Value) -> Result<Value> {
    let object = body
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Invalid YouTube list response"))?;
    if object.contains_key("error") {
        bail!("Unexpected YouTube error response");
    }
    let items = object.entry("items").or_insert_with(|| json!([]));
    if !items.is_array() {
        bail!("Invalid YouTube list items");
    }
    Ok(body)
}

struct CoverUpload {
    method: Method,
    metadata: Value,
    bytes: Vec<u8>,
    mime: String,
}

impl Publisher {
    async fn upload_podcast_cover(
        &self,
        input: &PodcastInput,
        encoded: &str,
        upload: CoverUpload,
        token: &str,
    ) -> Result<Value> {
        let saved: Option<String> =
            sqlx::query_scalar("SELECT upload_session FROM podcast_operations WHERE request_id=?")
                .bind(&input.request_id)
                .fetch_optional(&self.0.db)
                .await?
                .flatten();
        let mut pending = json!({"status":"upload_pending","request_id":input.request_id,
            "next_action":"Retry set_cover with the SAME request_id and identical inputs after five seconds to check and resume the saved cover upload. Do not create a new request ID.","retry_after_seconds":5});
        self.save_podcast_result(input, encoded, pending.clone())
            .await?;
        let size = upload.bytes.len() as i64;
        let session = if let Some(saved) = saved {
            self.0.vault.open_secret(&saved)?
        } else {
            pending["phase"] = json!("initialize");
            // Initiation sends metadata only. Losing its response cannot publish image bytes.
            let response = self
                .0
                .client
                .request(upload.method, &self.0.endpoints.playlist_images_upload)
                .query(&[("part", "snippet"), ("uploadType", "resumable")])
                .bearer_auth(token)
                .header("X-Upload-Content-Type", &upload.mime)
                .header("X-Upload-Content-Length", size)
                .json(&upload.metadata)
                .timeout(Duration::from_secs(30))
                .send()
                .await;
            let response = match response {
                Ok(r) => r,
                Err(e) => {
                    return self
                        .cover_transport_result(input, encoded, pending, &e)
                        .await;
                }
            };
            if !response.status().is_success() {
                return self
                    .cover_response_result(input, encoded, pending, response)
                    .await;
            }
            let Some(session) = response
                .headers()
                .get("location")
                .and_then(|h| h.to_str().ok())
            else {
                let mut pending = pending;
                pending["error"] = json!("upload_session_missing");
                return self.save_podcast_result(input, encoded, pending).await;
            };
            super::worker::validate_session(session, &self.0.endpoints.playlist_images_upload)?;
            sqlx::query("UPDATE podcast_operations SET upload_session=? WHERE request_id=?")
                .bind(self.0.vault.seal(session)?)
                .bind(&input.request_id)
                .execute(&self.0.db)
                .await?;
            session.to_owned()
        };
        super::worker::validate_session(&session, &self.0.endpoints.playlist_images_upload)?;
        pending["phase"] = json!("check_session");
        // Query before sending bytes, including after restart or a lost final response.
        let probe = self
            .0
            .client
            .put(&session)
            .bearer_auth(token)
            .header("Content-Length", 0)
            .header("Content-Range", format!("bytes */{size}"))
            .timeout(Duration::from_secs(30))
            .send()
            .await;
        let probe = match probe {
            Ok(r) => r,
            Err(e) => {
                return self
                    .cover_transport_result(input, encoded, pending, &e)
                    .await;
            }
        };
        if probe.status().as_u16() != 308 {
            return self
                .cover_response_result(input, encoded, pending, probe)
                .await;
        }
        let offset = super::worker::next_offset(probe.headers(), size)?;
        if offset == size {
            return self.save_podcast_result(input, encoded, pending).await;
        }
        pending["phase"] = json!("transfer");
        let response = self
            .0
            .client
            .put(&session)
            .bearer_auth(token)
            .header("Content-Type", &upload.mime)
            .header(
                "Content-Range",
                format!("bytes {offset}-{}/{size}", size - 1),
            )
            .body(upload.bytes[offset as usize..].to_vec())
            .timeout(Duration::from_secs(30))
            .send()
            .await;
        match response {
            Ok(r) => self.cover_response_result(input, encoded, pending, r).await,
            Err(e) => {
                self.cover_transport_result(input, encoded, pending, &e)
                    .await
            }
        }
    }

    async fn cover_transport_result(
        &self,
        input: &PodcastInput,
        encoded: &str,
        mut pending: Value,
        error: &reqwest::Error,
    ) -> Result<Value> {
        // Never persist reqwest's Display: it can include the credential-bearing session URL.
        pending["error"] = json!(if error.is_timeout() {
            "provider_timeout"
        } else if error.is_connect() {
            "provider_connection_failed"
        } else {
            "provider_transport_error"
        });
        self.save_podcast_result(input, encoded, pending).await
    }

    async fn cover_response_result(
        &self,
        input: &PodcastInput,
        encoded: &str,
        mut pending: Value,
        response: reqwest::Response,
    ) -> Result<Value> {
        let status = response.status();
        if status.as_u16() == 401 {
            *self.0.access_token.lock().await = None;
        }
        pending["http_status"] = json!(status.as_u16());
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        let result = if status.is_success() && body["id"].is_string() {
            json!({"status":"completed","resource":body})
        } else {
            let reason = body
                .pointer("/error/errors/0/reason")
                .and_then(Value::as_str)
                .unwrap_or("provider_response_unconfirmed");
            // Keep only a bounded error code, never arbitrary upstream HTML or URLs.
            pending["error"] = json!(
                reason
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .take(100)
                    .collect::<String>()
            );
            if [404, 410].contains(&status.as_u16()) && pending["phase"] != "initialize" {
                // A terminal session cannot accept more bytes. Cover is a desired-state
                // update, not playlist/video creation. The next identical retry re-reads
                // hero artwork and selects update versus insert before initiating again.
                sqlx::query("UPDATE podcast_operations SET upload_session=NULL WHERE request_id=?")
                    .bind(&input.request_id)
                    .execute(&self.0.db)
                    .await?;
                pending["error"] = json!("upload_session_expired");
                pending["next_action"] = json!(
                    "Retry set_cover with the SAME request_id and identical inputs after five seconds. AgentWay will re-read current cover artwork and start a replacement session for the same image."
                );
            } else if status.is_client_error() && ![408, 429, 401].contains(&status.as_u16()) {
                pending["status"] = json!("rejected");
                pending["next_action"] =
                    json!("Correct the provider rejection before submitting a new request ID.");
            }
            pending
        };
        self.save_podcast_result(input, encoded, result).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omitted_list_items_are_empty_but_malformed_responses_are_errors() {
        let body = normalize_list(
            json!({"kind":"youtube#playlistImageListResponse","nextPageToken":"next"}),
        )
        .unwrap();
        assert_eq!(body["items"], json!([]));
        assert_eq!(body["nextPageToken"], "next");
        for invalid in [
            json!(null),
            json!([]),
            json!({"items":null}),
            json!({"items":{}}),
            json!({"error":{"code":500}}),
        ] {
            assert!(normalize_list(invalid).is_err());
        }
    }
}
