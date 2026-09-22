# Reliability audit and implementation gates

Audited 21 September 2026 against commit `aa1aafb` and the [engineering contract](engineering-contract.md). This is a source review plus the checks recorded below, not a security certification or a claim that the full platform is implemented.

## Outcome

The Muse → AgentWay → YouTube flow works for the tested private and public videos. It is a successful integration slice, not yet the resilient, multiple-agent system described in the original requirements. The current implementation should not be expanded to more publishing platforms before fixing the transfer and operation foundations.

The first production change should integrate resumable agent-to-bridge media transfer. Use the established tus protocol and the tested tusd service behind the Rust authentication boundary; preserve the existing HTTP/MCP publishing contract and existing accounts, credentials, media IDs and publication IDs. The [upload decision](../adr/0001-resumable-media.md) defines the boundary and acceptance criteria. No domain purchase is required to implement or test it.

## Source findings

Priority means delivery order for this personal-use bridge. P0 blocks dependable large-file use; P1 blocks reliable ongoing operation or safe expansion; P2 completes the broader product. An open finding is not a claim that an exploit or data loss has already occurred.

| ID | Priority | Evidence in the current code | Consequence and required change |
|---|---|---|---|
| R01 | P0 | `publishing/http.rs::upload_media` accepts one entire PUT; `Publisher::new` deletes `.part` files. | Agent-to-bridge transfers cannot resume. Proxy request limits apply to the whole video. Add resumable, bounded requests and durable offset recovery. YouTube's separate 8 MiB resumable uploader already exists and should remain. |
| R02 | P1, with R01 | `create_media` reserves declared sizes up to a hard-coded 10 GiB total; `remove_media` refuses every file referenced by a publication. | Finished publications retain media indefinitely and abandoned reservations consume quota. Define retention, deletion, physical disk reserve, reference protection and crash reconciliation. Never remove files needed by active/retryable work. |
| R03 | P1, with R01 | Whole-file PUTs use different temporary paths and a two-transfer semaphore; there is no media-specific exclusion during receive. | Simultaneous transfers for one reservation can use more disk than reserved. Serialize writes/deletion by artifact and count temporary storage as well as declared sizes. Keep memory and active transfers bounded. |
| R04 | P1 | Publication insertion/updates in `mod.rs` and `worker.rs` precede separate `publish_event` calls. | A crash or event insert failure can commit state without its diagnostic record/SSE event. Commit each state transition and its journal/outbox entry in one SQLite transaction. Wake-up notifications must be optional hints after commit. |
| R05 | P1 | `http.rs::authenticate` checks a singleton bearer token; media/publications have no authenticated principal. | An owner-entered label is not proof of which agent acted. Add independent credentials, scoped grants and ownership before multiple-agent enrollment. Preserve the existing connection during migration; never infer separate identities from a shared token. |
| R06 | P1 | `worker.rs` holds `mutation` across provider calls; `main.rs` awaits background handles only after the management server exits. Compose allows 15 seconds to stop; provider requests allow 120 seconds. | A slow provider can delay permission changes; a failed background service is not immediately reflected in readiness. Add operation/account coordination, explicit in-flight revocation semantics, task supervision and bounded checkpointing/shutdown. Merely deleting the mutex would introduce authorization races. |
| R07 | P1 | `http.rs::Error` returns 400 for all publishing errors; worker errors are strings and retries are manual. | Agents cannot reliably distinguish malformed input, quota, busy, missing resource, unavailable storage and provider failure. Use typed safe errors with retryability and retry timing. Automatically retry only safe operations with bounded jitter; reconcile ambiguous writes first. |
| R08 | P1 | Activity history filters `publication.upsert`; upload receive, OAuth and authentication failures lack correlated operation history. | Troubleshooting cannot reconstruct the whole chain. Give each operation a stable ID and record redacted steps, attempts, timestamps and errors. Bound/rate-limit unauthenticated failure recording. Keep Tasks as work state and Activity log as diagnostics. |
| R09 | P1 | `Publisher::list` loads all publications; events have no retention; history filters IDs inside JSON. | Long-term use grows database queries and browser state without a bound. Introduce indexed operation IDs, cursor pagination and event retention with targeted client resynchronization. Preserve event ordering and existing record-level cache updates. |
| R10 | P1 | `run --share` uses a Quick Tunnel; the tunnel has no endpoint recovery integration. | Public agent reachability is independent of local app readiness. Select stable ingress separately and monitor actual reachability. Chunking fixes request size; it does not make a temporary hostname permanent or keep a sleeping machine available. |
| R11 | P1 | SQLite WAL, files and `secret.key` are separate resources; there is no automated restore drill in CI. | A database copy alone is not a verified backup. Add consistent backup manifests, artifact/key recovery, migration tests on populated databases and an exercised rollback/restore procedure. Secrets must never enter repository artifacts or test output. |
| R12 | P2 | `PublishInput` only accepts title, description, privacy and the two required disclosure booleans, plus IDs; category and subscriber notification are fixed in `worker.rs`. | The API does not support every YouTube setting. Implement the settings capability matrix incrementally, validate against documented provider support, and expose the same generated schema through HTTP/MCP guidance. Unknown fields must remain errors rather than silently ignored settings. |
| R13 | P2 | Legacy `agents`/`tasks` records are not connected to an execution adapter. There are no capacity observations, task leases or manager checkpoints. | Cross-platform delegation and token-limit handling remain unimplemented. Verify each actual agent product's execution/usage interfaces, then implement durable attempts, unknown/stale capacity, exhaustion handling and recovery before claiming cross-agent operation. |

