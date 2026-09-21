# AgentWay workspace preview

An isolated, interactive **design artifact**, not the production application. No API calls, account connections, credentials, uploads or database writes. Example state lives in memory and resets on document refresh. Links use hash routes; Back/Forward and refreshed routes work.

Run from the repository root:

```sh
python3 -m http.server 8791 --bind 127.0.0.1 --directory docs/design/workspace-preview
```

Open http://127.0.0.1:8791/. This is a preview-specific command; production continues to use `./run`.

Review paths:

1. **Agents:** multiple providers and two agents sharing one account allowance. Open a connection to inspect permissions, pause/resume it, and see honest unknown capacity.
2. **Connect agent:** product and connection name → permissions → platform-specific connection boundary → explicitly simulated verification. Native authorization steps are not invented or implemented here.
3. **Destinations:** account access and publishing defaults; only simulated changes. The catalog distinguishes existing YouTube integration from planned connectors.
4. **Tasks:** progress, partial completion, retrying an optional operation on the same video, and field-level video settings/disclosures.
5. **Settings:** agent connection endpoint.

The intended production behavior, migration, security model and acceptance gates are in [the workspace design](../multi-agent-workspace.md) and [YouTube settings design](../youtube-publishing-settings.md). The static renderer is disposable review code, not a proposed replacement for React/TanStack Query or production resource updates.

Validation: Chromium browser smoke check covered navigation, refreshed deep link, Back, pause/resume, enrollment simulation, caption retry, and narrow viewport overflow. Desktop/mobile screenshots were inspected. This does not certify accessibility or prove any new provider capability. No production code or live data was changed.
