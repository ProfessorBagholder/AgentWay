# YouTube publishing with an existing agent

This branch adds a real YouTube API upload path and an authenticated HTTP/MCP bridge. Account-backed validation with Muse is still required. AgentWay does not generate, edit or transcode the video.

## Start

`./run` builds and starts the app, waits for readiness, and opens the browser. Publishing settings are at http://127.0.0.1:8787/#publishing.

For a hosted agent such as Muse, `./run --share` additionally starts a temporary Cloudflare HTTPS tunnel to the **agent listener only**, saves that address in Publishing, and opens the app. No Cloudflare account is needed for a quick tunnel. The address changes when the tunnel is recreated; update the agent's connector then. `./stop` stops both services. Cloudflare carries the agent traffic; use your own HTTPS reverse proxy for a stable address. Never expose the management port through the tunnel.

The app port defaults to 8787; the agent port defaults to 8788. Override with `--port` and `--bridge-port`. The management app is loopback-only. HTTP/MCP clients authenticate to the separate agent port using a bearer token. HTTPS must terminate at a trusted reverse proxy/tunnel for remote access. Browser-origin requests to the agent listener are rejected.

Quick tunnels are for testing, have provider availability/body-size restrictions, and do not support long-lived SSE. AgentWay's HTTP API works without SSE; Muse should use that API for this test. Keep the first video under 90 MiB. Use a suitable stable reverse proxy for larger uploads; AgentWay's own file cap is 2 GiB and total reservation cap is 10 GiB. MCP over a normal HTTPS proxy supports Streamable HTTP, including clients using older session-based versions; quick-tunnel MCP compatibility is not claimed.

## Authorize YouTube

1. In Google Cloud, enable YouTube Data API v3 in your project.
2. Configure the OAuth consent screen. If it is in testing mode, add your Google account as a test user.
3. Create an OAuth client of type **Web application**. Add the exact redirect URI displayed in AgentWay: normally `http://127.0.0.1:8787/api/youtube/callback`.
4. Enter the client ID and client secret in **Publishing → YouTube**. Do not put them in chat or `.env`.
5. Click **Connect YouTube** and grant upload and channel-read permissions. Check the returned channel name/ID.
6. Leave **Only allow private uploads** enabled for the first test.

The extra read scope identifies the actual destination channel. Google tokens, client credentials and resumable session URLs are encrypted in SQLite with ChaCha20-Poly1305. The key is stored separately at `/data/publishing/secret.key` in the Docker data volume with owner-only file permissions. Back up both the database and this key; encryption does not protect against someone who has access to both. Lost keys are not regenerated over an existing key file.

Google's OAuth testing configuration may cause refresh tokens to expire. Reconnect when prompted. Disconnect removes AgentWay's local authorization and stops subsequent chunks; it does not delete existing videos or revoke the grant in Google. External revocation is available in Google account permissions.

Uploads from unaudited API projects are restricted by YouTube to private visibility, regardless of the visibility requested. Public publishing therefore requires the applicable Google project audit/approval. AgentWay cannot bypass that restriction.

## Connect Muse

Muse is the personal agent, not Muse Code or Muse Spark. Meta documents custom API/CLI connectors running in Muse's cloud VM. The connection instructions in Publishing describe the supported HTTP operations. Give those instructions to Muse and provide the AgentWay bearer token through Muse's secure credential prompt. Do not paste it into conversation text. The token permits publishing to the connected channel, bounded by the owner's private-only setting. Replacing it invalidates the previous token.

Once configured, ask Muse to upload a small finished video **privately**. Use an existing file or let Muse create it with its existing tools. Verify the returned URL in YouTube Studio. An `uploaded` result confirms YouTube returned a video ID, not that video processing has completed or that YouTube classified the video as a Short.

## Agent API

Every request requires `Authorization: Bearer <token>`. Paths are relative to the agent base URL.

- `GET /v1/status`: connected channel, owner policy and configured bridge address.
- `POST /v1/media`: JSON `{ "size": 12345, "mime": "video/mp4" }`. Returns `media_id` and `upload_path`.
- `PUT /v1/media/{media_id}`: raw complete file bytes, not JSON/base64 or a path. An interrupted transfer can be retried from the beginning. Ready media is immutable.
- `DELETE /v1/media/{media_id}`: remove unreferenced media reservations/files.
- `POST /v1/youtube/publish`: JSON with `request_id` (UUID), `media_id`, `title`, `description`, `privacy`, `made_for_kids`, `contains_synthetic_media`. Reuse the same UUID and identical arguments for a retried submission.
- `GET /v1/publications/{id}`: persisted progress, error or `video_url`. Poll only while queued/uploading, at intervals of at least five seconds.
- `POST /v1/publications/{id}/retry`: retry an interrupted job using its original session.
- `/mcp`: official Rust MCP SDK server exposing the same operations as tools. No model API or inference credentials are used.

A single worker streams 8 MiB chunks to YouTube. The session is encrypted and persisted before sending video bytes. On retry/restart it asks YouTube for the authoritative offset or completed video result. It never silently replaces an expired or ambiguous upload session with a new upload. If the session expires, inspect YouTube Studio before intentionally submitting a new request ID.

## Verified versus pending

Automated tests cover auth boundaries, incomplete media, policy enforcement, concurrent idempotent submissions, OAuth state binding/replay, MCP discovery/tool invocation, and recovery after a lost final response using a simulated YouTube server. Google consent and channel discovery have now succeeded against the user's real account. A browser return-navigation defect found during that test was fixed with backend and browser regression coverage. Google project audit approval, Muse connectivity and a real uploaded video are still unverified.

## References

- https://developers.google.com/youtube/v3/docs/videos/insert
- https://developers.google.com/youtube/v3/guides/using_resumable_upload_protocol
- https://developers.google.com/identity/protocols/oauth2/web-server
- https://github.com/modelcontextprotocol/rust-sdk
- https://research.meta.ai/blog/security-and-safety-for-ai-agents-our-approach-with-muse
- https://www.meta.com/help/artificial-intelligence/1687253048996149/
- https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/trycloudflare/
