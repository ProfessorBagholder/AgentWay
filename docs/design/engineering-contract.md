# Engineering and local-launch contract

21 September 2026. Accepted design requirements, not an implementation-status report. Supplements bridge-design.md v0.2. See [implementation status](../STATUS.md) and the [reliability audit](reliability-audit.md) for implemented behavior, gaps and acceptance evidence.

## Architecture and dependency policy

Use a modular Rust application with explicit boundaries between domain rules, application operations, persistence, provider adapters and transports. One deployable service initially; do not introduce a distributed service fleet for personal use. Domain rules must be testable without network access. HTTP and MCP handlers invoke the same application services and authorization checks.

Use established maintained libraries for runtime, HTTP, TLS, database access, migrations, serialization, authentication primitives and protocols. Review release stability, maintenance, advisories, license, compatibility and transitive dependencies before pinning versions. Prefer the official MCP Rust SDK over implementing MCP framing/session rules ourselves; its maturity and supported protocol features still need an integration check. Custom code is reserved for this product's domain behavior and missing provider-specific adaptations.

Initial selections:

| Concern | Selection |
|---|---|
| Async runtime and cancellation | Tokio; tokio-util cancellation primitives |
| HTTP and middleware | Axum, Tower and tower-http |
| MCP | Official Rust SDK, rmcp |
| Outbound HTTP/TLS | reqwest with rustls |
| Persistence/migrations | SQLx with SQLite initially |
| Serialization/errors/logging | Serde, thiserror, tracing |
| API schema | Generated OpenAPI and TypeScript client/types; validate generator compatibility before choosing its crate |
| Frontend | React, strict TypeScript, Vite, TanStack Query and TanStack Router |
| Accessible components | Established accessible primitives such as Radix; cohesive design tokens and composable components |
| Browser verification | Playwright; component tests with Testing Library and Vitest |
| Packaging | Multi-stage container build and Docker Compose; host launch scripts |

Library choices are specific proposals; Rust, async operation, selective UI updates and one-command launch are requirements. Lock Rust toolchain, Cargo dependencies, frontend dependencies and container build inputs. No runtime installs from floating Git branches or unbounded latest tags. Maintain small architecture decision records for consequential changes.

