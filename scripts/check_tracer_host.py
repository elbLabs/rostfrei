#!/usr/bin/env python3
"""Qualify the host from a standalone consumer outside the framework workspace."""
import argparse
import os
from pathlib import Path
import shutil
import secrets
import signal
import socket
import subprocess
import tempfile
import time
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener
from uuid import uuid4
import json

ROOT = Path(__file__).resolve().parent.parent


def check_entrypoint(consumer, environment):
    subprocess.run(["cargo", "build", "--locked", "--bin", "tracer-host-consumer"], cwd=consumer, env=environment, check=True)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    token = secrets.token_urlsafe(32)
    process = subprocess.Popen([str(Path(environment["CARGO_TARGET_DIR"]) / "debug/tracer-host-consumer")],
                               cwd=consumer, env={**environment, "ROSTFREI_APPLICATION": f"entry-{uuid4().hex}",
                                                "ROSTFREI_API_ADDR": f"127.0.0.1:{port}", "ROSTFREI_API_TOKEN": token})
    opener = build_opener(ProxyHandler({}))
    def request(path, method="GET"):
        assert path.startswith("/") and not path.startswith("//")
        with opener.open(Request(f"http://127.0.0.1:{port}{path}", method=method,
                                 headers={"Authorization": f"Bearer {token}"}), timeout=15) as response:
            return json.load(response)
    try:
        deadline = time.monotonic() + 30
        while True:
            if process.poll() is not None:
                raise RuntimeError("standalone entry point exited during startup")
            try:
                catalog = request("/catalog")
                break
            except (URLError, OSError):
                if time.monotonic() > deadline:
                    raise RuntimeError("standalone entry point did not become ready")
                time.sleep(0.1)
        definition = request(catalog["behavioralTest"]["definitionsHref"])["items"][0]
        assert request(definition["runHref"], "POST")["status"] == "passed"
    finally:
        process.send_signal(signal.SIGINT)
        try:
            process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            raise RuntimeError("standalone entry point did not stop")
    assert process.returncode == 0
    print("Standalone application entry point: discovery, suite and shutdown passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--working-tree", action="store_true", help="use source paths for pre-commit red/green development")
    args = parser.parse_args()
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    environment = dict(os.environ)
    environment["ROSTFREI_COMPLETED_OUTCOME_EPOCH"] = f"host-{uuid4().hex}"
    environment["CARGO_TARGET_DIR"] = str(ROOT / "target/host-consumer")
    with tempfile.TemporaryDirectory(prefix="rostfrei-host-", dir=os.environ.get("TMPDIR")) as directory:
        consumer = Path(directory) / "consumer"
        shutil.copytree(ROOT / "crates/rostfrei-tracer/tests/consumer", consumer, ignore=shutil.ignore_patterns("target"))
        manifest = consumer / "Cargo.toml"
        text = manifest.read_text()
        for relative, package in [("../../../rostfrei", "rostfrei"), ("../../../rostfrei-nats", "rostfrei-nats"), ("../..", "rostfrei-tracer")]:
            dependency = (f'path = "{ROOT / "crates" / package}"' if args.working_tree
                          else f'git = "{ROOT.as_uri()}", rev = "{revision}"')
            text = text.replace(f'path = "{relative}"', dependency)
        manifest.write_text(text)
        subprocess.run(["cargo", "test", "--", "--nocapture"], cwd=consumer, env=environment, check=True)
        check_entrypoint(consumer, environment)
        if not args.working_tree:
            metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=consumer, env=environment, text=True))
            packages = [p for p in metadata["packages"] if p["name"].startswith("rostfrei")]
            for package in packages:
                if not package["source"] or not package["source"].endswith(f"#{revision}"):
                    raise RuntimeError(f"{package['name']} did not resolve from the committed framework")
            print(f"External host qualified against Git-installed framework {revision}")


if __name__ == "__main__":
    main()
