# Implementation status

## Implemented and locally tested

- Async Rust/Axum server, SQLx/SQLite migrations and persistent event replay.
- Existing saved agent records and task cancellation; these do not execute agents.
- Publishing: Google OAuth with PKCE and browser-bound single-use state, encrypted credentials, connected channel identification, private-only owner policy, token rotation, media transfer, resumable YouTube upload queue and saved results.
- Agent publication status reads current YouTube visibility and reports requested versus actual privacy; verification errors preserve completed uploads. Covered with mock-provider tests.
- Separate authenticated HTTP/MCP agent listener; management and credentials remain loopback-only.
- Publishing UI consumes targeted status/record events without unrelated refetches.
- Compose launcher with optional temporary HTTPS tunnel for a hosted agent; `./run --share`.

## Live validation

The user completed the Muse → AgentWay → YouTube flow: Google consent, channel discovery, authenticated Muse connection, media transfer and video upload. The user confirmed playback and Private visibility with a YouTube screenshot. The user subsequently changed that video to public in Studio; the new authenticated visibility check read requested_privacy=private and actual_privacy=public from the live account. Public-from-outset upload remains untested. This validates the HTTP connector path with Muse; the MCP path has automated protocol coverage, not a separate live agent test. See [setup and verification](youtube-publishing.md).

## Not implemented

Cross-agent execution or wake adapters, quota observation/admission, manager failover, additional publishing platforms and RSS. AgentWay does not generate or edit content. Public YouTube publishing remains subject to Google's project audit restrictions.
