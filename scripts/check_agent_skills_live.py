#!/usr/bin/env python3
"""Qualify the installed example against actual bike-rental handlers and NATS.

Run with scripts/test_nats.py; this script owns a unique application namespace.
The caller owns broker cleanup. It records a passing example and an intentional
negative comparison control, not a human-agent feature-development TDD trial.
"""

import argparse
from contextlib import suppress
import json
import os
from pathlib import Path
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener
from uuid import uuid4

from check_agent_tracer_capabilities import Capabilities
from check_agent_candidate import clean_revision, digest, immutable_checkout, verify_crates, verify_release


ROOT = Path(__file__).resolve().parent.parent


def checked(command, cwd, env=None, expected=0, output=None):
    command = list(map(str, command))
    result = subprocess.run(command, cwd=cwd, env=env, check=False, capture_output=True, text=True)
    if output is not None:
        ledger = output / "commands.json"
        commands = json.loads(ledger.read_text()) if ledger.exists() else []
        log = f"command-{len(commands):02d}.log"
        (output / log).write_text(result.stdout + result.stderr)
        commands.append({"argv": command, "cwd": str(cwd), "exit": result.returncode, "expectedExit": expected, "log": log})
        ledger.write_text(json.dumps(commands, indent=2) + "\n")
    if result.returncode != expected:
        raise RuntimeError(f"Expected exit {expected}, observed {result.returncode}: {command[0]}")
    return result.stdout


