#!/usr/bin/env python3
"""Run tests against an isolated NATS container (Python 3.11+ and Docker)."""

import argparse
from contextlib import suppress
import json
import math
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
from urllib.error import URLError
from urllib.request import ProxyHandler, build_opener
from uuid import uuid4


ROOT = Path(__file__).resolve().parent.parent
IMAGE = "nats:2.12.1-alpine@sha256:b3f2bd84176ae7bd0afa9c48a00f06d7d0818ff4aaee898e4172e0b8340e5816"
DEFAULT_COMMAND = [
    "cargo", "test", "--locked", "--workspace", "--all-features",
    "--", "--test-threads=1",
]
LIMITS = {
    "ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES": "67108864",
    "ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES": "268435456",
    "ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES": "524288",
}


def docker(*arguments, timeout=30):
    result = subprocess.run(
        ["docker", *arguments], capture_output=True, text=True, timeout=timeout,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or "Docker command failed")
    # NATS writes its logs to stderr, which docker logs preserves on the client.
    output = result.stdout + result.stderr if arguments[0] == "logs" else result.stdout
    return output.strip()


def mapped_port(ports, container_port):
    mappings = ports.get(f"{container_port}/tcp") or []
    if len(mappings) != 1 or mappings[0]["HostIp"] != "127.0.0.1":
        raise RuntimeError("NATS test ports must be published only on loopback")
    return int(mappings[0]["HostPort"])


def wait_ready(port, timeout):
    deadline = time.monotonic() + timeout
    opener = build_opener(ProxyHandler({}))
    while time.monotonic() < deadline:
        try:
            with opener.open(
                f"http://127.0.0.1:{port}/healthz?js-enabled-only=true", timeout=1,
            ) as response:
                if response.status == 200 and json.load(response).get("status") == "ok":
                    return
        except (URLError, OSError, ValueError):
            pass
        time.sleep(0.2)
    raise RuntimeError(f"NATS JetStream did not become ready within {timeout:g}s")


def run_command(command, environment):
    # A separate process group lets cancellation clean up Cargo's test children too.
    # Defer cancellation during Popen so a signal cannot orphan a just-spawned
    # child before we have its PID. The child inherits normal, unblocked signals.
    pending = []
    previous = {
        signum: signal.signal(signum, lambda signum, _frame: pending.append(signum))
        for signum in (signal.SIGINT, signal.SIGTERM)
    }
    process = None
    try:
        try:
            process = subprocess.Popen(command, cwd=ROOT, env=environment, start_new_session=True)
        finally:
            for signum, handler in previous.items():
                signal.signal(signum, handler)
        if pending:
            raise SystemExit(128 + pending[0])
        code = process.wait()
        return code if code >= 0 else 128 - code
    except BaseException:
        if process is not None:
            with suppress(ProcessLookupError):
                os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                pass
            finally:
                with suppress(ProcessLookupError):
                    os.killpg(process.pid, signal.SIGKILL)
                process.wait()
        raise


def run(command, ready_timeout=30):
    docker("info", timeout=20)
    name = f"rostfrei-test-nats-{uuid4().hex}"
    created = False
    succeeded = False
    try:
        docker(
            "run", "--detach", "--rm", "--name", name,
            "--publish", "127.0.0.1::4222", "--publish", "127.0.0.1::8222",
            "--mount", f"type=bind,src={ROOT / 'scripts/nats-test.conf'},dst=/etc/nats/test.conf,readonly",
            IMAGE, "--config", "/etc/nats/test.conf", timeout=240,
        )
        created = True
        ports = json.loads(docker("inspect", "--format", "{{json .NetworkSettings.Ports}}", name))
        nats_port = mapped_port(ports, 4222)
        wait_ready(mapped_port(ports, 8222), ready_timeout)
        environment = {**os.environ, **LIMITS, "ROSTFREI_NATS_URL": f"nats://127.0.0.1:{nats_port}"}
        environment.setdefault("RUST_BACKTRACE", "1")
        print(f"NATS 2.12.1 ready on isolated port {nats_port}; running tests", flush=True)
        result = run_command(command, environment)
        succeeded = result == 0
        return result
    finally:
        if created and not succeeded:
            try:
                print(docker("logs", name), file=sys.stderr)
            except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                print(f"Could not retrieve NATS logs: {error}", file=sys.stderr)
        # Also try cleanup if docker run timed out after creating the container.
        try:
            docker("rm", "--force", "--volumes", name)
        except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
            if created:
                raise RuntimeError(f"Could not remove test container {name}: {error}") from error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ready-timeout", type=float, default=30)
    parser.add_argument("command", nargs=argparse.REMAINDER, help="optional command after --")
    args = parser.parse_args()
    if not math.isfinite(args.ready_timeout) or args.ready_timeout <= 0:
        parser.error("--ready-timeout must be positive and finite")
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]

    def terminate(signum, _frame):
        raise SystemExit(128 + signum)

    previous = signal.signal(signal.SIGTERM, terminate)
    try:
        return run(command or DEFAULT_COMMAND, args.ready_timeout)
    except KeyboardInterrupt:
        return 130
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"NATS test setup failed: {error}", file=sys.stderr)
        return 1
    finally:
        signal.signal(signal.SIGTERM, previous)


if __name__ == "__main__":
    sys.exit(main())
