# AgentWay workspace design

Status: design proposal, not implemented. The accompanying interactive preview uses illustrative data and never calls AgentWay, agents, or social APIs. This document supersedes the UI and connection-model assumptions in earlier plans where they conflict. The proven Muse-to-YouTube path is a regression requirement.

## Product model

AgentWay gives the user's existing agents access to publishing destinations and to one another. Agents create, plan, judge, select workers and make editorial decisions. AgentWay authenticates requests, enforces grants, moves files, delivers authorized assignments, records results and recovers infrastructure operations. It neither hosts models nor becomes another creative workspace.

The whole application must answer: which agent can do what, through which account, what is happening, and what needs the user's action? The current single shared credential, manually assigned Muse label, singleton YouTube account and disconnected saved-agent/task records cannot support that product. A visual refresh alone cannot fix them.

### Evidence baseline

- Live user validation: Muse via HTTP connector uploaded private and public YouTube videos. User demonstrated playback. Actual visibility readback worked against the live account.
- Automated evidence: MCP protocol/tool calls, resumable uploads, authentication, idempotency, current visibility reporting, UI event updates and navigation.
- Unverified: live connections from ChatGPT agents, Claude agents and Grok Bot; independent identities within a platform; cross-provider wake/delegation; usage-limit reporting; additional destinations. Do not substitute Codex, Claude Code, Muse Code, Grok's model API or another product for those products.
- Current screens show real publishing jobs but only one owner-labelled shared connection. That label is not proof of a particular client's identity.

## Information architecture

Five primary destinations, one sidebar, one wordmark. No overview dashboard, duplicate activity feed, slogan panel, ambient “Live/Local” badges or decorative metrics.

| Area         | Primary question                         | Contents                                                                                                   | Primary action                       |
| ------------ | ---------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| Agents       | Which of my agents can use AgentWay?     | Independently connected agents, authentication, permissions, task delivery and shared allowance accounts   | Connect agent                        |
| Destinations | Where may they publish?                  | Named social accounts/channels and podcast feeds, supported formats, permissions and connection issues     | Connect destination                  |
| Tasks        | What is happening and what needs action? | Actual agent-requested operations and delegations, related attempts, artifacts and per-destination results | Contextual recovery, not “Save task” |
| Events       | What happened, and why?                  | Cross-task event chains, connection failures and correlated errors                                         | Filter events and export diagnostics |
| Settings     | How is this installation configured?     | Reachability, storage/retention, backup, deployment access                                                 | Contextual settings actions          |

Agents is the first-use landing page. After setup, Tasks is the default when opening the root route; bookmarked/deep-linked routes always win. Connections can be made in either order. Do not require a manager or backup before a simple upload. A single publishing agent is a complete useful configuration.

Agents owns agent endpoint/authorization instructions. Destinations owns YouTube OAuth and publishing defaults. Tasks owns task outcomes and task-scoped event chains; Events owns the cross-task operational journal. Settings owns external URL/tunnel details. These responsibilities replace the ambiguous Publishing screen deliberately, in a later implementation slice; do not silently move controls during unrelated fixes.

Stable URLs: /agents, /agents/:id, /agents/connect; /destinations, /destinations/:id; /tasks?state=attention&agent=:id, /tasks/:id; /settings/connections. Hash routing may encode the same hierarchy for local packaging. Production uses TanStack Router with typed route/search state. Refresh, Back/Forward, middle-click and copied links must work. Dialog steps and details have routes; closing returns to the prior filtered list and scroll position. Use real navigation links, not clickable nonsemantic rows.

## Agents: first-class independent connections

An agent row contains owner-chosen name, exact product, account label, authorization state, delivery capability and allowance summary. A provider icon is secondary to text. Two Claude agents have distinct rows and credentials; if they share an account allowance, both reference the same pool rather than showing two independent budgets. There is no required manager/worker dropdown at connection time.

Open a row to see Connection, Access, Capacity and Activity. The detail view exposes the connection's evidence and when it was last checked. Rename and pause are ordinary actions. Reconnect preserves identity and history. Revoke requires a concise impact confirmation: which pending assignments pause and which already-started external operations may complete. Never delete publication history or pretend revocation undoes a published video.

Connection status is scoped and literal:

