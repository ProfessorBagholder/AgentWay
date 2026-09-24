"""Exercise the agent task SSE feed over an isolated HTTP listener."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def request(base: str, method: str, path: str, token: str | None = None,
            body: dict | None = None) -> dict:
    headers = {"Content-Type": "application/json"}
    if token is not None:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(
        base + path,
        data=json.dumps(body).encode() if body is not None else None,
        method=method,
        headers=headers,
    )
    with urllib.request.urlopen(req, timeout=10) as response:
        return json.load(response)


def event(response) -> dict:
    fields = {}
    while True:
        raw = response.readline()
        if not raw:
            raise EOFError("SSE stream closed before an event")
        line = raw.decode("utf-8").rstrip("\r\n")
        if not line:
            if fields:
                fields["data"] = json.loads(fields["data"])
                return fields
            continue
        if line.startswith(":"):
            continue
        key, value = line.split(":", 1)
        fields[key] = value.lstrip(" ")


def run() -> None:
    binary = ROOT / "target/debug/agentway-server"
    if not binary.exists():
        raise RuntimeError("Build first: cargo build --locked -p agentway-server")
    with tempfile.TemporaryDirectory(prefix="agentway-sse-test-") as temp:
        directory = Path(temp)
        owner_port = free_port()
        agent_port = free_port()
        while agent_port == owner_port:
            agent_port = free_port()
        owner_url = f"http://127.0.0.1:{owner_port}"
        agent_url = f"http://127.0.0.1:{agent_port}"
        env = os.environ.copy()
        env.update(
            DATABASE_URL=f"sqlite://{directory / 'agentway.db'}",
            PUBLISHING_DIR=str(directory / "publishing"),
            BIND_ADDR=f"127.0.0.1:{owner_port}",
            BRIDGE_BIND_ADDR=f"127.0.0.1:{agent_port}",
            ASSET_DIR=str(ROOT / "web/dist"),
        )

        def start(log):
            process = subprocess.Popen(
                [str(binary)], cwd=ROOT, env=env, stdout=log, stderr=log
            )
            for _ in range(100):
                if process.poll() is not None:
                    raise RuntimeError("Isolated AgentWay server exited")
                try:
                    request(owner_url, "GET", "/health/ready")
                    return process
                except (urllib.error.URLError, ValueError):
                    time.sleep(0.1)
            raise TimeoutError("Isolated AgentWay server did not start")

        def subscribe(token: str, last: str | None = None):
            headers = {
                "Authorization": f"Bearer {token}",
                "Accept": "text/event-stream",
            }
            if last is not None:
                headers["Last-Event-ID"] = last
            return urllib.request.urlopen(
                urllib.request.Request(agent_url + "/v1/agent-tasks/stream", headers=headers),
                timeout=5,
            )

        with (directory / "server.log").open("w") as log:
            server = start(log)
            try:
                sender = request(owner_url, "POST", "/api/agent-connections", body={
                    "product": "Codex", "publish_enabled": False,
                })
                recipient = request(owner_url, "POST", "/api/agent-connections", body={
                    "product": "Muse", "publish_enabled": False,
                })
                sender_token = request(owner_url, "POST", f"/api/agent-connections/{sender['id']}/token")["token"]
                recipient_token = request(owner_url, "POST", f"/api/agent-connections/{recipient['id']}/token")["token"]
                request(agent_url, "GET", "/v1/status", sender_token)
                request(agent_url, "GET", "/v1/status", recipient_token)
                request(owner_url, "POST", f"/api/agent-handoff-grants/{sender['id']}", body={
                    "recipient_id": recipient["id"], "enabled": True,
                })
                for credential in (None, "not-a-token"):
                    try:
                        subscribe(credential or "")
                        raise AssertionError("Unauthenticated SSE stream was accepted")
                    except urllib.error.HTTPError as error:
                        assert error.code == 401, error.code
                with subscribe(recipient_token) as incoming:
                    assert incoming.headers["Content-Type"].startswith("text/event-stream")
                    task = request(agent_url, "POST", "/v1/agent-tasks", sender_token, {
                        "request_id": str(uuid.uuid4()), "recipient_id": recipient["id"],
                        "title": "First stream test", "instructions": "Do not publish.",
                        "timeout_seconds": 600,
                    })
                    queued = event(incoming)
                    assert queued["event"] == "task-queued", queued
                    assert queued["data"]["task_id"] == task["id"]
                    assert "instructions" not in queued["data"]
                    first_id = queued["id"]
                server.terminate()
                server.wait(timeout=10)
                server = start(log)
                second = request(agent_url, "POST", "/v1/agent-tasks", sender_token, {
                    "request_id": str(uuid.uuid4()), "recipient_id": recipient["id"],
                    "title": "Second stream test", "instructions": "No external action.",
                    "timeout_seconds": 600,
                })
                with subscribe(recipient_token, first_id) as incoming:
                    replay = event(incoming)
                    assert replay["event"] == "task-queued", replay
                    assert replay["data"]["task_id"] == second["id"], replay
                    assert int(replay["id"]) > int(first_id)
                with subscribe(sender_token) as outgoing:
                    claim = str(uuid.uuid4())
                    request(agent_url, "POST", f"/v1/agent-tasks/{second['id']}/claim", recipient_token, {
                        "claim_token": claim,
                    })
                    request(agent_url, "POST", f"/v1/agent-tasks/{second['id']}/complete", recipient_token, {
                        "claim_token": claim, "message": "SSE result received.",
                    })
                    completed = event(outgoing)
                    assert completed["event"] == "task-completed", completed
                    assert completed["data"] == {
                        "task_id": second["id"], "result": "SSE result received."
                    }
                print("PASS: authenticated SSE, queued hint, restart replay and sender result")
            finally:
                server.terminate()
                server.wait(timeout=10)


if __name__ == "__main__":
    run()
