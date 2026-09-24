#!/usr/bin/env python3
"""One-shot AgentWay-to-Grok Bot webhook feasibility probe.

This is intentionally not a production dispatcher. It sends an opaque task ID
once, then requires AgentWay readback and Grok Bot run history for verification.
"""

import argparse
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
import uuid


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        return None


def webhook_url(value: str) -> str:
    parsed = urllib.parse.urlsplit(value)
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or parsed.username
        or parsed.password
        or parsed.fragment
        or parsed.port not in (None, 443)
        or parsed.hostname not in ("api2.cursor.sh", "cursor.com")
        and not parsed.hostname.endswith(".cursor.com")
    ):
        raise ValueError("Expected an HTTPS Cursor webhook URL without embedded credentials")
    return value


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("task_id", type=uuid.UUID)
    parser.add_argument("--delivery-id", type=uuid.UUID, default=uuid.uuid4())
    args = parser.parse_args()
    try:
        url = webhook_url(os.environ["GROK_BOT_WEBHOOK_URL"])
        key = os.environ["GROK_BOT_WEBHOOK_KEY"]
        if not key or any(c in key for c in "\r\n"):
            raise ValueError("Invalid webhook key")
    except (KeyError, ValueError) as error:
        print(f"Configuration error: {error}", file=sys.stderr)
        return 2

    body = json.dumps(
        {
            "protocol_version": 1,
            "kind": "task_offered",
            "delivery_id": str(args.delivery_id),
            "task_id": str(args.task_id),
        },
        separators=(",", ":"),
    ).encode()
    request = urllib.request.Request(
        url,
        data=body,
        headers={
            "Authorization": f"Bearer {key}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.build_opener(NoRedirect()).open(request, timeout=15) as response:
            status = response.status
    except urllib.error.HTTPError as error:
        status = error.code
    except (urllib.error.URLError, TimeoutError, OSError):
        print(
            "Webhook outcome unknown. Check Grok run history before any retry; "
            f"delivery_id={args.delivery_id}",
            file=sys.stderr,
        )
        return 1
    if status != 200:
        print(
            f"Grok did not start a run (HTTP {status}); delivery_id={args.delivery_id}",
            file=sys.stderr,
        )
        return 1
    print(
        f"Grok accepted webhook run; task_id={args.task_id} "
        f"delivery_id={args.delivery_id}. Verify the AgentWay claim and result separately."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