## Existing behavior to retain

- Rust/Axum/Tokio, SQLx migrations and bounded SQLite pool/WAL; do not rewrite the app or introduce a general microservice fleet.
- Separate agent and management listeners, origin checks, encrypted provider credentials, token revocation, OAuth PKCE and single-use state.
- Required per-video audience and synthetic-media declarations; no silent change to requested privacy.
- Stable publication request IDs, persisted encrypted YouTube upload session, remote-offset reconciliation and duplicate prevention when completion responses are lost.
- Existing real UI, route persistence, theme, record-level SSE updates and one-command launch. This work does not redesign pages or insert explanatory product copy.
- Provider results must distinguish upload completion, processing and observed visibility. Missing verification must not turn a completed upload into a new publish request.

## Delivery sequence and completion evidence

Each production change gets a focused PR, its own executable acceptance tests and a status update distinguishing mocked, local and real-agent verification. Do not bundle the entire table into one rewrite.

1. **Resumable transfer and artifact lifecycle (R01–R03).** Integrate tusd behind Rust; retain legacy PUT; add durable reservation/transfer reconciliation and safe cleanup. Gate: >100 MB file across bounded requests, interrupted network, process restart, stale/concurrent writes, storage exhaustion, canceled/deleted uploads, completion-response loss, credential revocation and unchanged YouTube publication behavior. R04's transactional journal applies to the new transfer transitions.
2. **Operation reliability (R04, R06–R09, R11).** Apply transactional transitions throughout publishing, typed errors/retry policy, supervision, bounded histories, correlated diagnostics and backup/restore. Gate: crash-injected state/event consistency, unavailable provider without frozen management, graceful and forced restart, replay without irrelevant refetching, and restored fixtures with valid credentials/artifact references.
3. **Independent agent connections (R05).** Issue distinct credentials with platform permissions and resource ownership. Gate: two independent clients, one revoked without affecting the other, denied cross-agent access, correct attribution and migration of the existing Muse connection. Actual named-product compatibility requires live verification; two synthetic clients do not prove it.
4. **Provider settings and delegation (R12–R13).** Deliver settings from the existing capability design, then verified product adapters and capacity-aware delegation. Gate: each supported field's provider round trip, unresolved decisions surfaced to the agent, manager/worker failure recovery, stale/unknown capacity and no duplicate external writes.

R10 is a parallel deployment choice, not a prerequisite to building transfer recovery. A stable domain cannot substitute for R01–R09. No cloud migration, new paid service or domain purchase is part of this audit.

## Verification actually performed

- Existing Rust suite at audited commit: **15 tests passed**. Includes mocked YouTube disclosure/visibility cases and recovery after a lost completion response, HTTP/MCP authentication, OAuth state and workspace history/permissions. This rerun did not publish any video.
- `python3 tests/architecture/tusd_contract.py`: **seven checks passed** against tusd v2.10.1 pinned by image digest. A 104 MiB artifact completed using requests no larger than 8 MiB; resumed a partially transmitted request and SIGKILL/restart; concurrent same-offset requests yielded one success and one conflict; stale retry left offset unchanged; stored SHA-256 matched; deletion and declared-size limit worked.
- The proof used an isolated temporary container and synthetic bytes. It did **not** exercise AgentWay's future proxy/adapter, Cloudflare, Muse, disk-full behavior, power-loss durability or a real provider. The remaining integration gates above are intentionally open.
- Prior user-reported Muse private/public uploads remain the real-provider evidence. They do not establish another agent product's compatibility.

## Release rule

"Implemented" requires integrated code. "Tested" names the actual scenario and environment. "Deployed" identifies the running revision. Passing the dependency proof does not make resumable uploads available in AgentWay yet. Check off the gates with evidence rather than replacing them with a blanket statement that best practices were followed.
