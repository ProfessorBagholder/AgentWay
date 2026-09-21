# Implementation status

## Foundation milestone

Implemented: async Rust HTTP server, SQLite schema/migrations, agent registry, idempotent saved assignments, cancellation, durable event replay, same-origin React frontend, selectively subscribed caches, local-only browser protections, Compose packaging, launch scripts and CI.

Not implemented: provider authentication, MCP tool transport, real agent execution or wake adapters, quota observation/admission, manager failover, artifact storage/transfers, social publishing and RSS. Queued tasks intentionally do not run. The UI explicitly marks these limitations.

## Next vertical slice

1. Split application/domain and adapter interfaces as the first real connector is added.
2. Add authenticated remote MCP using the official Rust SDK, while keeping local UI and remote agent credentials distinct.
3. Verify one actual agent product can claim an assignment, report a checkpoint and return an artifact.
4. Connect a second actual provider and test manager-to-worker handoff without manual relay.
5. Add allowance-pool observations and deterministic interruption/recovery tests before unattended execution.
6. Add the first real publishing adapter against an explicitly designated test destination.

Do not substitute coding-agent CLIs or model APIs for the user's Grok Bot, Muse, ChatGPT and Claude agent products.