| State            | Evidence and UI treatment                                                                           |
| ---------------- | --------------------------------------------------------------------------------------------------- |
| Setup incomplete | Saved connection intent, no authenticated round trip; Resume setup                                  |
| Authorized       | Current credential plus successful initial authenticated call; show last request time, not “Online” |
| Reconnect needed | Expired/rejected/revoked authorization; inline reason and Reconnect                                 |
| Paused           | Owner stopped new bridge work; existing attempt behavior stated                                     |
| Revoked          | Credential unusable; history retained, reconnect creates a new credential version                   |

Delivery is independent: “While active”, “Scheduled check · every N minutes”, “Can be triggered”, or “Not verified”. Only label a wake route after testing the exact product/account. MCP connectivity does not imply that AgentWay can start an idle agent. “Authorized” is not proof of current runtime availability.

### Connection flow

1. **Choose the actual agent.** Product and connection name, with an optional account label. Preserve multiple agents on the same product. Show only supported connection methods for the selected exact client. Unverified products are explicitly “Compatibility test”, with no promise of unattended work.
2. **Choose access.** Destination accounts and concrete operations, plus delegation targets. Clear presets: read status, prepare/transfer files, upload privately, publish publicly; detail can expand to native operations. Public access is explicit. Delegation is separate from publishing. Start with no grants; a purpose choice selects a visible preset for review. No destinations yet offers Connect destination or Continue without publishing access.
3. **Connect in the existing agent.** Prefer a verified native install/deep link and OAuth when the client supports it. Otherwise show one endpoint copy control and that client's secure credential field instructions. Technical transport details are secondary. Do not copy secrets into an agent conversation or generic prompt. No native password form inside AgentWay. Server setup state survives refresh and can resume.
4. **Verify automatically.** Receive an authenticated handshake, bind the credential to the pending connection, validate tool access and record a capability report. A safe read check is automatic; sample upload is a separate explicit test. Confirm the named destination and allowed actions. Agent row appears without a second “Add agent” step.

The existing agent may initiate setup instead: an enrollment link leads to the same owner-controlled authorization screen; approval links it to a pending connection or creates one. A client-provided name/platform is advisory until confirmed by the owner. Reject duplicate enrollment redemption; do not deduplicate agents by display name.

Preparation does not start an OAuth timer. Create the short-lived OAuth transaction only when the user chooses Continue to authorization. Show the actual remaining time if expiry matters; refresh the authorization attempt without losing agent name or permission choices. The preparatory wizard can last indefinitely. Respect provider-mandated token lifetimes rather than lengthening them blindly. Open login in the user's normal browser; offer a normal copyable link if opening fails. Never require the automation browser for credentials.

If the public endpoint changes, mark affected connections “Address changed”, retain grants and history, and present the specific reconnect step. A random trycloudflare hostname is not a persistent identity. The product can offer the existing temporary route without requiring a domain purchase. Stable ingress is a deployment choice with explicit prerequisites, not an invisible paid service.

### Connection verification record

Store observed product/client version if available, transport, authenticated principal, installed tools, completed test, timestamp and evidence source. Capabilities are individually verified: call tools, transfer files, receive assignments, wake while idle, checkpoint, cancellation, usage observation, and execution settings. “Unknown” and “not supported” are different. Re-test only affected capabilities after endpoint/client changes.

## Destination accounts and grants

A destination is an account/channel/feed, not a provider logo. One owner may connect two YouTube channels with different grants. The agent must address destination_id explicitly once more than one is available; never choose the first account as a hidden default. Display the channel identity returned by the provider before confirming connection.

The connection flow checks prerequisites before launching OAuth. Where user-managed developer credentials are unavoidable, explain once why and give exact callback/copy controls; validate before continuing. Do not promise a universal one-click OAuth flow for distributed self-hosted apps that require confidential developer credentials or provider review.

Destination detail shows supported actions, connected agents and their grants, defaults/constraints, authorization health and recent related tasks. Editing a grant has one canonical server record, visible from either the agent or destination view. A manager badge cannot confer permission. Remove/reconnect one account without affecting others. Provider credentials stay on the bridge; agents receive scoped handles, not Google refresh tokens.

Only installed, exercised connectors are offered as functional choices. Keep planned platform catalogs out of the normal connect picker. The design can accommodate YouTube, TikTok, Instagram/Facebook Reels, podcast hosts/RSS and promotional destinations without implying they exist now. Platform formats and native linking rules remain explicit capabilities.

