import contextlib
import io
import os
import sys
import unittest
import urllib.error
import uuid
from unittest import mock

import dispatch


class Response:
    status = 200

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False


class ProbeTests(unittest.TestCase):
    def test_rejects_non_cursor_and_embedded_credentials(self):
        self.assertEqual(
            dispatch.webhook_url("https://api2.cursor.sh/automations/webhook/test"),
            "https://api2.cursor.sh/automations/webhook/test",
        )
        for url in (
            "http://hooks.cursor.com/hook",
            "https://cursor.com.evil.test/hook",
            "https://user:key@hooks.cursor.com/hook",
            "https://localhost/hook",
        ):
            with self.subTest(url=url), self.assertRaises(ValueError):
                dispatch.webhook_url(url)

    def test_posts_only_opaque_ids_and_does_not_print_secret(self):
        task_id = str(uuid.uuid4())
        delivery_id = str(uuid.uuid4())
        captured = {}

        class Opener:
            def open(self, request, timeout):
                captured["request"] = request
                captured["timeout"] = timeout
                return Response()

        output = io.StringIO()
        with (
            mock.patch.dict(
                os.environ,
                {
                    "GROK_BOT_WEBHOOK_URL": "https://hooks.cursor.com/probe",
                    "GROK_BOT_WEBHOOK_KEY": "private-test-key",
                },
            ),
            mock.patch.object(
                sys,
                "argv",
                ["dispatch.py", task_id, "--delivery-id", delivery_id],
            ),
            mock.patch.object(dispatch.urllib.request, "build_opener", return_value=Opener()),
            contextlib.redirect_stdout(output),
        ):
            self.assertEqual(dispatch.main(), 0)

        request = captured["request"]
        self.assertEqual(captured["timeout"], 15)
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(request.get_header("Authorization"), "Bearer private-test-key")
        self.assertEqual(
            request.data,
            (
                '{"protocol_version":1,"kind":"task_offered","delivery_id":"'
                + delivery_id
                + '","task_id":"'
                + task_id
                + '"}'
            ).encode(),
        )
        self.assertNotIn("private-test-key", output.getvalue())

    def test_uncertain_result_is_not_retried(self):
        class Opener:
            def open(self, _request, timeout):
                raise urllib.error.URLError("private destination")

        error = io.StringIO()
        with (
            mock.patch.dict(
                os.environ,
                {
                    "GROK_BOT_WEBHOOK_URL": "https://hooks.cursor.com/probe",
                    "GROK_BOT_WEBHOOK_KEY": "private-test-key",
                },
            ),
            mock.patch.object(sys, "argv", ["dispatch.py", str(uuid.uuid4())]),
            mock.patch.object(dispatch.urllib.request, "build_opener", return_value=Opener()),
            contextlib.redirect_stderr(error),
        ):
            self.assertEqual(dispatch.main(), 1)
        self.assertIn("outcome unknown", error.getvalue())
        self.assertNotIn("private-test-key", error.getvalue())
        self.assertNotIn("private destination", error.getvalue())


if __name__ == "__main__":
    unittest.main()
