# YouTube publishing with an existing agent

This branch adds a real YouTube API upload path and an authenticated HTTP/MCP bridge. The user has verified the HTTP path with Muse and real private and public YouTube uploads. AgentWay does not generate, edit or transcode the video.

## Start

`./run` builds and starts the app, waits for readiness, and opens the browser. YouTube setup is at http://127.0.0.1:8787/#/platforms/youtube.

For a hosted agent such as Muse, `./run --share` additionally starts a temporary Cloudflare HTTPS tunnel to the **agent listener only**, saves that address under Settings → Agent endpoint, and opens the app. No Cloudflare account is needed for a quick tunnel. The address changes when the tunnel is recreated; update the agent's connector then. `./stop` stops both services. Cloudflare carries the agent traffic. For a stable address, configure a named tunnel as described below. Never expose the management port through the tunnel.

The app port defaults to 8787; the agent port defaults to 8788. Override with `--port` and `--bridge-port`. The management app is loopback-only. HTTP/MCP clients authenticate to the separate agent port using a bearer token. HTTPS must terminate at a trusted reverse proxy/tunnel for remote access. Browser-origin requests to the agent listener are rejected.

Quick tunnels are for testing, have provider availability/body-size restrictions, and do not support long-lived SSE. AgentWay's HTTP API works without SSE; Muse should use that API for this test. Keep videos sent through a Cloudflare Free-plan proxy under 90 MiB until resumable receiving uploads are integrated. A named tunnel fixes hostname stability, not the per-request body limit; AgentWay's own file cap is 2 GiB and total reservation cap is 10 GiB. MCP over a normal HTTPS proxy supports Streamable HTTP, including clients using older session-based versions; quick-tunnel MCP compatibility is not claimed.

## Authorize YouTube

1. In Google Cloud, enable YouTube Data API v3 in your project.
2. Configure the OAuth consent screen. If it is in testing mode, add your Google account as a test user.
3. Create an OAuth client of type **Web application**. Add the exact redirect URI displayed in AgentWay: normally `http://127.0.0.1:8787/api/youtube/callback`.
4. Enter the client ID and client secret in **Platforms → YouTube**. Do not put them in chat or `.env`.
5. Click **Connect YouTube** and grant upload and channel-read permissions. Check the returned channel name/ID.
6. Leave **Settings → Publishing restrictions → Restrict uploads to private** enabled for the first test.

The extra read scope identifies the actual destination channel. Google tokens, client credentials and resumable session URLs are encrypted in SQLite with ChaCha20-Poly1305. The key is stored separately at `/data/publishing/secret.key` in the Docker data volume with owner-only file permissions. Back up both the database and this key; encryption does not protect against someone who has access to both. Lost keys are not regenerated over an existing key file.

Google's OAuth testing configuration may cause refresh tokens to expire. Reconnect when prompted. Disconnect removes AgentWay's local authorization and stops subsequent chunks; it does not delete existing videos or revoke the grant in Google. External revocation is available in Google account permissions.

Uploads from unaudited API projects are restricted by YouTube to private visibility, regardless of the visibility requested. Public publishing therefore requires the applicable Google project audit/approval. AgentWay cannot bypass that restriction.

## Connect Muse

Muse is the personal agent, not Muse Code or Muse Spark. Meta documents custom API/CLI connectors running in Muse's cloud VM. The connection instructions under Agents → your connection describe the supported HTTP operations. Give those instructions to Muse and provide the AgentWay bearer token through Muse's secure credential prompt. Do not paste it into conversation text. The token permits publishing to the connected channel, bounded by the owner's private-only setting. Replacing it invalidates the previous token.

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

Automated tests cover auth boundaries, incomplete media, policy enforcement, concurrent idempotent submissions, OAuth state binding/replay, MCP discovery/tool invocation, and recovery after a lost final response using a simulated YouTube server. Google consent and channel discovery have now succeeded against the user's real account. A browser return-navigation defect found during that test was fixed with backend and browser regression coverage. The user subsequently confirmed Muse connectivity and a real video upload, with playback and Private visibility shown in YouTube. Google project audit approval and a live agent test over MCP remain unverified.

## References

- https://developers.google.com/youtube/v3/docs/videos/insert
- https://developers.google.com/youtube/v3/guides/using_resumable_upload_protocol
- https://developers.google.com/identity/protocols/oauth2/web-server
- https://github.com/modelcontextprotocol/rust-sdk
- https://research.meta.ai/blog/security-and-safety-for-ai-agents-our-approach-with-muse
- https://www.meta.com/help/artificial-intelligence/1687253048996149/
- https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/trycloudflare/

