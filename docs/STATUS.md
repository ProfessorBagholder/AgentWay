# Implementation status

## Implemented and locally tested

- Async Rust/Axum server, SQLx/SQLite migrations and persistent event replay.
- The production interface has Agents, Platforms, Tasks, Activity log and Settings, using the approved prototype layout with the original dark palette and persisted light/dark preference.
- Agents manages the existing owner-named publishing connection: instructions, publishing permission and explicit disconnect. Disconnect revokes its shared token and preserves history. Independent per-agent credentials/enrollment remain unimplemented; shared credentials cannot identify individual clients.
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

YouTube correction operations: agents can read processing status and change post-upload visibility with persistent request IDs, replacement links and retry reconciliation. Existing channels need additional video-management consent; upload credentials are preserved. These operations have mock-provider coverage; real visibility mutation still needs verification after consent. General metadata edits, deletion and in-place video replacement remain unsupported.
