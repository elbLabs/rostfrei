#!/usr/bin/env python3
"""Build the portable Agent Skills kit using the shared Rust release version."""

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tomllib


ROOT = Path(__file__).resolve().parent.parent
NAMES = ("rostfrei-develop-feature", "rostfrei-tracer")


def json_bytes(value):
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def release_metadata():
    subprocess.run([sys.executable, str(ROOT / "scripts/versions.py"), "check"], check=True)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    cli = os.environ.get("ROSTFREI_CLI", str(ROOT / "target/debug/cargo-rostfrei"))
    release = json.loads(subprocess.check_output([cli, "rostfrei", "skills", "info"], text=True))
    if release["frameworkVersion"] != version or release["sourceRevision"] != revision:
        raise ValueError("CLI build is stale; rebuild/install rostfrei-structure from this checkout")
    release["sourceTreeDirty"] = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip())
    catalog = (ROOT / "crates/rostfrei-tracer/src/catalog.rs").read_text()
    if release["contracts"]["catalog"] != int(re.search(r"CATALOG_VERSION: u32 = (\d+)", catalog)[1]):
        raise ValueError("CLI catalog contract differs from the framework")
    return release


def generated_resources(release):
    """Archive metadata/schema come from the CLI and authoritative runtime inputs."""
    resources = {}
    for name in NAMES:
        resources[f"{name}/references/release.json"] = json_bytes(release)
        resources[f"{name}/LICENSE"] = (ROOT / "LICENSE").read_bytes()
    resources["rostfrei-develop-feature/assets/behavioral-test-v2.schema.json"] = (
        ROOT / "crates/rostfrei-tracer/schema/behavioral-test-v2.schema.json"
    ).read_bytes()
    return resources


def build(output):
    release = release_metadata()
    version = release["frameworkVersion"]
    files = {
        "README.md": (ROOT / "docs/agent-skills-install.md").read_bytes(),
        "QUALIFICATION.md": (ROOT / "docs/agent-skills-qualification.md").read_bytes(),
        "LICENSE": (ROOT / "LICENSE").read_bytes(),
        "release.json": json_bytes(release),
    }
    for name in NAMES:
        source = ROOT / ".agents/skills" / name
        if not (source / "SKILL.md").is_file():
            raise ValueError(f"Missing skill: {name}")
        # Only tracked, intended portable resources; no ignored caches, local
        # generated files, credentials, or arbitrary untracked application work.
        tracked = subprocess.check_output(["git", "ls-files", "-z", "--", str(source.relative_to(ROOT))], cwd=ROOT).decode().split("\0")
        for relative in sorted(filter(None, tracked)):
            path = ROOT / relative
            if path.is_symlink():
                raise ValueError(f"Skill resources must be regular files: {path}")
            if path.is_file() and (path.suffix in (".md", ".json") or path.name == "LICENSE"):
                files[f"skills/{name}/{path.relative_to(source).as_posix()}"] = path.read_bytes()
    files.update({f"skills/{name}": data for name, data in generated_resources(release).items()})
    files["manifest.json"] = json_bytes({
        "sha256": {name: hashlib.sha256(data).hexdigest() for name, data in sorted(files.items())},
    })
    output.mkdir(parents=True, exist_ok=True)
    stem = f"rostfrei-agent-skills-{version}"
    archive = output / f"{stem}.tar.gz"
    # Reproducible bytes for the same source state, irrespective of local mtimes,
    # checkout path, ownership, or archive output path.
    with archive.open("wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as bundle:
            for name, data in sorted(files.items()):
                info = tarfile.TarInfo(f"{stem}/{name}")
                info.size = len(data)
                info.mode = 0o644
                bundle.addfile(info, io.BytesIO(data))
    checksum = archive.with_suffix(archive.suffix + ".sha256")
    checksum.write_text(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n")
    print(archive)
    print(checksum)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        build(args.output.resolve())
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Kit build failed: {error}\n")


if __name__ == "__main__":
    main()
