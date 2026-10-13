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
    dispatch_token = secrets.token_urlsafe(32)
    inspection_token = secrets.token_urlsafe(32)
    origin = f"http://127.0.0.1:{port}"
    environment = {**os.environ, "ROSTFREI_APPLICATION": f"host-{uuid4().hex}",
                   "ROSTFREI_API_ADDR": f"127.0.0.1:{port}", "ROSTFREI_API_TOKEN": token,
                   "ROSTFREI_DISPATCH_TOKEN": dispatch_token,
                   "ROSTFREI_INSPECTION_TOKEN": inspection_token}
    opener = build_opener(ProxyHandler({}))

    def request(path, method="GET", *, payload=None, capability=token, key=None, location=False):
        if not path.startswith("/") or path.startswith("//"):
            raise RuntimeError("host advertised a non-local link")
        headers = {"Authorization": f"Bearer {capability}"}
        if key:
            headers["Idempotency-Key"] = key
        if payload is not None:
            headers["Content-Type"] = "application/json"
        with opener.open(Request(origin + path, method=method,
                                 data=json.dumps(payload).encode() if payload is not None else None,
                                 headers=headers), timeout=45) as response:
            value = json.load(response)
            return (value, response.headers["Location"]) if location else value

    def dispatch(catalog, name, key, expected):
        command = next(command for context in catalog["contexts"] for command in context["commands"] if command["id"] == name)
        version, = command["versions"]
        _, href = request(version["dispatchHrefTemplate"], "POST", capability=dispatch_token,
                          payload={"schemaVersion": version["schemaVersion"], "payload": {"fleet_id": "city-fleet", "bicycle_id": "bike-42"}}, key=key, location=True)
        deadline = time.monotonic() + 35
        while True:
            operation = request(href, capability=dispatch_token)
            if operation["status"] == "completed":
                assert operation["result"]["decision"] == expected, operation
                return
            if operation["status"] in ("failed", "indeterminate") or time.monotonic() > deadline:
                raise RuntimeError(f"production dispatch did not complete: {operation}")
            time.sleep(0.05)

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
        for capability, scope in [(token, "test"), (inspection_token, "production")]:
            observation, = request("/catalog", capability=capability)["observation"]
            assert observation["scope"] == scope
            assert observation["application"] == environment["ROSTFREI_APPLICATION"]
            assert request(observation["listHref"], capability=capability)["status"] == "live"
        print("Test and Production Observe discovery/readiness passed", flush=True)
        dispatch(catalog, "rent-bicycle", "normal-before-test-reset", "accepted")
        definitions = request(catalog["behavioralTest"]["definitionsHref"])["items"]
        assert len(definitions) == 5
        for definition in definitions:
            report = request(definition["runHref"], "POST")
            assert report["status"] == "passed", report
            assert report["comparison"]["status"] == "passed", report
            print(f"HTTP passed: {definition['id']}", flush=True)
        dispatch(catalog, "rent-bicycle", "normal-after-test-reset", "rejected")
        dispatch(catalog, "return-bicycle", "normal-return", "accepted")
        print("Production dispatch survived all Test resets", flush=True)
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
