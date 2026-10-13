#!/usr/bin/env python3
"""Install from this extracted kit into an explicit client skill directory."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tempfile


def files_under(directory):
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError(f"Preserving existing non-directory or symlink: {directory}")
    files = {}
    for path in sorted(directory.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"Preserving symlink: {path}")
        if path.is_file():
            files[path.relative_to(directory).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
        elif not path.is_dir():
            raise ValueError(f"Unsupported resource: {path}")
    return files


def install(kit, destination):
    actual = files_under(kit)
    actual.pop("manifest.json", None)
    expected = json.loads((kit / "manifest.json").read_text())["sha256"]
    if actual != expected:
        raise ValueError("Kit integrity check failed; extract a fresh verified archive")
    release = json.loads((kit / "release.json").read_text())
    names = release["skills"]
    for name in names:
        if not isinstance(name, str) or Path(name).name != name or name in (".", ".."):
            raise ValueError("Invalid skill name in release metadata")
    pending = []
    # Preflight every skill before writing anything. An old or modified skill is
    # deliberately a conflict: upgrades need a new destination and a human merge.
    for name in names:
        source = kit / "skills" / name
        source_files = files_under(source)
        target = destination / name
        if target.exists() or target.is_symlink():
            if files_under(target) != source_files:
                raise ValueError(f"Preserving existing skill: {target}; install elsewhere and merge deliberately")
        else:
            pending.append((source, target))
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".rostfrei-install-", dir=destination) as temporary:
        for source, target in pending:
            staged = Path(temporary) / target.name
            shutil.copytree(source, staged)
            # Do not replace a directory created since preflight.
            target.mkdir()
            # A failed copy leaves its partial directory visible for inspection.
            for item in staged.iterdir():
                item.rename(target / item.name)
    print(f"Installed Rostfrei {release['frameworkVersion']} skills in {destination}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--destination", type=Path, required=True, help="e.g. /path/to/app/.agents/skills")
    args = parser.parse_args()
    try:
        install(Path(__file__).resolve().parent, args.destination.expanduser().absolute())
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"Installation stopped: {error}\n")


if __name__ == "__main__":
    main()
