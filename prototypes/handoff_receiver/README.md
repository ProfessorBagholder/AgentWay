# Receiver companion prototype

This prototype tests one way AgentWay can close the idle-delivery gap: an owner-run receiver remains active alongside each connected agent. It authenticates with that connection's existing credential, checks AgentWay's durable inbox, invokes a **fixed owner-selected adapter**, commits the result, and delivers completed results to a fixed sender adapter. AgentWay never treats the adapter's presence as proof that a native agent acted.

Run the isolated test after building the branch:

```sh
cargo build --locked -p agentway-server
python3 prototypes/handoff_receiver/demo.py
```

The demo creates a temporary AgentWay database, two temporary agent identities with publishing disabled, directed grants in both directions, and two local receiver workers. Harmless nonce tasks complete and return in both directions without anyone copying a task ID. It tests a lost local completion acknowledgment, recreates a worker, and checks that neither callback repeats. The temporary database and credentials are deleted when the demo exits. It does not touch the normal 8787/8788 instance or YouTube account.

`receiver.py` is also runnable with `AGENTWAY_TOKEN` in its environment, `--url`, a private `--journal` path and one or both of `--task-command` and `--result-command`. A task command reads one task JSON object on standard input and writes a nonempty result to standard output. A result command reads the terminal task JSON object. Neither command is taken from task content. Do not point this experimental worker at a production account or use it for consequential tasks.

The prototype deliberately fails closed on uncertain external execution: a crash after invoking an adapter leaves that task in `running`/`delivering` in the local journal for inspection, instead of running it again. It pages through the pull inbox and records an authenticated sender result receipt after its fixed callback succeeds. A receipt proves only that this callback completed; it cannot prove delivery into a native conversation. There is no server-side push, native agent binding or production recovery UI. A Codex or Grok label in the disposable demo database is **only a test identity**; no native Codex or Grok Bot conversation is started. Replacing the fixed callback with a supported native adapter, then passing the [same conformance runbook](../../docs/design/handoff-conformance-v1.md) for every advertised product, is the next gate.

Run `python3 prototypes/handoff_receiver/transport_conformance.py` after the same build to check the dormant automatic-delivery transport. It uses an isolated server and database, synthetically marks two disposable bindings verified, and exercises an offer, transport admission, server restart, native-principal claim and completion, return admission, sender receipt, credential separation and grant revocation. The synthetic verification exists only inside the temporary test database: this test does **not** verify any product's native wake, grant automatic delivery to a real connection, or send a webhook.
