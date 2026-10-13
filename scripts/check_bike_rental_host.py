#!/usr/bin/env python3
"""Exercise the standard bike-rental startup and its suite, optionally through Studio."""
import argparse
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import time
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener
from uuid import uuid4

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--studio", action="store_true")
    args = parser.parse_args()
    if not os.environ.get("ROSTFREI_NATS_URL"):
        raise RuntimeError("Use scripts/test_nats.py to supply a disposable broker")
    subprocess.run(["cargo", "build", "--locked", "-p", "bike-rental", "--bin", "bike-rental-api"], cwd=ROOT, check=True)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    token = secrets.token_urlsafe(32)
    origin = f"http://127.0.0.1:{port}"
    environment = {**os.environ, "ROSTFREI_APPLICATION": f"host-{uuid4().hex}",
                   "ROSTFREI_API_ADDR": f"127.0.0.1:{port}", "ROSTFREI_API_TOKEN": token,
                   "ROSTFREI_DISPATCH_TOKEN": secrets.token_urlsafe(32)}
    opener = build_opener(ProxyHandler({}))

    def request(path, method="GET"):
        if not path.startswith("/") or path.startswith("//"):
            raise RuntimeError("host advertised a non-local link")
        with opener.open(Request(origin + path, method=method,
                                 headers={"Authorization": f"Bearer {token}"}), timeout=45) as response:
            return json.load(response)

    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    process = subprocess.Popen([str(target / "debug/bike-rental-api"), "--tracer"], cwd=ROOT, env=environment)
    try:
        deadline = time.monotonic() + 45
        while True:
            if process.poll() is not None:
                raise RuntimeError(f"host startup failed: exit {process.returncode}")
            try:
                catalog = request("/catalog")
                break
            except (URLError, OSError):
                if time.monotonic() > deadline:
                    raise RuntimeError("host did not become ready")
                time.sleep(0.1)
        assert catalog["catalogVersion"] == 1
        definitions = request(catalog["behavioralTest"]["definitionsHref"])["items"]
        assert len(definitions) == 5
        for definition in definitions:
            report = request(definition["runHref"], "POST")
            assert report["status"] == "passed", report
            assert report["comparison"]["status"] == "passed", report
            print(f"HTTP passed: {definition['id']}", flush=True)
        if args.studio:
            subprocess.run(["node", str(ROOT / "scripts/check_tracer_studio.mjs")], cwd=ROOT / "studio",
                           env={**environment, "VITE_TRACER_TARGET": origin, "VITE_TRACER_TOKEN": token}, check=True)
    finally:
        process.send_signal(signal.SIGINT)
        try:
            process.wait(timeout=50)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            raise RuntimeError("host did not finish supervised shutdown")
    if process.returncode:
        raise RuntimeError(f"host shutdown failed: exit {process.returncode}")
    print("Application-linked host shutdown passed", flush=True)


if __name__ == "__main__":
    main()
