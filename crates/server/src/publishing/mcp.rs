use super::*;
use rmcp::{
    RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerConfig},
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
#[derive(Clone)]
pub struct PublishingTools {
    tool_router: ToolRouter<Self>,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct UploadId {
    pub id: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VisibilityRequest {
    pub id: String,
    #[serde(flatten)]
    pub input: VisibilityInput,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteRequest {
    pub id: String,
    #[serde(flatten)]
    pub input: DeleteInput,
}
#[tool_router]
impl PublishingTools {
    #[tool(
        description = "Update an existing video's metadata/settings without reuploading. Read get_youtube_video first and supply its exact etag. Omitted fields are preserved; settings.localizations replaces the complete translation map. Stable request_id and identical retries only reconcile once a write was attempted. completed means verified, verification_pending means accepted, outcome_unknown means uncertain: never send a new UUID to bypass uncertainty. Scheduling requires an unpublished private video and user authorization; clear_schedule cancels it. Existing video-management consent required."
    )]
    async fn update_youtube_video(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<settings::VideoUpdate>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .update_video_settings(input)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Read the saved outcome of a video settings operation by request_id. To reconcile a pending operation, retry update_youtube_video with its original arguments; this only reads, never rewrites."
    )]
    async fn get_youtube_settings_operation(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .settings_operation(&input.id)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "List assignable YouTube video categories for a region. Use returned IDs for settings.category_id; do not infer a category from the agent's identity."
    )]
    async fn list_youtube_categories(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<assets::CategoryQuery>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .youtube_categories(input)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Read caption tracks and their processing status for an AgentWay publication. serving means processed; failed includes failureReason. Use caption IDs for replacement/deletion."
    )]
    async fn list_youtube_captions(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .youtube_captions(&input.id)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Set a custom video thumbnail, create/replace timed captions, or delete a caption track as authorized by the user. Reserve and PUT media first (PNG/JPEG or UTF-8 SRT/WebVTT, 2 MiB max). Reuse request_id and identical arguments on retry. upload_pending requires a same-ID retry after five seconds to query/resume its persisted session, including after restart. accepted acknowledges the write; poll captions for serving/failed. outcome_unknown never permits a blind duplicate write. Existing management consent required. For caption replacement supply caption_id and is_draft; language/name are preserved."
    )]
    async fn manage_youtube_video_asset(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<assets::VideoAssetInput>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .manage_video_asset(input)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Read the durable result of a thumbnail/caption operation by request UUID. Unknown outcomes require provider inspection, not a new request ID."
    )]
    async fn get_youtube_asset_operation(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .video_asset_operation(&input.id)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    fn new(_publisher: Publisher) -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
    fn publisher_for(&self, ctx: &RequestContext<RoleServer>) -> Result<Publisher, String> {
        ctx.extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<Publisher>())
            .cloned()
            .ok_or_else(|| "Authenticated connection is required".into())
    }
    #[tool(
        description = "List the connected channel's playlists and podcastStatus, or supply playlist_id to read that playlist and its episodes. Follow nextPageToken with page_token. Before podcast setup, ask whether the user wants their full episodes organized as a YouTube podcast unless they already decided. Reuse existing playlists/videos; keep promotional shorts separate."
    )]
    async fn list_youtube_playlists(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<podcast::PlaylistQuery>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .youtube_playlists(input)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Manage a YouTube podcast using action=create_playlist, set_cover, enable_podcast or add_episode. First obtain the user's podcast preference and show details. Create or select their playlist, add the selected full episodes (allowed before podcast designation), upload a square PNG/JPEG cover (at most 2 MiB) via create_media_upload and HTTP PUT, set_cover with media_id, then enable_podcast. Add full episodes by existing YouTube video_id without reupload. Uses existing management consent. Reuse request_id and identical arguments on retries. Inspect status: completed means YouTube acknowledged the operation; rejected means no success; outcome_unknown requires read-only reconciliation, never a fresh duplicate write. Exception: retry set_cover with identical inputs and the SAME request_id to recover legacy unknown covers or upload_pending results via its persisted resumable session. Does not guarantee YouTube Music eligibility."
    )]
    async fn manage_youtube_podcast(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<podcast::PodcastInput>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .manage_podcast(input)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Read a saved podcast operation by request UUID after interruption. Outcome_unknown requires listing playlists/episodes to reconcile; never blindly repeat the write with a new UUID."
    )]
    async fn get_youtube_podcast_operation(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .podcast_operation(&input.id)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Read the connected YouTube channel ID, title and current channel description before editing it."
    )]
    async fn get_youtube_channel(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .youtube_channel()
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Set the connected channel description (up to 1000 characters; empty clears it). Supply channel_id and exact expected_description from get_youtube_channel. Preserves other channel settings and verifies readback. Requires existing management consent. Accepted writes with delayed readback return status=verification_pending, verified=false; poll get_youtube_channel after five seconds, do not submit another write. Identical retries of pending accepted writes only verify. Stale descriptions are rejected. Changes the channel blurb, not video descriptions or channel name."
    )]
    async fn set_youtube_channel_description(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<ChannelDescriptionInput>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .set_channel_description(input)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Permanently delete a faulty original after user-authorized cleanup. Original must be private; replacement_id must identify a different processed public video in the same channel. Supply confirm_delete=true only with user authorization. Reuse request_id on retries; inspect operation status/error. No new OAuth permission is needed beyond video management. Publication history remains, marked deleted."
    )]
    async fn delete_replaced_youtube_video(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(request): Parameters<DeleteRequest>,
    ) -> Result<rmcp::Json<VideoOperation>, String> {
        self.publisher_for(&ctx)?
            .delete_replaced_video(&request.id, request.input)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "List the latest 100 AgentWay publications, including publication IDs and YouTube URLs, to identify originals and corrections without uploading duplicates."
    )]
    async fn list_publications(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .list()
            .await
            .and_then(|v| Ok(serde_json::to_value(v)?))
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Read current YouTube visibility, processing status, failure reasons and duration for a completed AgentWay publication. ready=true requires processing success; it does not assess content quality. Poll no faster than every five seconds."
    )]
    async fn get_youtube_video(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .youtube_video_status(&input.id)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Change an existing AgentWay video's visibility. Reuse request_id and identical inputs on retries, including uncertain failures. Returns a durable operation: inspect status/error, not just HTTP success. For retiring an original use privacy=private and replacement_id pointing to the corrected publication; AgentWay verifies replacement processing and public visibility first. Requires YouTube video-management consent. Does not replace bytes, delete videos or guarantee atomic switching of two videos."
    )]
    async fn set_youtube_visibility(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(request): Parameters<VisibilityRequest>,
    ) -> Result<rmcp::Json<VideoOperation>, String> {
        self.publisher_for(&ctx)?
            .set_video_visibility(&request.id, request.input)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Read a visibility operation by request UUID after interruption. Completed result is the last verified outcome; get_youtube_video reads current state."
    )]
    async fn get_video_operation(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<VideoOperation>, String> {
        self.publisher_for(&ctx)?
            .video_operation(&input.id)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "List the latest 100 durable visibility operations for a publication, including replacement links and errors."
    )]
    async fn list_video_operations(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<Vec<VideoOperation>>, String> {
        self.publisher_for(&ctx)?
            .video_operations(&input.id)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Read the connected YouTube channel and owner's private-only policy before publishing."
    )]
    async fn youtube_status(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .status()
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Reserve video, PNG/JPEG artwork, or timed UTF-8 SRT/WebVTT caption storage. Returns an authenticated HTTP PUT path for raw file bytes. Transfer the file from your own environment; do not pass a local filesystem path or base64 through MCP."
    )]
    async fn create_media_upload(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<MediaInput>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .create_media(input)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Upload finished media to the connected YouTube channel. Requires a completed media PUT. Returns a persisted job; use get_publication until uploaded or interrupted. Reuse request_id on retries. A video_url confirms upload only. Use get_publication and confirm actual_privacy matches requested_privacy before claiming the requested visibility. YouTube still processes uploaded videos, and determines Shorts classification. Explicit made_for_kids and contains_synthetic_media booleans are required. Optional settings support category, tags, languages, scheduled publication, embedding, license, public statistics, paid product placement, recording date and localized titles/descriptions. Omitted category retains legacy Entertainment (24); choose category explicitly. subscriber notifications default to on; set notify_subscribers=false to disable them. Only schema fields are supported; report any required unsupported setting before uploading. Read get_youtube_video for metadata/disclosure readback and etag; use update_youtube_video for edits."
    )]
    async fn publish_youtube(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<PublishInput>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .enqueue(input)
            .await
            .and_then(|v| Ok(serde_json::to_value(v)?))
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Read one upload's saved progress and URL, plus current YouTube actual_privacy and requested_privacy. Null actual_privacy means unverified; visibility_error does not mean upload failure. Never re-upload to retry visibility verification. Poll only while queued or uploading, no more often than every five seconds."
    )]
    async fn get_publication(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .verified_publication(&input.id)
            .await
            .and_then(|v| Ok(serde_json::to_value(v)?))
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Resume an interrupted upload using its existing YouTube session. Never substitutes a new video upload for an expired or uncertain session."
    )]
    async fn retry_publication(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher_for(&ctx)?
            .retry(&input.id)
            .await
            .and_then(|v| Ok(serde_json::to_value(v)?))
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
}
#[tool_handler(router = self.tool_router)]
impl ServerHandler for PublishingTools {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(guidance::INSTRUCTIONS)
    }
}
impl Publisher {
    pub(super) fn mcp_service(
        &self,
    ) -> StreamableHttpService<PublishingTools, LocalSessionManager> {
        let p = self.clone();
        let mut config = StreamableHttpServerConfig::default();
        config.cancellation_token = self.0.shutdown.child_token();
        config.json_response = true;
        // No session IDs or resumable SSE streams to cross connection boundaries.
        // Every tool invocation authenticates its own HTTP request.
        config.legacy_session_mode = false;
        // This isolated agent listener authenticates every request and rejects all browser
        // Origins before the SDK. It accepts changing HTTPS tunnel hostnames.
        config.allowed_hosts.clear();
        StreamableHttpService::new(
            move || Ok(PublishingTools::new(p.clone())),
            Default::default(),
            config,
        )
    }
}