## Verify requested visibility

Public and unlisted uploads are supported when the owner disables **Settings → Publishing restrictions → Restrict uploads to private**. Agents must read `/v1/status` first, use the visibility requested by the user (private when unspecified), and use a new request ID for each new upload. Retries retain the original ID and identical arguments.

After upload, `GET /v1/publications/{id}` and MCP `get_publication` return `requested_privacy`, `actual_privacy`, and `visibility_error` alongside existing publication fields. Each status check for a completed video reads its current visibility from YouTube, so manual Studio changes are reflected. A public request is verified only when `actual_privacy` is `public`; a URL or `uploaded` alone is insufficient. A mismatch must be reported to the user.

If YouTube cannot be queried or returns no usable visibility, `actual_privacy` is null and `visibility_error` explains that verification failed. Upload state remains complete. Retry the status check, not the upload. Queued jobs have null actual visibility without a verification error. Status queries do not establish processing completion or Shorts classification.

Mock-provider coverage verifies public requests resulting in private or public visibility, missing videos, provider failures, and preservation of completed uploads. A live authenticated status check against the existing test upload returned requested_privacy=private, actual_privacy=public, and visibility_error=null, confirming the user's manual Studio change. The user subsequently confirmed a successful Muse upload with public visibility requested from the outset.

## Agents and Tasks

Agents shows the existing owner-named publishing connection. It does not infer client identity from the shared token or claim that an agent is online. Platform permission changes are enforced at admission, retry and worker transfer boundaries. Disconnect replaces the credential and disables access while preserving uploads; clients must explicitly set up again with the replacement credential. No existing credential or permission changes during app upgrade.

Tasks lists actual publication jobs and submitted video settings. Activity log groups the persisted publication transitions and errors by upload, with paginated history. New events include timestamps; old steps do not acquire invented timestamps. This log does not yet cover every pre-publication or infrastructure error. Legacy saved placeholder agent/task rows remain in storage but do not appear in these screens. Browser tests use intercepted fixtures and never create sample records in the user's database.

## Exact agent publishing contract

Use **Agents → your connection → Connection instructions → Copy instructions** for the current bridge address and complete procedure. Those instructions contain no token; supply the token through the agent's secure credential facility. Existing agents can use the same configured connection and receive the updated instructions without reconnecting or rotating credentials. MCP exposes the same input schema through tools/list.

[Example request](examples/youtube-publish.json) is valid JSON, checked by a Rust regression test. Replace both sample UUIDs: request_id is freshly generated for a new upload; media_id comes from this bridge after the video transfer. Replace all content and both disclosure values appropriately; the sample booleans are not defaults.

| Field | Required | Meaning / constraints |
| --- | --- | --- |
| request_id | Yes | UUID for one logical upload. Retried submissions use the same UUID and identical input. |
| media_id | Yes | UUID returned by create_media_upload / POST /v1/media, with completed raw-byte PUT. |
| title | Yes | Nonempty after trimming, at most 100 characters; no angle brackets. |
| description | No | Defaults to empty; at most 5000 UTF-8 bytes, no angle brackets. |
| privacy | No | private, unlisted or public; defaults to private. Owner policy may reject non-private. |
| made_for_kids | Yes | Explicit JSON boolean declaring whether the video is child-directed. |
| contains_synthetic_media | Yes | Explicit JSON boolean for realistic altered/synthetic content disclosure. |
| notify_subscribers | No | Defaults to true; set false to disable subscriber notifications for this upload. |

YouTube's disclosure guidance distinguishes realistic altered/synthetic content from production assistance such as script drafting. The creating agent determines the appropriate declaration from the content and owner instructions; the bridge transports it. If the agent cannot determine a required declaration, resolve that before submission. See [YouTube guidance](https://support.google.com/youtube/answer/14328491).

**Current limits:** these are the only supported publish fields. Category is fixed to Entertainment (24); subscriber notifications default to on unless explicitly disabled. No tags, publication scheduling, paid-promotion control, language setting, thumbnail/caption upload, playlist association or metadata editing is exposed yet. Unknown input fields are rejected. A required unsupported setting is a pre-upload capability gap, not permission to omit it. The design specification describes future support, not current functionality.

