"""Credential/link boundaries of the installed skill's small HTTP client."""

import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import unittest
from unittest.mock import patch
from urllib.error import HTTPError, URLError


SOURCE = Path(__file__).resolve().parent.parent / ".agents/skills/rostfrei-develop-feature/scripts/run_behavior.py"
SPEC = importlib.util.spec_from_file_location("skill_behavior_client", SOURCE)
CLIENT = importlib.util.module_from_spec(SPEC)
with patch.object(sys, "dont_write_bytecode", True):
    SPEC.loader.exec_module(CLIENT)


class SkillHttpTests(unittest.TestCase):
    def setUp(self):
        environment = patch.dict(os.environ, {
            "ROSTFREI_TRACER_URL": "http://127.0.0.1:1309",
            "ROSTFREI_API_TOKEN": "test-only-control-capability",
        })
        environment.start()
        self.addCleanup(environment.stop)
        self.client = CLIENT.Tracer()

    def test_unsafe_links_never_send_credentials(self):
        with patch.object(self.client.opener, "open") as send:
            for href in ("//other.invalid/catalog", "https://other.invalid/catalog", "/\\other",
                         "/path#fragment", "/path\nheader", "/path/{unresolved}"):
                with self.subTest(href=href), self.assertRaises(CLIENT.RunnerError) as caught:
                    self.client.request(href)
                self.assertEqual(caught.exception.kind, CLIENT.ErrorKind.INVALID_RESPONSE)
            send.assert_not_called()

    def test_request_keeps_capability_in_header_and_exact_definition_in_body(self):
        definition = {"schemaVersion": 2, "id": "test-definition"}
        response = io.BytesIO(b'{"status":"passed"}')
        with patch.object(self.client.opener, "open", return_value=response) as send:
            self.assertEqual(self.client.request("/test-runs", definition), {"status": "passed"})
        request = send.call_args.args[0]
        self.assertEqual(request.full_url, "http://127.0.0.1:1309/test-runs")
        self.assertEqual(json.loads(request.data), definition)
        self.assertEqual(request.get_header("Authorization"), "Bearer test-only-control-capability")
        self.assertEqual(send.call_args.kwargs["timeout"], 90)

    def test_ambiguous_submission_and_redirect_are_not_retried(self):
        for error in (URLError("disconnected"), HTTPError("http://127.0.0.1:1309", 302, "redirect", {}, None)):
            with self.subTest(error=type(error).__name__):
                with patch.object(self.client.opener, "open", side_effect=error) as send:
                    with self.assertRaises(CLIENT.RunnerError) as caught:
                        self.client.request("/test-runs", {"id": "test"})
                    self.assertEqual(caught.exception.kind, CLIENT.ErrorKind.REQUEST_FAILED)
                    send.assert_called_once()
        self.assertIsNone(CLIENT.NoRedirect().redirect_request(None, None, 302, "", {}, "https://other.invalid"))


if __name__ == "__main__":
    unittest.main()
