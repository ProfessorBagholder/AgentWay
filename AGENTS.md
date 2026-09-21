# AgentWay development

- Work in `~/dev/AgentWay`. Do not use `~/Documents` for this project.
- AgentWay connects users' existing agents to services and to one another. It does not host models or create content itself.
- Treat Muse, Grok Bot, ChatGPT agents and Claude agents as distinct products. Verify their actual connector capabilities; do not substitute coding products or model APIs.
- Infer the narrowest coherent change from the request and existing behavior. Preserve unrelated screens, navigation, styling and behavior. Propose optional improvements separately.
- Implement on a focused branch. Deliver a reviewable PR; do not merge without authorization.
- Before editing UI, inspect the existing screen and code. Review the final diff against the original request and compare rendered screens, not just build/test results.
- Record what was actually exercised. Mock provider tests are not proof that a real account or agent integration works.
- Use mature libraries and async Rust. Keep React updates scoped to the affected records; no full-page reloads or blanket refetches for in-app actions. External OAuth navigation is expected.
- Do not send credentials or tokens to logs, git or chat. Keep management endpoints separate from the agent-facing API.
- Commits use author and committer `ProfessorBagholder <322563513+ProfessorBagholder@users.noreply.github.com>` and `Co-authored-by: Codex <codex@openai.com>`. Preserve the existing identity hooks.
- Relevant checks: `cargo fmt --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace`, `npm --prefix web run format:check`, `npm --prefix web run build`, and browser tests against the running app for changed interactions.
- See `docs/design/` for intended product scope; `docs/STATUS.md` distinguishes implemented behavior from plans.

- UI copy must serve an immediate user decision, status or recovery action. Do not place design rationale, implementation commentary, slogans, obvious feature explanations or redundant instructional footers anywhere in the app. Keep those in design documents or conversation. This applies to prototypes too: no preview/disclaimer banners or repeated explanatory notices in the UI. Put artifact limitations in the README or delivery message.
- Do not render empty tiles or feature placeholders for planned functionality. A section must contain useful information or a usable action; keep future feature plans in design documents. This includes planned-connector tables, coming-soon catalogs and unavailable-feature cards. Audit all screens against this rule before presenting UI changes.
- Responsive layouts must retain meaningful fields and actions. Reflow into labelled rows or cards instead of hiding columns at smaller widths. Distinguish destination account connection status from agents’ permissions to use that account.
- Information architecture: operational activity, events, logs and troubleshooting belong in primary operational navigation or the relevant resource detail, never under Settings. Settings contains configuration. Apply established conventions before presenting designs; do not wait for the user to identify basic hierarchy mistakes.
- Events are user operations, each shown once with consistent agent, destination and outcome fields. Expand an operation for its steps, errors and retries; never flatten mixed component logs into the main event list.
