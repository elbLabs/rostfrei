#!/usr/bin/env python3
"""Native setup acceptance against actual Cargo resolution in external applications."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent.parent
CLI = Path(os.environ.get("ROSTFREI_CLI", ROOT / "target/debug/cargo-rostfrei"))


def command(arguments, cwd, env=None):
    return subprocess.run(list(map(str, arguments)), cwd=cwd, env=env,
                          capture_output=True, text=True, check=False)


class ExistingApplicationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="rostfrei-onboarding-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        environment = patch.dict(os.environ, {
            "ROSTFREI_TOOLS_DIR": str(self.root / "tools"),
            "CARGO_TARGET_DIR": str(ROOT / "target/onboarding-local"),
        })
        environment.start()
        self.addCleanup(environment.stop)

    def test_default_install_resolves_renamed_inherited_dependency(self):
        # The requirement uses inheritance/renaming; resolved package identity is
        # still rostfrei. A real lockfile and Cargo graph are authoritative.
        (self.root / "Cargo.toml").write_text(
            '[workspace]\nresolver = "3"\nmembers = ["application"]\n'
            '[workspace.dependencies]\nframework = { package = "rostfrei", path = '
            + json.dumps(str(ROOT / "crates/rostfrei")) + ' }\n')
        app = self.root / "application"
        (app / "src").mkdir(parents=True)
        (app / "src/main.rs").write_text("fn main() {}\n")
        (app / "Cargo.toml").write_text('[package]\nname = "consumer"\nversion = "0.1.0"\n'
                                      'edition = "2024"\n[dependencies]\nframework.workspace = true\n')
        locked = command(["cargo", "generate-lockfile", "--offline"], app)
        self.assertEqual(locked.returncode, 0, locked.stderr)
        result = command([CLI, "rostfrei", "skills", "install"], app)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse((self.root / ".agents").exists())
        info = json.loads(command([CLI, "rostfrei", "skills", "info"], app).stdout)
        for name in ("rostfrei-develop-feature", "rostfrei-tracer"):
            skill = app / ".agents/skills" / name
            self.assertTrue((skill / "SKILL.md").is_file())
            self.assertEqual(json.loads((skill / "references/release.json").read_text()), info)
        self.assertEqual(command([CLI, "rostfrei", "skills", "install"], app).returncode, 0)

    def test_missing_dependency_is_reported_without_installation(self):
        (self.root / "src").mkdir()
        (self.root / "src/main.rs").write_text("fn main() {}\n")
        (self.root / "Cargo.toml").write_text('[package]\nname = "plain-app"\nversion = "0.1.0"\n')
        self.assertEqual(command(["cargo", "generate-lockfile", "--offline"], self.root).returncode, 0)
        result = command([CLI, "rostfrei", "skills", "install"], self.root)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no resolved rostfrei dependency", result.stderr)
        self.assertFalse((self.root / ".agents").exists())

    def test_workspace_requires_explicit_package_and_honors_manifest_and_destination(self):
        manifest = self.root / "Cargo.toml"
        manifest.write_text('[workspace]\nresolver = "3"\nmembers = ["one", "two"]\n')
        for name in ("one", "two"):
            package = self.root / name
            (package / "src").mkdir(parents=True)
            (package / "src/main.rs").write_text("fn main() {}\n")
            (package / "Cargo.toml").write_text('[package]\nname = ' + json.dumps(name)
                + '\nversion = "0.1.0"\nedition = "2024"\n[dependencies]\nrostfrei = { path = '
                + json.dumps(str(ROOT / "crates/rostfrei")) + ' }\n')
        self.assertEqual(command(["cargo", "generate-lockfile", "--offline"], self.root).returncode, 0)
        ambiguous = command([CLI, "rostfrei", "skills", "install"], self.root)
        self.assertNotEqual(ambiguous.returncode, 0)
        self.assertIn("--package", ambiguous.stderr)
        result = command([CLI, "rostfrei", "skills", "install", "--manifest-path", manifest,
                          "--package", "two", "--destination", "selected-skills"], self.root)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue((self.root / "selected-skills/rostfrei-tracer/SKILL.md").is_file())
        self.assertFalse((self.root / "one/.agents").exists())
        self.assertFalse((self.root / "two/.agents").exists())

    def test_published_starter_reports_its_real_pre_kit_capability_gap(self):
        app = self.root / "published-starter"
        created = command([CLI, "rostfrei", "new", app], self.root)
        self.assertEqual(created.returncode, 0, created.stderr)
        self.assertIn("predates skills-enabled tooling", created.stdout)
        locked = command(["cargo", "generate-lockfile"], app)
        self.assertEqual(locked.returncode, 0, locked.stderr)
        result = command([CLI, "rostfrei", "skills", "install"], app,
                         {**os.environ, "CARGO_PROFILE_RELEASE_OPT_LEVEL": "0",
                          "CARGO_PROFILE_RELEASE_DEBUG": "0"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("predates skills-enabled tooling", result.stderr)
        self.assertFalse((app / ".agents").exists())

    def test_versioned_tools_follow_upgrade_and_preserve_local_edits_atomically(self):
        from test_agent_skill_source import fresh_source_checkout, commit
        source = fresh_source_checkout(self.root / "framework")
        first_revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip()
        environment = {**os.environ, "ROSTFREI_TOOLS_DIR": str(self.root / "tools"),
                       "CARGO_TARGET_DIR": str(ROOT / "target/onboarding-tools"),
                       "CARGO_PROFILE_RELEASE_OPT_LEVEL": "0", "CARGO_PROFILE_RELEASE_DEBUG": "0"}
        app = self.root / "app"
        (app / "src").mkdir(parents=True)
        (app / "src/main.rs").write_text("fn main() {}\n")
        manifest = app / "Cargo.toml"
        manifest.write_text('[package]\nname = "versioned-consumer"\nversion = "0.1.0"\nedition = "2024"\n'
                            '[dependencies]\nframework = { package = "rostfrei", git = '
                            + json.dumps(source.as_uri()) + ', rev = ' + json.dumps(first_revision) + ' }\n')
        def lock():
            result = command(["cargo", "generate-lockfile"], app, environment)
            self.assertEqual(result.returncode, 0, result.stderr)
        def setup(action):
            return command([CLI, "rostfrei", "skills", action], app, environment)
        lock()
        result = setup("install")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        skills = app / ".agents/skills"
        develop = skills / "rostfrei-develop-feature/references/worked-example.md"
        tracer = skills / "rostfrei-tracer/SKILL.md"
        original_develop, original_tracer = develop.read_bytes(), tracer.read_bytes()
        baseline = (skills / ".rostfrei-managed.json").read_bytes()
        release = json.loads((skills / "rostfrei-tracer/references/release.json").read_text())
        self.assertEqual(release["sourceRevision"], first_revision)
        global_info = json.loads(command([CLI, "rostfrei", "skills", "info"], app).stdout)
        self.assertNotEqual(release["sourceRevision"], global_info["sourceRevision"])
        self.assertEqual(setup("install").returncode, 0)
        tracer.write_bytes(original_tracer + b"\nLocal consumer decision\n")
        notes = skills / "rostfrei-tracer/user-notes.md"
        notes.write_text("keep my notes\n")
        # Actual second framework/tool revision, with changed and renamed resources.
        worked = source / ".agents/skills/rostfrei-develop-feature/references/worked-example.md"
        worked.write_bytes(worked.read_bytes() + b"\nUpstream revision B\n")
        runtime = source / ".agents/skills/rostfrei-tracer/references/runtime.md"
        runtime.rename(runtime.with_name("runtime-next.md"))
        instructions = source / ".agents/skills/rostfrei-tracer/SKILL.md"
        instructions.write_text(instructions.read_text().replace("references/runtime.md", "references/runtime-next.md"))
        link = source / "crates/rostfrei-structure/resources/runtime.md"
        link.unlink()
        link.symlink_to(os.path.relpath(runtime.with_name("runtime-next.md"), link.parent))
        inputs = source / "crates/rostfrei-structure/src/agent_tools/bundle_files.rs"
        inputs.write_text(inputs.read_text().replace("rostfrei-tracer/references/runtime.md", "rostfrei-tracer/references/runtime-next.md"))
        subprocess.run(["git", "add", "."], cwd=source, check=True)
        commit(source)
        second_revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip()
        manifest.write_text(manifest.read_text().replace(first_revision, second_revision))
        lock()
        upgrade_install = setup("install")
        self.assertNotEqual(upgrade_install.returncode, 0)
        self.assertIn("skills update", upgrade_install.stderr)
        environment["CARGO_NET_OFFLINE"] = "true"  # Matching tools are now cached.
        rejected = setup("update")
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn("Preserving", rejected.stderr)
        self.assertEqual(develop.read_bytes(), original_develop)
        self.assertEqual((skills / ".rostfrei-managed.json").read_bytes(), baseline)
        self.assertTrue(tracer.read_bytes().endswith(b"Local consumer decision\n"))
        self.assertTrue((skills / "rostfrei-tracer/references/runtime.md").exists())
        # Deliberately resolve the local conflict, then the complete update succeeds.
        tracer.write_bytes(original_tracer)
        updated = setup("update")
        self.assertEqual(updated.returncode, 0, updated.stdout + updated.stderr)
        self.assertTrue(develop.read_bytes().endswith(b"Upstream revision B\n"))
        self.assertFalse((skills / "rostfrei-tracer/references/runtime.md").exists())
        self.assertTrue((skills / "rostfrei-tracer/references/runtime-next.md").is_file())
        self.assertEqual(notes.read_text(), "keep my notes\n")
        self.assertEqual(setup("update").returncode, 0)
        release = json.loads((skills / "rostfrei-tracer/references/release.json").read_text())
        self.assertEqual(release["sourceRevision"], second_revision)
        # Runtime routing remains strictly matched, without Cargo/Git/Python or
        # downloads. A configuration blocker must not be a release-mismatch red.
        routed = command([CLI, "rostfrei", "test", skills / "rostfrei-develop-feature/assets/add-first-bicycle.json",
                          "--output", app / "unused-evidence.json"], app,
                         {**environment, "PATH": "", "ROSTFREI_TRACER_URL": "unsupported://routing-check"})
        self.assertEqual(routed.returncode, 2, routed.stderr)
        self.assertEqual(json.loads(routed.stderr)["error"]["phase"], "configuration")
        self.assertFalse((app / "unused-evidence.json").exists())
        release["skills"] = ["unexpected-skill"]
        invalid_release = app / "invalid-release.json"
        invalid_release.write_text(json.dumps(release))
        rejected_release = command([CLI, "rostfrei", "test", skills / "rostfrei-develop-feature/assets/add-first-bicycle.json",
                                    "--release", invalid_release, "--output", app / "unused-evidence.json"],
                                   app, {**environment, "PATH": ""})
        self.assertEqual(rejected_release.returncode, 2)
        self.assertIn("release or protocol mismatch", rejected_release.stderr)
        # Two reachable framework revisions must never select an arbitrary one.
        single_dependency = manifest.read_text()
        manifest.write_text(manifest.read_text() + 'other = { package = "rostfrei", git = '
                            + json.dumps(source.as_uri()) + ', rev = ' + json.dumps(first_revision) + ' }\n')
        lock()
        ambiguous = setup("update")
        self.assertNotEqual(ambiguous.returncode, 0)
        self.assertIn("multiple rostfrei", ambiguous.stderr)
        # A Git-backed path checkout can change without advancing its revision.
        # Explicit setup must rebuild it, and a dirty cached build must never be
        # substituted for a later immutable Git dependency at that same revision.
        worked.write_bytes(worked.read_bytes() + b"\nUncommitted local source edit\n")
        manifest.write_text('[package]\nname = "versioned-consumer"\nversion = "0.1.0"\nedition = "2024"\n'
                            '[dependencies]\nframework = { package = "rostfrei", path = '
                            + json.dumps(str(source / "crates/rostfrei")) + ' }\n')
        lock()
        local = setup("update")
        self.assertEqual(local.returncode, 0, local.stdout + local.stderr)
        self.assertTrue(develop.read_bytes().endswith(b"Uncommitted local source edit\n"))
        self.assertTrue(json.loads((skills / "rostfrei-tracer/references/release.json").read_text())["sourceTreeDirty"])
        local_baseline = (skills / ".rostfrei-managed.json").read_bytes()
        manifest.write_text(single_dependency)
        lock()
        dirty_cache = setup("update")
        self.assertNotEqual(dirty_cache.returncode, 0)
        self.assertIn("pristine framework revision", dirty_cache.stderr)
        self.assertEqual((skills / ".rostfrei-managed.json").read_bytes(), local_baseline)


if __name__ == "__main__":
    unittest.main()
