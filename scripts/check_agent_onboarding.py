#!/usr/bin/env python3
"""Native setup acceptance against actual Cargo resolution in external applications."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
