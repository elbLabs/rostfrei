#!/usr/bin/env python3
"""Probe an explicit Claude binary with disposable config; never start a model.

Run from the repository root. Exit 0 means configuration checks passed, not that
a skill executed or a UI rendered. The isolated login check should return false.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--claude", required=True, type=Path)
    parser.add_argument(
        "--scratch-root", type=Path, default=Path("artifacts/studio-adapters/tools")
    )
    args = parser.parse_args()
    binary = args.claude.resolve(strict=True)
    base_args = ["--bare", "--setting-sources", ""]
    report = {
        "binary": str(binary), "baseArgs": base_args,
        "modelInvoked": False, "checks": [],
    }
    with tempfile.TemporaryDirectory(
        prefix="claude-config-probe-", dir=args.scratch_root.resolve(strict=True)
    ) as temporary:
        root = Path(temporary)
        env = {
            "PATH": os.defpath,
            "HOME": str(root),
            "CLAUDE_CONFIG_DIR": str(root / ".claude"),
            "XDG_CONFIG_HOME": str(root / "config"),
            "XDG_CACHE_HOME": str(root / "cache"),
            "XDG_DATA_HOME": str(root / "data"),
            "TMPDIR": str(root),
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
            "CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL": "1",
            "DISABLE_UPDATES": "1",
        }
        plugin = root / "plugin"
        manifest = plugin / ".claude-plugin/plugin.json"
        manifest.parent.mkdir(parents=True)
        manifest.write_text(json.dumps({
            "name": "studio-synthetic-config-probe",
            "version": "0.0.1",
            "description": "Synthetic fixture for plugin configuration validation only",
            "author": {"name": "Rostfrei adapter experiment"},
        }) + "\n")
        skill = plugin / "skills/inspect-fixture"
        skill.mkdir(parents=True)
        fixture = (
            "SYNTHETIC CONFIGURATION FIXTURE — not a Tracer recording.\n"
            "Illustrative operationId: synthetic-config-operation\n"
            "Illustrative messageId: synthetic-config-message\n"
        )
        (skill / "fixture.txt").write_text(fixture)
        (skill / "SKILL.md").write_text(
            "---\n"
            "name: inspect-fixture\n"
            "description: Inspect this plugin's labeled synthetic fixture on request.\n"
            "disable-model-invocation: true\n"
            "---\n\n"
            "Read fixture.txt beside this skill. Preserve its synthetic label and IDs.\n"
            "Explain it as text. Do not execute commands or change any evidence.\n"
        )
        invalid = root / "invalid/.claude-plugin/plugin.json"
        invalid.parent.mkdir(parents=True)
        invalid.write_text("{}\n")
        commands = [
            (["--version"], 0),
            (["--help"], 0),
            (["auth", "status", "--help"], 0),
            (["plugin", "validate", "--help"], 0),
            (["plugin", "list", "--help"], 0),
            (["plugin", "details", "--help"], 0),
            (["auth", "status", "--json"], 1),
            (["plugin", "validate", str(plugin), "--strict", "--json"], 0),
            (["plugin", "validate", str(invalid.parent.parent), "--json"], 1),
            (["--plugin-dir", str(plugin), "plugin", "list", "--json"], 0),
            (["--plugin-dir", str(plugin), "plugin", "details",
              "studio-synthetic-config-probe"], 0),
        ]
        # --bare avoids keychain reads; empty setting sources exclude user,
        # project and local settings. Only this explicit plugin is loaded.
        for command, expected in commands:
            run = subprocess.run(
                [str(binary), *base_args, *command],
                cwd=root, env=env, stdin=subprocess.DEVNULL,
                capture_output=True, text=True, timeout=30,
            )
            entry = {"args": command, "exit": run.returncode}
            if command == ["auth", "status", "--json"]:
                status = json.loads(run.stdout)
                entry["isolatedAuth"] = {
                    key: status.get(key) for key in ("loggedIn", "authMethod")
                }
                assert status.get("loggedIn") is False, "Expected empty isolated login"
            else:
                entry.update(stdout=run.stdout, stderr=run.stderr)
            report["checks"].append(entry)
            assert run.returncode == expected, entry
            if command[:2] == ["plugin", "validate"] and "--help" not in command:
                assert json.loads(run.stdout)["success"] is (expected == 0)
            if command[-3:] == ["plugin", "list", "--json"]:
                plugins = json.loads(run.stdout)
                assert len(plugins) == 1
                assert plugins[0]["id"] == "studio-synthetic-config-probe@inline"
                assert plugins[0]["scope"] == "session" and plugins[0]["enabled"]
        assert (skill / "fixture.txt").read_text() == fixture
    report["temporaryFilesRemoved"] = True
    report["result"] = "plugin configuration validated; skill and rendering untested"
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