The worker sends audience and synthetic declarations with the initial upload metadata, together with requested visibility. Completion/readback currently verifies visibility only. Do not tell users that declarations were independently read back, processing finished, or Shorts classification was confirmed.

### Compatibility boundary

The workspace design under docs/design/workspace-preview is isolated from the deployed React app. The notification option does not change endpoint paths, media limits, owner policy, session recovery, OAuth scope or credentials. Previously stored jobs retain their original notification behavior; new jobs persist the resolved notification setting. Existing Muse requests remain valid. New metadata fields must be introduced in a separately tested change that preserves old persisted inputs and idempotency behavior.

### Automatic agent onboarding

MCP initialization returns server instructions. Authenticated `GET /v1/status` and the MCP `youtube_status` tool return the same instructions under `agent_guidance`, with a version and a publishing JSON Schema generated from the server's request type. Existing status fields remain unchanged. HTTP clients must call status to receive this guidance; this is not an unsolicited push message.

The instructions ask agents to briefly introduce publishing options on first use, use established user preferences, ask only for missing decisions, and explain unsupported settings before upload. Clients can remember the guidance version to avoid repeated onboarding. AgentWay cannot guarantee an external agent follows prose instructions; required booleans, allowed fields and publishing policy are enforced by the server. No new credential or reconnection is required for HTTP clients to read updated guidance; MCP clients receive initialization instructions on a new session and can read guidance through status in an existing session.

## Stable named Cloudflare Tunnel

