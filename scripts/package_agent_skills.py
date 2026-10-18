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
import tempfile
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
    catalog = (ROOT / "crates/rostfrei-tracer/src/catalog.rs").read_text()
    if release["contracts"]["catalog"] != int(re.search(r"CATALOG_VERSION: u32 = (\d+)", catalog)[1]):
        raise ValueError("CLI catalog contract differs from the framework")
    return release


def bundled_resources(release):
    """Export the native bundle; require it to match authoritative source inputs."""
    cli = os.environ.get("ROSTFREI_CLI", str(ROOT / "target/debug/cargo-rostfrei"))
    with tempfile.TemporaryDirectory(prefix="rostfrei-bundle-export-") as temporary:
        root = Path(temporary)
        subprocess.run([cli, "rostfrei", "skills", "install", "--destination", str(root / "skills")],
                       cwd=root, check=True)
        resources = {str(path.relative_to(root / "skills")): path.read_bytes()
                     for name in NAMES for path in (root / "skills" / name).rglob("*") if path.is_file()}
    expected = {}
    for name in NAMES:
        source = ROOT / ".agents/skills" / name
        tracked = subprocess.check_output(["git", "ls-files", "-z", "--", str(source.relative_to(ROOT))], cwd=ROOT).decode().split("\0")
        for relative in filter(None, tracked):
            path = ROOT / relative
            if path.is_symlink():
                raise ValueError(f"Authoritative skill files must be regular files: {path}")
            if path.is_file() and path.suffix in (".md", ".json"):
                expected[f"{name}/{path.relative_to(source).as_posix()}"] = path.read_bytes()
        expected[f"{name}/LICENSE"] = (ROOT / "LICENSE").read_bytes()
        metadata = f"{name}/references/release.json"
        if json.loads(resources[metadata]) != release:
            raise ValueError("Bundled release metadata differs from skills info")
        expected[metadata] = resources[metadata]
    expected["rostfrei-develop-feature/assets/behavioral-test-v2.schema.json"] = (ROOT / "crates/rostfrei-tracer/schema/behavioral-test-v2.schema.json").read_bytes()
    if resources != expected:
        raise ValueError("Bundled CLI resources differ from authoritative source; rebuild the CLI and check its explicit resource inputs")
    return resources


def build(output, require_clean=False):
    release = release_metadata()
    if require_clean:
        from check_agent_candidate import clean_revision, verify_release
        verify_release(release, clean_revision(ROOT), release["frameworkVersion"])
    resources = bundled_resources(release)
    version = release["frameworkVersion"]
    files = {
        "README.md": (ROOT / "docs/agent-skills-install.md").read_bytes(),
        "LICENSE": (ROOT / "LICENSE").read_bytes(),
        "release.json": resources["rostfrei-develop-feature/references/release.json"],
    }
    files.update({f"skills/{name}": data for name, data in resources.items()})
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
    parser.add_argument("--require-clean", action="store_true", help="release gate: reject dirty source or build provenance")
    args = parser.parse_args()
    try:
        build(args.output.resolve(), args.require_clean)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Kit build failed: {error}\n")


if __name__ == "__main__":
    main()