## Tasks: requests, attempts and results

A task represents an actual request accepted by AgentWay. No freely editable “queued” record that can never execute. The initial adapter wraps the current publication jobs; richer requests later group several operations. The bridge groups by an explicit parent/request ID supplied by the agent, never by guessing from titles or silently planning a campaign.

List columns: task, originating agent, destination/assignee, state, last update. Filter by Needs attention, In progress, Completed or All, with optional agent and destination filters. Counts derive from the matching dataset; no disconnected headline KPIs. Sort newest activity first; preserve a selected row and announce changes without moving focus. Paginate on the server.

Task detail contains the submitted objective/metadata, originating agent, current assignee, attempts, related artifacts, chronological events and destination outcomes. For cross-posting, one successful destination cannot make the whole task successful. Example: YouTube published, Instagram authorization expired: show “Needs attention · 1 of 2 destinations complete” and Reconnect Instagram. Already successful uploads are never repeated by retry-all.

States distinguish: waiting for agent, waiting for allowance, queued, running, processing at destination, completed, needs user action, interrupted, failed, cancellation requested, cancelled and outcome unknown. Use short labels with a concrete reason; distinguish “uploaded” from verified published visibility. Display requested and observed visibility separately with observation time. AgentWay cannot certify Shorts classification from a vertical file alone.

Recovery belongs next to the failure: retry a recoverable transfer, reconnect one destination, wait until a reported reset, or request reassignment by the manager. Never label a byte-transfer restart as a safe retry when remote publication is uncertain. Preserve idempotency IDs and reconcile provider receipts first. Cancellation says what it can stop and what has already completed. Copy redacted diagnostics is secondary.

Approval UI exists only when required by an owner policy or provider. Healthy pre-authorized publishing runs without a new dashboard confirmation for every action. No generic safety modal for every normal operation.

## Events and troubleshooting

Task detail includes an **Events** tab for the full observed execution chain. Events in the main navigation is the installation-wide entry point, including enrollment, OAuth callbacks, authentication rejection, tunnel/reachability, worker startup, storage and database failures that precede task creation. Agent and destination details link into this same journal with resource filters; do not create disconnected log views.

Capture request receipt, authentication and grant decisions, validation, admission, delegation dispatch/acknowledgment, allowance waits, artifact transfer, queue transitions, each provider operation, processing observations, verification, failures, retries, cancellation and recovery. Preserve each attempt and its causal links; retry success never erases the original error. Client-visible tasks are scoped to the principal; installation diagnostics are owner-only.

Each structured event has a stable ID, durable sequence, occurred_at and recorded_at, severity, source/component, operation, trace/span/parent IDs, request/task/parent-task/attempt IDs and relevant principal/destination references. Show durations, HTTP status, provider error code, sanitized message/cause chain and actionable recovery when available. Logs and UI share correlation IDs. Use Rust tracing and standard OpenTelemetry propagation where supported, alongside a persisted SQLite event journal; standard telemetry alone is not durable task history. Server warnings/errors must reach the journal or a correlated diagnostic record, not only terminal output.

Agent-originated steps must be explicitly reported or observed; do not fabricate the internal reasoning or unseen actions of a remote agent. Mark trace boundaries and missing observations when a provider offers no telemetry. Track delegation receipt, worker result and exhaustion separately. Do not describe a trace as complete when evidence is missing.

The user-facing event is one operation, such as publishing a video. List it once with a consistent action, subject, agent, destination, started time and outcome. Expand it to see ordered steps, component diagnostics, errors and attempts. Muse, AgentWay and YouTube are components participating in that operation, not three peer event categories. Use the same hierarchy in global and task-scoped views. Group by durable operation/trace identity, never time or title heuristics. Filtering for errors selects operations containing errors and retains the full child chain; a successful retry stays under its original operation. Default to chronological steps inside each operation, with causal nesting for delegation and parallel operations. Offer errors/warnings, source, attempt and time filters, plus search by error text or correlation ID. Errors are expandable in place; display details and export/copy controls without forcing a terminal visit. A full-chain export includes filtered-out events and child attempts within the owner's requested scope; label time range and truncation. Deep links identify the exact event and preserve filter context.

