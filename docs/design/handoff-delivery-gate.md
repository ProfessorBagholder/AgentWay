# Agent handoff delivery gate

Status: revised pull-inbox product contract, 2026-09-24. The current feature branch supports a common, truthful pull handoff; **automatic native delivery remains unverified**.

See [prior-art research](handoff-prior-art.md) and the [receiver architecture proposal](handoff-delivery-architecture.md) for existing systems, the proposed implementation, and the product-specific proof still needed.

## Product contract

Every connected agent may use the same permissioned pull inbox: AgentWay stores an authorized task; the recipient sees it when that agent next checks AgentWay, claims it under its own credential, and records a result or failure. The sender sees the result when it next checks and acknowledges only after reading it. Both sides use the same task states, timeout policy, audit trail and recovery. Discovery and UI must say that queued means awaiting pickup, not unattended delivery or acceptance. Initial guidance must describe finite checks and avoid creating an unbounded routine. A connected MCP credential proves that an agent can call AgentWay; it does not prove AgentWay can start a turn in that agent.

Connection authentication and **automatic** handoff readiness are separate facts. Native wake is earned only by a live unattended test for that exact product and connection. Discovery may offer a connected recipient for pull-inbox work, with that delivery mode explicit; it must exclude recipients without verified readiness from any automatic delivery option. Existing queued work remains visible and recoverable. Never rename a merely queued task to “delivered.”

## Current evidence

| Product | AgentWay connection evidence | Inbound delivery evidence | Handoff readiness |
| --- | --- | --- | --- |
| Muse | Real AgentWay-to-YouTube publishing through Muse was confirmed by the user. | One scheduled pickup was confirmed. On 2026-09-24, task `b11862ee-0525-4a33-9c31-13b3a2dc08ea` was claimed and completed by the Muse connection after a live SSE event; Muse's supplied run history attributes the wake to its subscriber and hook, with its scheduled fallback disabled. Active Codex acknowledged the result. | One SSE-triggered receiver path; idle sender return and failure recovery remain unproved. |
| Grok Bot | Real MCP publishing and two Grok-originated tasks to the Codex inbox were confirmed. Grok Bot read a completed result while already active and polling. | First reverse probe `277785fe-44a6-48b7-851f-2ba5f6d67164` failed: the routine reported `never run` and required manual owner prompting. After recreation, task `ce69ec4d-0646-4df7-88fc-2a03a0adc65a` was claimed by Grok on a scheduled run and acknowledged by active Codex. A later idle-to-idle task `d9b6ecc0-ace6-4579-8c93-3078e2f4a648` remained unclaimed for over two routine intervals and was cancelled. A separate one-shot provider webhook accepted task `ccd0ac5b-98c5-45c4-b938-33def097b0a1`, which Grok then claimed and completed. | Event-triggered ingress is feasible; durable dispatch and idle sender return remain unproved. |
| Codex desktop | A Codex credential can authenticate, list, claim and complete AgentWay tasks. | Task `8387e338-6d2b-4272-b1e6-2924eaaaca78` was picked up without owner relay by a scheduled heartbeat in this exact conversation, then completed. This demonstrates same-thread scheduled pickup, not event-triggered wake or a measured production deadline. A second app-server process could not resume the desktop-owned task. | Partial feasibility only. |
| Claude / ChatGPT | Connection products exist in the UI; live AgentWay connector and inbound handoff tests have not been performed. | No exact-product wake route has been verified. | Not established. |

Sources: [Grok Bot collaboration](https://docs.x.ai/grok-bot/chat-and-collaboration), [Grok custom MCP connectors](https://docs.x.ai/grok/connectors), [Muse product capabilities](https://ai.meta.com/muse/), [Claude scheduled tasks](https://support.claude.com/en/articles/13854387-schedule-recurring-tasks-in-claude-cowork), [MCP protocol changes](https://blog.modelcontextprotocol.io/posts/2026-07-28/). These sources describe ways agents work or call tools; none is evidence that AgentWay can wake an existing third-party conversation. The Codex active-writer and ephemeral-session findings are local probes, not a published support guarantee.

## Architecture required before automatic delivery

1. Define one [versioned receiver contract](handoff-receiver-contract.md) shared by every agent, instantiated per connection: destination binding, authenticated delivery or pickup, receipt/acknowledgment, heartbeat or liveness, cancellation, completion/failure, and a maximum pickup latency. A bearer token alone is an identity, not a receiver registration.
2. Persist a delivery outbox in the same transaction as task creation. A dispatcher retries with bounded backoff and idempotency, records attempts and safe errors, and never confuses a successful HTTP/MCP call with agent acceptance. A stale receiver becomes unavailable before new work is assigned.
3. Provide a supported receiver adapter for each listed agent product. Use that product's native message/trigger interface if it can address the intended user agent or conversation. If the product only supports scheduled polling, measure its latency and session continuity; do not silently substitute a new agent session or a model API for the user's existing agent.
4. Gate discovery and assignment on a verified, live receiver. Show connection state separately from task-delivery state only where that distinction enables a decision or recovery. Apply the same labels and behavior to all products. Do not fill Agents with speculative per-product controls.
5. Test sender-to-recipient exchange with two actual products as an intermediate interoperability proof, then run the same end-to-end conformance suite for **every product in the advertised handoff scope** before release. Include receiver offline/restart, lost acknowledgment, duplicate event, claim expiry, cancellation race, revoked permission, and an agent that cannot receive work. Verify that the user does not have to copy a task between apps.

The current pull inbox remains useful as durable storage and a diagnostic surface, but is only a lower-level component. Scheduled checks in this one Codex task, browser automation that types into another product, and a separate OpenAI model/API session would not satisfy the product contract.

## Release decision

The pull-inbox mode can be reviewed as a common capability across connected agents, provided its status and guidance remain truthful and the same operational rules apply to all. Do not release **automatic** handoffs until native ingress and return delivery pass the common conformance suite for every product offered in that mode. A two-product round trip is an intermediate proof, not a platform-wide automatic-delivery release gate. Publishing connection status must not imply automatic handoff readiness.
