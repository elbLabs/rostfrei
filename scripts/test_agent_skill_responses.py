"""Fault-server regression: HTTP truncation is typed incomplete evidence, not red."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest


ROOT = Path(__file__).resolve().parent.parent
TOKEN = "test-only-response-capability"
UNTRUSTED = "untrusted-partial-server-body"


class ResponseFailureTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="rostfrei-response-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.skill = self.root / "relocated-skill"
        (self.skill / "scripts").mkdir(parents=True)
        (self.skill / "references").mkdir()
        source = ROOT / ".agents/skills/rostfrei-develop-feature"
        shutil.copyfile(source / "scripts/run_behavior.py", self.skill / "scripts/run_behavior.py")
        (self.skill / "references/release.json").write_text(json.dumps({"contracts": {"catalog": 1}}))
        shutil.copyfile(source / "assets/add-first-bicycle.json", self.root / "definition.json")
        self.mode = "partial_run"
        self.submissions = 0
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def respond(self, value, partial=False, malformed=False):
                data = UNTRUSTED.encode() if partial or malformed else json.dumps(value).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data) + (100 if partial else 0)))
                self.end_headers()
                self.wfile.write(data)
                self.wfile.flush()
                self.close_connection = True

            def do_GET(self):
                if self.path == "/catalog":
                    self.respond({"catalogVersion": 1, "behavioralTest": {
                        "schemaHref": "/schema", "validateHref": "/validate", "runHref": "/run",
                    }, "testScenario": {"fixturesHref": "/fixtures"}})
                elif self.path == "/schema":
                    self.respond({"type": "object"})
                elif self.path == "/fixtures":
                    self.respond({"items": [{"id": "no-rental-fleets", "fixtureHref": "/fixture"}]})
                elif self.path == "/fixture":
                    self.respond({"id": "no-rental-fleets", "revision": "1", "messages": []})
                elif self.path.startswith("/series?"):
                    self.respond({"capture": {"settled": True, "fidelity": "exact"}}, partial=owner.mode.startswith("partial_capture"))
                else:
                    self.send_error(404)

            def do_POST(self):
                self.rfile.read(int(self.headers["Content-Length"]))
                if self.path == "/validate":
                    self.respond({"valid": True})
                elif self.path == "/run":
                    owner.submissions += 1
                    verdict = "failed" if owner.mode in ("failed_verdict", "partial_capture_failed") else "passed"
                    self.respond({"status": "unexpected" if owner.mode == "unknown_verdict" else verdict,
                                  "comparison": {"status": verdict, "diagnostics": [{"code": "payload-mismatch"}] if verdict == "failed" else []},
                                  "operation": {"messageSeriesHref": "/series"}},
                                 partial=owner.mode == "partial_run", malformed=owner.mode == "malformed_run")
                else:
                    self.send_error(404)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(self.server.server_close)
        self.addCleanup(thread.join)
        self.addCleanup(self.server.shutdown)

    def execute(self, mode):
        self.mode = mode
        evidence = self.root / "evidence.json"
        result = subprocess.run([
            sys.executable, "-I", str(self.skill / "scripts/run_behavior.py"),
            str(self.root / "definition.json"), "--output", str(evidence),
        ], cwd=self.root, capture_output=True, text=True, env={**os.environ,
            "ROSTFREI_TRACER_URL": f"http://127.0.0.1:{self.server.server_port}",
            "ROSTFREI_API_TOKEN": TOKEN,
        })
        self.assertEqual(self.submissions, 1)
        for text in (result.stdout, result.stderr, evidence.read_text()):
            self.assertNotIn(TOKEN, text)
            self.assertNotIn(UNTRUSTED, text)
        return result, json.loads(evidence.read_text())

    def test_partial_run_is_typed_incomplete_response_and_unknown_outcome(self):
        result, evidence = self.execute("partial_run")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(evidence["runState"], "submission-outcome-unknown")
        self.assertNotIn("report", evidence)
        self.assertEqual(evidence["error"]["kind"], "incomplete_response")
        self.assertEqual(evidence["error"]["phase"], "run")
        self.assertEqual(evidence["error"]["details"]["method"], "POST")
        self.assertEqual(evidence["error"]["details"]["expectedAdditionalBytes"], 100)
        self.assertEqual(json.loads(result.stderr)["error"], evidence["error"])

    def test_partial_capture_preserves_the_complete_report(self):
        result, evidence = self.execute("partial_capture")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(evidence["runState"], "reported")
        self.assertEqual(evidence["report"]["status"], "passed")
        self.assertEqual(evidence["captureState"], "incomplete")
        self.assertEqual(evidence["error"]["kind"], "incomplete_response")
        self.assertEqual(evidence["error"]["phase"], "capture")
        self.assertEqual(evidence["error"]["details"]["reportedVerdict"], "passed")

    def test_partial_capture_preserves_a_failed_report_too(self):
        result, evidence = self.execute("partial_capture_failed")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(evidence["runState"], "reported")
        self.assertEqual(evidence["report"]["status"], "failed")
        self.assertEqual(evidence["error"]["details"]["reportedVerdict"], "failed")
        self.assertEqual(evidence["error"]["phase"], "capture")

    def test_complete_malformed_json_is_a_different_error_kind(self):
        result, evidence = self.execute("malformed_run")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(evidence["runState"], "submission-outcome-unknown")
        self.assertEqual(evidence["error"]["kind"], "invalid_response")

    def test_unknown_report_status_is_not_a_behavioral_failure(self):
        result, evidence = self.execute("unknown_verdict")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(evidence["runState"], "response-received")
        self.assertEqual(evidence["error"]["kind"], "invalid_response")

    def test_reported_assertion_failure_is_exit_one(self):
        result, evidence = self.execute("failed_verdict")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(evidence["runState"], "reported")
        self.assertEqual(evidence["report"]["status"], "failed")
        self.assertNotIn("error", evidence)

    def test_reported_pass_is_exit_zero(self):
        result, evidence = self.execute("passed_verdict")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(evidence["report"]["status"], "passed")
        self.assertNotIn("error", evidence)


if __name__ == "__main__":
    unittest.main()