Persist state changes and their event/outbox records transactionally. Use bounded async collection, indexed pagination and revisioned SSE for only the active subscription. Never rebuild the whole screen or collapse open error details on new events. Auto-follow is optional; scrolling back preserves position and offers a new-events indicator. Retention gaps and dropped records are explicit. Keep meaningful transition/error events without storing every byte/chunk as a separate row.

Redact before persistence and export, using allowlisted provider-response fields. Exclude Authorization headers, OAuth codes, cookies, refresh/access tokens, API keys, signed URLs, resumable session URLs, raw prompts, media and unrestricted request/response bodies. Preserve useful sanitized stack/cause context for internal failures; escape external messages as text. A provider message may contain secrets even when its field name looks harmless. Audit exports and apply owner access checks without leaking diagnostics through agent tools or public endpoints.

When the database or event sink itself fails, retain a bounded sanitized fallback log and expose journal health once management access recovers. A stopped server cannot display its own UI; the launcher must report startup failures and the local diagnostic file location. Document this limit instead of implying that all outages are observable from the app.

Acceptance: one failed provider operation is traceable from the originating request through every attempt; delegation parent/child chains are navigable; a retry preserves errors and the original publication ID; pre-task auth failures appear in installation events; reconnect/restart preserves history; sentinel secrets never appear in storage, UI or exports; pagination/SSE gaps are visible; UI filters and exports remain scoped; only affected rows update. Fake event fixtures validate the design, not production logging coverage.

## Delegation and allowance-aware operation

Delegation configuration lives in the manager agent's Access detail, not a duplicate “team builder”. Select allowed worker agents, permitted execution presets, task/context scope and optional fallback. Any agent can be a manager, worker or both in a particular task. Display “Coordinates tasks” only if configured, not as a provider-defined identity.

The external manager chooses the worker, task and effort. AgentWay checks grants, capabilities and account availability; it transports the request. Settings such as model/effort are shown only when an exact client's adapter can actually apply them. Native subscriptions remain native; no model API key is requested as a substitute.

Allowance is associated with an account/pool and may have several windows. Each observation has unit, value, observed_at, source, expires_at and reset_at where known. Tokens, credits and percentage are not interchangeable. A context window is not an account usage balance. Two agents sharing an account share a pool; reservations are local estimates, not provider-side guarantees.

UI example: “Not reported” with last attempted check; “18% remaining · weekly · reported by agent · 12 min ago”; “Limit reached · reset time not supplied”. Do not show fake zero or full bars. Mark stale data. Do not imply that checking the account in a browser produces a continuously reliable API feed. No usage observation = explicit uncertainty, not impossible connection.

If capacity is unknown, default to one outstanding delegated assignment per pool, mark the estimate, and use checkpoint/recovery. Owner can require a fresh observation before dispatch. Exhausted pools stop new admissions, retain work and display the reason. Wake failures become “Waiting for agent”, not inferred quota exhaustion. All workers exhausted means durable waiting, not an invented fallback model.

Manager exhaustion requires a pre-authorized backup, a verified wake route, an explicit checkpoint and a fenced coordination lease. Otherwise pause with a recoverable reason. Never claim transfer of hidden reasoning or native memories. Native tasks may continue after the bridge loses contact; a cancelled lease prevents stale bridge writes but does not magically stop provider inference.

## Visual and interaction specification

Use a restrained neutral palette, one accent for actions/selection and distinct semantic colors accompanied by text. System sans-serif font; 14px body, 12px metadata, 28px page title; comfortable line height; 4/8px spacing rhythm. 208px desktop sidebar, fluid content capped around 1200px; 40–44px controls; 8px surface radius. One wordmark, no second initial-logo. Density comes from aligned rows and clear hierarchy, not full-screen empty cards or oversized marketing headings.

Primary action is page-specific. Agent/destination details use route-backed side panels on desktop and full-width pages/dialogs on small screens. Consistent fields, segmented filters, status labels, menus, loading/empty/error states and confirmation patterns. Destructive controls sit in a secondary area. No “Live”, “Local”, “your infrastructure” slogans or unexplained internal acronyms in primary content.

Production primitives: React + TypeScript + TanStack Query/Router, established accessible dialog/menu/tab primitives (Radix proposal), existing Lucide icons. Reuse the current Rust/Axum/Tokio/SQLx core and official MCP SDK; evaluate maintained OAuth authorization-server components rather than building an authorization server from scratch. Vite remains build tooling, not a second production server.

