# Implementation status

## Implemented and locally tested

- Async Rust/Axum server, SQLx/SQLite migrations and persistent event replay.
- The production interface has Agents, Platforms, Tasks, Activity log and Settings, using the approved prototype layout with the original dark palette and persisted light/dark preference.
- Agents supports independent connections with encrypted bearer credentials, per-connection YouTube publishing/management permission, authenticated status, attribution and revocation. The legacy Muse credential is migrated unchanged. Media reservations and upload retries are connection-bound; the channel and published history are owner-shared. MCP uses stateless Streamable HTTP, authenticating every invocation. Grok Bot's live connector test is pending; synthetic MCP tests are not proof of that integration.
- Tasks shows real publishing jobs, progress, errors, retries and YouTube links. Legacy saved agent/task records remain stored but are no longer presented as live activity.
- Hash routes support details, task filters, refresh and browser history. Old #publishing links open YouTube setup. Agents owns connection instructions; Platforms owns YouTube OAuth; Settings owns appearance, external endpoint and private-only restriction.
- Tasks shows submitted video settings and offers on-demand YouTube visibility verification. Activity log groups persisted publication transitions and errors by upload with paginated steps. New publication events include timestamps; older step timestamps are unavailable. It does not yet cover all authentication, media-transfer, OAuth or infrastructure failures.
- Publishing: Google OAuth with PKCE and browser-bound single-use state, encrypted credentials, connected channel identification, private-only owner policy, token rotation, media transfer, resumable YouTube upload queue and saved results.
- Agent publication status reads current YouTube visibility and reports requested versus actual privacy; verification errors preserve completed uploads. Covered with mock-provider tests.
- Versioned agent onboarding through MCP initialization and authenticated status, with a generated publishing schema and explicit per-video disclosure requirements.
- Separate authenticated HTTP/MCP agent listener; management and credentials remain loopback-only.
- Resource-level SSE updates patch records and opened journals without page reloads or unrelated refetches.
- Compose launcher with optional temporary HTTPS tunnel for a hosted agent; `./run --share`.

## Live validation

The user completed the Muse → AgentWay → YouTube flow: Google consent, channel discovery, authenticated Muse connection, media transfer and video upload. The user confirmed playback and Private visibility with a YouTube screenshot. The user subsequently changed that video to public in Studio; the new authenticated visibility check read requested_privacy=private and actual_privacy=public from the live account. The user subsequently confirmed Muse successfully uploaded a public video from the outset. This validates the HTTP connector path with Muse; the MCP path has automated protocol coverage, not a separate live agent test. See [setup and verification](youtube-publishing.md).

## Not implemented

Cross-agent execution or wake adapters, quota observation/admission, manager failover, additional publishing platforms and RSS. AgentWay does not generate or edit content. Public YouTube publishing remains subject to Google's project audit restrictions.

## Engineering audit and dependency proof

The [reliability audit](design/reliability-audit.md) records source-level gaps against the original engineering contract and executable acceptance gates. The [resumable-media decision](adr/0001-resumable-media.md) selects tusd behind the Rust boundary. Its isolated dependency test passed interruption/restart/concurrency and 104 MiB integrity checks. Resumable agent-to-AgentWay uploads are **not integrated yet**; the current receiving endpoint still requires a whole-file PUT. This audit/proof does not change the deployed application.

YouTube correction operations: agents can read processing status and change post-upload visibility with persistent request IDs, replacement links and retry reconciliation. Existing channels need additional video-management consent; upload credentials are preserved. These operations have mock-provider coverage; real visibility mutation still needs verification after consent. Authorized cleanup now supports permanent deletion of a private original after verifying its distinct same-channel replacement is processed and public; deletion is covered by mock-provider tests, not live destructive testing. General metadata edits and in-place video replacement remain unsupported.

- Channel description: HTTP/MCP read and update with existing management consent, channel binding, stale-edit detection, preserved branding fields and verified readback. Guidance version 6. Mock tests cover authorization, preservation, uncertain-write reconciliation and readback mismatch; real content editing remains an agent/user operation.

Channel-description verification tolerates delayed reads with bounded backoff. If YouTube accepted the write but matching readback is unavailable, HTTP returns 202 (`verification_pending`, `verified=false`), with recovery guidance; MCP returns the same result. Accepted pending inputs are persisted so identical retries verify without resubmitting. Regression tests simulate delayed propagation and persistent mismatch.

- Podcast playlists (guidance version 10): HTTP/MCP channel playlist and episode discovery, create playlist, upload/update square show cover, enable podcast designation and add existing same-channel full episodes. Durable operation IDs prevent repeat mutations after interruption; existing membership is not duplicated. Unknown outcomes require read-only reconciliation. Automated provider tests cover ownership, preservation, image constraints, ambiguous-response restart/retry behavior and tool discovery. Real podcast mutations still require user/agent testing. No UI changes or automatic channel changes accompany discovery.

Podcast setup recovery: empty provider lists may omit `items` and are normalized to empty arrays; malformed values still fail. Episodes can be added before podcast designation. Guidance orders setup as playlist → episodes → cover → enable, and resumes existing resources from partially completed setup.

Cover recovery: durable encrypted resumable sessions replace multipart uploads. Same-ID retries check/resume the saved session and reconcile lost completion responses; safe provider diagnostics are persisted. Legacy uncertain covers can resume the same requested artwork. Regression tests cover legacy recovery and a lost completion response across restart without retransmitting image bytes.

Expired cover sessions (404/410) now retire safely; same-ID retries re-read current hero artwork before starting a replacement session for the requested cover. Phase, HTTP status and safe reason codes distinguish initialization, status probes and transfer failures. No playlist or video is recreated by this recovery.

Live podcast validation (2026-09-22): recovered the existing cover operation under its original request ID through the public AgentWay endpoint. YouTube returned completion and one cover resource. The corrected enable-podcast request preserved existing title/description/privacy; its response reported enabled, and a delayed read confirmed podcastStatus=enabled, privacyStatus=public, eight episodes and one cover. No episode videos were reuploaded. The user confirmed the podcast in Studio; the podcast changes were merged to main before this connection work.

Independent-connection validation (2026-09-22): 27 Rust tests and 10 browser tests passed. Checked the connection screens in dark/light themes at 1440, 900 and 390 px. Existing Muse credential was preserved during the test deployment; authenticated public HTTP and stateless MCP discovery/status succeeded with guidance 11 and the same YouTube account. No YouTube content was changed. Actual Grok Bot installation and publishing remain for the user's live test.
