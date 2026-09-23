"""Experimental AgentWay receiver companion. Never use with production credentials.

This exercises the existing pull-inbox API with an unattended local worker. The
callback is a fixed command chosen by the owner, not task-supplied executable
text. It does not wake a third-party chat application by itself.
"""

from __future__ import annotations

import argparse
import json
import os
import sqlite3
import stat
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path
from typing import Callable


class AgentWayError(Exception):
    def __init__(self, status: int, message: str):
        super().__init__(f"AgentWay HTTP {status}: {message}")
        self.status = status


class Client:
    def __init__(self, base_url: str, token: str):
        parsed = urllib.parse.urlparse(base_url)
        if parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username or parsed.password or parsed.path not in ("", "/") or parsed.query or parsed.fragment:
            raise ValueError("Use a plain AgentWay origin URL")
        if parsed.scheme == "http" and parsed.hostname not in ("127.0.0.1", "localhost", "::1"):
            raise ValueError("Use loopback HTTP or HTTPS")
        self.base_url = base_url.rstrip("/")
        self.token = token
        class NoRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, request, fp, code, msg, headers, newurl):
                return None
        self.opener = urllib.request.build_opener(NoRedirect)

    def request(self, method: str, path: str, body: dict | None = None) -> dict:
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(
            self.base_url + path,
            data=data,
            method=method,
            headers={
                "Authorization": "Bearer " + self.token,
                "Content-Type": "application/json",
            },
        )
        try:
            with self.opener.open(req, timeout=15) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            try:
                message = json.load(error).get("error", "request failed")
            except (ValueError, OSError):
                message = "request failed"
            raise AgentWayError(error.code, message) from None


class Journal:
    def __init__(self, path: Path):
        path.parent.mkdir(parents=True, exist_ok=True)
        if not path.exists():
            fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_RDWR, 0o600)
            os.close(fd)
        mode = path.lstat().st_mode
        if not stat.S_ISREG(mode) or mode & 0o077:
            raise ValueError("Journal must be a private regular file (mode 0600)")
        self.db = sqlite3.connect(path)
        self.db.execute(
            "CREATE TABLE IF NOT EXISTS inbound ("
            "task_id TEXT PRIMARY KEY, claim_token TEXT NOT NULL, "
            "phase TEXT NOT NULL, result TEXT)"
        )
        self.db.execute(
            "CREATE TABLE IF NOT EXISTS returned ("
            "task_id TEXT PRIMARY KEY, phase TEXT NOT NULL)"
        )
        self.db.commit()

    def inbound(self, task_id: str) -> tuple[str, str, str | None]:
        self.db.execute(
            "INSERT OR IGNORE INTO inbound(task_id,claim_token,phase) VALUES(?,?,'prepared')",
            (task_id, str(uuid.uuid4())),
        )
        self.db.commit()
        return self.existing_inbound(task_id)

    def existing_inbound(self, task_id: str) -> tuple[str, str, str | None] | None:
        return self.db.execute(
            "SELECT claim_token,phase,result FROM inbound WHERE task_id=?", (task_id,)
        ).fetchone()

    def set_inbound(self, task_id: str, phase: str, result: str | None = None) -> None:
        self.db.execute(
            "UPDATE inbound SET phase=?,result=COALESCE(?,result) WHERE task_id=?",
            (phase, result, task_id),
        )
        self.db.commit()

    def returned(self, task_id: str) -> str | None:
        row = self.db.execute(
            "SELECT phase FROM returned WHERE task_id=?", (task_id,)
        ).fetchone()
        return row[0] if row else None

    def set_returned(self, task_id: str, phase: str) -> None:
        self.db.execute(
            "INSERT INTO returned(task_id,phase) VALUES(?,?) "
            "ON CONFLICT(task_id) DO UPDATE SET phase=excluded.phase",
            (task_id, phase),
        )
        self.db.commit()


