# Agent handoff implementation plan

Status: **planned; receiver implementation has not started**. Last reviewed 2026-09-23. This is the durable work plan across context compactions and future Codex tasks. Read it with [the receiver architecture](handoff-delivery-architecture.md), [delivery gate](handoff-delivery-gate.md), [prior art](handoff-prior-art.md), [UI system](ui-system.md), and the repository's `AGENTS.md`. Update the checkpoint after each phase or changed feasibility result. This document is not evidence of implemented behavior.

## Resume checkpoint

- Repo: `/Users/md/dev/AgentWay`; branch at plan creation: `feat/agent-discovery-handoff`; draft [PR #19](https://github.com/ProfessorBagholder/AgentWay/pull/19) targets `main`. Check `git status --short --branch`, `git log -1 --oneline`, and PR state before editing; do not trust a saved commit hash over Git.
- Baseline: clean after `1300b2f`. PR #19 contains a permissioned durable **pull inbox**, not unattended receiver delivery. It is deployed only for testing. Do not merge or call it complete. Follow the user's workflow: test branch first; explicit merge approval; then switch/rebuild/restart and verify `main`.
- Real evidence: Grok Bot created task `80774cfa-1d40-444d-bd95-445ba7633d51` for Codex. Codex completed it only after the user relayed the ID. A separate Codex app-server could not resume this desktop conversation while its active writer was held. No AgentWay wake of an existing native conversation is verified.
- Current phase: **0 — native receiver feasibility**. No receiver binding, dispatcher, callback, or return-delivery code exists. Next action: investigate and test supported ingress to real agents; do not begin with UI or a synthetic Codex process.
- Core dependency: supported unattended delivery **into** the intended native agent/session and **back** to the original sender. Never substitute a new model/API session, browser automation, an unsupported app-server writer, a one-off poll, or user relay. Never put tokens, native session IDs or real user task text in Git, test fixtures, PR text or chat.

After each phase update this checkpoint with the date, actual branch/PR and commit, completed steps, automated checks, **live native evidence**, blocker, and single next action. Update `docs/STATUS.md` separately for implemented facts. If an experiment disproves an assumption, correct the architecture and plan before proceeding. An agent's self-report or a mock test alone cannot close a live gate.

## Product contract

An authorized sender assigns a permitted, independently connected agent. AgentWay records the task durably; that intended idle agent receives it without human relay, accepts or rejects it, acts under its **own** grants, and returns a result or actionable failure. The original sender receives the result without human relay. Connection authentication, receiver admission, agent acceptance, execution, result commit and result delivery are distinct facts. All advertised recipients share the same user-visible states, deadlines, diagnostics and recovery behavior; native transports may vary. A mere queued task is never called delivered.

## Phases and exit gates

### 0. Prove native receiver feasibility

Inspect official APIs, plugins and native automation for **Codex desktop and Grok Bot first**, then Muse, Claude and ChatGPT. Record exact product/version, supported ingress, whether it targets the existing agent/session, authentication/owner binding, idle wake, return delivery, cancellation, liveness, limits and vendor restrictions. Use `verified`, `documented but untested`, `unknown`, or `unsupported`; do not infer a product API from its model API, MCP client, or internal agent-to-agent messaging.

Run the smallest isolated harmless probe for each plausible route: idle target receives a task, authenticates back to AgentWay, acknowledges, and returns a harmless result. A product-native recurring routine may qualify only if it is durable, unattended, same-agent, within a measured pickup deadline, and reports failure. No publishing or user-content changes in feasibility probes. Record evidence in a receiver compatibility matrix and update [the delivery gate](handoff-delivery-gate.md). **Exit:** two real products have reproducible supported ingress to their intended existing agents and a credible return route; the full AgentWay-to-AgentWay flow is proven in Phase 3. If two real products have no viable path, stop autonomous handoff work at this gate and report the missing provider capability; do not build infrastructure with no receiver.

### 1. Freeze contract and add state safely

Specify versioned receiver register, dispatch, receipt, observation, result and cancel schemas with typed errors, binding generation and deadlines. Keep task execution state separate from outbound and return-delivery state. Add an **additive** migration after `migrations/0018_agent_handoffs.sql` for binding, delivery outbox/attempt and receipt tables. Preserve every existing task, grant, credential, publication, media ID and request ID. Do not backfill old queued tasks with invented receipts. Commit task + audit event + outbox row in one SQLite transaction.

Add owner-approved target binding and agent-facing receipt APIs behind a disabled flag. Authority comes from the connection credential; a caller-supplied agent ID is never authority. Reject cross-connection binding, stale generation and revoked grants. **Exit tests:** old API compatibility and YouTube regressions, migration on copied old data, failed journal/outbox insert rollback, wrong principal, replayed pairing, stale target and grant revocation. No new UI is needed yet.

### 2. Implement durable delivery with a fake receiver harness

An async dispatcher scans due outbox work at startup and after writes. Bound concurrency, retry/backoff, deadlines and attempts; an in-memory wake signal only accelerates processing. Send a short task/delivery-ID envelope; the recipient fetches task content with its own credential. Store native run correlation and reconcile a lost response before retrying. `admitted` is not `accepted`; a provider HTTP 2xx means only what its docs guarantee. Add a second outbox for delivering the committed result to the original sender. Exhausted retries become visible recovery, never dropped work. At-least-once delivery plus deduplication is the target; do not promise exactly-once external execution.

Build a fake receiver to simulate delayed admission, no agent acceptance, duplicate/out-of-order messages, offline/restart, wrong target, rejected work, input required, cancellation, and lost result delivery. **Exit tests:** crash between task commit and dispatch; restart scans; duplicate/concurrent dispatch; lost admission; expired claim/lease; stale binding; recipient and sender offline; cancel/complete race; idempotent result return; no leaked secrets in errors/events. This harness proves AgentWay mechanics only, not native product support.

### 3. Prove two real adapters in both directions

Implement the first product adapter using only the supported path proven in Phase 0. Pair the owner-approved target; verify idle delivery to the **exact existing native agent/session**, authenticated claim, completion, and return to the original sender. Inspect native transcript/run evidence, not just AgentWay status. Repeat after restart and revocation. Add the second real product and test **both directions**, idle-to-idle without user relay. If a provider only starts a new assistant or cannot return results to the sender's existing session, do not call the adapter complete.

Record per-product route/version, private native correlation, binding evidence, measured pickup and return latency, and failures. Recheck after client or endpoint changes. **Exit:** two real products bidirectional, plus busy-session, offline/restart, duplicate, revoked-grant, cancellation and lost-response cases. Every reported acceptance/delivery has native receipt or visible transcript evidence. Do not extrapolate one product's pass to another.

### 4. Expose verified capability and finish the UX

Only a currently verified binding may appear in recipient discovery, receive a new directed grant or accept `create_agent_task` as automatic handoff. Existing pull-only tasks remain readable/recoverable. If the sender cannot receive an automatic return, reject or explicitly label that limitation **before** accepting work; never silently downgrade. Update agent guidance only when live behavior exists; preserve older MCP/HTTP clients and publishing tools.

Use [the UI system](ui-system.md). Keep Agents rows concise; put handoff readiness and a concrete recovery action in detail only when relevant. Tasks shows title, sender → recipient and truthful state. Activity log holds correlated attempts/errors; link to it from task detail rather than duplicate it. No new nav, placeholder tile, vague online badge or decorative explanation. **Exit checks:** 390/900/1440 px, light/dark, keyboard/focus/labels, no hidden fields, targeted SSE updates and actual recovery states. Browser fixtures are not proof of native delivery.

### 5. Security, operations and release

Review prompt-injection boundary (agent-supplied task text is untrusted), task/asset scopes, callback SSRF, auth in both directions, secret redaction, per-principal rate limits, loop/fan-out/concurrency limits, retention/backup and cancellation honesty. A recipient never inherits the sender's YouTube permission; guessed IDs and duplicate notifications cannot create access or double-publish. Run `AGENTS.md` Rust fmt, strict Clippy, workspace tests, web format/build and affected browser tests. Preserve verified Muse/Grok publishing, credentials, public URL and database through test deployment.

Deploy the **feature branch** as a labelled test build. Record live bidirectional acceptance over real ingress, restart and recovery, matching AgentWay UI/Activity log to native transcripts. Update `docs/STATUS.md`, evidence table and PR text with only verified claims. Keep the PR draft if return delivery or any advertised recipient fails parity. Merge **only** on the user's explicit approval, then switch to `main`, rebuild/restart from `main`, verify connections and public URL, and report the deployed commit. Never call an unmerged test build `main`.

## Implementation map and scope

- Foundation: `migrations/0018_agent_handoffs.sql`, `crates/server/src/publishing/handoffs.rs`, `mcp.rs`, `guidance.rs`, `connections.rs`, and `web/src/workspace.tsx`. Tests include handoff cases in `handoffs.rs` and `web/tests/workspace.spec.ts`. Inspect current paths/callers before editing.
- Preserve `/v1/agents`, `/v1/agent-tasks`, MCP tool names, recipient-generated claim UUID behavior, agent-scoped reads and loopback owner routes unless a versioned compatibility change is justified. Publishing/media behavior is a regression constraint, not an incidental refactor target.
- PR #19 stays draft while it is a pull-inbox prototype. Use focused commits/reviewable diffs. If scope grows beyond one coherent review, split later phases into clearly based dependent PRs; do not merge a partial feature as a complete handoff.
- No new platform connector, dashboard, speculative controls, unrelated restyle, or full Temporal/Redis runtime in this task without measured need. Reuse patterns and standards; check third-party licensing before copying code.

## Stop conditions

Stop a particular adapter if its native route is undocumented/unsupported, reaches a different agent/session, misses the pickup deadline, or cannot provide trustworthy receipt and return delivery. Keep the product's existing publishing connection; mark handoff unavailable, with an accurate reason. If fewer than two products pass the full contract, do not release autonomous cross-agent handoff. Report the provider dependency and continue only with independently useful foundation work.
