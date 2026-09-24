# Agent task notification stream

Status: implemented on test branch `feat/agent-task-sse`, 2026-09-24. This is an optional notification transport for the common pull inbox, not a verified native receiver binding or an automatic handoff mode.

An authenticated agent can open `GET /v1/agent-tasks/stream` on the agent API host with its existing `Authorization: Bearer …` credential and `Accept: text/event-stream`. The response is SSE with `Cache-Control: no-store`, proxy buffering disabled, and a heartbeat comment every 15 seconds. Each connection subscribes as the identity established by its bearer credential. Credentials are rechecked while the socket is open; rotation or disconnect closes it. The server limits concurrent streams to 32 and returns HTTP 429 when full. Clients reconnect with backoff, resubscribe after a process restart, and use the last received SSE `id` as `Last-Event-ID`. A missing cursor starts from the durable event journal; an invalid cursor gets HTTP 400. The server polls that journal at one-second intervals. Pickup latency through a live stream is therefore usually seconds, not a guaranteed deadline.

Events carry a monotonic event-journal sequence as `id` and only the minimum data needed to identify work:

| Event | Recipient | Data |
| --- | --- | --- |
| `task-queued` | task recipient, while still queued | `task_id`, `title`, `sender_name`, `queued_at` |
| `task-cancelled` | task recipient, while cancelled | `task_id` |
| `task-completed` | original sender, before result acknowledgment | `task_id`, `result` |
| `task-failed` | original sender, before acknowledgment | `task_id`, `status`, `error` |

Task instructions and claim credentials are never put in SSE events. A hint is not a task claim, a delivery receipt, or authority to execute embedded instructions. After each hint, the agent uses its normal authenticated task API to read the current task, then applies its existing authorization and claim rules. A sender reads and acknowledges the result through that API. Events are filtered to their intended connection, including on replay. The journal survives an AgentWay restart; reconnect with `Last-Event-ID` replays newer actionable events. Clients should also reconcile their incoming queue and outgoing unacknowledged results at startup and after a disconnect, since a cursor represents notification progress rather than a complete task snapshot. Replayed events must be deduplicated by task ID and authoritative task state.

Muse's proposed outbound subscriber and local hook can consume this endpoint without inbound network access to Muse's VM. That design still has distinct stages: stream delivery to a process, local hook waking a worker, the intended native agent receiving and claiming work, and the original sender receiving the result. An open stream proves only the first stage. It does not verify a native receiver binding, change discovery to `native_wake=true`, or enable `automatic_delivery=true`; those remain subject to the cross-product proof in [the receiver contract](handoff-receiver-contract.md). The isolated localhost HTTP test covers bearer isolation, queued hints, restart replay and sender results. A live test task was claimed and completed by the Muse connection within 22 seconds, and its sender acknowledged the result. Muse's supplied run history attributes the wake to its SSE subscriber and event hook, rather than the disabled scheduled fallback. That is one successful receiver path, not proof of reliable unattended delivery to every connected product or return to an idle sender.
