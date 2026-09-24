# AgentWay receiver contract

Status: common requirements for feasibility testing, 2026-09-23. **Full wire routes and schemas are not yet frozen; a pull-inbox sender receipt is implemented, but no native receiver has passed the complete suite.** This document specifies what **every** agent must do to participate in handoffs. The [architecture](handoff-delivery-architecture.md) owns the system design; the [compatibility matrix](handoff-receiver-compatibility.md) records whether each product can host a compliant receiver. The [conformance runbook](handoff-conformance-v1.md) is the same test for every product.

## Product boundary

AgentWay is the durable coordinator. An agent integration is both a sender and a receiver, regardless of whether the user's agent is Muse, Grok Bot, Codex, Claude or ChatGPT. Every integration uses the same AgentWay task API, permission checks, delivery envelope, acknowledgment rules, deadlines, error taxonomy and result route. A provider-specific edge may use a different native trigger to start its existing agent, but it may not invent different task semantics. A connected publishing agent is not implicitly handoff-ready.

The receiver has two distinct actors:

1. A **transport adapter** bound to one AgentWay connection can subscribe to or fetch delivery envelopes and report transport admission. It has a receiver-scoped credential and cannot claim work or publish on behalf of the agent.
2. The **native agent/session** is the connected product the owner intended. Once woken, it uses its own AgentWay connection credential to claim/reject a task, read the authorized instructions, report progress and commit a result. It cannot borrow the sender's permissions. The same rule applies when the original sender receives a result.

This split prevents a healthy background socket from being misreported as an agent that actually accepted work. If a product cannot wake the intended native agent or cannot establish that distinction, its integration fails the contract.

The existing pull-inbox API now also has `POST /v1/agent-tasks/{id}/ack-result` (`acknowledge_agent_task_result` over MCP). Only the original sender's authenticated connection can call it after a terminal outcome. It is idempotent and stores `result_acknowledged_at` plus one history event. This is a **sender-declared receipt**, not proof that the result reached a native conversation; a compliant native adapter must call it only after that agent actually receives the result. Older rows remain unacknowledged. New pull tasks also have optional `timeout_seconds` (60–604800; default 86400) and immutable absolute `expires_at`; a server worker makes overdue queued or claimed work terminal `timed_out` and fences late claim/result writes. Legacy tasks retain no invented deadline. This bounds the AgentWay task only: it cannot pause a product-native routine or prove that external effects stopped. These are tested slices of the contract, not the binding, outbox or native-delivery implementation below.

## Wire contract

Use versioned, platform-neutral HTTP/MCP operations. The transport adapter makes an outbound authenticated subscription or bounded long poll to AgentWay, so the user's machine does not need an inbound port. AgentWay stores the envelope until an authenticated acknowledgment or deadline. SSE/WebSocket is a latency optimization; the database outbox remains the source of truth. Native wake is a separate adapter obligation: a successful subscription does not demonstrate that the existing agent can be started.

```json
{
  "protocol_version": 1,
  "delivery_id": "uuid",
  "kind": "task_offered | result_available | cancel_requested",
  "task_id": "uuid",
  "binding_generation": 3,
  "correlation_id": "uuid",
  "expires_at": "RFC3339 timestamp"
}
```

The envelope contains no task instructions, media URL or credential. The native agent fetches content through AgentWay after claiming. The adapter may acknowledge **transport admission** with the delivery ID, binding generation and an opaque native run reference; only the native agent may acknowledge **task acceptance** using its existing connection credential and claim token. The same distinction applies to return delivery. Each operation has an idempotency key or stable ID. A stale binding generation, wrong connection, revoked directed grant, expired offer or duplicate claim is rejected with a typed code, not a generic 500. Every API response includes the authoritative task/delivery state and correlation ID; a retry can read that state before repeating an action.

The binding is an owner-approved association of connection ID, native product, exact target reference, receiver credential hash, generation, proof timestamp and expiry. The target reference and receiver secret are never returned through agent discovery or journal payloads. Changing target or credential increments the generation and invalidates outstanding transport admissions. A binding is only **verified** after an idle task reaches the intended native agent and a result returns to the original sender; heartbeat and provider HTTP acceptance cannot set that flag. A disconnected connection, expired proof or revoked grant becomes unavailable for new automatic handoffs. Existing pull-inbox tasks remain accessible under their original API and are not retroactively marked delivered.

For new automatic handoffs, the server checks both parties' verified, current receiver bindings and the directed grant before accepting the task. It commits task, audit event and outbound delivery in one transaction, or commits none. If either side cannot receive, it returns a typed unavailable error rather than silently falling back to pull. Terminal task state and a result-delivery outbox item commit together. Delivery attempts are separate, append-only records; retries reuse the delivery ID and reconcile the native run reference before invoking a second side effect. The sender's native agent acknowledges reading the result; only then is `result_delivered` true.

Protocol v1 error codes are `receiver_unavailable`, `binding_stale`, `grant_revoked`, `delivery_expired`, `delivery_duplicate`, `claim_conflict`, `claim_expired`, `input_required`, `native_outcome_unknown` and `return_undeliverable`. An error includes a safe correlation ID and recovery action, never a bearer token, native target identifier or task instructions. HTTP status codes distinguish invalid input (400), unauthenticated (401), unauthorized (403), absent (404), state conflict (409), expired (410), accepted but unconfirmed native work (202), and server failure (5xx). Retrying an identical request ID must return the authoritative existing state rather than duplicate work.

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
