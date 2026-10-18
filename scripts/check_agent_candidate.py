#!/usr/bin/env python3
"""Prepare matched local artifacts from one clean candidate; no publication or trial.

Outputs include the externally installed packaged CLI, kit/checksum, external
bike-rental app, full Cargo metadata/locks, all Rostfrei identities, retained
runner and separate host build provenance. Actual NATS qualification is a next
step (check_agent_skills_live.py), not a claim made by this artifact builder.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
CONTRACTS = {"catalog": 1, "behavioralTest": 2, "messageSeries": 2, "fixture": 1}
SKILLS = ["rostfrei-develop-feature", "rostfrei-tracer"]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def clean_revision(source):
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip()
    if subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=all"], cwd=source):
        raise ValueError("Candidate source must be clean; put evidence/build outputs outside the checkout")
    return revision


def verify_release(release, revision, version):
    expected = {"kitVersion": 1, "frameworkVersion": version, "sourceRevision": revision,
                "sourceTreeDirty": False, "contracts": CONTRACTS, "skills": SKILLS}
    if release != expected or not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("CLI/kit candidate revision, clean build identity, version or protocol mismatch")


def verify_crates(metadata, source, revision, version):
    records = []
    for package in metadata["packages"]:
        if not package["name"].startswith("rostfrei"):
            continue
        origin = package["source"]
        manifest = Path(package["manifest_path"]).resolve()
        if package["version"] != version or (origin is None and not manifest.is_relative_to(source.resolve())) or (
                origin is not None and (not origin.startswith("git+") or origin.rsplit("#", 1)[-1] != revision)):
            raise ValueError(f'Mixed candidate crate: {package["name"]}: {origin or manifest}')
        records.append({key: package[key] for key in ("id", "name", "version", "source", "manifest_path")})
    if not records:
        raise ValueError("No resolved Rostfrei crates")
    return records


def verify_rustls(lock):
    versions = [item["version"] for item in tomllib.loads(lock.read_text())["package"] if item["name"] == "rustls"]
    if not versions or any(tuple(map(int, version.split("."))) < (0, 23, 45) for version in versions):
        raise ValueError(f"{lock}: rustls must be patched >=0.23.45; found {versions}. Report to lockfile owner.")
    return versions


def immutable_checkout(source, destination):
    revision = clean_revision(source)
    subprocess.run(["git", "clone", "--quiet", "--no-hardlinks", "--no-checkout", str(source), str(destination)], check=True)
    subprocess.run(["git", "-c", "maintenance.auto=false", "checkout", "--quiet", "--detach", revision], cwd=destination, check=True)
    if clean_revision(destination) != revision:
        raise ValueError("Source-checkout revision changed")
    return destination


def toml_value(value):
    if isinstance(value, dict):
        return "{ " + ", ".join(f"{key} = {toml_value(item)}" for key, item in value.items()) + " }"
    return json.dumps(value)


def prepare_bike_rental(source, destination, revision):
    """Copy tracked example source only; pin every framework dependency to HEAD.

    This prepares a consumer, not a feature solution. The trial owner alone edits
    behavior and records its red/green sequence after human confirmation.
    """
    if destination.exists() or destination.resolve().is_relative_to(source.resolve()):
        raise ValueError("Choose a new external app directory")
    workspace = tomllib.loads((source / "Cargo.toml").read_text())["workspace"]
    example = source / "examples/bike-rental"
    names = subprocess.check_output(["git", "ls-files", "-z", "--", "examples/bike-rental"], cwd=source).decode().split("\0")
    for name in filter(None, names):
        original = source / name
        target = destination / original.relative_to(example)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(original, target)
    manifest = destination / "Cargo.toml"
    text = manifest.read_text()
    text = re.sub(r'^([\w-]+)\.workspace = true$',
                  lambda match: f'{match[1]} = {toml_value(workspace["package"].get(match[1], workspace["dependencies"].get(match[1])))}',
                  text, flags=re.MULTILINE)
    # Expand dependency tables retaining member feature overrides; all framework
    # crates share a single immutable Git source, including transitive crates.
    parsed = tomllib.loads(manifest.read_text())
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        for name, specification in parsed.get(section, {}).items():
            if not isinstance(specification, dict) or not specification.get("workspace"):
                continue
            inherited = workspace["dependencies"][name]
            dependency = {**inherited} if isinstance(inherited, dict) else {"version": inherited}
            features = [*dependency.get("features", []), *specification.get("features", [])]
            dependency.update({key: value for key, value in specification.items() if key not in ("workspace", "features")})
            if features:
                dependency["features"] = sorted(set(features))
            if dependency.get("package", name).startswith("rostfrei"):
                dependency.pop("path", None)
                dependency["git"] = source.as_uri()
                dependency["rev"] = revision
            text = re.sub(rf'^{re.escape(name)}\s*=.*$', f"{name} = {toml_value(dependency)}", text, flags=re.MULTILINE)
    text = text.replace("[lints]\nworkspace = true", "\n".join(
        f'[lints.{kind}]\n' + "\n".join(f'{name} = {toml_value(value)}' for name, value in entries.items())
        for kind, entries in workspace["lints"].items()))
    manifest.write_text(text)
    shutil.copy2(source / "Cargo.lock", destination / "Cargo.lock")
    shutil.copy2(source / "rust-toolchain.toml", destination / "rust-toolchain.toml")
    return destination


class Recorder:
    def __init__(self, output):
        self.output = output
        self.commands = []

    def save(self, name, value):
        (self.output / name).write_text(json.dumps(value, indent=2) + "\n")

    def run(self, command, cwd=ROOT, env=None):
        command = list(map(str, command))
        result = subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True)
        log = f"command-{len(self.commands):02d}.log"
        (self.output / log).write_text(result.stdout + result.stderr)
        self.commands.append({"argv": command, "cwd": str(cwd), "exit": result.returncode, "log": log})
        self.save("commands.json", self.commands)
        if result.returncode:
            raise RuntimeError(f"Command exited {result.returncode}; inspect {self.output / log}")
        return result.stdout

    def metadata(self, label, source, revision, version, cwd=ROOT, env=None):
        metadata = json.loads(self.run(["cargo", "metadata", "--locked", "--format-version", "1"], cwd, env))
        self.save(f"{label}-metadata.json", metadata)
        shutil.copy2(cwd / "Cargo.lock", self.output / f"{label}-Cargo.lock")
        crates = verify_crates(metadata, source, revision, version)
        self.save(f"{label}-rostfrei-crates.json", crates)
        return metadata

    def host_build(self, label, source, revision, cwd, target, env):
        command = ["cargo", "build", "--locked", "-p", "bike-rental", "--bin", "bike-rental-api", "--target-dir", str(target)]
        self.run(command, cwd, env)
        executable = target / "debug/bike-rental-api"
        provenance = {"sourceRevision": revision, "sourceTreeDirty": False, "source": str(source),
                      "manifest": str(cwd / "Cargo.toml"), "lockSha256": digest(cwd / "Cargo.lock"),
                      "command": command, "executable": str(executable), "executableSha256": digest(executable),
                      "attestation": "local build command and binary hash; HTTP catalog does not attest Git SHA"}
        self.save(f"{label}-host-build.json", provenance)


def qualify(output):
    revision = clean_revision(ROOT)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    output.mkdir(parents=True)
    record = Recorder(output)
    record.save("candidate.json", {"revision": revision, "version": version, "source": str(ROOT),
                                  "liveQualification": "pending", "freshSessionTrial": "parent-owned; unexecuted",
                                  "humanFeedback": "pending"})
    environment = {**os.environ, "ROSTFREI_TOOLS_DIR": str(output / "tools")}
    record.metadata("workspace", ROOT, revision, version)
    record.save("workspace-rustls.json", verify_rustls(ROOT / "Cargo.lock"))
    target = output / "target"
    record.run(["cargo", "package", "--locked", "-p", "rostfrei-structure", "--target-dir", target])
    crate = target / f"package/rostfrei-structure-{version}.crate"
    extraction = output / "package-source"
    with tarfile.open(crate) as package:
        package.extractall(extraction, filter="data")
    packaged, = extraction.iterdir()
    vcs = json.loads((packaged / ".cargo_vcs_info.json").read_text())
    if vcs["git"]["sha1"] != revision or vcs["git"].get("dirty", False):
        raise ValueError("Packaged CLI source provenance differs from the clean candidate")
    record.save("packaged-cli-vcs.json", vcs)
    packaged_metadata = json.loads(record.run(["cargo", "metadata", "--locked", "--format-version", "1"], packaged, environment))
    record.save("packaged-cli-metadata.json", packaged_metadata)
    record.save("packaged-cli-rostfrei-crates.json", [
        {key: package[key] for key in ("id", "name", "version", "source", "manifest_path")}
        for package in packaged_metadata["packages"] if package["name"].startswith("rostfrei")])
    record.run(["cargo", "tree", "--locked", "-i", "rustls"], packaged, environment)
    shutil.copy2(packaged / "Cargo.lock", output / "packaged-cli-Cargo.lock")
    record.save("packaged-cli-rustls.json", verify_rustls(packaged / "Cargo.lock"))
    record.run(["cargo", "install", "--locked", "--path", packaged, "--debug", "--root", output / "cli",
                "--target-dir", output / "package-target"], env=environment)
    cli = output / "cli/bin/cargo-rostfrei"
    shutil.rmtree(extraction)
    release = json.loads(record.run([cli, "rostfrei", "skills", "info"], env={**environment, "PATH": ""}))
    verify_release(release, revision, version)
    record.save("cli-release.json", release)
    environment["ROSTFREI_CLI"] = str(cli)
    record.run([sys.executable, ROOT / "scripts/package_agent_skills.py", "--require-clean", "--output", output / "kit"], env=environment)
    archive, = (output / "kit").glob("*.tar.gz")
    checksum, filename = archive.with_suffix(".gz.sha256").read_text().split()
    if checksum != digest(archive) or filename != archive.name:
        raise ValueError("Kit checksum mismatch")
    with tarfile.open(archive) as kit:
        kit_release = json.load(kit.extractfile(f"rostfrei-agent-skills-{version}/release.json"))
    verify_release(kit_release, revision, version)
    record.host_build("workspace", ROOT, revision, ROOT, target, environment)
    app = prepare_bike_rental(ROOT, output / "external-bike-rental", revision)
    # Seed from the candidate lock, then explicitly resolve the standalone graph.
    record.run(["cargo", "metadata", "--format-version", "1"], app, environment)
    record.metadata("external", ROOT, revision, version, app, environment)
    record.save("external-rustls.json", verify_rustls(app / "Cargo.lock"))
    record.run([cli, "rostfrei", "skills", "install"], app, environment)
    record.run([cli, "rostfrei", "skills", "update"], app, environment)
    for name in SKILLS:
        installed = json.loads((app / f".agents/skills/{name}/references/release.json").read_text())
        verify_release(installed, revision, version)
        if installed != kit_release:
            raise ValueError("Installed kit identity mismatch")
    cached, = (output / "tools").rglob("cargo-rostfrei")
    cached_release = json.loads(record.run([cached, "rostfrei", "skills", "info"], app, {**environment, "PATH": ""}))
    verify_release(cached_release, revision, version)
    record.save("retained-runner.json", {"path": str(cached), "sha256": digest(cached), "release": cached_release,
                                        "cliSha256": digest(cli), "packageSourceRemoved": not extraction.exists()})
    record.host_build("external", ROOT, revision, app, output / "external-target", environment)
    if clean_revision(ROOT) != revision:
        raise ValueError("Candidate changed during qualification")
    record.save("artifacts.json", {"status": "passed", "revision": revision, "release": release,
                                   "archive": str(archive), "sha256": checksum,
                                   "liveQualification": "pending", "humanFeedback": "pending"})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--output", type=Path, help="new external artifact/evidence directory")
    group.add_argument("--prepare-app", type=Path, help="only copy a new external pinned bike-rental app (trial owner setup)")
    args = parser.parse_args()
    try:
        if args.prepare_app:
            prepare_bike_rental(ROOT, args.prepare_app.resolve(), clean_revision(ROOT))
        else:
            output = args.output.resolve()
            if output.exists() or output.is_relative_to(ROOT):
                raise ValueError("Choose a new output directory outside the source checkout")
            qualify(output)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Candidate qualification failed: {error}\n")


if __name__ == "__main__":
    main()
