#!/usr/bin/env python3
"""Run a selected Cargo command against an owned, authenticated native NATS child.

No installation, Docker, live broker, or cache deletion. Retains logs/data even on
failure. Use the repository-pinned Rust toolchain on PATH and cached NATS 2.12.15.
"""
import argparse
from contextlib import suppress
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import time
import tomllib
from urllib.request import ProxyHandler, build_opener
from uuid import uuid4

ROOT = Path(__file__).resolve().parent.parent


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nats-binary", type=Path, required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command:
        parser.error("supply the exact command after --")
    run = args.evidence_dir.resolve() / ("run-" + uuid4().hex)
    run.mkdir(parents=True)
    channel = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    rustup_home = Path(os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup")))
    toolchain = rustup_home / "toolchains" / f"{channel}-x86_64-unknown-linux-gnu" / "bin"
    if not (toolchain / "rustc").is_file():
        raise RuntimeError(f"pinned cached toolchain missing: {toolchain}; will not install")
    nats_port, monitor_port = port(), port()
    user, password = "contract-tests", uuid4().hex
    config = run / "nats.conf"
    config.write_text(f'''host: "127.0.0.1"
port: {nats_port}
http: "127.0.0.1:{monitor_port}"
max_payload: 2113536
authorization {{ user: "{user}", password: "{password}" }}
jetstream {{ store_dir: "{run / 'jetstream'}" }}
''')
    environment = {**os.environ,
                   "PATH": str(toolchain) + os.pathsep + os.environ.get("PATH", ""),
                   "RUSTUP_TOOLCHAIN": channel,
                   "RUSTC": str(toolchain / "rustc"),
                   "RUSTDOC": str(toolchain / "rustdoc"),
                   "CARGO_BUILD_JOBS": "2",
                   "CARGO_TARGET_DIR": str(ROOT / "target" / f"release-contract-{channel}"),
                   "ROSTFREI_NATS_URL": f"nats://{user}:{password}@127.0.0.1:{nats_port}",
                   "ROSTFREI_CONTRACT_NATS_USER": user,
                   "ROSTFREI_CONTRACT_NATS_PASSWORD": password,
                   "ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES": "67108864",
                   "ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES": "268435456",
                   "ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES": "524288",
                   "RUST_BACKTRACE": "1"}
    manifest = {"root": str(ROOT), "command": command,
                "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                "environment": {key: value for key, value in environment.items() if key.startswith("ROSTFREI_") or key in ("RUSTUP_TOOLCHAIN", "RUSTC", "RUSTDOC", "CARGO_BUILD_JOBS", "CARGO_TARGET_DIR", "RUST_BACKTRACE")},
                "nats_version": subprocess.check_output([str(args.nats_binary), "--version"], text=True).strip(),
                "rust_version": subprocess.check_output([str(toolchain / "rustc"), "--version"], text=True).strip(),
                "broker_config": str(config)}
    broker = None
    cargo = None
    previous = {}
    def terminate(signum, _frame):
        raise SystemExit(128 + signum)
    try:
        for signum in (signal.SIGTERM, signal.SIGINT):
            previous[signum] = signal.signal(signum, terminate)
        with (run / "broker.log").open("w") as log:
            broker = subprocess.Popen([str(args.nats_binary), "--config", str(config)], stdout=log, stderr=subprocess.STDOUT)
        manifest["owned_broker_pid"] = broker.pid
        print(f"Evidence: {run}; owned broker PID={broker.pid}", flush=True)
        opener = build_opener(ProxyHandler({}))
        deadline = time.monotonic() + 15
        while True:
            if broker.poll() is not None:
                raise RuntimeError("owned NATS exited during setup")
            try:
                with opener.open(f"http://127.0.0.1:{monitor_port}/healthz?js-enabled-only=true", timeout=1) as response:
                    if json.load(response).get("status") == "ok":
                        break
            except OSError:
                pass
            if time.monotonic() >= deadline:
                raise RuntimeError("owned NATS readiness failed")
            time.sleep(0.05)
        manifest["broker_ready"] = True
        with (run / "cargo.log").open("w") as log:
            cargo = subprocess.Popen(command, cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            manifest["cargo_pid"] = cargo.pid
            manifest["cargo_exit_code"] = cargo.wait(timeout=1200)
        for route, filename in [("varz", "broker-varz.json"), ("jsz?streams=true&consumers=true&config=true", "broker-jsz.json")]:
            with opener.open(f"http://127.0.0.1:{monitor_port}/{route}", timeout=3) as response:
                (run / filename).write_bytes(response.read())
        print((run / "cargo.log").read_text(), end="")
        return manifest["cargo_exit_code"]
    except BaseException as error:
        manifest["runner_error"] = repr(error)
        raise
    finally:
        if cargo is not None and cargo.poll() is None:
            with suppress(ProcessLookupError):
                os.killpg(cargo.pid, signal.SIGTERM)
            try:
                cargo.wait(timeout=10)
            except subprocess.TimeoutExpired:
                with suppress(ProcessLookupError):
                    os.killpg(cargo.pid, signal.SIGKILL)
                cargo.wait()
            manifest["cargo_cleanup_exit_code"] = cargo.returncode
        if broker is not None:
            if broker.poll() is None:
                broker.terminate()
                try:
                    broker.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    broker.kill()
                    broker.wait()
            manifest["owned_broker_exit_code"] = broker.returncode
        for signum, handler in previous.items():
            signal.signal(signum, handler)
        (run / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    sys.exit(main())
