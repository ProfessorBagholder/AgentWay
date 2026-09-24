"""Exercise the dormant automatic-delivery transport against an isolated server.

This test deliberately marks disposable bindings verified in its temporary
database. No production API can do that, and this does not prove native wake.
"""

from __future__ import annotations

import json
import os
import sqlite3
import subprocess
import tempfile
import uuid
from pathlib import Path

from demo import ROOT, free_port, owner, wait_ready
from receiver import AgentWayError, Client


def expect_error(status: int, call) -> None:
    try:
        call()
    except AgentWayError as error:
        assert error.status == status, (error.status, status)
    else:
        raise AssertionError(f"Expected HTTP {status}")


def run() -> None:
    binary = ROOT / "target/debug/agentway-server"
    if not binary.exists():
        raise RuntimeError("Build first: cargo build --locked -p agentway-server")
    with tempfile.TemporaryDirectory(prefix="agentway-transport-test-") as temp:
        directory = Path(temp)
        db_path = directory / "agentway.db"
        management_port = free_port()
        agent_port = free_port()
        while agent_port == management_port:
            agent_port = free_port()
        management = f"http://127.0.0.1:{management_port}"
        agents = f"http://127.0.0.1:{agent_port}"
        env = os.environ.copy()
        env.update(
            DATABASE_URL=f"sqlite://{db_path}",
            PUBLISHING_DIR=str(directory / "publishing"),
            BIND_ADDR=management.removeprefix("http://"),
            BRIDGE_BIND_ADDR=agents.removeprefix("http://"),
            ASSET_DIR=str(ROOT / "web/dist"),
        )

        def start(log):
            process = subprocess.Popen(
                [str(binary)], cwd=ROOT, env=env, stdout=log, stderr=log
            )
            wait_ready(management, process)
            return process

        with (directory / "server.log").open("w") as log:
            server = start(log)
            try:
                sender = owner(management, "POST", "/api/agent-connections", {
                    "product": "Codex", "publish_enabled": False,
                })
                recipient = owner(management, "POST", "/api/agent-connections", {
                    "product": "Grok Bot", "publish_enabled": False,
                })
                sender_agent = Client(agents, owner(
                    management, "POST", f"/api/agent-connections/{sender['id']}/token"
                )["token"])
                recipient_agent = Client(agents, owner(
                    management, "POST", f"/api/agent-connections/{recipient['id']}/token"
                )["token"])
                sender_agent.request("GET", "/v1/status")
                recipient_agent.request("GET", "/v1/status")
                owner(management, "POST", f"/api/agent-handoff-grants/{sender['id']}", {
                    "recipient_id": recipient["id"], "enabled": True,
                })
                sender_binding = owner(management, "POST", f"/api/handoff-receivers/{sender['id']}", {
                    "native_target": "disposable-sender",
                })
                recipient_binding = owner(management, "POST", f"/api/handoff-receivers/{recipient['id']}", {
                    "native_target": "disposable-recipient",
                })
                sender_transport = Client(agents, sender_binding["receiver_token"])
                recipient_transport = Client(agents, recipient_binding["receiver_token"])
                expect_error(409, lambda: sender_agent.request("POST", "/v1/agent-tasks", {
                    "request_id": str(uuid.uuid4()),
                    "recipient_id": recipient["id"],
                    "title": "Unverified receiver test",
                    "instructions": "No external action.",
                    "automatic_delivery": True,
                }))
                # Synthetic proof in a disposable database only. This bypasses
                # the production gate to exercise the transport state machine.
                with sqlite3.connect(db_path) as db:
                    db.execute("UPDATE handoff_receiver_bindings SET verified_at=unixepoch(), proof_expires_at=unixepoch()+3600")
                    db.commit()

                task = sender_agent.request("POST", "/v1/agent-tasks", {
                    "request_id": str(uuid.uuid4()),
                    "recipient_id": recipient["id"],
                    "title": "Disposable transport test",
                    "instructions": "Return the exact test result. No external action.",
                    "automatic_delivery": True,
                    "timeout_seconds": 600,
                })
                assert task["delivery_mode"] == "automatic"
                feed = recipient_transport.request("GET", "/v1/handoff-receiver/deliveries")
                assert len(feed["items"]) == 1
                offer = feed["items"][0]
                assert offer["kind"] == "task_offered" and offer["task_id"] == task["id"]
                assert "instructions" not in json.dumps(feed)
                assert sender_transport.request("GET", "/v1/handoff-receiver/deliveries")["items"] == []
                expect_error(401, lambda: recipient_transport.request("GET", "/v1/status"))
                expect_error(401, lambda: recipient_agent.request("GET", "/v1/handoff-receiver/deliveries"))
                expect_error(409, lambda: sender_transport.request(
                    "POST", f"/v1/handoff-receiver/deliveries/{offer['delivery_id']}/admit",
                    {"generation": offer["binding_generation"], "native_reference": "wrong-target"},
                ))
                admission = {"generation": offer["binding_generation"], "native_reference": "fake-native-run"}
                recipient_transport.request("POST", f"/v1/handoff-receiver/deliveries/{offer['delivery_id']}/admit", admission)
                assert recipient_agent.request("GET", f"/v1/agent-tasks/{task['id']}")["status"] == "queued"

                # Restart after transport admission, before native acceptance.
                server.terminate()
                server.wait(timeout=10)
                server = start(log)
                recipient_transport.request("POST", f"/v1/handoff-receiver/deliveries/{offer['delivery_id']}/admit", admission)
                expect_error(409, lambda: recipient_transport.request(
                    "POST", f"/v1/handoff-receiver/deliveries/{offer['delivery_id']}/admit",
                    {**admission, "native_reference": "different-run"},
                ))
                claim = str(uuid.uuid4())
                recipient_agent.request("POST", f"/v1/agent-tasks/{task['id']}/claim", {
                    "claim_token": claim, "delivery_id": offer["delivery_id"],
                })
                recipient_agent.request("POST", f"/v1/agent-tasks/{task['id']}/complete", {
                    "claim_token": claim, "message": "Transport test received.",
                })
                returns = sender_transport.request("GET", "/v1/handoff-receiver/deliveries")["items"]
                assert len(returns) == 1 and returns[0]["kind"] == "result_available"
                result = returns[0]
                assert result["task_id"] == task["id"]
                sender_transport.request("POST", f"/v1/handoff-receiver/deliveries/{result['delivery_id']}/admit", {
                    "generation": result["binding_generation"], "native_reference": "fake-sender-run",
                })
                assert sender_agent.request("GET", f"/v1/agent-tasks/{task['id']}")["result_acknowledged_at"] is None
                sender_agent.request("POST", f"/v1/agent-tasks/{task['id']}/ack-result", {
                    "delivery_id": result["delivery_id"],
                })
                finished = sender_agent.request("GET", f"/v1/agent-tasks/{task['id']}")
                assert finished["status"] == "completed"
                assert finished["result"] == "Transport test received."
                assert finished["result_acknowledged_at"] is not None
                with sqlite3.connect(db_path) as db:
                    states = dict(db.execute(
                        "SELECT kind,state FROM handoff_delivery_outbox WHERE task_id=?",
                        (task["id"],),
                    ).fetchall())
                assert states == {"task_offered": "delivered", "result_available": "delivered"}, states

                # Grant revocation fences a queued offer after its transaction commits.
                blocked = sender_agent.request("POST", "/v1/agent-tasks", {
                    "request_id": str(uuid.uuid4()), "recipient_id": recipient["id"],
                    "title": "Revoked test", "instructions": "Do nothing.",
                    "automatic_delivery": True, "timeout_seconds": 600,
                })
                owner(management, "POST", f"/api/agent-handoff-grants/{sender['id']}", {
                    "recipient_id": recipient["id"], "enabled": False,
                })
                assert recipient_transport.request("GET", "/v1/handoff-receiver/deliveries")["items"] == []
                with sqlite3.connect(db_path) as db:
                    state = db.execute(
                        "SELECT state,safe_error_code FROM handoff_delivery_outbox WHERE task_id=?",
                        (blocked["id"],),
                    ).fetchone()
                assert state == ("blocked", "grant_revoked"), state
                print("PASS: isolated offer, admission, restart, acceptance, return, receipt and revocation")
            finally:
                server.terminate()
                server.wait(timeout=10)


if __name__ == "__main__":
    run()
