#!/usr/bin/env python3
"""Qualify the installed example against actual bike-rental handlers and NATS.

Run with scripts/test_nats.py; this script owns a unique application namespace.
The caller owns broker cleanup. It records a passing example and an intentional
negative comparison control, not a human-agent feature-development TDD trial.
"""

import argparse
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import sys
import tarfile
import tempfile
import time
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener
from uuid import uuid4


ROOT = Path(__file__).resolve().parent.parent


def checked(command, cwd, env=None, expected=0):
    result = subprocess.run(command, cwd=cwd, env=env, check=False)
    if result.returncode != expected:
        raise RuntimeError(f"Expected exit {expected}, observed {result.returncode}: {command[0]}")


def qualify(output, source_checkout=False, host_command=None):
    if not os.environ.get("ROSTFREI_NATS_URL"):
        raise ValueError("Run through scripts/test_nats.py with a disposable NATS broker")
    with tempfile.TemporaryDirectory(prefix="rostfrei-skills-live-") as temporary:
        consumer = Path(temporary)
        no_tools = consumer / "no-tools"
        no_tools.mkdir()
        native = consumer / "cargo-rostfrei"
        cli = os.environ.get("ROSTFREI_CLI", str(ROOT / "target/debug/cargo-rostfrei"))
        if source_checkout:
            from test_agent_skill_source import fresh_source_checkout
            source = fresh_source_checkout(consumer / "checkout")
            skill = source / ".agents/skills/rostfrei-develop-feature"
            if (skill / "references/release.json").exists():
                raise ValueError("Source regression must start without generated resources")
            target = ROOT / "target/skills-source"
            checked(["cargo", "build", "--locked", "--offline", "--manifest-path", str(source / "crates/rostfrei-structure/Cargo.toml"),
                     "--bin", "cargo-rostfrei", "--target-dir", str(target)], source)
            cli = target / "debug/cargo-rostfrei"
            shutil.copy2(cli, native)
        else:
            artifacts = consumer / "artifacts"
            checked([sys.executable, str(ROOT / "scripts/package_agent_skills.py"), "--output", str(artifacts)], ROOT)
            archive, = artifacts.glob("*.tar.gz")
            with tarfile.open(archive) as bundle:
                bundle.extractall(consumer / "extracted", filter="data")
            kit, = (consumer / "extracted").iterdir()
            destination = consumer / "application/.agents/skills"
            shutil.copy2(cli, native)
            checked([str(native), "rostfrei", "skills", "install", "--kit", str(kit), "--destination", str(destination)],
                    consumer, {**os.environ, "PATH": str(no_tools)})
            skill = destination / "rostfrei-develop-feature"
            shutil.rmtree(kit)
            shutil.rmtree(artifacts)
        with socket.socket() as address:
            address.bind(("127.0.0.1", 0))
            port = address.getsockname()[1]
        application = f"skills-{uuid4().hex}"
        environment = {
            **os.environ,
            "ROSTFREI_APPLICATION": application,
            "ROSTFREI_API_ADDR": f"127.0.0.1:{port}",
            "ROSTFREI_TRACER_URL": f"http://127.0.0.1:{port}",
            "ROSTFREI_API_TOKEN": secrets.token_hex(24),
            "ROSTFREI_DISPATCH_TOKEN": secrets.token_hex(24),
        }
        command = host_command or ["cargo", "run", "--locked", "-p", "bike-rental", "--bin", "bike-rental-api"]
        client_environment = {**environment, "PATH": str(no_tools)}
        output.mkdir(parents=True, exist_ok=True)
        with (output / "host.log").open("w") as log:
            host = subprocess.Popen(command, cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT)
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
                runner = [str(native), "rostfrei", "test"]
                checked([*runner, str(definition), "--output", str(output / "example-passed.json")], consumer, client_environment)
                passed = json.loads((output / "example-passed.json").read_text())
                schema = ROOT / "crates/rostfrei-tracer/schema/behavioral-test-v2.schema.json" if source_checkout else skill / "assets/behavioral-test-v2.schema.json"
                if passed["schema"] != json.loads(schema.read_text()):
                    raise ValueError("Packaged schema differs from live schema")
                capture = passed["messageSeries"]["capture"]
                if not capture["settled"] or capture["fidelity"] != "exact":
                    raise ValueError("Expected a settled, exact real-NATS capture")
                if source_checkout:
                    print(f"Fresh source Rust CLI and real-NATS execution passed without generated resources; evidence: {output}")
                    return
                wrong = json.loads(definition.read_text())
                wrong["expected"]["graphs"][0]["nodes"][1]["payload"]["bicycle_id"] = "negative-control-wrong-id"
                negative = consumer / "negative-control.json"
                negative.write_text(json.dumps(wrong))
                checked([*runner, str(negative), "--output", str(output / "negative-control-failed.json")], consumer, client_environment, expected=1)
                checked([*runner, str(definition), "--output", str(output / "example-rerun-passed.json")], consumer, client_environment)
                summary = {
                    "release": passed["release"], "catalogVersion": catalog["catalogVersion"],
                    "application": application, "applicationCommand": command,
                    "fixture": {"id": passed["fixture"]["id"], "revision": passed["fixture"]["revision"]},
                    "example": "passed", "negativeComparisonControl": "failed-as-expected",
                    "exampleAfterReset": "passed", "capture": capture,
                    "freshSessionTrial": "unexecuted", "humanFeedback": "not-collected",
                    "brokerCleanup": "owned by scripts/test_nats.py caller",
                    "consumerPath": "empty tools directory; Python/Cargo/Git unavailable",
                    "extractedKitRemovedBeforeExecution": True,
                }
                (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
                print(f"Installed-kit live qualification passed; evidence: {output}")
            finally:
                host.terminate()
                try:
                    host.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    host.kill()
                    host.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="new or empty evidence directory")
    parser.add_argument("--source-checkout", action="store_true", help="qualify fresh source Rust CLI and one real execution")
    parser.add_argument("--host-command", nargs=argparse.REMAINDER, help="explicit already-built host command (last option)")
    args = parser.parse_args()
    output = args.output.resolve()
    if output.exists() and any(output.iterdir()):
        parser.error("Preserving existing evidence: choose a new or empty output directory")
    try:
        qualify(output, args.source_checkout, args.host_command)
    except (OSError, ValueError, RuntimeError) as error:
        parser.exit(1, f"Live qualification failed: {error}\n")


if __name__ == "__main__":
    main()