Accessibility target WCAG 2.2 AA: semantic tables/lists and links, text labels, visible unobscured focus, focus trapping/restoration for dialogs, keyboard access, inline errors associated with inputs, restrained live announcements, minimum contrast 4.5:1 for normal text, usable targets, reduced motion, 200% zoom and 320px reflow. Desktop tables become labelled stacked rows rather than horizontally clipped mini-tables. Async events never steal focus or reset drafts.

## Publishing settings are part of the agent contract

An agent must be able to discover and set all appropriate destination-specific publishing settings, including disclosures, without a separate manual UI step for API-supported operations. See [YouTube publishing settings](youtube-publishing-settings.md) for the capability matrix, validation, orchestration and verification contract. Destination detail owns defaults and owner policy; task detail shows requested, effective and observed values. Defaults never silently answer content-dependent disclosure questions.

## Frontend contract

Resource keys: agent/id, credential/id, destination/id, grant/id, allowance-pool/id, task/id, attempt/id and filtered paginated lists. Mutation results patch affected records. Transactional SSE/outbox events carry ID, revision and cursor. Reject old revisions, deduplicate requests, and update only rows, memberships or counts affected by an event. One resource can feed several components without separate network calls.

No document reloads, periodic whole-app fetches or focus-triggered blanket invalidation. Expiring capacity observations get resource-level stale treatment while visible. SSE reconnect replays a cursor; a retention gap rehydrates only active resource subscriptions. Snapshot/event boundaries must not drop events or overwrite newer state. Route/filter changes preserve scroll, drafts and keyboard focus. Native OAuth navigation is a distinct external journey with saved return state.

Empty Agents: “Connect your first agent” with one actual setup action. Empty Destinations: “Connect a publishing account”. Empty Tasks: “No tasks yet. Requests from your agents appear here.” Errors show the affected service and next action. Placeholder test records never seed the user's database.

## Backend changes required by the design

| Entity                         | Key responsibilities                                                                                                                                             |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| agent_connections              | Stable UUID, owner name, exact product, native identity when verified, client installation, account/pool reference, setup/connection state, revision             |
| agent_credentials              | Principal reference, hashed bearer-secret verifier or OAuth grant reference, audience, version, expiry and revocation; never plaintext secrets in event payloads |
| agent_capabilities             | Operation, support/evidence state, tested client, observed_at; separate from grant                                                                               |
| destination_accounts           | Provider/account identity, credential reference, enabled operations, defaults, authorization status                                                              |
| grants                         | Principal, resource and operation scope; explicit delegation edges and task-limited grants                                                                       |
| allowance_pools / observations | Shared funding identity, windows, source, freshness and enforcement limits                                                                                       |
| tasks / attempts               | Origin principal, assignee, parent ID, idempotency key, lease/fence, checkpoint, durable state                                                                   |
| publication_operations         | Task, destination, original request, media, provider receipt, observed outcome                                                                                   |
| setup_intents / outbox         | Resumable enrollment and transactional events/delivery                                                                                                           |

Authentication middleware resolves a credential into a principal; all HTTP/MCP operations receive an AuthContext. Do not accept agent_id in request JSON as authority. Check principal grants when admitting work and again immediately before a provider write or resumed upload. Key task idempotency by principal + request_id. Authorize every media read/write, task query, retry and artifact reference. Unique endpoint paths aid enrollment/routing but are not secrets or authentication.

If a native product shares one connector configuration among several agents, record that connection as shared. Do not promise independent revocation or attribution without independently issued credentials or a verified native identity claim. Owner labels do not turn self-reported identity into cryptographic evidence. Connection uniqueness is credential/installation identity, not product name.

Use standard MCP protected-resource discovery and OAuth audience/scope validation for compatible clients. An OAuth client ID identifies software, not a particular agent. Pairing binds the owner's authorized connection to the resulting grant. Static-token fallback remains per connection; never one installation-wide token. Refresh/rotation must invalidate the intended principal only. Auth tests must span both transports and long-lived sessions.

### Migration without breaking Muse

