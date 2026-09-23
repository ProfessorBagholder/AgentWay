# AgentWay receiver contract

Status: proposed common protocol, 2026-09-23. This document specifies what **every** agent must do to participate in handoffs. It is not implemented. The [architecture](handoff-delivery-architecture.md) owns the system design; the [compatibility matrix](handoff-receiver-compatibility.md) records whether each product can host a compliant receiver.

## Product boundary

AgentWay is the durable coordinator. An agent integration is both a sender and a receiver, regardless of whether the user's agent is Muse, Grok Bot, Codex, Claude or ChatGPT. Every integration uses the same AgentWay task API, permission checks, delivery envelope, acknowledgment rules, deadlines, error taxonomy and result route. A provider-specific edge may use a different native trigger to start its existing agent, but it may not invent different task semantics. A connected publishing agent is not implicitly handoff-ready.

The receiver has two distinct actors:

1. A **transport adapter** bound to one AgentWay connection can subscribe to or fetch delivery envelopes and report transport admission. It has a receiver-scoped credential and cannot claim work or publish on behalf of the agent.
2. The **native agent/session** is the connected product the owner intended. Once woken, it uses its own AgentWay connection credential to claim/reject a task, read the authorized instructions, report progress and commit a result. It cannot borrow the sender's permissions. The same rule applies when the original sender receives a result.

This split prevents a healthy background socket from being misreported as an agent that actually accepted work. If a product cannot wake the intended native agent or cannot establish that distinction, its integration fails the contract.

## Wire contract

Use versioned, platform-neutral HTTP/MCP operations; the wire format below is illustrative and must be finalized before migration. The transport adapter makes an outbound authenticated subscription or bounded long poll to AgentWay, so the user's machine does not need an inbound port. AgentWay stores the envelope until an authenticated acknowledgment or deadline. SSE/WebSocket is a latency optimization; the database outbox remains the source of truth.

```json
{
  "protocol_version": 1,
  "delivery_id": "uuid",
  "kind": "task_offered | result_available | cancel_requested",
  "task_id": "uuid",
  "binding_generation": 3,
  "expires_at": "RFC3339 timestamp"
}
```

The envelope contains no task instructions, media URL or credential. The native agent fetches content through AgentWay after claiming. Each operation has an idempotency key or stable ID. A stale binding generation, wrong connection, revoked directed grant, expired offer or duplicate claim is rejected with a typed code, not a generic 500. Every API response includes the authoritative task/delivery state and correlation ID; a retry can read that state before repeating an action.

| Common operation | Required evidence |
| --- | --- |
| Register binding | Owner-approved mapping from one AgentWay connection to one intended native agent/session, generation and verified capability. |
| Receive envelope | Transport adapter has the durable delivery ID; this is **admitted**, not accepted by the agent. |
| Claim or reject | The intended native agent, authenticated as that connection, accepts the task or returns a typed reason. A lease fences concurrent and late claims. |
| Renew/progress | Agent renews its lease and reports bounded, correlated progress; silence becomes a timeout, not inferred success. |
| Complete/fail | Agent commits an idempotent result or actionable failure. AgentWay records this before initiating return delivery. |
| Deliver result | Original sender's bound native agent/session is woken with a result envelope and acknowledges it after reading. The recipient's completion alone is not return delivery. |
| Cancel | AgentWay records cancellation intent, fences future bridge writes and requests native cancellation where supported; it never claims external effects were undone without proof. |

Task execution, outbound delivery and return delivery are separate state machines. The user-visible milestones are recorded, dispatched, transport admitted, native agent accepted, running, result committed and result delivered. Offline, stale binding, declined, input required, timed out, cancellation requested and outcome unknown are distinct outcomes. At-least-once transport with deduplication is the guarantee; exactly-once external action is not. A lost acknowledgment must be reconciled from the stable delivery/task ID before retrying a side effect.

## Uniform installation and release contract

Provide one AgentWay receiver SDK/conformance harness, not one task workflow per vendor. Each product integration supplies only target binding and native wake/observation functions behind that SDK. Installation verifies an idle round trip to the **exact** selected native agent and a result back to the original sender; a heartbeat or successful MCP call alone does not pass. The binding expires or becomes unavailable when the native target changes or the proof goes stale. A receiver that merely launches a new unrelated model session does not pass.

The harness runs the same cases for every product: idle acceptance in the existing conversation; two-way result return; busy and offline/restart; wrong target; duplicate/out-of-order delivery; lost acknowledgment; claim lease expiry; grant revocation; cancellation race; input required; unsafe task instructions; and an ambiguous native outcome. It records AgentWay IDs, safe errors, measured latency and native transcript/run evidence. Provider-specific behavior is evidence for implementing an adapter, never a reason to weaken the common suite.

Handoffs remain a test feature until **every product advertised for handoffs** passes. If a product cannot host a compliant receiver, AgentWay can retain its publishing connection, but the project must resolve the advertised handoff scope with the owner before any partial release. Do not present a queued pull-inbox task as an unattended handoff.
