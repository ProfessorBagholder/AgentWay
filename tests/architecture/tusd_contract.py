#!/usr/bin/env python3
"""Isolated dependency proof; never connects to AgentWay or a real provider.

Requires Python 3 and Docker. Creates one temporary container, binds a random
loopback port, and removes it in finally. This tests tusd, NOT its future
integration with AgentWay. Run from any directory with python3 <this file>.
"""

import concurrent.futures
import hashlib
import http.client
import json
from pathlib import Path
import subprocess
import tempfile
import time
import uuid
from urllib.parse import urlsplit

IMAGE = "tusproject/tusd:v2.10.1@sha256:7b1c552a8b42f4b36cb01f2a3bd49f82ab078b2eefd191459e716141dd50376c"
CHUNK = 8 * 1024 * 1024
TOTAL = 104 * 1024 * 1024
MAX_MEDIA = 2 * 1024 * 1024 * 1024
PATTERN = bytes(range(256))


def docker(*args):
    return subprocess.check_output(["docker", *args], text=True).strip()


def payload(offset, size):
    start = offset % len(PATTERN)
    return (PATTERN * ((start + size + 255) // 256))[start : start + size]


def check(condition, message):
    if not condition:
        raise AssertionError(message)


def main():
    name = "agentway-tusd-proof-" + uuid.uuid4().hex[:12]
    started = False
    results = []
    try:
        docker(
            "run", "--detach", "--name", name,
            "--publish", "127.0.0.1::8080", "--cap-drop=ALL",
            "--security-opt=no-new-privileges:true", IMAGE,
            "-upload-dir=/tmp/uploads", "-disable-download",
            "-disable-cors", "-max-size=" + str(MAX_MEDIA),
        )
        started = True

        def port():
            return int(docker("port", name, "8080/tcp").rsplit(":", 1)[1])

        active_port = port()

        def request(method, path, data=b"", headers=None):
            conn = http.client.HTTPConnection("127.0.0.1", active_port, timeout=20)
            try:
                try:
                    conn.request(method, path, body=data, headers={
                        "Tus-Resumable": "1.0.0", **(headers or {})
                    })
                except BrokenPipeError:
                    # A server can reject headers before consuming a large body.
                    # Read and assert its actual response; never count the socket
                    # error itself as a successful conflict/rejection test.
                    pass
                response = conn.getresponse()
                result = response.status, dict(response.getheaders()), response.read()
                return result
            finally:
                conn.close()

        def wait_ready():
            for _ in range(100):
                try:
                    if request("OPTIONS", "/files/")[0] == 200:
                        return
                except (OSError, http.client.HTTPException):
                    pass
                time.sleep(0.1)
            raise AssertionError("tusd failed to become ready")

        def offset(path):
            status, headers, _ = request("HEAD", path)
            check(status == 200, f"HEAD status {status}")
            check(headers.get("Upload-Length") == str(TOTAL), "Declared length changed")
            check(headers.get("Cache-Control") == "no-store", "Offset was cacheable")
            return int(headers["Upload-Offset"])

        def patch(path, start, size):
            return request("PATCH", path, payload(start, size), {
                "Upload-Offset": str(start),
                "Content-Type": "application/offset+octet-stream",
            })

        wait_ready()
        check(request("POST", "/files/", headers={"Upload-Length": str(MAX_MEDIA + 1)})[0] == 413,
              "Oversized reservation was accepted")
        status, headers, _ = request("POST", "/files/", headers={"Upload-Length": str(TOTAL)})
        check(status == 201, f"Creation failed: {status}")
        path = urlsplit(headers["Location"]).path
        check(offset(path) == 0, "Initial offset must be zero")
        check(request("GET", path)[0] == 405, "Unauthenticated download was enabled")
        results.append("creation limit and disabled downloads")

        check(patch(path, 0, CHUNK)[0] == 204, "First chunk failed")
        # Act as a client whose successful response was lost. Query before retry.
        check(offset(path) == CHUNK, "Committed offset could not be recovered")
        check(patch(path, 0, CHUNK)[0] == 409, "Stale retry was not rejected")
        check(offset(path) == CHUNK, "Stale retry changed stored offset")
        results.append("lost-response recovery and stale-offset rejection")

        # Two concurrent writes to the same offset: exactly one must commit.
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            attempts = list(pool.map(lambda _: patch(path, CHUNK, CHUNK)[0], range(2)))
        check(sorted(attempts) == [204, 409], f"Concurrent statuses: {attempts}")
        check(offset(path) == 2 * CHUNK, "Concurrent write duplicated data")
        results.append("concurrent same-offset writes")

        # Terminate the client connection partway through a declared request.
        before = offset(path)
        conn = http.client.HTTPConnection("127.0.0.1", active_port, timeout=20)
        try:
            conn.putrequest("PATCH", path)
            for key, value in {
                "Tus-Resumable": "1.0.0", "Upload-Offset": str(before),
                "Content-Type": "application/offset+octet-stream", "Content-Length": str(CHUNK),
            }.items():
                conn.putheader(key, value)
            conn.endheaders()
            conn.send(payload(before, CHUNK // 4))
        finally:
            conn.close()
        current = offset(path)
        check(before <= current <= before + CHUNK // 4, "Invalid partial-request offset")
        check(current > before, "Partial transfer made no resumable progress")
        results.append("interrupted request preserves partial bytes")

        # Process crash; not a simulated filesystem/power failure.
        docker("kill", "--signal=KILL", name)
        docker("start", name)
        active_port = port()
        wait_ready()
        check(offset(path) == current, "Process restart lost the saved offset")
        results.append("SIGKILL/restart preserves upload and offset")

        while current < TOTAL:
            length = min(CHUNK, TOTAL - current)
            status, headers, _ = patch(path, current, length)
            check(status == 204, f"Resume failed: {status}")
            current += length
            check(int(headers["Upload-Offset"]) == current, "Wrong acknowledged offset")
        check(offset(path) == TOTAL, "Final offset does not equal declared length")

        # Compare completed bytes with the expected digest without buffering a file.
        upload_id = path.rsplit("/", 1)[1]
        check(len(upload_id) == 32 and all(c in "0123456789abcdef" for c in upload_id),
              "Unexpected upload ID")
        with tempfile.TemporaryDirectory(prefix="agentway-tusd-proof-") as directory:
            target = Path(directory) / "uploaded.bin"
            docker("cp", f"{name}:/tmp/uploads/{upload_id}", str(target))
            actual = hashlib.sha256()
            expected = hashlib.sha256()
            with target.open("rb") as stream:
                while block := stream.read(CHUNK):
                    actual.update(block)
            for start in range(0, TOTAL, CHUNK):
                expected.update(payload(start, min(CHUNK, TOTAL - start)))
            check(target.stat().st_size == TOTAL, "Stored size mismatch")
            check(actual.digest() == expected.digest(), "Stored content digest mismatch")
        results.append("104 MiB completed with <=8 MiB requests; SHA-256 matches")

        check(request("DELETE", path)[0] == 204, "Termination failed")
        check(request("HEAD", path)[0] == 404, "Terminated upload still exists")
        results.append("termination deletes upload")
        print(json.dumps({"image": IMAGE, "scope": "tusd dependency only", "passed": results}, indent=2))
    finally:
        if started:
            subprocess.run(["docker", "rm", "--force", name], check=False, stdout=subprocess.DEVNULL)


if __name__ == "__main__":
    main()
