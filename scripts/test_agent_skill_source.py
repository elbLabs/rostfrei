"""Source-discovered skills use the Rust CLI without generating skill resources."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parent.parent


def fresh_source_checkout(destination):
    """Copy tracked working files, but no generated resources, into a fresh Git repo."""
    paths = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0")
    for name in filter(None, paths):
        source = ROOT / name
        if source.is_symlink():
            target = destination / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.symlink_to(os.readlink(source))
        elif source.is_file():
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

    def build_cli(self):
        target = ROOT / "target/skills-source"
        subprocess.run([
            "cargo", "build", "--locked", "--offline", "--manifest-path",
            str(self.root / "crates/rostfrei-structure/Cargo.toml"),
            "--bin", "cargo-rostfrei", "--target-dir", str(target),
        ], cwd=self.root, check=True)
        return target / "debug/cargo-rostfrei"

    def test_fixture_does_not_launch_background_git_maintenance(self):
        commit(self.root)
        writers = [
            event["argv"]
            for line in self.git_trace.read_text().splitlines()
            if (event := json.loads(line)).get("event") == "child_start"
            and any(command in event.get("argv", []) for command in ("gc", "repack", "pack-objects"))
        ]
        self.assertEqual(writers, [], "Fixture commits must not leave writers racing temporary-directory cleanup")

    def test_discovered_source_uses_current_cli_without_generated_resources(self):
        cli = self.build_cli()
        environment = {**os.environ, "PATH": ""}
        metadata = json.loads(subprocess.check_output([str(cli), "rostfrei", "skills", "info"], env=environment))
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.root, text=True).strip()
        self.assertEqual(metadata["sourceRevision"], head)
        self.assertFalse(metadata["sourceTreeDirty"])
        self.assertFalse(self.release.exists())
        self.assertFalse((self.skill / "assets/behavioral-test-v2.schema.json").exists())
        self.assertEqual(metadata["contracts"], {"catalog": 1, "behavioralTest": 2, "messageSeries": 2, "fixture": 1})
        self.assertEqual(subprocess.check_output(["git", "status", "--porcelain"], cwd=self.root), b"")

        # Merely changing HEAD must not leave a silently stale generated revision.
        commit(self.root)
        new_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.root, text=True).strip()
        self.assertNotEqual(head, new_head)
        cli = self.build_cli()
        metadata = json.loads(subprocess.check_output([str(cli), "rostfrei", "skills", "info"], env=environment))
        self.assertEqual(metadata["sourceRevision"], new_head)
        self.assertFalse(self.release.exists())
        # The source link's target is authoritative: a normal Rust rebuild picks
        # up instruction edits without synchronizing a second copy or Python prep.
        instructions = self.skill / "SKILL.md"
        instructions.write_text(instructions.read_text() + "\nSource freshness regression\n")
        cli = self.build_cli()
        installed = self.root.parent / "installed"
        result = subprocess.run([str(cli), "rostfrei", "skills", "install", "--destination", str(installed)],
                                cwd=self.root.parent, env=environment, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((installed / "rostfrei-develop-feature/SKILL.md").read_text().endswith("Source freshness regression\n"))
        self.assertTrue(json.loads((installed / "rostfrei-develop-feature/references/release.json").read_text())["sourceTreeDirty"])
        self.assertFalse(self.release.exists())

    def test_stale_legacy_metadata_is_preserved_and_rejected(self):
        cli = os.environ.get("ROSTFREI_CLI", str(ROOT / "target/debug/cargo-rostfrei"))
        environment = {**os.environ, "PATH": "", "ROSTFREI_TRACER_URL": "unsupported://source-regression"}
        metadata = json.loads(subprocess.check_output([cli, "rostfrei", "skills", "info"], env=environment))
        metadata["sourceRevision"] = "0" * 40
        edited = json.dumps(metadata).encode()
        self.release.parent.mkdir(exist_ok=True)
        self.release.write_bytes(edited)
        result = subprocess.run([cli, "rostfrei", "test", str(self.skill / "assets/add-first-bicycle.json"),
                                 "--output", "unused.json"], cwd=self.root, env=environment, capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("release or protocol mismatch", result.stderr)
        self.assertEqual(self.release.read_bytes(), edited)

    def test_cargo_git_marker_does_not_make_clean_revision_dirty(self):
        # Cargo writes this cache-completeness marker into --git checkouts.
        (self.root / ".cargo-ok").write_text("{}")
        cli = self.build_cli()
        command = [str(cli), "rostfrei", "skills", "info"]
        metadata = json.loads(subprocess.check_output(command, env={**os.environ, "PATH": ""}))
        self.assertFalse(metadata["sourceTreeDirty"], "Cargo's marker is not a source modification")
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.root, text=True).strip()
        self.assertEqual(metadata["sourceRevision"], head)
        source = self.root / "crates/rostfrei-structure/src/main.rs"
        source.write_text(source.read_text() + "\n// consumer modification\n")
        self.build_cli()
        metadata = json.loads(subprocess.check_output(command, env={**os.environ, "PATH": ""}))
        self.assertTrue(metadata["sourceTreeDirty"], "Actual source modifications must remain visible")


if __name__ == "__main__":
    unittest.main()
