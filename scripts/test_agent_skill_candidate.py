"""Release gates: missing selected diagnostics and mixed same-version sources fail."""

import copy
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import tempfile
import threading
import tomllib
import unittest

from check_agent_tracer_capabilities import Capabilities


class CandidateGates(unittest.TestCase):
    def test_selected_handler_capability_is_required_but_older_absence_is_recorded(self):
        with tempfile.TemporaryDirectory() as temporary:
            client = Capabilities({"ROSTFREI_TRACER_URL": "http://127.0.0.1:1",
                                   "ROSTFREI_APPLICATION": "owned",
                                   "ROSTFREI_API_TOKEN": "test-only",
                                   "ROSTFREI_INSPECTION_TOKEN": "inspection-only"}, Path(temporary))
            client.catalogs["test"] = {"contexts": [{"id": "bike-rental", "commands": [
                {"id": "add-bicycle", "versions": [{"schemaVersion": 2}]}]}]}
            definition = {"expected": {"graphs": [{"nodes": [
                {"context": "bike-rental", "name": "add-bicycle", "schemaVersion": 2}]}]}}
            self.assertEqual(client.simulate_handler(definition), "unavailable")
            with self.assertRaisesRegex(ValueError, "required.*simulateHandlerHrefTemplate"):
                client.simulate_handler(definition, required=True)
            self.assertEqual(json.loads((Path(temporary) / "handler-simulation.json").read_text())["status"], "unavailable")

    def test_same_version_cannot_hide_a_mixed_crate_or_tool_identity(self):
        from check_agent_candidate import verify_crates, verify_release
        revision = "a" * 40
        release = {"sourceRevision": revision, "sourceTreeDirty": False,
                   "frameworkVersion": "0.0.5-alpha", "kitVersion": 1,
                   "contracts": {"catalog": 1, "behavioralTest": 2, "messageSeries": 2, "fixture": 1},
                   "skills": ["rostfrei-develop-feature", "rostfrei-tracer"]}
        verify_release(release, revision, "0.0.5-alpha")
        for field, value in [("sourceRevision", "b" * 40), ("sourceTreeDirty", True),
                             ("contracts", {**release["contracts"], "messageSeries": 1})]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                verify_release({**release, field: value}, revision, "0.0.5-alpha")
        metadata = {"packages": [
            {"name": name, "id": name, "version": "0.0.5-alpha",
             "source": f"git+file:///owned/candidate?rev={revision}#{revision}",
             "manifest_path": f"/cargo/checkouts/{name}/Cargo.toml"}
            for name in ("rostfrei", "rostfrei-core", "rostfrei-nats")]}
        self.assertEqual(len(verify_crates(metadata, Path("/owned/candidate"), revision, "0.0.5-alpha")), 3)
        mixed = copy.deepcopy(metadata)
        mixed["packages"][1]["source"] = "git+file:///other#" + "b" * 40
        with self.assertRaisesRegex(ValueError, "rostfrei-core"):
            verify_crates(mixed, Path("/owned/candidate"), revision, "0.0.5-alpha")
        mixed["packages"][1]["source"] = None
        with self.assertRaisesRegex(ValueError, "rostfrei-core"):
            verify_crates(mixed, Path("/owned/candidate"), revision, "0.0.5-alpha")

    def test_external_recipe_expands_inheritance_and_pins_every_framework_dependency(self):
        from check_agent_candidate import ROOT, prepare_bike_rental
        revision = "a" * 40
        with tempfile.TemporaryDirectory() as temporary:
            app = prepare_bike_rental(ROOT, Path(temporary) / "app", revision)
            manifest = tomllib.loads((app / "Cargo.toml").read_text())
            self.assertIsInstance(manifest["package"]["version"], str)
            self.assertNotIn("workspace", manifest["lints"])
            count = 0
            for section in ("dependencies", "dev-dependencies"):
                for name, dependency in manifest[section].items():
                    if isinstance(dependency, dict):
                        self.assertNotIn("workspace", dependency)
                        if dependency.get("package", name).startswith("rostfrei"):
                            self.assertEqual(name, "rostfrei")
                            count += 1
                            self.assertNotIn("path", dependency)
                            self.assertEqual(dependency["rev"], revision)
                            self.assertEqual(dependency["git"], ROOT.as_uri())
            self.assertEqual(count, 2, "One facade dependency in each dependency section")
            self.assertEqual(manifest["dependencies"]["rostfrei"]["features"], ["nats", "tracer"])
            self.assertEqual(manifest["dev-dependencies"]["rostfrei"]["features"], ["testing"])
            self.assertEqual((app / "src/bin/bike-rental-api.rs").read_bytes(),
                             (ROOT / "examples/bike-rental/src/bin/bike-rental-api.rs").read_bytes())
            self.assertFalse((app / ".agents").exists(), "Copy must not implicitly install skills")

    def test_unpatched_packaged_cli_lock_is_not_qualified(self):
        from check_agent_candidate import verify_rustls
        with tempfile.TemporaryDirectory() as temporary:
            lock = Path(temporary) / "Cargo.lock"
            lock.write_text('[[package]]\nname="rustls"\nversion="0.23.43"\n')
            with self.assertRaisesRegex(ValueError, "lockfile owner"):
                verify_rustls(lock)
            lock.write_text(lock.read_text().replace("0.23.43", "0.23.45"))
            self.assertEqual(verify_rustls(lock), ["0.23.45"])