1. Snapshot/backup metadata and schema before conversion; preserve vault key and media. Never write credentials into Git.
2. Create a legacy principal representing the current shared credential. Preserve its existing effective YouTube permissions and endpoint, idempotency behavior, queued jobs and resumable sessions. The displayed name may remain Muse, but historical attribution remains “Legacy shared connection”; do not retroactively assert that all requests came from Muse.
3. Migrate the singleton YouTube account into destination_accounts with a stable mapping. Associate old jobs with the legacy principal and destination. Preserve provider IDs, URLs and visibility observations.
4. Issue a new scoped credential for Muse through the new flow. Leave the legacy credential valid during an explicit migration window. Identify any pending operations before offering revocation. Revoke legacy only after successful new-client verification and owner action; no surprise rotation.
5. Require independent credentials for a second agent and test that revoking it cannot affect Muse. Pause/reconnect must retain history and recoverable jobs.
6. Retain old URLs through redirects/route aliases and a compatibility API layer until migrated; queued/resuming operations must pass the destination/grant policy corresponding to their admission plus any later revocation.

## Implementation slices and release gates

1. **Identity and authorization foundation.** Principal/credential/destination/grant schema and compatibility adapter. Tests: cross-agent media/task isolation, independent revocation, stale MCP sessions, per-principal idempotency, restart and migration with existing completed/in-flight jobs. Existing Muse private/public publishing must still work.
2. **Agent and destination management.** Implement route shell and components from this design; guided native connector enrollment, persistent setup state, live verification and capabilities. Support multiple independently authenticated clients and multiple destinations in the model. Expose only exercised adapters. Real second-product test is the release gate; user supplies native authorization, not UI design decisions.
3. **Task model, publishing settings and recovery.** Implement the YouTube settings contract, including discovery, disclosures, per-field verification and capability gaps. Wrap current uploads in task/attempt/outcome records, preserve live per-row updates, add attribution and partial-result views. Verify retries do not republish completed outcomes. A single operation still gets a simple detail view.
4. **Delegation and capacity.** Exact-product wake/pull adapters, shared allowance pools, checkpoints, leases and fallback. Before claiming autonomous cross-platform delegation, demonstrate manager→different-provider worker→result, worker exhaustion and manager exhaustion recovery. Provider limitations remain visible.
5. **Destination expansion.** Add format/account adapters from the destination catalog after their access gates; reuse identity, grants, artifacts and outcomes. No redesign per platform.

Each slice is a focused PR with visual and behavioral checks, no unrelated restyling. Do not swap out the current interface merely because the prototype exists. No whole-product completion claim until the acceptance gates are met. Free/local-first operation and ./run remain requirements; Cloudflare deployment stays a later adapter/runtime decision.

## Design acceptance checklist

- Two agents from the same provider and one from another provider can be represented without ambiguous identity or duplicate quota pools.
- An owner can connect, restrict, pause, reconnect and revoke one agent independently; setup survives refresh/expired authorization.
- Agent-originated setup and UI-originated setup converge on the same credential/grant model.
- A grant change is reflected from both agent and destination detail; roles never escalate permissions.
- A worker with no publish grant cannot upload publicly through either protocol, guessed IDs or a manager's artifacts.
- Task history contains only real accepted requests. Partial completion and uncertain publication are not global success.
- Capacity is useful even when unknown, shared or stale; supported model/effort controls are not invented.
- Back/Forward, deep links, keyboard, small screens and targeted updates behave consistently.
- New tests use isolated state; no fabricated records in a user's running installation.
- Muse's current working connection survives migration. Other products become “verified” only after actual tests.

## References

- [MCP authorization specification](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization): protected resources, scopes, audience binding and client interoperability. Pin and test the version supported by each actual client.
- [WCAG 2.2](https://www.w3.org/TR/WCAG22/): accessibility requirements; a visual preview alone does not establish conformance.
- Existing repository: engineering-contract.md, bridge-design.md, connector-package-plan.md and ../STATUS.md. Earlier provider claims are hypotheses unless backed by the evidence baseline above.

## UI copy rule

Across every screen, include only labels, meaningful status, and concise help necessary for a decision or recovery. Omit slogans, design rationale, implementation commentary, obvious explanations and redundant footers. Keep technical/design discussion in this document. Prototypes follow the same rule: no preview banners or disclaimer furniture. Document artifact limitations in the README and delivery message. Group destinations under prominent platform headings, with account identities below.