Create a remotely managed tunnel with a route from your chosen hostname to `http://app:8788`, plus the required final `http_status:404` catch-all. Create the proxied CNAME pointing to `<tunnel-id>.cfargotunnel.com`. Do not route the management listener on port 8787. The official [Cloudflare tunnel setup guide](https://developers.cloudflare.com/tunnel/get-started/) describes these account-side steps.

Store the hostname only (for example `dev.example.com`, without scheme or path) in `.agentway/tunnel-hostname`. Store the tunnel token in `.agentway/tunnel-token` with mode `0600`. Both are local configuration, ignored by git. Enter credentials directly into the local file, never into chat or a committed config file.

Once both files exist, `./run` automatically selects `compose.named-tunnel.yaml`, starts the named tunnel with a restart policy, and checks its public HTTPS endpoint before saving the address in AgentWay. `./run --share` also retains the named tunnel when configured; it does not create a new temporary address. The token is mounted read-only and read with `--token-file`, not passed in the container's command or environment. The launcher runs the tunnel with the current host user's UID/GID so the restricted file remains readable.

The expected public probe is AgentWay's own 401 response without a bearer token. A DNS/TLS/network failure or unrelated proxy response leaves the previously saved endpoint unchanged and exits with an error. Resolve DNS filtering or tunnel configuration before retrying; the launcher does not bypass those controls. The local app can be healthy while the remote endpoint is unavailable.

`./stop` stops the app and tunnel. To return to temporary/local mode, stop the stack and move both tunnel files out of `.agentway` into secure storage before running again. The cloud-side tunnel and DNS record remain until explicitly removed. Keep the machine and Docker running for hosted agents to reach it; restart resilience does not provide availability while the host is asleep.

### Required ingress checks for every agent platform

An agent endpoint is an authenticated machine API. Cloudflare Browser Integrity Check can reject non-browser headers with error 1010 even when ordinary curl succeeds. Before enabling a dedicated agent hostname, create a **Configuration Rule** matching its exact hostname and set **Browser Integrity Check → Off**. For this deployment the expression is `(http.host eq "dev.agentway.win")` and the API setting is `action_parameters: {"bic": false}` in the `http_config_settings` phase. API changes require **Config Settings Write**. Scope this to the agent hostname, not the entire zone.

Keep AgentWay authentication, management isolation and applicable WAF/rate-limit protections. Check other enabled rules for JavaScript/CAPTCHA or browser requirements and resolve any conflict explicitly; this single exception does not guarantee every Cloudflare feature is compatible. An allowlist for a particular agent's current egress IP is not a general compatibility strategy.

The named-tunnel launcher checks an AgentWay client header, Python's client header and an absent User-Agent. All must receive AgentWay's unauthenticated 401 before startup advertises remote readiness or saves the endpoint. Test authenticated status, media methods and the actual configured HTTP/MCP transport separately with each real agent. A successful synthetic probe is a prerequisite, not proof of Muse/Claude/ChatGPT/Grok Bot compatibility.

Incident evidence: Muse reported 1010, and a `Python-urllib/3.12` probe reproduced HTTP 403 with `error code: 1010` while normal curl reached AgentWay. The zone had Browser Integrity Check enabled. After granting Config Settings Write, the hostname-scoped exception was deployed on 22 September 2026 (UTC). The AgentWay, Python and absent-User-Agent probes each returned 401 without credentials and 200 with the existing credential; the actual Python urllib client also returned 200. The full named-tunnel launcher passed. Zone-wide Browser Integrity Check remains on. Muse must still confirm its own retry succeeds.

Sources: [Cloudflare error 1010](https://developers.cloudflare.com/support/troubleshooting/http-status-codes/cloudflare-1xxx-errors/error-1010/), [Browser Integrity Check](https://developers.cloudflare.com/waf/tools/browser-integrity-check/), [configuration rules](https://developers.cloudflare.com/rules/configuration-rules/settings/#browser-integrity-check).

### Diagnosing agent authentication

A 401 with `AgentWay bearer token required` does not distinguish an absent header from a mismatched token. Check `docker compose logs --since 5m app` for `Agent authentication rejected`. The safe `reason` field distinguishes `authorization_missing`, `authorization_multiple`, `authorization_malformed`, `authorization_wrong_scheme` and `token_mismatch`. Diagnostics are limited to one entry per five seconds per server process; wait five seconds before a controlled retry. Credentials and request URLs are never included. Launcher readiness probes intentionally omit authorization and can produce `authorization_missing`; correlate the time with the agent's retry.

The Bearer scheme is case-insensitive; the credential remains case-sensitive. Use the same existing credential in the agent connector's secure credential store. Do not rotate it merely because a request returned 401. Compare an authenticated local request with the public endpoint, then the real agent, to distinguish connector configuration from ingress or server failures.


### Correcting published videos (guidance version 3)

Existing YouTube connections need one additional consent: Platforms → YouTube → Authorize video management. This requests `youtube.force-ssl`, required by `videos.update`; it does not rotate AgentWay's bearer token. The status endpoint reports `video_management_authorized`. Existing uploads/readback continue with their original authorization. Select the same channel during consent.

HTTP and MCP operations:

| HTTP | MCP | Purpose |
| --- | --- | --- |
| GET /v1/publications | list_publications | Latest 100 publication IDs and YouTube URLs for locating original uploads. |
| GET /v1/publications/{id}/youtube | get_youtube_video | Current privacy, processing/upload status, duration, failure reasons and ready flag. |
| POST /v1/publications/{id}/visibility | set_youtube_visibility | Set privacy using a stable UUID request_id; optional replacement_id guards retiring an original. MCP additionally takes id. |
| GET /v1/video-operations/{request_id} | get_video_operation | Read the saved outcome after interruption. |
| GET /v1/publications/{id}/operations | list_video_operations | Latest 100 operations for a publication, including correction links and errors. |

The agent validates corrected content, uploads it privately with notifications disabled, waits for `ready=true`, publishes the replacement, then makes the original private with `replacement_id` referencing the corrected publication. The server verifies that replacement is processed and public before retiring the original. Only AgentWay-tracked completed publications in the connected channel can be modified. Private-only and agent publishing permissions still apply.

Visibility intent is stored before calling YouTube; completion/error and its event are committed together. A response may have HTTP 200 while the operation is `interrupted`: agents must inspect status/error. Repeat the same request ID and input to reconcile uncertain results. A completed operation returns its recorded outcome without reapplying it; a newer operation prevents an unfinished older request from reverting it. Read the video endpoint for current state. The original publication request remains immutable; its requested privacy is the upload-time choice, not a post-upload visibility target.

Whole-part YouTube status updates preserve documented writable status fields. Scheduled-video changes are rejected rather than silently removing schedules. Provider failure details are sanitized. The source/replacement switch is not atomic across YouTube videos; temporary overlap is possible, and failed retirement must be retried. No delete, in-place media replacement, content rendering or quality assessment is performed by AgentWay. Agents remain responsible for batch coordination and content validation. Operation history is exposed over HTTP/MCP and in the management history response; rendering those additional records in the Activity log UI is not implemented in this change.

References: [YouTube videos.update](https://developers.google.com/youtube/v3/docs/videos/update), [video processing fields](https://developers.google.com/youtube/v3/docs/videos#processingDetails).