class Receiver:
    def __init__(
        self,
        client: Client,
        journal: Journal,
        task_handler: Callable[[dict], str] | None,
        result_handler: Callable[[dict], None] | None,
    ):
        self.client = client
        self.journal = journal
        self.task_handler = task_handler
        self.result_handler = result_handler
        self.connection_id = client.request("GET", "/v1/status")["connection"]["id"]

    def tick(self) -> None:
        tasks = []
        before = None
        while True:
            path = "/v1/agent-tasks" + (f"?before={before}" if before else "")
            page = self.client.request("GET", path)
            tasks.extend(page["items"])
            next_cursor = page.get("next")
            if next_cursor is None:
                break
            if not isinstance(next_cursor, int) or (before is not None and next_cursor >= before):
                raise RuntimeError("Inbox pagination did not advance")
            before = next_cursor
        for task in reversed(tasks):
            if task["recipient_id"] == self.connection_id and self.task_handler:
                self._receive(task)
            if task["sender_id"] == self.connection_id and self.result_handler:
                self._return(task)

    def _receive(self, task: dict) -> None:
        task_id = task["id"]
        if task["status"] == "completed":
            saved = self.journal.existing_inbound(task_id)
            if saved and saved[1] == "result_ready" and saved[2] == task["result"]:
                self.journal.set_inbound(task_id, "done")
            return
        if task["status"] not in ("queued", "claimed"):
            return
        claim_token, phase, result = self.journal.inbound(task_id)
        if phase == "done" or phase == "running":
            # A crash during an external action has unknown outcome. Never
            # silently run it twice; inspect the native system first.
            return
        if phase == "prepared":
            try:
                self.client.request(
                    "POST", f"/v1/agent-tasks/{task_id}/claim", {"claim_token": claim_token}
                )
            except AgentWayError as error:
                if error.status == 409:
                    return  # Another claimant or expired task.
                raise
            self.journal.set_inbound(task_id, "claimed")
            phase = "claimed"
        if phase == "claimed":
            self.journal.set_inbound(task_id, "running")
            result = self.task_handler(task)
            if not isinstance(result, str) or not result.strip():
                raise ValueError("Task handler must return a nonempty result")
            self.journal.set_inbound(task_id, "result_ready", result)
            phase = "result_ready"
        if phase == "result_ready":
            try:
                self.client.request(
                    "POST",
                    f"/v1/agent-tasks/{task_id}/complete",
                    {"claim_token": claim_token, "message": result},
                )
            except AgentWayError as error:
                current = self.client.request("GET", f"/v1/agent-tasks/{task_id}")
                if error.status != 409 or current["status"] != "completed" or current["result"] != result:
                    raise
            self.journal.set_inbound(task_id, "done")

    def _return(self, task: dict) -> None:
        if task["status"] not in ("completed", "failed", "cancelled"):
            return
        task_id = task["id"]
        phase = self.journal.returned(task_id)
        if phase == "delivering":
            return  # External callback outcome unknown; inspect before reconciling.
        if phase is None:
            self.journal.set_returned(task_id, "delivering")
            self.result_handler(task)
            self.journal.set_returned(task_id, "delivered")
        # If the HTTP acknowledgment was lost, retrying it is safe and does
        # not repeat the external callback.
        if task.get("result_acknowledged_at") is None:
            self.client.request("POST", f"/v1/agent-tasks/{task_id}/ack-result")


def command_handler(command: list[str], returns_text: bool):
    def invoke(task: dict):
        child_env = {key: value for key, value in os.environ.items() if key != "AGENTWAY_TOKEN"}
        completed = subprocess.run(
            command,
            input=json.dumps(task),
            text=True,
            capture_output=True,
            timeout=120,
            env=child_env,
            check=True,
        )
        return completed.stdout.strip() if returns_text else None

    return invoke


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", required=True, help="AgentWay agent listener URL")
    parser.add_argument("--journal", type=Path, required=True)
    parser.add_argument("--task-command", nargs="+", help="Fixed executable that reads task JSON and prints a result")
    parser.add_argument("--result-command", nargs="+", help="Fixed executable that reads completed task JSON")
    parser.add_argument("--interval", type=float, default=2.0)
    parser.add_argument("--once", action="store_true")
    args = parser.parse_args()
    token = os.environ.get("AGENTWAY_TOKEN")
    if not token:
        parser.error("AGENTWAY_TOKEN must be set in the environment")
    receiver = Receiver(
        Client(args.url, token),
        Journal(args.journal),
        command_handler(args.task_command, True) if args.task_command else None,
        command_handler(args.result_command, False) if args.result_command else None,
    )
    while True:
        receiver.tick()
        if args.once:
            break
        time.sleep(args.interval)


if __name__ == "__main__":
    main()
