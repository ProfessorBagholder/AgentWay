use super::*;
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
#[derive(Clone)]
pub struct PublishingTools {
    publisher: Publisher,
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
    fn new(publisher: Publisher) -> Self {
        Self {
            publisher,
            tool_router: Self::tool_router(),
        }
    }
    #[tool(
        description = "Read the connected YouTube channel ID, title and current channel description before editing it."
    )]
    async fn get_youtube_channel(&self) -> Result<rmcp::Json<Value>, String> {
        self.publisher
            .youtube_channel()
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Set the connected channel description (up to 1000 characters; empty clears it). Supply channel_id and exact expected_description from get_youtube_channel. Preserves other channel settings and verifies readback. Requires existing management consent. Retry identical inputs after uncertain errors; stale descriptions are rejected. Changes the channel blurb, not video descriptions or channel name."
    )]
    async fn set_youtube_channel_description(
        &self,
        Parameters(input): Parameters<ChannelDescriptionInput>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher
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
        Parameters(request): Parameters<DeleteRequest>,
    ) -> Result<rmcp::Json<VideoOperation>, String> {
        self.publisher
            .delete_replaced_video(&request.id, request.input)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "List the latest 100 AgentWay publications, including publication IDs and YouTube URLs, to identify originals and corrections without uploading duplicates."
    )]
    async fn list_publications(&self) -> Result<rmcp::Json<Value>, String> {
        self.publisher
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
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher
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
        Parameters(request): Parameters<VisibilityRequest>,
    ) -> Result<rmcp::Json<VideoOperation>, String> {
        self.publisher
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
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<VideoOperation>, String> {
        self.publisher
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
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<Vec<VideoOperation>>, String> {
        self.publisher
            .video_operations(&input.id)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Read the connected YouTube channel and owner's private-only policy before publishing."
    )]
    async fn youtube_status(&self) -> Result<rmcp::Json<Value>, String> {
        self.publisher
            .status()
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Reserve media storage. Returns an authenticated HTTP PUT path for raw video bytes. Transfer the file from your own environment; do not pass a local filesystem path or base64 through MCP."
    )]
    async fn create_media_upload(
        &self,
        Parameters(input): Parameters<MediaInput>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher
            .create_media(input)
            .await
            .map(rmcp::Json)
            .map_err(|e| e.to_string())
    }
    #[tool(
        description = "Upload finished media to the connected YouTube channel. Requires a completed media PUT. Returns a persisted job; use get_publication until uploaded or interrupted. Reuse request_id on retries. A video_url confirms upload only. Use get_publication and confirm actual_privacy matches requested_privacy before claiming the requested visibility. YouTube still processes uploaded videos, and determines Shorts classification. Explicit made_for_kids and contains_synthetic_media booleans are required. Current category is Entertainment (24); subscriber notifications default to on; set notify_subscribers=false to disable them. Only schema fields are supported; report any required unsupported setting before uploading. Disclosure readback and metadata edits are not implemented."
    )]
    async fn publish_youtube(
        &self,
        Parameters(input): Parameters<PublishInput>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher
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
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher
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
        Parameters(input): Parameters<UploadId>,
    ) -> Result<rmcp::Json<Value>, String> {
        self.publisher
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