def qualify(output, source_checkout=False, host_command=None, require_handler_simulation=False,
            host_provenance=None, external_app=None):
    if not os.environ.get("ROSTFREI_NATS_URL"):
        raise ValueError("Run through scripts/test_nats.py with a disposable NATS broker")
    revision = clean_revision(ROOT)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="rostfrei-skills-live-") as temporary:
        consumer = Path(temporary)
        no_tools = consumer / "no-tools"
        no_tools.mkdir()
        native = consumer / "cargo-rostfrei"
        cli = os.environ.get("ROSTFREI_CLI", str(ROOT / "target/debug/cargo-rostfrei"))
        host_source = external_app or ROOT
        if source_checkout:
            source = immutable_checkout(ROOT, consumer / "checkout")
            host_source = source
            skill = source / ".agents/skills/rostfrei-develop-feature"
            if (skill / "references/release.json").exists():
                raise ValueError("Source regression must start without generated resources")
            target = ROOT / "target/skills-source"
            checked(["cargo", "build", "--locked", "--offline", "--manifest-path", str(source / "crates/rostfrei-structure/Cargo.toml"),
                     "--bin", "cargo-rostfrei", "--target-dir", str(target)], source, output=output)
            cli = target / "debug/cargo-rostfrei"
            shutil.copy2(cli, native)
        elif external_app:
            skill = external_app / ".agents/skills/rostfrei-develop-feature"
            shutil.copy2(cli, native)
            verify_release(json.loads((skill / "references/release.json").read_text()), revision, version)
        else:
            artifacts = consumer / "artifacts"
            checked([sys.executable, str(ROOT / "scripts/package_agent_skills.py"), "--require-clean", "--output", str(artifacts)], ROOT, output=output)
            archive, = artifacts.glob("*.tar.gz")
            with tarfile.open(archive) as bundle:
                bundle.extractall(consumer / "extracted", filter="data")
            kit, = (consumer / "extracted").iterdir()
            destination = consumer / "application/.agents/skills"
            shutil.copy2(cli, native)
            checked([str(native), "rostfrei", "skills", "install", "--kit", str(kit), "--destination", str(destination)],
                    consumer, {**os.environ, "PATH": str(no_tools)}, output=output)
            skill = destination / "rostfrei-develop-feature"
            shutil.rmtree(kit)
            shutil.rmtree(artifacts)
        cli_release = json.loads(checked([native, "rostfrei", "skills", "info"], consumer, output=output))
        verify_release(cli_release, revision, version)
        metadata = json.loads(checked(["cargo", "metadata", "--locked", "--format-version", "1"], host_source, output=output))
        crates = verify_crates(metadata, host_source if not external_app else ROOT, revision, version)
        (output / "host-metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
        (output / "host-rostfrei-crates.json").write_text(json.dumps(crates, indent=2) + "\n")
        shutil.copy2(host_source / "Cargo.lock", output / "host-Cargo.lock")
        if host_command:
            if host_provenance is None:
                raise ValueError("An explicit host command requires --host-build-provenance")
            build_provenance = json.loads(host_provenance.read_text())
            executable = Path(host_command[0]).resolve()
            if (build_provenance["sourceRevision"] != revision or build_provenance["sourceTreeDirty"]
                    or executable != Path(build_provenance["executable"]).resolve()
                    or digest(executable) != build_provenance["executableSha256"]
                    or digest(host_source / "Cargo.lock") != build_provenance["lockSha256"]):
                raise ValueError("Explicit host binary/build provenance differs from candidate")
        else:
            build_provenance = {"sourceRevision": revision, "sourceTreeDirty": False, "source": str(host_source),
                                "lockSha256": digest(host_source / "Cargo.lock"),
                                "attestation": "local standard cargo run from recorded source; HTTP client does not attest Git SHA"}
        with socket.socket() as address:
            address.bind(("127.0.0.1", 0))
            port = address.getsockname()[1]
        application = f"skills-{uuid4().hex}"
        environment = {
            **os.environ,
            "ROSTFREI_APPLICATION": application,
            "ROSTFREI_COMPLETED_OUTCOME_EPOCH": f"skills-{uuid4().hex}",
            "ROSTFREI_API_ADDR": f"127.0.0.1:{port}",
            "ROSTFREI_TRACER_URL": f"http://127.0.0.1:{port}",
            "ROSTFREI_API_TOKEN": secrets.token_hex(24),
            "ROSTFREI_DISPATCH_TOKEN": secrets.token_hex(24),
            "ROSTFREI_INSPECTION_TOKEN": secrets.token_hex(24),
        }
        command = host_command or ["cargo", "run", "--locked", "-p", "bike-rental", "--bin", "bike-rental-api", "--", "--tracer"]
        client_environment = {**environment, "PATH": str(no_tools)}
        with (output / "host.log").open("w") as log:
            host = subprocess.Popen(command, cwd=host_source, env=environment, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                opener = build_opener(ProxyHandler({}))
                deadline = time.monotonic() + 300
                while True:
                    if host.poll() is not None:
                        raise RuntimeError(f"Application host exited; inspect {output / 'host.log'}")
                    try:
                        request = Request(environment["ROSTFREI_TRACER_URL"] + "/catalog",
                                          headers={"Authorization": "Bearer " + environment["ROSTFREI_API_TOKEN"]})
                        with opener.open(request, timeout=1) as response:
                            catalog = json.load(response)
                        break
                    except (URLError, TimeoutError):
                        if time.monotonic() >= deadline:
                            raise RuntimeError("Application startup exceeded 300 seconds")
                        time.sleep(0.2)
                definition = skill / "assets/add-first-bicycle.json"
                original_definition = definition.read_bytes()
                capabilities = Capabilities(environment, output)
                capabilities.discover()
                capabilities.seed_normal(json.loads(original_definition))
                runner = [str(native), "rostfrei", "test"]
                checked([*runner, str(definition), "--output", str(output / "example-passed.json")], consumer, client_environment, output=output)
                passed = json.loads((output / "example-passed.json").read_text())
                schema = ROOT / "crates/rostfrei-tracer/schema/behavioral-test-v2.schema.json" if source_checkout else skill / "assets/behavioral-test-v2.schema.json"
                if passed["schema"] != json.loads(schema.read_text()):
                    raise ValueError("Packaged schema differs from live schema")
                capture = passed["messageSeries"]["capture"]
                if not capture["settled"] or capture["fidelity"] != "exact":
                    raise ValueError("Expected a settled, exact real-NATS capture")
                observed = capabilities.observed_run(passed)
                capabilities.normal_preserved("first-suite")
                wrong = json.loads(definition.read_text())
                wrong["expected"]["graphs"][0]["nodes"][1]["payload"]["bicycle_id"] = "negative-control-wrong-id"
                negative = consumer / "negative-control.json"
                negative.write_text(json.dumps(wrong))
                release_args = [] if source_checkout else ["--release", str(skill / "references/release.json")]
                checked([*runner, str(negative), *release_args, "--output", str(output / "negative-control-failed.json")], consumer, client_environment, expected=1, output=output)
                capabilities.after_reset(observed, "second-suite")
                capabilities.normal_preserved("second-suite")
                second_observed = capabilities.observed_run(json.loads((output / "negative-control-failed.json").read_text()))
                assert definition.read_bytes() == original_definition, "Passing definition changed"
                checked([*runner, str(definition), "--output", str(output / "example-rerun-passed.json")], consumer, client_environment, output=output)
                for filename, verdict in [("negative-control-failed.json", "failed"), ("example-rerun-passed.json", "passed")]:
                    evidence = json.loads((output / filename).read_text())
                    assert evidence["report"]["status"] == verdict
                    assert evidence["captureState"] == "complete"
                    assert evidence["messageSeries"]["capture"]["settled"]
                    assert evidence["messageSeries"]["capture"]["fidelity"] == "exact"
                capabilities.after_reset(second_observed, "third-suite")
                capabilities.normal_preserved("third-suite")
                capabilities.verify_normal_history()
                handler_simulation = capabilities.simulate_handler(json.loads(original_definition), required=require_handler_simulation)
                assert clean_revision(ROOT) == revision, "Candidate changed during qualification"
                if source_checkout:
                    assert clean_revision(host_source) == revision, "Source snapshot changed during qualification"
                if not host_command:
                    executable = Path(metadata["target_directory"]) / "debug/bike-rental-api"
                    build_provenance.update({"executable": str(executable), "executableSha256": digest(executable), "command": command})
                (output / "host-build-provenance.json").write_text(json.dumps(build_provenance, indent=2) + "\n")
                summary = {
                    "release": passed["release"], "catalogVersion": catalog["catalogVersion"],
                    "application": application, "applicationCommand": command,
                    "hostSourceRevision": revision, "hostBuildProvenance": "host-build-provenance.json",
                    "cliRelease": cli_release, "cliSha256": digest(native),
                    "completedOutcomeEpoch": environment["ROSTFREI_COMPLETED_OUTCOME_EPOCH"],
                    "normalPreservation": "observed history retained through three suite resets; new duplicate command rejected",
                    "requiredHandlerSimulation": require_handler_simulation,
                    "sourceCheckout": source_checkout,
                    "fixture": {"id": passed["fixture"]["id"], "revision": passed["fixture"]["revision"]},
                    "example": "passed", "negativeComparisonControl": "failed-as-expected",
                    "exampleAfterReset": "passed", "capture": capture,
                    "freshSessionTrial": "unexecuted", "humanFeedback": "not-collected",
                    "brokerCleanup": "owned by scripts/test_nats.py caller",
                    "consumerPath": "empty tools directory; Python/Cargo/Git unavailable",
                    "extractedKitRemovedBeforeExecution": not source_checkout and not external_app,
                    "externalApplication": str(external_app) if external_app else None,
                    "capabilities": ["scoped catalogs", "Test/Production observation lists and bounded initial SSE snapshots",
                                     "Test observed event detail and explicit causation", "Test observation expiry after reset",
                                      "Production observation generation retained", "scoped quarantine lists"],
                    "simulationAdvertised": any("simulateHrefTemplate" in version
                        for context in catalog["contexts"] for command in context["commands"]
                        for version in command["versions"]),
                    "handlerSimulation": handler_simulation,
                }
                (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
                print(f"{'Source' if source_checkout else 'Installed-kit'} live qualification passed; evidence: {output}")
            finally:
                with suppress(ProcessLookupError):
                    os.killpg(host.pid, signal.SIGINT)
                try:
                    host.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    os.killpg(host.pid, signal.SIGKILL)
                    host.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="new or empty evidence directory")
    parser.add_argument("--source-checkout", action="store_true", help="qualify the CLI and host from the same fresh source snapshot")
    parser.add_argument("--require-handler-simulation", action="store_true", help="selected PR112 gate: absence is failure")
    parser.add_argument("--external-app", type=Path, help="external pinned bike-rental app with explicitly installed skills")
    parser.add_argument("--host-build-provenance", type=Path, help="local build record required for an explicit host binary")
    parser.add_argument("--host-command", nargs=argparse.REMAINDER, help="explicit already-built host command (last option)")
    args = parser.parse_args()
    output = args.output.resolve()
    if output.is_relative_to(ROOT):
        parser.error("Evidence must be outside the immutable source checkout")
    if output.exists() and any(output.iterdir()):
        parser.error("Preserving existing evidence: choose a new or empty output directory")
    try:
        if args.source_checkout and (args.external_app or args.host_command):
            parser.error("Source-checkout qualification builds its own matched standard host")
        qualify(output, args.source_checkout, args.host_command, args.require_handler_simulation,
                args.host_build_provenance, args.external_app.resolve() if args.external_app else None)
    except (OSError, ValueError, RuntimeError, AssertionError) as error:
        parser.exit(1, f"Live qualification failed: {error}\n")


if __name__ == "__main__":
    main()