Sources: [Tokio](https://docs.rs/tokio/latest/tokio/), [Axum](https://docs.rs/axum/latest/axum/), [SQLx](https://docs.rs/sqlx/latest/sqlx/), [official MCP Rust SDK](https://rust.sdk.modelcontextprotocol.io/).

## Async and durability rules

Use bounded queues and explicit per-account/per-provider concurrency limits. Reuse HTTP connection pools. Set operation-specific connection, request and processing timeouts. Handle rate limits with provider retry guidance and bounded jittered backoff only for retry-safe operations. A timeout is not proof that a remote write failed.

Supervise background tasks and propagate cancellation. Graceful shutdown stops admission, drains or checkpoints in-flight work and closes resources within a bound. CPU-heavy hashing or blocking libraries run through bounded blocking workers; stream large files rather than buffering them into RAM. Do not hold database transactions or locks while awaiting external networks. Do not spawn unlimited tasks or use unbounded channels for progress streams.

Use SQLx migrations, foreign keys, indexes and explicit transaction boundaries. SQLite WAL and a bounded pool suit a single server with short writes; configure busy timeouts and test contention. Keep the SQLite database on a local filesystem rather than a network share. Do not assume a future PostgreSQL transition will be automatic. Durable task/outbox state belongs in the database, not only in async channels. Backups must be database-consistent and include artifact references. Schema upgrades must be restart-safe; refuse unsupported downgrades and document rollback/restore.

## Frontend data and rendering contract

Client-side navigation and interactions must not reload the document. Model state by resource identity: individual agent, allowance pool, task, publication and paginated list/filter. Multiple components reading one resource share a cached query; isolated UI updates do not imply a separate network request for every visual element.

Use one authenticated server-sent event stream per application instance for server-to-browser state changes. HTTP handles commands and file transfers. Each event carries a durable cursor, resource ID, revision and minimal authorized payload or invalidation hint. Produce events from committed state through the outbox, so the UI cannot observe a change that later rolls back.

Apply complete event or mutation results directly to the relevant cache entry. Otherwise invalidate only the affected resource and genuinely affected lists/aggregates. Fetch active affected queries; inactive entries remain marked stale until needed. Coalesce bursts, deduplicate concurrent requests and reject out-of-order revisions. Do not clear or refetch the entire application cache after an action.

Configure TanStack Query defaults deliberately: no automatic whole-dashboard polling, no indiscriminate refetch on focus, reconnect or component remount. Stream-controlled resources remain fresh until an event or explicit recovery condition invalidates them. Independently aging resources use a documented expiry policy only while needed. Show quota observation age even if its value remains unchanged.

Establish a race-free snapshot/event boundary: obtain the snapshot cursor transactionally and replay events after that cursor, or subscribe and buffer while loading snapshots. On disconnect, show stale status, reconnect with bounded backoff and replay from the last cursor. If retention has expired, resynchronize only subscribed/visible resources. Never require a page refresh to recover. SSE transport keepalives must not trigger component refreshes or provider API calls.

Use narrow subscriptions/selectors and structural sharing. Preserve form drafts, keyboard focus, scroll and expanded rows during updates. Memoize measured hot paths rather than every component. React may evaluate components without changing the DOM; acceptance focuses on unnecessary network reads, DOM changes and costly render work. Virtualize long histories, paginate on the server and lazy-load routes. Optimistic updates are appropriate for reversible local settings, not invented publication or task-completion states.

Reusable loading, empty, error, stale and reconnect states; responsive layouts; keyboard operation; semantic labels; accessible dialogs; reduced-motion support; and component-level error boundaries are part of delivery.

Sources: [TanStack Query defaults](https://tanstack.com/query/latest/docs/framework/react/guides/important-defaults), [render optimizations](https://tanstack.com/query/latest/docs/framework/react/guides/render-optimizations).

## One-command local operation

After the initial clone, the normal macOS/Linux workflow is:

```sh
git pull
./run
```

Windows receives an equivalent `./run.ps1` launcher. The only intended preinstalled runtime is Docker with Compose and a running compatible container engine; no Rust, Node, database CLI, manual dependency install or hand-edited environment file is needed for normal local testing. The launcher diagnoses missing prerequisites rather than silently installing privileged host software. Initial image builds need network access and take longer than cached starts.

The host launcher validates prerequisites and config, creates persistent data/config paths with restrictive permissions, generates initial secrets once, builds the checked-out revision using locked dependencies, starts Compose, waits for migrations and readiness, and then opens the actual URL in the host's default browser. Default URL: http://127.0.0.1:8787. The frontend production bundle, API and events share this origin. The browser opener runs on the host, not inside the container.

The command is idempotent and reports whether the same stack is already running. Preserve accounts, artifacts and database state across pulls/rebuilds. Health checks must distinguish a live process from readiness. If startup fails, report the failing stage and relevant redacted logs, exit nonzero, and do not open an unusable page.

If port 8787 is occupied by an unrelated process, report the conflict and the one-command override (`./run --port 8788`); never terminate the other process or silently change a port registered in OAuth callbacks. Bind host ports to loopback by default. Explicit LAN mode configures a reachable IP, authentication and appropriate transport security, and opens/reports that address. A headless server prints the URL if no local browser is available. Support `--no-open`, `./stop` and documented logs/backup commands.

Account authorization still happens through the setup UI; a launcher cannot pre-authorize third-party accounts. Optional external connectors being unconfigured must not prevent the local app from starting. Developing frontend HMR or running Rust directly is a separate contributor path, not required for testing the stack.

[Docker Compose readiness behavior](https://docs.docker.com/compose/how-tos/startup-order/).

## Repository, security and verification

Repository layout: Cargo workspace under crates/ (domain, application, storage, adapters, server); frontend under web/; migrations/; deployment/; docs/adr/; tests/; root run and stop launchers. Use consistent formatting/linting, structured errors and request/task correlation IDs. No secrets in source, browser bundles, URLs, build arguments or logs. Serve content-hashed frontend assets with immutable caching and appropriate index caching; notify about a new application version without forcing a document reload.

Authenticate event subscriptions and filter their contents by account/task permissions. Apply request-size limits, origin/host validation, CSRF protections where cookie authentication applies, secure session handling and scoped credential access. Container runs without root where feasible. Use established cryptographic/authentication implementations, not custom algorithms. Keep developer tools and debug endpoints out of the normal distribution.

CI must check Rust formatting/Clippy, frontend lint/type checks, domain and adapter contract tests, migrations against real SQLite, generated schema/client drift, container build, and Playwright flows. Add dependency/license/advisory checks with actionable handling of findings. Integration tests cover interruption, leases, quota recovery and ambiguous publication; mocked providers supplement rather than replace real-provider certification.

Browser acceptance tests must assert: no document navigation/reload during normal actions; one task update causes no unrelated resource reads; duplicate/replayed events cause no redundant fetches; reconnect preserves drafts and catches up; inactive views do not poll; large histories remain usable; access revocation stops sensitive event delivery. Profile representative screens instead of claiming zero internal framework work.

Launch acceptance: on a fresh machine with Docker available, clone plus one command produces a healthy app and automatically opens its IP:port; git pull plus the same command upgrades it while preserving data. Test this first on the user's macOS setup, then on documented Linux/Windows environments. Repository URL, real credentials and explicit live-test targets will be needed at their respective implementation stages, not to finish this design.
