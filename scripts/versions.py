#!/usr/bin/env python3
"""Check or bump the shared Rust crate version (Python 3.11+)."""

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
FIXTURE = Path("crates/rostfrei-macros/tests/dependency-matrix")
DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")
SEMVER = re.compile(
    r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)


def read(path):
    return tomllib.loads(path.read_text())


def validate_version(version):
    match = SEMVER.fullmatch(version)
    if not match or any(
        part.isdigit() and len(part) > 1 and part.startswith("0")
        for part in (match.group(4) or "").split(".")
    ):
        raise ValueError(f"Invalid semantic version: {version!r} (omit the v prefix)")


def workspace(root):
    manifest = read(root / "Cargo.toml")
    members = {}
    excluded = {
        path.resolve()
        for pattern in manifest["workspace"].get("exclude", [])
        for path in root.glob(pattern)
    }
    for pattern in manifest["workspace"]["members"]:
        for path in root.glob(pattern):
            if path.resolve() not in excluded:
                members[path / "Cargo.toml"] = read(path / "Cargo.toml")
    if not members:
        raise ValueError("No workspace members found")
    return manifest, members


def dependency_tables(manifest):
    for scope in (manifest, *manifest.get("target", {}).values()):
        for name in DEPENDENCY_TABLES:
            yield scope.get(name, {})


def internal_dependencies(root, manifest, members):
    names = {member["package"]["name"] for member in members.values()}
    paths = {path.parent.resolve() for path in members}
    return {
        alias: dependency
        for alias, dependency in manifest["workspace"].get("dependencies", {}).items()
        if (
            (dependency.get("package", alias) if isinstance(dependency, dict) else alias)
            in names
            or (
                isinstance(dependency, dict)
                and "path" in dependency
                and (root / dependency["path"]).resolve() in paths
            )
        )
    }


def check(root, tag=None):
    manifest, members = workspace(root)
    version = manifest["workspace"]["package"]["version"]
    validate_version(version)
    internal = internal_dependencies(root, manifest, members)
    names = {member["package"]["name"] for member in members.values()}
    paths = {path.parent.resolve() for path in members}
    errors = []
    for alias, dependency in internal.items():
        if not isinstance(dependency, dict) or dependency.get("version") != version:
            errors.append(f"workspace.dependencies.{alias} must specify version = {version!r}")
        if not isinstance(dependency, dict) or "path" not in dependency:
            errors.append(f"workspace.dependencies.{alias} must specify its local path")
        elif (root / dependency["path"]).resolve() not in paths:
            errors.append(f"workspace.dependencies.{alias} must point to a workspace member")
    for path, member in members.items():
        label = path.relative_to(root)
        if member["package"].get("version") != {"workspace": True}:
            errors.append(f"{label}: package version must inherit with version.workspace = true")
        for table in dependency_tables(member):
            for alias, dependency in table.items():
                detail = dependency if isinstance(dependency, dict) else {}
                is_internal = (
                    detail.get("package", alias) in names
                    or alias in internal
                    or ("path" in detail and (path.parent / detail["path"]).resolve() in paths)
                )
                if is_internal and (
                    detail.get("workspace") is not True
                    or alias not in internal
                    or any(key in detail for key in ("version", "path", "git", "package"))
                ):
                    errors.append(f"{label}: {alias} must inherit its workspace dependency")
    if tag is not None and tag != f"v{version}":
        errors.append(f"Release tag {tag!r} must match workspace version: v{version}")
    if errors:
        raise ValueError("\n".join(errors))
    return version


def replace_version(text, section, new_version):
    """Replace one version in a known TOML section while retaining formatting."""
    pattern = re.compile(r"(?m)^\[" + re.escape(section) + r"\][^\n]*\n(?P<body>(?:(?!\[).*(?:\n|$))*)")
    match = pattern.search(text)
    if not match:
        raise ValueError(f"Cannot locate [{section}]")
    body, count = re.subn(
        r'(?m)^(version\s*=\s*)"[^"\n]*"',
        lambda item: item.group(1) + f'"{new_version}"',
        match.group("body"),
    )
    if count != 1:
        raise ValueError(f"Expected one version in [{section}]")
    return text[:match.start("body")] + body + text[match.end("body"):]


def bumped_manifest(root, new_version):
    manifest, members = workspace(root)
    text = replace_version((root / "Cargo.toml").read_text(), "workspace.package", new_version)
    for alias in internal_dependencies(root, manifest, members):
        # The root manifest uses inline dependency tables. Also support full tables.
        section = re.search(r"(?ms)^\[workspace\.dependencies\][^\n]*\n(.*?)(?=^\[|\Z)", text)
        pattern = re.compile(r"(?m)^(" + re.escape(alias) + r"\s*=\s*\{[^\n]*?\bversion\s*=\s*)\"[^\"\n]*\"")
        body, count = pattern.subn(lambda match: match.group(1) + f'"{new_version}"', section.group(1) if section else "")
        if count == 1:
            text = text[:section.start(1)] + body + text[section.end(1):]
        else:
            text = replace_version(text, f"workspace.dependencies.{alias}", new_version)
    return text


def bump(root, new_version):
    validate_version(new_version)
    old_version = check(root)
    updated = bumped_manifest(root, new_version)
    manifests = [root / "Cargo.toml", root / FIXTURE / "Cargo.toml"]
    lockfiles = [path.with_name("Cargo.lock") for path in manifests]
    touched = [manifests[0], *lockfiles]
    originals = {path: path.read_bytes() if path.exists() else None for path in touched}
    try:
        manifests[0].write_text(updated)
        for manifest in manifests:
            # --workspace retains locked registry versions; offline prevents index updates.
            subprocess.run(
                ["cargo", "update", "--workspace", "--offline", "--manifest-path", str(manifest)],
                cwd=root,
                check=True,
            )
        check(root)
    except BaseException:
        for path, original in originals.items():
            if original is None:
                path.unlink(missing_ok=True)
            else:
                path.write_bytes(original)
        raise
    print(f"Updated shared version: {old_version} -> {new_version}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    checker = commands.add_parser("check", help="Check shared versions and optional release tag")
    checker.add_argument("--tag", help="Require this release tag to equal v<workspace version>")
    bumper = commands.add_parser("bump", help="Update manifest and both Cargo lockfiles")
    bumper.add_argument("version", help="New semantic version without a v prefix")
    args = parser.parse_args()
    try:
        if args.command == "check":
            print(f"Shared crate versions are consistent: {check(ROOT, args.tag)}")
        else:
            bump(ROOT, args.version)
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        print(f"Version check failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
