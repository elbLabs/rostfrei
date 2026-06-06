"""Regression coverage for release/version safeguards; no Rust toolchain needed."""

import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import versions


class VersionTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.manifest = self.root / "Cargo.toml"
        self.manifest.write_text('''[workspace]
members = ["crates/a", "crates/b"]
[workspace.package]
version = "1.2.3-alpha"
[workspace.dependencies]
a = { path = "crates/a", version = "1.2.3-alpha" }
alias = { package = "b", path = "crates/b", version = "1.2.3-alpha" }
external = "1.2.3-alpha"
''')
        for name in ("a", "b"):
            path = self.root / "crates" / name / "Cargo.toml"
            path.parent.mkdir(parents=True)
            path.write_text(f'[package]\nname = "{name}"\nversion.workspace = true\n')
        self.member = self.root / "crates/a/Cargo.toml"
        self.member.write_text(self.member.read_text() + '[dependencies]\nalias.workspace = true\n')

    def test_valid_version_and_tag(self):
        self.assertEqual(versions.check(self.root, "v1.2.3-alpha"), "1.2.3-alpha")

    def test_tag_must_match_exactly(self):
        for tag in ("v1.2.4-alpha", "1.2.3-alpha", "v1.2.3-alpha\n", "v1.2.3"):
            with self.subTest(tag=tag), self.assertRaisesRegex(ValueError, "Release tag"):
                versions.check(self.root, tag)

    def test_stale_alias_version(self):
        self.manifest.write_text(self.manifest.read_text().replace(
            'package = "b", path = "crates/b", version = "1.2.3-alpha"',
            'package = "b", path = "crates/b", version = "1.2.2"',
        ))
        with self.assertRaisesRegex(ValueError, "workspace.dependencies.alias"):
            versions.check(self.root)

    def test_missing_internal_version(self):
        self.manifest.write_text(self.manifest.read_text().replace(
            'path = "crates/a", version = "1.2.3-alpha"', 'path = "crates/a"',
        ))
        with self.assertRaisesRegex(ValueError, "workspace.dependencies.a"):
            versions.check(self.root)

    def test_member_override_and_target_path_only_dependency(self):
        original = self.member.read_text()
        for declaration in (
            '\n[dev-dependencies]\nb = "1.2.2"\n',
            '\n[target.\'cfg(unix)\'.build-dependencies]\nrenamed = { package = "b", path = "../b" }\n',
        ):
            self.member.write_text(original + declaration)
            with self.subTest(declaration=declaration), self.assertRaisesRegex(ValueError, "must inherit"):
                versions.check(self.root)

    def test_package_version_must_be_inherited(self):
        self.member.write_text(self.member.read_text().replace('version.workspace = true', 'version = "1.2.3-alpha"'))
        with self.assertRaisesRegex(ValueError, "package version must inherit"):
            versions.check(self.root)

    def test_semver(self):
        for version in ("1.2.3", "1.2.3-rc.1", "1.2.3+build.01"):
            versions.validate_version(version)
        for version in ("v1.2.3", "01.2.3", "1.2", "1.2.3-01", "1.2.3\n"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                versions.validate_version(version)

    def test_bump_changes_only_shared_versions(self):
        fixture_manifest = self.root / versions.FIXTURE / "Cargo.toml"
        fixture_manifest.parent.mkdir(parents=True)
        fixture_manifest.write_text('[workspace]\nmembers = []\n')
        with patch("versions.subprocess.run") as run:
            versions.bump(self.root, "2.0.0-rc.1")
        self.assertEqual(versions.check(self.root), "2.0.0-rc.1")
        self.assertIn('external = "1.2.3-alpha"', self.manifest.read_text())
        self.assertEqual(run.call_count, 2)
        self.assertEqual(Path(run.call_args_list[1].args[0][-1]), fixture_manifest)

    def test_failed_second_lockfile_update_restores_every_file(self):
        fixture_manifest = self.root / versions.FIXTURE / "Cargo.toml"
        fixture_manifest.parent.mkdir(parents=True)
        fixture_manifest.write_text('[workspace]\nmembers = []\n')
        lock = self.root / "Cargo.lock"
        fixture_lock = fixture_manifest.with_name("Cargo.lock")
        lock.write_text("original root lockfile\n")
        fixture_lock.write_text("original fixture lockfile\n")
        original = {path: path.read_bytes() for path in (self.manifest, lock, fixture_lock)}

        def update(command, **kwargs):
            lock.write_text("partially updated\n")
            fixture_lock.write_text("partially updated\n")
            if command[-1] == str(fixture_manifest):
                raise subprocess.CalledProcessError(1, command)

        with patch("versions.subprocess.run", side_effect=update):
            with self.assertRaises(subprocess.CalledProcessError):
                versions.bump(self.root, "2.0.0")
        self.assertEqual({path: path.read_bytes() for path in original}, original)


if __name__ == "__main__":
    unittest.main()
