# Agent handoff delivery gate

Status: design and integration proof, 2026-09-23. The current feature branch is **not** a complete cross-agent handoff release.

See [prior-art research](handoff-prior-art.md) and the [receiver architecture proposal](handoff-delivery-architecture.md) for existing systems, the proposed implementation, and the product-specific proof still needed.

## Product contract

Every agent offered as a handoff recipient must have the same observable behavior: AgentWay accepts an authorized task, the intended agent receives it without the user relaying it, acknowledges or rejects it, and reports a result or actionable failure. Internal delivery mechanisms may differ, but the task states, timeout policy, audit trail, and recovery must not. A connected MCP credential proves that an agent can call AgentWay; it does not prove AgentWay can start a turn in that agent.

Connection authentication and handoff readiness are separate facts. Handoff readiness is earned only by a live unattended test for that exact product and connection. Discovery and grant creation must exclude recipients that are not ready; the UI must not offer a task-access control that creates undeliverable work. Existing queued work must remain visible and recoverable if readiness later fails. Do not rename a merely queued task to “delivered.”

## Current evidence

| Product | AgentWay connection evidence | Inbound delivery evidence | Handoff readiness |
| --- | --- | --- | --- |
| Muse | Real AgentWay-to-YouTube publishing through Muse was confirmed by the user. | No unattended AgentWay-to-Muse task pickup has been verified. | Not established. |
| Grok Bot | Real MCP publishing and two Grok-originated tasks to the Codex inbox were confirmed. Grok Bot read a completed result while already active and polling. | In reverse probe `277785fe-44a6-48b7-851f-2ba5f6d67164`, Grok's saved routine reported `never run`; the task stayed queued until the owner prompted Grok to pull it manually. Grok later reported that no runnable job file or completion-inbox wake existed. No idle AgentWay-to-Grok Bot pickup is verified. | Not established; reverse unattended test failed. |
| Codex desktop | A Codex credential can authenticate, list, claim and complete AgentWay tasks. | Task `8387e338-6d2b-4272-b1e6-2924eaaaca78` was picked up without owner relay by a scheduled heartbeat in this exact conversation, then completed. This demonstrates same-thread scheduled pickup, not event-triggered wake or a measured production deadline. A second app-server process could not resume the desktop-owned task. | Partial feasibility only. |
| Claude / ChatGPT | Connection products exist in the UI; live AgentWay connector and inbound handoff tests have not been performed. | No exact-product wake route has been verified. | Not established. |

Sources: [Grok Bot collaboration](https://docs.x.ai/grok-bot/chat-and-collaboration), [Grok custom MCP connectors](https://docs.x.ai/grok/connectors), [Muse product capabilities](https://ai.meta.com/muse/), [Claude scheduled tasks](https://support.claude.com/en/articles/13854387-schedule-recurring-tasks-in-claude-cowork), [MCP protocol changes](https://blog.modelcontextprotocol.io/posts/2026-07-28/). These sources describe ways agents work or call tools; none is evidence that AgentWay can wake an existing third-party conversation. The Codex active-writer and ephemeral-session findings are local probes, not a published support guarantee.

## Architecture required before release

1. Define one [versioned receiver contract](handoff-receiver-contract.md) shared by every agent, instantiated per connection: destination binding, authenticated delivery or pickup, receipt/acknowledgment, heartbeat or liveness, cancellation, completion/failure, and a maximum pickup latency. A bearer token alone is an identity, not a receiver registration.
2. Persist a delivery outbox in the same transaction as task creation. A dispatcher retries with bounded backoff and idempotency, records attempts and safe errors, and never confuses a successful HTTP/MCP call with agent acceptance. A stale receiver becomes unavailable before new work is assigned.
3. Provide a supported receiver adapter for each listed agent product. Use that product's native message/trigger interface if it can address the intended user agent or conversation. If the product only supports scheduled polling, measure its latency and session continuity; do not silently substitute a new agent session or a model API for the user's existing agent.
4. Gate discovery and assignment on a verified, live receiver. Show connection state separately from task-delivery state only where that distinction enables a decision or recovery. Apply the same labels and behavior to all products. Do not fill Agents with speculative per-product controls.
5. Test sender-to-recipient exchange with two actual products as an intermediate interoperability proof, then run the same end-to-end conformance suite for **every product in the advertised handoff scope** before release. Include receiver offline/restart, lost acknowledgment, duplicate event, claim expiry, cancellation race, revoked permission, and an agent that cannot receive work. Verify that the user does not have to copy a task between apps.

The current pull inbox remains useful as durable storage and a diagnostic surface, but is only a lower-level component. Scheduled checks in this one Codex task, browser automation that types into another product, and a separate OpenAI model/API session would not satisfy the product contract.

## Release decision

Keep this branch unmerged as a test integration until one common receiver contract and live, equal-behavior integrations are proven for every agent product in the advertised handoff scope. A two-product round trip is an intermediate proof, not a platform-wide release gate. If one offered product cannot meet the contract, do not ship a privileged subset or silently lower the standard; resolve the scope with the owner first. Shipping authenticated connections and publishing without handoffs remains valid; publishing connection status must not imply handoff readiness.
