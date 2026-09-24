"""Run an isolated, no-publishing AgentWay handoff receiver round trip."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

from receiver import AgentWayError, Client, Journal, Receiver

ROOT = Path(__file__).resolve().parents[2]


class LoseFirstResultAck(Client):
    """Commit the first sender receipt but simulate losing its HTTP response."""

    def __init__(self, base_url: str, token: str):
        super().__init__(base_url, token)
        self.lost = False

    def request(self, method: str, path: str, body: dict | None = None) -> dict:
        result = super().request(method, path, body)
        if method == "POST" and path.endswith("/ack-result") and not self.lost:
            self.lost = True
            raise AgentWayError(503, "simulated lost receipt response")
        return result


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def owner(base: str, method: str, path: str, body: dict | None = None) -> dict:
    request = urllib.request.Request(
        base + path,
        data=json.dumps(body).encode() if body is not None else None,
        method=method,
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        return json.load(response)


def wait_ready(base: str, process: subprocess.Popen) -> None:
    for _ in range(100):
        if process.poll() is not None:
            raise RuntimeError("Isolated AgentWay server exited during startup")
        try:
            owner(base, "GET", "/health/ready")
            return
        except (urllib.error.URLError, ValueError):
            time.sleep(0.1)
    raise TimeoutError("Isolated AgentWay server did not start")


def run() -> None:
    binary = ROOT / "target/debug/agentway-server"
    if not binary.exists():
        raise RuntimeError("Build first: cargo build --locked -p agentway-server")
    with tempfile.TemporaryDirectory(prefix="agentway-handoff-demo-") as tmp:
        directory = Path(tmp)
        management_port = free_port()
        agent_port = free_port()
        management = f"http://127.0.0.1:{management_port}"
        agents = f"http://127.0.0.1:{agent_port}"
        env = os.environ.copy()
        env.update(
            DATABASE_URL=f"sqlite://{directory / 'agentway.db'}",
            PUBLISHING_DIR=str(directory / "publishing"),
            BIND_ADDR=f"127.0.0.1:{management_port}",
            BRIDGE_BIND_ADDR=f"127.0.0.1:{agent_port}",
            ASSET_DIR=str(ROOT / "web/dist"),
        )
        with (directory / "server.log").open("w") as log:
            server = subprocess.Popen([str(binary)], cwd=ROOT, env=env, stdout=log, stderr=log)
            try:
                wait_ready(management, server)
                sender = owner(
                    management, "POST", "/api/agent-connections",
                    {"product": "Codex", "publish_enabled": False},
                )
                recipient = owner(
                    management, "POST", "/api/agent-connections",
                    {"product": "Grok Bot", "publish_enabled": False},
                )
                sender_token = owner(
                    management, "POST", f"/api/agent-connections/{sender['id']}/token"
                )["token"]
                recipient_token = owner(
                    management, "POST", f"/api/agent-connections/{recipient['id']}/token"
                )["token"]
                sender_client = LoseFirstResultAck(agents, sender_token)
                recipient_client = Client(agents, recipient_token)
                sender_client.request("GET", "/v1/status")
                recipient_client.request("GET", "/v1/status")
                owner(
                    management,
                    "POST",
                    f"/api/agent-handoff-grants/{sender['id']}",
                    {"recipient_id": recipient["id"], "enabled": True},
                )
                owner(
                    management,
                    "POST",
                    f"/api/agent-handoff-grants/{recipient['id']}",
                    {"recipient_id": sender["id"], "enabled": True},
                )
                delivered: list[dict] = []
                sender_worker = Receiver(
                    sender_client, Journal(directory / "sender.sqlite"), None,
                    lambda task: delivered.append(task),
                )
                recipient_worker = Receiver(
                    recipient_client, Journal(directory / "recipient.sqlite"),
                    lambda task: "Acknowledged: " + task["instructions"], None,
                )
                nonce = uuid.uuid4().hex
                created = sender_client.request(
                    "POST", "/v1/agent-tasks",
                    {
                        "request_id": str(uuid.uuid4()),
                        "recipient_id": recipient["id"],
                        "title": "Harmless receiver probe",
                        "instructions": nonce,
                    },
                )
                for _ in range(40):
                    recipient_worker.tick()
                    try:
                        sender_worker.tick()
                    except AgentWayError as error:
                        assert error.status == 503 and sender_client.lost
                    if delivered and sender_client.lost:
                        break
                    time.sleep(0.1)
                assert len(delivered) == 1, "No result returned to sender worker"
                assert delivered[0]["id"] == created["id"]
                assert delivered[0]["result"] == "Acknowledged: " + nonce
                assert delivered[0]["status"] == "completed"
                assert delivered[0]["result_acknowledged_at"] is None
                assert sender_client.request(
                    "GET", f"/v1/agent-tasks/{created['id']}"
                )["result_acknowledged_at"] is not None
                sender_worker.tick()  # Reconcile lost HTTP response, no second callback.
                assert len(delivered) == 1
                try:
                    sender_client.request(
                        "POST", f"/v1/agent-tasks/{created['id']}/claim",
                        {"claim_token": str(uuid.uuid4())},
                    )
                    raise AssertionError("Sender claimed recipient work")
                except AgentWayError as error:
                    assert error.status == 409
                recipient_worker.journal.set_inbound(
                    created["id"], "result_ready", delivered[0]["result"]
                )
                recipient_worker.tick()
                assert recipient_worker.journal.existing_inbound(created["id"])[1] == "done"
                # Recreated workers use saved journals; duplicate scans do not
                # execute either callback a second time.
                sender_worker = Receiver(
                    sender_client, Journal(directory / "sender.sqlite"), None,
                    lambda task: delivered.append(task),
                )
                sender_worker.tick()
                recipient_worker.tick()
                assert len(delivered) == 1, "Result delivered twice after restart"
                assert sender_client.request(
                    "GET", f"/v1/agent-tasks/{created['id']}"
                )["result_acknowledged_at"] is not None
                reverse_results: list[dict] = []
                reverse_sender = Receiver(
                    recipient_client, Journal(directory / "recipient.sqlite"), None,
                    lambda task: reverse_results.append(task),
                )
                reverse_recipient = Receiver(
                    sender_client, Journal(directory / "sender.sqlite"),
                    lambda task: "Reverse acknowledged: " + task["instructions"], None,
                )
                reverse_nonce = uuid.uuid4().hex
                reverse = recipient_client.request(
                    "POST", "/v1/agent-tasks",
                    {
                        "request_id": str(uuid.uuid4()),
                        "recipient_id": sender["id"],
                        "title": "Reverse harmless receiver probe",
                        "instructions": reverse_nonce,
                    },
                )
                for _ in range(40):
                    reverse_recipient.tick()
                    reverse_sender.tick()
                    if reverse_results:
                        break
                    time.sleep(0.1)
                assert len(reverse_results) == 1
                assert reverse_results[0]["id"] == reverse["id"]
                assert reverse_results[0]["result"] == "Reverse acknowledged: " + reverse_nonce
                assert recipient_client.request(
                    "GET", f"/v1/agent-tasks/{reverse['id']}"
                )["result_acknowledged_at"] is not None
                attempts = []
                uncertain = sender_client.request(
                    "POST", "/v1/agent-tasks",
                    {
                        "request_id": str(uuid.uuid4()),
                        "recipient_id": recipient["id"],
                        "title": "Uncertain adapter probe",
                        "instructions": "No external action",
                    },
                )

                def interrupted(_task):
                    attempts.append("started")
                    raise RuntimeError("simulated adapter interruption")

                interrupted_worker = Receiver(
                    recipient_client, Journal(directory / "recipient.sqlite"), interrupted, None
                )
                try:
                    interrupted_worker.tick()
                    raise AssertionError("Interrupted callback unexpectedly completed")
                except RuntimeError as error:
                    assert str(error) == "simulated adapter interruption"
                restarted_worker = Receiver(
                    recipient_client, Journal(directory / "recipient.sqlite"),
                    lambda _task: attempts.append("replayed") or "bad", None,
                )
                restarted_worker.tick()
                assert attempts == ["started"], "Uncertain adapter was run twice"
                assert recipient_client.request(
                    "GET", f"/v1/agent-tasks/{uncertain['id']}"
                )["status"] == "claimed"
                print("PASS: isolated AgentWay tasks completed and returned in both directions")
                print("PASS: restart scan did not repeat either callback")
                print("PASS: lost sender receipt response reconciled without callback replay")
                print("PASS: wrong-principal claim denied; uncertain adapter not replayed")
                print("Scope: synthetic local workers, not the native Codex or Grok Bot apps")
            finally:
                server.terminate()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()


if __name__ == "__main__":
    try:
        run()
    except Exception as error:
        print(f"FAIL: {error}", file=sys.stderr)
        raise
