# Handoff receiver conformance v1

Status: test contract frozen on 2026-09-23; no product has passed. This runbook tests the native agent users actually connected to AgentWay. Run it unchanged for Muse, Grok Bot, Codex, Claude and ChatGPT before offering automatic handoffs. A mock receiver validates AgentWay mechanics but cannot pass native conformance.

## Evidence required for each connection

Record the AgentWay task, delivery and connection IDs; product and client version; owner-approved exact target (store its reference privately); binding generation; timestamps for creation, transport admission, native acceptance, completion, result admission and sender acknowledgment; native transcript or run references; and safe errors. Never put credentials, task content or private target references in Git, PRs or reports. The owner must be able to inspect a correlated task and Activity log without reconstructing the workflow from separate records.

The tester starts with two idle, independently authenticated native agents, A and B. Neither may be prompted manually after the test begins. A creates a harmless nonce task for B through AgentWay. B must receive it in its existing bound agent/session, claim it with its own credential, return the nonce, and acknowledge any cancellation. AgentWay must commit B's result, notify A's existing bound agent/session, and record A's authenticated result acknowledgment. Reverse A and B. A human reading a task ID in AgentWay, provider acceptance alone, a new unrelated model session, or an agent pulling only after manual prompting is a failed run.

## Mandatory cases

| Case | Pass evidence |
| --- | --- |
| Idle round trip, both directions | Native delivery and sender return receipts with correlated IDs and measured pickup/return times, no human relay. |
| Busy target | Task reaches the same agent after its current turn, once; no substitute session. |
| Offline and restart | Durable task and delivery survive app/server restart; exactly one accepted claim after recovery. |
| Duplicate and lost admission | Same delivery ID/native reference reconciles; no second task or external action. |
| Wrong target or binding generation | Rejected before content is exposed; no misleading delivered state. |
| Revoked grant or disconnected connection | New task denied; in-flight work shows truthful cancellation or outcome-unknown state. |
| Expired offer and claim lease | Stale acceptance/result is fenced; recoverable failure is visible. |
| Cancel/complete race | One authoritative terminal task state; external effects are not falsely described as undone. |
| Input required or native rejection | Sender gets actionable, correlated status without lost work. |
| Return path unavailable | Completed execution remains recorded, but result delivery remains pending/failed with retry and recovery. |
| Untrusted instructions | Receiving agent treats task text as untrusted and retains its own platform permissions and approval rules. |

Every product must meet the same published pickup deadline and retry policy; choose the numeric deadline only after measuring supported native routes. Preserve each provider's native receipt/run evidence. A provider 2xx proves admission only if that provider documents it; AgentWay's native-acceptance milestone requires the agent's authenticated claim. Re-test after an endpoint, credential, agent target or client-version change. No automatic-handoff release follows from only one pair or one product passing.

## Current execution result

Not runnable end to end yet. `/v1/agent-tasks` is a pull inbox; a sender receipt now records result readback separately from completion, but no verified binding, native wake adapter or idle-sender return path exists. The Grok Bot → Codex task `80774cfa-1d40-444d-bd95-445ba7633d51` required owner ID relay. The later task `8387e338-6d2b-4272-b1e6-2924eaaaca78` was picked up by this same Codex conversation through a scheduled heartbeat, without ID relay; Grok Bot was active and polling for its result. That passes only the scheduled Codex pickup slice, not the idle bidirectional round trip or full suite. A synthetic queue/claim test cannot substitute for native evidence.
