#!/usr/bin/env python3
"""Run tests against an owned, isolated NATS container (Python 3.11+ and Docker).

The default workspace command and --contracts use real authentication plus private
restart storage. They require a cached native nats-server on PATH or supplied via
ROSTFREI_NATS_SERVER; this runner never downloads it. Other commands keep the
ordinary host fixture. An inherited ROSTFREI_NATS_URL is always overridden, never
managed as an external broker.
"""

import argparse
from contextlib import suppress
import json
import math
import os
from pathlib import Path
import secrets
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from urllib.error import URLError
from urllib.request import ProxyHandler, build_opener
from uuid import uuid4


ROOT = Path(__file__).resolve().parent.parent
# 2.12.5+ fixes pinned-consumer handoff of existing backlog after UNPIN.
IMAGE = "nats:2.12.15-alpine@sha256:b270f5e2428354c0335612694d7dd2fb588148e567a5757fdff325ef9c9332e6"
DEFAULT_COMMAND = [
    "cargo", "test", "--locked", "--workspace", "--all-features",
    "--", "--test-threads=1",
]
FIXTURE_MANIFEST = ROOT / "crates/rostfrei-macros/tests/dependency-matrix/Cargo.toml"
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


def redact(text, redactions):
    for value in redactions:
        text = text.replace(value, "[redacted]")
    return text


def run_command(command, environment, *, redactions=()):
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
            output = {"stdout": subprocess.PIPE, "stderr": subprocess.STDOUT, "text": True} if redactions else {}
            process = subprocess.Popen(command, cwd=ROOT, env=environment, start_new_session=True, **output)
        finally:
            for signum, handler in previous.items():
                signal.signal(signum, handler)
        if pending:
            raise SystemExit(128 + pending[0])
        if redactions:
            for line in process.stdout:
                print(redact(line, redactions), end="", flush=True)
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
    finally:
        if process is not None:
            # Also contain descendants left behind by a failing test process.
            with suppress(ProcessLookupError):
                os.killpg(process.pid, signal.SIGKILL)
            if redactions and process.stdout is not None:
                process.stdout.close()


def run(command, ready_timeout=30, *, contracts=False):
    environment = {**os.environ, **LIMITS}
    environment.setdefault("RUST_BACKTRACE", "1")
    config = ROOT / "scripts/nats-test.conf"
    if not (contracts or command == DEFAULT_COMMAND):
        return run_container(command, ready_timeout, config, environment)

    binary = shutil.which(os.environ.get("ROSTFREI_NATS_SERVER", "nats-server"))
    if binary is None:
        raise RuntimeError("contract tests require native NATS: set ROSTFREI_NATS_SERVER to a cached nats-server binary")
    binary = str(Path(binary).resolve())
    user, password = "contract-tests", secrets.token_hex(24)
    with tempfile.TemporaryDirectory(prefix="rostfrei-contract-") as directory:
        private_config = Path(directory) / "nats.conf"
        with private_config.open("x") as output:
            os.chmod(private_config, 0o600)
            output.write(config.read_text())
            output.write(f'\nauthorization {{ user: "{user}", password: "{password}" }}\n')
        environment.update({
            "ROSTFREI_CONTRACT_NATS_USER": user,
            "ROSTFREI_CONTRACT_NATS_PASSWORD": password,
            "ROSTFREI_CONTRACT_NATS_BINARY": binary,
            "ROSTFREI_NATS_SERVER": binary,
            "ROSTFREI_CONTRACT_EVIDENCE_DIR": directory,
        })
        return run_container(command, ready_timeout, private_config, environment, (user, password))


def run_container(command, ready_timeout, config, environment, credentials=None):
    redactions = (credentials[1],) if credentials else ()
    docker("info", timeout=20)
    name = f"rostfrei-test-nats-{uuid4().hex}"
    created = False
    succeeded = False
    try:
        docker(
            "run", "--detach", "--rm", "--name", name,
            "--publish", "127.0.0.1::4222", "--publish", "127.0.0.1::8222",
            "--mount", f"type=bind,src={config},dst=/etc/nats/test.conf,readonly",
            IMAGE, "--config", "/etc/nats/test.conf", timeout=240,
        )
        created = True
        ports = json.loads(docker("inspect", "--format", "{{json .NetworkSettings.Ports}}", name))
        nats_port = mapped_port(ports, 4222)
        wait_ready(mapped_port(ports, 8222), ready_timeout)
        userinfo = f"{credentials[0]}:{credentials[1]}@" if credentials else ""
        environment["ROSTFREI_NATS_URL"] = f"nats://{userinfo}127.0.0.1:{nats_port}"
        print(f"{IMAGE.split('@', 1)[0]} ready on isolated port {nats_port}; running tests", flush=True)
        result = run_command(command, environment, redactions=redactions) if redactions else run_command(command, environment)
        succeeded = result == 0
        return result
    finally:
        if created and not succeeded:
            try:
                print(redact(docker("logs", name), redactions), file=sys.stderr)
            except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                print(redact(f"Could not retrieve NATS logs: {error}", redactions), file=sys.stderr)
        # Also try cleanup if docker run timed out after creating the container.
        try:
            docker("rm", "--force", "--volumes", name)
        except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
            if created:
                raise RuntimeError(f"Could not remove test container {name}: {error}") from error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ready-timeout", type=float, default=30)
    parser.add_argument("--contracts", action="store_true", help="authenticated fault/restart fixture (automatic for the default workspace command)")
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
        if not command:
            # The macro dependency-matrix test deliberately builds offline with
            # its own lockfile, which can pin different crates than the workspace.
            result = run_command(
                ["cargo", "fetch", "--locked", "--manifest-path", str(FIXTURE_MANIFEST)],
                dict(os.environ),
            )
            if result:
                return result
        if args.contracts:
            return run(command or DEFAULT_COMMAND, args.ready_timeout, contracts=True)
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
