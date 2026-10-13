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


def qualify(output):
    if not os.environ.get("ROSTFREI_NATS_URL"):
        raise ValueError("Run through scripts/test_nats.py with a disposable NATS broker")
    with tempfile.TemporaryDirectory(prefix="rostfrei-skills-live-") as temporary:
        consumer = Path(temporary)
        artifacts = consumer / "artifacts"
        checked([sys.executable, str(ROOT / "scripts/package_agent_skills.py"), "--output", str(artifacts)], ROOT)
        archive, = artifacts.glob("*.tar.gz")
        with tarfile.open(archive) as bundle:
            bundle.extractall(consumer / "extracted", filter="data")
        kit, = (consumer / "extracted").iterdir()
        destination = consumer / "application/.agents/skills"
        checked([sys.executable, "-I", str(kit / "install.py"), "--destination", str(destination)], consumer)
        skill = destination / "rostfrei-develop-feature"
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
        command = ["cargo", "run", "--locked", "-p", "bike-rental", "--bin", "bike-rental-api"]
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
                runner = [sys.executable, "-I", str(skill / "scripts/run_behavior.py")]
                checked([*runner, str(definition), "--output", str(output / "example-passed.json")], consumer, environment)
                passed = json.loads((output / "example-passed.json").read_text())
                if passed["schema"] != json.loads((skill / "assets/behavioral-test-v2.schema.json").read_text()):
                    raise ValueError("Packaged schema differs from live schema")
                capture = passed["messageSeries"]["capture"]
                if not capture["settled"] or capture["fidelity"] != "exact":
                    raise ValueError("Expected a settled, exact real-NATS capture")
                wrong = json.loads(definition.read_text())
                wrong["expected"]["graphs"][0]["nodes"][1]["payload"]["bicycle_id"] = "negative-control-wrong-id"
                negative = consumer / "negative-control.json"
                negative.write_text(json.dumps(wrong))
                checked([*runner, str(negative), "--output", str(output / "negative-control-failed.json")], consumer, environment, expected=1)
                checked([*runner, str(definition), "--output", str(output / "example-rerun-passed.json")], consumer, environment)
                summary = {
                    "release": passed["release"], "catalogVersion": catalog["catalogVersion"],
                    "application": application, "applicationCommand": command,
                    "fixture": {"id": passed["fixture"]["id"], "revision": passed["fixture"]["revision"]},
                    "example": "passed", "negativeComparisonControl": "failed-as-expected",
                    "exampleAfterReset": "passed", "capture": capture,
                    "freshSessionTrial": "unexecuted", "humanFeedback": "not-collected",
                    "brokerCleanup": "owned by scripts/test_nats.py caller",
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
    args = parser.parse_args()
    output = args.output.resolve()
    if output.exists() and any(output.iterdir()):
        parser.error("Preserving existing evidence: choose a new or empty output directory")
    try:
        qualify(output)
    except (OSError, ValueError, RuntimeError) as error:
        parser.exit(1, f"Live qualification failed: {error}\n")


if __name__ == "__main__":
    main()
