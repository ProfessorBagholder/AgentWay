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
#[tool_router]
impl PublishingTools {
    fn new(publisher: Publisher) -> Self {
        Self {
            publisher,
            tool_router: Self::tool_router(),
        }
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
        description = "Upload finished media to the connected YouTube channel. Requires a completed media PUT. Returns a persisted job; use get_publication until uploaded or interrupted. Reuse request_id on retries. A video_url confirms upload only. Use get_publication and confirm actual_privacy matches requested_privacy before claiming the requested visibility. YouTube still processes uploaded videos, and determines Shorts classification."
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
