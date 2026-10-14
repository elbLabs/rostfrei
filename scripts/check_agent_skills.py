#!/usr/bin/env python3
"""Acceptance: build, relocate, and install the release kit outside the workspace."""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import unittest


ROOT = Path(__file__).resolve().parent.parent
NAMES = ("rostfrei-develop-feature", "rostfrei-tracer")


class InstalledKitTests(unittest.TestCase):
    def setUp(self):
        # Intentionally outside ROOT. No source checkout or Python import path is
        # passed to the extracted installer, including after relocation.
        self.temporary = tempfile.TemporaryDirectory(prefix="rostfrei-skills-consumer-")
        self.addCleanup(self.temporary.cleanup)
        self.consumer = Path(self.temporary.name)
        self.assertFalse(self.consumer.is_relative_to(ROOT))
        self.cli = self.consumer / "cargo-rostfrei"
        shutil.copy2(os.environ.get("ROSTFREI_CLI", ROOT / "target/debug/cargo-rostfrei"), self.cli)
        self.no_tools = self.consumer / "no-tools"
        self.no_tools.mkdir()
        self.output = self.consumer / "build"
        result = self.command(ROOT / "scripts/package_agent_skills.py", "--output", self.output)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.archive, = self.output.glob("*.tar.gz")
        digest, name = self.archive.with_suffix(self.archive.suffix + ".sha256").read_text().split()
        self.assertEqual(name, self.archive.name)
        self.assertEqual(digest, hashlib.sha256(self.archive.read_bytes()).hexdigest())
        self.archive_digest = digest
        with tarfile.open(self.archive) as archive:
            for item in archive.getmembers():
                self.assertFalse(item.issym() or item.islnk())
                self.assertFalse(Path(item.name).is_absolute() or ".." in Path(item.name).parts)
            archive.extractall(self.consumer / "extracted", filter="data")
        extracted, = (self.consumer / "extracted").iterdir()
        self.kit = self.consumer / "relocated kit"
        extracted.rename(self.kit)
        shutil.rmtree(self.output)
        self.destination = self.consumer / "app/.agents/skills"

    def command(self, script, *args):
        return subprocess.run(
            [sys.executable, "-I", str(script), *map(str, args)],
            cwd=self.consumer, capture_output=True, text=True, check=False,
        )

    def install(self):
        return subprocess.run([
            str(self.cli), "rostfrei", "skills", "install", "--kit", str(self.kit),
            "--destination", str(self.destination),
        ], cwd=self.consumer, env={**os.environ, "PATH": str(self.no_tools)},
            capture_output=True, text=True, check=False)

    def test_external_install_is_self_contained_and_release_matched(self):
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(list(self.kit.rglob("*.py")), [], "The consumer kit must not ship Python")
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        release = json.loads((self.kit / "release.json").read_text())
        self.assertEqual(release["frameworkVersion"], version)
        self.assertRegex(release["sourceRevision"], r"^[0-9a-f]{40}$")
        self.assertEqual(release["sourceRevision"], subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True,
        ).strip())
        self.assertEqual(release["contracts"]["behavioralTest"], 2)
        self.assertEqual(release["contracts"]["messageSeries"], 2)
        catalog_source = (ROOT / "crates/rostfrei-tracer/src/catalog.rs").read_text()
        self.assertEqual(release["contracts"]["catalog"], int(re.search(r"CATALOG_VERSION: u32 = (\d+)", catalog_source)[1]))
        # Reference validator is pinned separately, not a home-grown substitute.
        from skills_ref_validator import reference_validator
        with reference_validator() as validate:
            for name in NAMES:
                skill = self.destination / name
                self.assertEqual(validate(skill), [])
                self.assertEqual(json.loads((skill / "references/release.json").read_text()), release)
                self.assertEqual((skill / "LICENSE").read_bytes(), (ROOT / "LICENSE").read_bytes())
                for path in skill.rglob("*"):
                    self.assertFalse(path.is_symlink())
                    if path.is_file():
                        text = path.read_text()
                        self.assertNotIn(str(ROOT), text)
                        self.assertNotIn("agent-harness/", text)
                        if path.suffix == ".md":
                            for href in re.findall(r"\]\(([^)]+)\)", text):
                                if "://" not in href and not href.startswith("#"):
                                    target = (path.parent / href.split("#")[0]).resolve()
                                    self.assertTrue(target.is_relative_to(skill), href)
                                    self.assertTrue(target.is_file(), href)
        # Each skill remains usable after deleting the extracted release too.
        shutil.rmtree(self.kit)
        definition = json.loads((self.destination / NAMES[0] / "assets/add-first-bicycle.json").read_text())
        self.assertEqual(definition["schemaVersion"], 2)
        self.assertEqual(definition["setup"]["fixture"], "no-rental-fleets")

    def test_same_source_build_is_reproducible(self):
        result = self.command(ROOT / "scripts/package_agent_skills.py", "--output", self.output)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        archive, = self.output.glob("*.tar.gz")
        self.assertEqual(hashlib.sha256(archive.read_bytes()).hexdigest(), self.archive_digest)
        self.assertFalse((ROOT / ".opencode/skills/rostfrei-tracer/SKILL.md").exists())

    def test_preserves_consumer_edits_and_unrelated_instructions(self):
        self.destination.mkdir(parents=True)
        instructions = self.destination.parent.parent / "AGENTS.md"
        instructions.write_text("Consumer instructions\n")
        unrelated = self.destination / "local-skill/SKILL.md"
        unrelated.parent.mkdir()
        unrelated.write_text("Local skill\n")
        self.assertEqual(self.install().returncode, 0)
        self.assertEqual(self.install().returncode, 0)  # identical is a no-op
        modified = self.destination / NAMES[1] / "SKILL.md"
        modified.write_text(modified.read_text() + "\nConsumer addition\n")
        # Preflight must reject conflicts before installing even the first skill.
        shutil.rmtree(self.destination / NAMES[0])
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("preserv", result.stderr.lower())
        self.assertTrue(modified.read_text().endswith("Consumer addition\n"))
        self.assertFalse((self.destination / NAMES[0]).exists())
        self.assertEqual(instructions.read_text(), "Consumer instructions\n")
        self.assertEqual(unrelated.read_text(), "Local skill\n")

    def test_modified_artifact_and_destination_symlink_are_refused(self):
        target = self.consumer / "user-work"
        target.mkdir()
        self.destination.mkdir(parents=True)
        (self.destination / NAMES[0]).symlink_to(target, target_is_directory=True)
        self.assertNotEqual(self.install().returncode, 0)
        self.assertEqual(list(target.iterdir()), [])
        (self.destination / NAMES[0]).unlink()
        (self.kit / "skills" / NAMES[0] / "SKILL.md").write_text("corrupted")
        self.assertNotEqual(self.install().returncode, 0)
        self.assertEqual(list(self.destination.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
