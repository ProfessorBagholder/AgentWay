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
| Grok Bot | Real MCP publishing and a Grok-originated task to the Codex AgentWay inbox were confirmed. | Grok Bot's documented asynchronous Bot-to-Bot messaging is within its own product. No AgentWay-to-Grok Bot wake has been verified. | Not established. |
| Codex desktop | A Codex credential can authenticate, list, claim and complete AgentWay tasks when Codex runs. | The Grok-originated task did not wake the open Codex task. A second `codex app-server` process could read that task but `thread/resume` failed because the desktop app had an active writer. An isolated app-server process could run a *different* ephemeral task; that does not deliver to the user's open task. | Not established. |
| Claude / ChatGPT | Connection products exist in the UI; live AgentWay connector and inbound handoff tests have not been performed. | No exact-product wake route has been verified. | Not established. |

Sources: [Grok Bot collaboration](https://docs.x.ai/grok-bot/chat-and-collaboration), [Grok custom MCP connectors](https://docs.x.ai/grok/connectors), [Muse product capabilities](https://ai.meta.com/muse/), [Claude scheduled tasks](https://support.claude.com/en/articles/13854387-schedule-recurring-tasks-in-claude-cowork), [MCP protocol changes](https://blog.modelcontextprotocol.io/posts/2026-07-28/). These sources describe ways agents work or call tools; none is evidence that AgentWay can wake an existing third-party conversation. The Codex active-writer and ephemeral-session findings are local probes, not a published support guarantee.

## Architecture required before release

1. Define a versioned receiver contract per connection: destination binding, authenticated delivery or pickup, receipt/acknowledgment, heartbeat or liveness, cancellation, completion/failure, and a maximum pickup latency. A bearer token alone is an identity, not a receiver registration.
2. Persist a delivery outbox in the same transaction as task creation. A dispatcher retries with bounded backoff and idempotency, records attempts and safe errors, and never confuses a successful HTTP/MCP call with agent acceptance. A stale receiver becomes unavailable before new work is assigned.
3. Provide a supported receiver adapter for each listed agent product. Use that product's native message/trigger interface if it can address the intended user agent or conversation. If the product only supports scheduled polling, measure its latency and session continuity; do not silently substitute a new agent session or a model API for the user's existing agent.
4. Gate discovery and assignment on a verified, live receiver. Show connection state separately from task-delivery state only where that distinction enables a decision or recovery. Apply the same labels and behavior to all products. Do not fill Agents with speculative per-product controls.
5. Test sender-to-recipient exchange with two actual products, then repeat inbound tests for every product before calling the feature broadly available. Include receiver offline/restart, lost acknowledgment, duplicate event, claim expiry, cancellation race, revoked permission, and an agent that cannot receive work. Verify that the user does not have to copy a task between apps.

The current pull inbox remains useful as durable storage and a diagnostic surface, but is only a lower-level component. Scheduled checks in this one Codex task, browser automation that types into another product, and a separate OpenAI model/API session would not satisfy the product contract.

## Release decision

Keep this branch unmerged as a test integration until the receiver contract and live, equal-behavior adapters are proven. If one product cannot meet the contract, do not advertise handoffs for that product or silently lower the standard for everybody. Shipping authenticated connections and publishing without handoffs remains valid; publishing connection status must not imply handoff readiness.
