#!/usr/bin/env python3
"""Stage media without publishing. Resume with the same --state file.

AGENTWAY_TOKEN=<secret> python3 resumable-upload.py \
  --base https://dev.agentway.win --file episode.mp4 --mime video/mp4 \
  --state episode.upload.json

Uses only Python's standard library. Never logs the bearer token. The state file
contains request identity and source fingerprint, not credentials or media bytes.
"""
import argparse
import base64
import hashlib
import http.client
import json
import os
from pathlib import Path
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix=path.name + ".", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as f:
            json.dump(value, f)
            f.flush()
            os.fsync(f.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True)
    parser.add_argument("--file", type=Path, required=True)
    parser.add_argument("--mime", required=True)
    parser.add_argument("--state", type=Path, required=True)
    args = parser.parse_args()
    base = args.base.rstrip("/")
    url = urllib.parse.urlsplit(base)
    if url.username or url.password or url.query or url.fragment or url.path:
        parser.error("base must be an origin without credentials, path, query or fragment")
    if url.scheme != "https" and not (url.scheme == "http" and url.hostname in ("127.0.0.1", "localhost")):
        parser.error("HTTPS is required except for local testing")
    if args.state.resolve() == args.file.resolve():
        parser.error("state must be a different file from the media")
    token = os.environ.get("AGENTWAY_TOKEN")
    if not token:
        parser.error("Set AGENTWAY_TOKEN in your environment")
    opener = urllib.request.build_opener(NoRedirect())

    def request(method, path, data=None, headers=None):
        # Returned paths must stay on the authenticated origin.
        if not path.startswith("/v1/") or path.startswith("//"):
            raise RuntimeError("Unexpected upload path")
        req = urllib.request.Request(base + path, data=data, method=method,
            headers={"Authorization": "Bearer " + token, **(headers or {})})
        return opener.open(req, timeout=150)

    def json_request(method, path, value=None):
        data = json.dumps(value).encode() if value is not None else None
        with request(method, path, data, {"Content-Type": "application/json"}) as response:
            return json.load(response)

    def retry(action):
        for attempt in range(6):
            try:
                return action()
            except urllib.error.HTTPError as e:
                if e.code not in (408, 429, 500, 502, 503, 504):
                    raise RuntimeError(f"AgentWay returned HTTP {e.code}; resolve the error before retrying") from None
                try:
                    delay = min(60, max(1, int(e.headers.get("Retry-After", 2 ** attempt))))
                except ValueError:
                    delay = min(30, 2 ** attempt)
                e.close()
            except (urllib.error.URLError, TimeoutError, ConnectionError, http.client.HTTPException):
                delay = min(30, 2 ** attempt)
            if attempt == 5:
                raise RuntimeError("Transfer paused after repeated transient errors; rerun with the same state file")
            time.sleep(delay)

    with args.file.open("rb") as source:
        digest = hashlib.sha256()
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
        fingerprint = digest.hexdigest()
        size = source.seek(0, 2)
        identity = {"base": base, "size": size, "mime": args.mime, "sha256": fingerprint}
        if args.state.exists():
            state = json.loads(args.state.read_text())
            if state["source"] != identity:
                raise RuntimeError("Source or destination changed; do not reuse this upload state")
        else:
            state = {"source": identity, "request_id": str(uuid.uuid4())}
            save(args.state, state)  # Before the first request, including lost creation responses.
        if "media_id" not in state:
            upload = retry(lambda: json_request("POST", "/v1/media/uploads", {
                "request_id": state["request_id"], "size": size, "mime": args.mime, "sha256": fingerprint}))
            state["media_id"] = upload["media_id"]
            save(args.state, state)
        status_path = "/v1/media/uploads/" + state["media_id"]
        upload = retry(lambda: json_request("GET", status_path))
        upload_path = upload["upload_path"]
        chunk_size = min(upload["max_chunk_bytes"], 8 * 1024 * 1024)
        tus_headers = {"Tus-Resumable": "1.0.0"}

        def step():
            # Every attempt starts with HEAD, including a retry after lost PATCH acknowledgement.
            with request("HEAD", upload_path, headers=tus_headers) as response:
                offset = int(response.headers["Upload-Offset"])
                if int(response.headers["Upload-Length"]) != size:
                    raise RuntimeError("Server size changed")
            if not 0 <= offset <= size:
                raise RuntimeError("Invalid server offset")
            if offset == size:
                return True
            source.seek(offset)
            chunk = source.read(min(chunk_size, size - offset))
            if not chunk:
                raise RuntimeError("Source became shorter during transfer")
            checksum = base64.b64encode(hashlib.sha256(chunk).digest()).decode()
            try:
                with request("PATCH", upload_path, chunk, {**tus_headers,
                    "Content-Type": "application/offset+octet-stream", "Upload-Offset": str(offset),
                    "Upload-Checksum": "sha256 " + checksum}) as response:
                    if response.status != 204:
                        raise RuntimeError("Unexpected chunk response")
            except urllib.error.HTTPError as e:
                if e.code != 409:
                    raise
                e.close()  # Another attempt advanced the offset; HEAD next time.
                raise urllib.error.URLError("Offset changed; rechecking") from None
            return False

        if not upload["ready"]:
            while not retry(step):
                pass
        result = retry(lambda: json_request("POST", status_path + "/complete", {}))
        if not result["ready"]:
            raise RuntimeError("Media has not passed whole-file verification")
        print(json.dumps({"media_id": state["media_id"], "ready": True, "size": size}))


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, ValueError, KeyError) as error:
        raise SystemExit(str(error)) from None
