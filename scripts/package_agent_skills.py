#!/usr/bin/env python3
"""Build the portable Agent Skills kit using the shared Rust release version."""

import argparse
import gzip
import hashlib
import io
import json
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


def build(output):
    subprocess.run([sys.executable, str(ROOT / "scripts/versions.py"), "check"], check=True)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip())
    catalog = (ROOT / "crates/rostfrei-tracer/src/catalog.rs").read_text()
    release = {
        "kitVersion": 1,
        "frameworkVersion": version,
        "sourceRevision": revision,
        "sourceTreeDirty": dirty,
        "contracts": {
            "catalog": int(re.search(r"CATALOG_VERSION: u32 = (\d+)", catalog)[1]),
            "behavioralTest": 2,
            "messageSeries": 2,
            "fixture": 1,
        },
        "skills": list(NAMES),
    }
    files = {
        "install.py": (ROOT / "scripts/install_agent_skills.py").read_bytes(),
        "README.md": (ROOT / "docs/agent-skills-install.md").read_bytes(),
        "QUALIFICATION.md": (ROOT / "docs/agent-skills-qualification.md").read_bytes(),
        "LICENSE": (ROOT / "LICENSE").read_bytes(),
        "release.json": json_bytes(release),
    }
    for name in NAMES:
        source = ROOT / ".agents/skills" / name
        if not (source / "SKILL.md").is_file():
            raise ValueError(f"Missing skill: {name}")
        for path in sorted(source.rglob("*")):
            if path.is_symlink():
                raise ValueError(f"Skill resources must be regular files: {path}")
            if path.is_file():
                files[f"skills/{name}/{path.relative_to(source).as_posix()}"] = path.read_bytes()
        files[f"skills/{name}/references/release.json"] = json_bytes(release)
        files[f"skills/{name}/LICENSE"] = files["LICENSE"]
    # Generated from the runtime's authoritative schema; no manually maintained copy.
    files["skills/rostfrei-develop-feature/assets/behavioral-test-v2.schema.json"] = (
        ROOT / "crates/rostfrei-tracer/schema/behavioral-test-v2.schema.json"
    ).read_bytes()
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
