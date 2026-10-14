"""Source-discovered skills prepare current resources without replacing edits."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parent.parent
SKILLS = ("rostfrei-develop-feature", "rostfrei-tracer")


def fresh_source_checkout(destination):
    """Copy tracked working files, but no generated resources, into a fresh Git repo."""
    paths = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0")
    for name in filter(None, paths):
        source = ROOT / name
        if source.is_file():
            target = destination / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
    subprocess.run(["git", "init", "--quiet", str(destination)], check=True)
    # These short-lived repos must not leave detached writers racing their cleanup.
    for key, value in (("maintenance.auto", "false"), ("gc.auto", "0")):
        subprocess.run(["git", "config", "--local", key, value], cwd=destination, check=True)
    subprocess.run(["git", "add", "."], cwd=destination, check=True)
    commit(destination)
    return destination


def commit(root):
    subprocess.run([
        "git", "-c", "user.name=Skills regression", "-c", "user.email=skills@example.invalid",
        "-c", "commit.gpgsign=false", "commit", "--quiet", "--allow-empty", "-m", "Source fixture",
    ], cwd=root, check=True)


class SourceSkillTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="rostfrei-source-skills-")
        self.addCleanup(temporary.cleanup)
        # Exercise eager Git maintenance without inheriting or changing user config.
        git_config = Path(temporary.name) / "gitconfig"
        git_config.write_text("[gc]\n\tauto = 1\n\tautoDetach = true\n[maintenance]\n\tauto = true\n")
        self.git_trace = Path(temporary.name) / "git-trace.jsonl"
        environment = patch.dict(os.environ, {
            "GIT_CONFIG_GLOBAL": str(git_config),
            "GIT_TRACE2_EVENT": str(self.git_trace),
        })
        environment.start()
        self.addCleanup(environment.stop)
        self.root = fresh_source_checkout(Path(temporary.name) / "checkout")
        self.skill = self.root / ".agents/skills/rostfrei-develop-feature"
        self.release = self.skill / "references/release.json"
        self.assertFalse(self.release.exists())

    def prepare(self):
        return subprocess.run([
            sys.executable, "-I", str(self.root / "scripts/package_agent_skills.py"), "--prepare-source",
        ], cwd=self.root, capture_output=True, text=True)

    def test_fixture_does_not_launch_background_git_maintenance(self):
        commit(self.root)
        writers = [
            event["argv"]
            for line in self.git_trace.read_text().splitlines()
            if (event := json.loads(line)).get("event") == "child_start"
            and any(command in event.get("argv", []) for command in ("gc", "repack", "pack-objects"))
        ]
        self.assertEqual(writers, [], "Fixture commits must not leave writers racing temporary-directory cleanup")

    def test_discovered_source_bootstrap_and_runner_refresh_revision(self):
        result = self.prepare()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.root, text=True).strip()
        for name in SKILLS:
            skill = self.root / ".agents/skills" / name
            metadata = json.loads((skill / "references/release.json").read_text())
            self.assertEqual(metadata["sourceRevision"], head)
            self.assertFalse(metadata["sourceTreeDirty"])
            self.assertEqual(metadata["contracts"], {
                "catalog": 1, "behavioralTest": 2, "messageSeries": 2, "fixture": 1,
            })
            self.assertEqual((skill / "LICENSE").read_bytes(), (self.root / "LICENSE").read_bytes())
        schema = self.skill / "assets/behavioral-test-v2.schema.json"
        self.assertEqual(schema.read_bytes(), (self.root / "crates/rostfrei-tracer/schema/behavioral-test-v2.schema.json").read_bytes())
        self.assertEqual(subprocess.check_output(["git", "status", "--porcelain"], cwd=self.root), b"")

        # Merely changing HEAD must not leave a silently stale generated revision.
        commit(self.root)
        new_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.root, text=True).strip()
        self.assertNotEqual(head, new_head)
        result = subprocess.run([
            sys.executable, "-I", str(self.skill / "scripts/run_behavior.py"),
            str(self.skill / "assets/add-first-bicycle.json"), "--output", "unused-evidence.json",
        ], cwd=self.root, capture_output=True, text=True,
            env={**os.environ, "ROSTFREI_TRACER_URL": "unsupported://source-regression"})
        # Deliberately stop before networking, after the automatic source refresh.
        self.assertIn("must be an HTTP(S) origin", result.stderr)
        self.assertEqual(json.loads(self.release.read_text())["sourceRevision"], new_head)
        self.assertFalse((self.root / "unused-evidence.json").exists())

    def test_bootstrap_preserves_modified_generated_resources(self):
        result = self.prepare()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        other = self.root / ".agents/skills/rostfrei-tracer/references/release.json"
        original = other.read_bytes()
        edited = self.release.read_bytes() + b"\nconsumer note\n"
        self.release.write_bytes(edited)
        commit(self.root)
        result = self.prepare()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Preserving", result.stderr)
        self.assertEqual(self.release.read_bytes(), edited)
        self.assertEqual(other.read_bytes(), original)  # preflight before all writes


if __name__ == "__main__":
    unittest.main()
