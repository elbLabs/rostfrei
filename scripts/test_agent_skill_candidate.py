"""Release gates: missing selected diagnostics and mixed same-version sources fail."""

import copy
import json
from pathlib import Path
import tempfile
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
                            count += 1
                            self.assertNotIn("path", dependency)
                            self.assertEqual(dependency["rev"], revision)
                            self.assertEqual(dependency["git"], ROOT.as_uri())
            self.assertGreater(count, 3)
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
if __name__ == "__main__":
    unittest.main()
