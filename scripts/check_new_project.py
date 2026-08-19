#!/usr/bin/env python3
"""Build an external starter using its unmodified public dependencies (Python 3.11+)."""

import json
import os
import subprocess
import tempfile
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def main():
    env = os.environ.copy()
    # Share build artifacts, not workspace configuration or dependency overrides.
    env["CARGO_TARGET_DIR"] = str(Path(env.get("CARGO_TARGET_DIR", ROOT / "target")).resolve())
    with tempfile.TemporaryDirectory(prefix="rostfrei-starter-") as directory:
        external = Path(directory).resolve()
        if external.is_relative_to(ROOT):
            raise RuntimeError("TMPDIR must be outside the Rostfrei checkout")
        project = external / "review-starter"
        subprocess.run(
            ["cargo", "run", "--locked", "-p", "rostfrei-structure", "--bin",
             "cargo-rostfrei", "--", "rostfrei", "new", str(project)],
            cwd=ROOT, env=env, check=True,
        )
        manifest_path = project / "Cargo.toml"
        original_manifest = manifest_path.read_bytes()
        manifest = tomllib.loads(original_manifest.decode())
        dependency = manifest["dependencies"]["rostfrei"]
        if dependency != manifest["dependencies"]["rostfrei-nats"]:
            raise RuntimeError("Both Rostfrei dependencies must use the same release pin")
        expected_source = f"git+{dependency['git']}?rev={dependency['rev']}#{dependency['rev']}"

        # Run from the generated directory so the checkout's .cargo config is not used.
        # The first check resolves dependencies and creates the user's Cargo.lock.
        for command in (["cargo", "check"], ["cargo", "test", "--locked"]):
            print(f"Running {' '.join(command)} in {project}", flush=True)
            subprocess.run(command, cwd=project, env=env, check=True)

        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--format-version", "1"],
            cwd=project, env=env, text=True,
        ))
        packages = [p for p in metadata["packages"] if p["name"].startswith("rostfrei")]
        if not {"rostfrei", "rostfrei-nats"}.issubset({p["name"] for p in packages}):
            raise RuntimeError("Generated project did not resolve both Rostfrei dependencies")
        for package in packages:
            if package["source"] != expected_source:
                raise RuntimeError(f"{package['name']} resolved outside the public release pin")
        if manifest_path.read_bytes() != original_manifest:
            raise RuntimeError("The generated manifest changed during acceptance checks")
        print(f"External starter passed check and test using {expected_source}", flush=True)


if __name__ == "__main__":
    main()