class NormalPublicationTests(unittest.TestCase):
    root = {"context": "bike-rental", "name": "add-bicycle", "schemaVersion": 2,
            "payload": {"fleet_id": "normal", "bicycle_id": "one", "condition": "serviceable"}}

    @staticmethod
    def operation(status="queued"):
        # OperationSnapshot plus the flattened CommandSubmissionResponse field,
        # matching http.rs's #[serde(flatten)] contract. It is not {operation: ...}.
        operation = {"operationId": "owned-operation", "correlationId": "owned-correlation",
                     "operationEventsHref": "/returned-events", "correlationEventsHref": "/returned-correlation",
                     "messageSeriesHref": "/returned-series", "events": {"kind": "observed", "href": "/returned-events"},
                     "mode": "dispatch", "status": status, "command": "add-bicycle", "schemaVersion": 2,
                     "context": "bike-rental", "latestEventId": 1}
        if status == "completed":
            operation["result"] = {"decision": "accepted", "published": True,
                                   "commandMessageId": "owned-command", "responseMessageId": "owned-response", "duplicate": False}
        return operation

    @contextmanager
    def host(self, submission, polled=None, location="/returned-operation", truncated=False):
        requests = []
        capture = {"operationId": "owned-operation", "correlationId": "owned-correlation", "mode": "dispatch",
                   "messageSeries": {"messages": [], "commandOutcomes": []},
                   "capture": {"settled": True, "settledFor": "500ms", "fidelity": "exact"}}

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def reply(self, status, value):
                requests.append((self.command, self.path))
                body = value if isinstance(value, bytes) else json.dumps(value).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body) + (10 if truncated else 0)))
                if self.command == "POST" and location is not None:
                    self.send_header("Location", location)
                self.end_headers()
                self.wfile.write(body)

            def do_POST(self):
                self.rfile.read(int(self.headers["Content-Length"]))
                self.reply(202, submission)

            def do_GET(self):
                if self.path == "/returned-operation":
                    self.reply(200, polled)
                elif self.path == "/returned-series?within=10s&settleFor=500ms":
                    self.reply(200, capture)
                else:
                    self.reply(404, {"unexpected": self.path})

        with tempfile.TemporaryDirectory() as temporary:
            server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
            worker = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.01})
            worker.start()
            try:
                client = Capabilities({"ROSTFREI_TRACER_URL": f"http://127.0.0.1:{server.server_port}",
                    "ROSTFREI_APPLICATION": "owned", "ROSTFREI_API_TOKEN": "test-fixture",
                    "ROSTFREI_INSPECTION_TOKEN": "inspection-fixture", "ROSTFREI_DISPATCH_TOKEN": "dispatch-fixture"}, Path(temporary))
                client.catalogs["test"] = {"contexts": [{"id": "bike-rental", "commands": [
                    {"id": "add-bicycle", "versions": [{"schemaVersion": 2, "dispatchHrefTemplate": "/advertised-dispatch"}]}]}]}
                yield client, requests
            finally:
                server.shutdown()
                server.server_close()
                worker.join()

    def test_flat_202_is_polled_to_a_terminal_result_before_capture(self):
        submitted = {**self.operation(), "idempotencyKey": "seed"}
        with self.host(submitted, self.operation("completed")) as (client, requests):
            result = client.normal_command(self.root, "seed", "accepted")
            self.assertEqual(result["status"], "completed")
            self.assertEqual(requests, [("POST", "/advertised-dispatch"), ("GET", "/returned-operation"),
                                        ("GET", "/returned-series?within=10s&settleFor=500ms")])
            retained = json.loads((client.output / "seed-submission.json").read_text())
            self.assertEqual(retained["status"], 202)
            self.assertEqual(json.loads(retained["body"]), submitted)

    def test_invalid_202_stops_with_retained_response_and_no_retry_or_capture(self):
        missing_id = self.operation()
        del missing_id["operationId"]
        no_result = self.operation("completed")
        del no_result["result"]
        cases = [("nested", {"operation": self.operation()}, "/returned-operation"),
                 ("missing-id", missing_id, "/returned-operation"),
                 ("missing-location", self.operation(), None),
                 ("unsafe-location", self.operation(), "https://example.invalid/operation"),
                 ("completed-without-result", no_result, "/returned-operation"),
                 ("broken-json", b'{"operationId":', "/returned-operation"),
                 ("truncated-http", self.operation(), "/returned-operation")]
        for name, submission, location in cases:
            with self.subTest(name=name), self.host(submission, location=location, truncated=name == "truncated-http") as (client, requests):
                with self.assertRaises((ValueError, RuntimeError)):
                    client.normal_command(self.root, "invalid", "accepted")
                self.assertEqual(requests, [("POST", "/advertised-dispatch")])
                retained = json.loads((client.output / "invalid-submission.json").read_text())
                self.assertEqual(retained["status"], 202)
                self.assertEqual(retained["location"], location)
                self.assertTrue(retained["body"])
                self.assertEqual(retained["bodyComplete"], name != "truncated-http")
                if name == "truncated-http":
                    self.assertEqual(retained["readError"], "IncompleteRead")
                self.assertFalse((client.output / "invalid.json").exists())

    def test_changed_poll_identity_is_retained_without_capture_or_resubmission(self):
        changed = {**self.operation("completed"), "operationId": "different-operation"}
        with self.host(self.operation(), changed) as (client, requests):
            with self.assertRaisesRegex(ValueError, "identity"):
                client.normal_command(self.root, "changed", "accepted")
            self.assertEqual(requests, [("POST", "/advertised-dispatch"), ("GET", "/returned-operation")])
            retained = json.loads((client.output / "changed-poll-0.json").read_text())
            self.assertEqual(json.loads(retained["body"]), changed)

    def test_indeterminate_poll_stops_without_capture_or_resubmission(self):
        with self.host(self.operation(), self.operation("indeterminate")) as (client, requests):
            with self.assertRaisesRegex(RuntimeError, "indeterminate.*do not resubmit"):
                client.normal_command(self.root, "uncertain", "accepted")
            self.assertEqual(requests, [("POST", "/advertised-dispatch"), ("GET", "/returned-operation")])
            retained = json.loads((client.output / "uncertain-poll-0.json").read_text())
            self.assertEqual(json.loads(retained["body"])["status"], "indeterminate")


if __name__ == "__main__":
    unittest.main()
