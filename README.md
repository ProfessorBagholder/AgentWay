# AgentWay

A personal bridge between the AI agents you already use and the places you publish.

**Status: first Muse-to-YouTube publishing path verified.** The Publishing screen configures Google OAuth, displays upload progress/results, and supplies authenticated HTTP/MCP connection instructions. Media transfer and resumable YouTube uploads are implemented. The user completed a real Muse-to-YouTube upload and confirmed playback and Private visibility. Automated failure/recovery tests additionally use a simulated provider. Cross-agent execution, quota-aware delegation and other publishing destinations remain unimplemented. Saved agent records and tasks still do not execute.

## Run locally

Prerequisite: Docker with Compose installed and the container engine running.

```sh
git clone https://github.com/ProfessorBagholder/AgentWay.git
cd AgentWay
./run
```

The launcher builds the checked-out code, applies migrations, waits for readiness, and opens the browser. Subsequent updates:

```sh
git pull
./run
```

Default address: loopback port 8787. Use `./run --port 8790` for another port or `./run --no-open` on a headless machine. Windows: `./run.ps1` (PowerShell); use `-Port 8790` or `-NoOpen`. Stop with `./stop` or `docker compose stop`. Logs: `docker compose logs -f`.

Data lives in the `agentway_agentway-data` Docker volume and survives rebuilds and stops. **Do not run `docker compose down -v` unless you intend to erase your data.** The management interface accepts only local browser access. A separate bearer-authenticated agent listener uses port 8788. No model API keys are needed.

## Current behavior

- Register actual agent identities and manager/worker roles. Every registration is clearly unconfigured.
- Save and cancel assignments. Idempotent task requests avoid duplicate assignments.
- Configure YouTube OAuth and receive real upload jobs from existing agents through HTTP or MCP.
- Watch changes arrive in other open windows through replayable server-sent events. Mutation responses update individual resource caches; no document reload or dashboard refetch.
- Restart the application without losing saved state.

## Architecture

Rust/Tokio + Axum + SQLx/SQLite. React/TypeScript + TanStack Query. Vite runs at build time; Rust serves the compiled frontend and API on one origin. There is no Node server in the running container. Native SQLite event polling is currently bounded at two reads/second per connected browser; browsers do not poll API resources. This simple implementation will gain centralized event fanout and retention before remote deployment. Bootstrap currently loads all locally saved registrations/tasks; pagination is a pre-scale milestone.

Publishing uses the official Rust MCP SDK, oauth2, reqwest and authenticated encryption. See [YouTube setup and Muse connection](docs/youtube-publishing.md) for account setup, HTTPS access, limits and verification status. For a temporary hosted-agent endpoint, use `./run --share`.

## Contributing and checks

Rust toolchain is pinned in `rust-toolchain.toml`; frontend dependencies are locked by `web/package-lock.json`.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm ci --prefix web
npm run build --prefix web
```

For browser tests, start the local stack, then run `cd web && npx playwright install chromium && npm run test:e2e`. Tests create uniquely named registrations and tasks in the test server. Use `AGENTWAY_TEST_URL` to target a disposable instance; don't target a workspace containing important data.

For native development, build the frontend then `cargo run -p agentway-server`. Override `DATABASE_URL`, `ASSET_DIR` and `BIND_ADDR` as needed. Frontend-only development can use `npm run dev --prefix web`; this is not required for normal use.

## Design

- [Architecture and recovery](docs/design/bridge-design.md)
- [Engineering and startup requirements](docs/design/engineering-contract.md)
- [Publishing destination catalog](docs/design/connector-package-plan.md)

The design documents describe the target, not current connector coverage. No live social publishing has been performed.
