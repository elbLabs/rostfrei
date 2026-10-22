#!/usr/bin/env python3
"""Measure one real NATS aggregate at 100k/1m/10m with periodic snapshots.

Owned native broker, disk-backed temporary data, bounded child memory and explicit
cleanup. Only JSON/log evidence survives. Requires the release snapshot-scaling
binary and a cached NATS 2.12.15 executable. No external broker is ever used.
"""
import argparse
import json
import os
from pathlib import Path
import re
import resource
import shutil
import signal
import socket
import subprocess
import tempfile
import time
import platform
from urllib.request import ProxyHandler, build_opener
from uuid import uuid4

ROOT = Path(__file__).resolve().parent.parent
INTERVALS = {"a": 50_000, "b": 500_000, "c": 2_000_000}


def unused_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def atomic_json(path, value):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.replace(path)


def disk_usage(path):
    files = [p for p in path.rglob("*") if p.is_file()]
    return {"apparent_bytes": sum(p.stat().st_size for p in files),
            "allocated_bytes": sum(p.stat().st_blocks * 512 for p in files)}


class Runner:
    def __init__(self, args, output, environment, result):
        self.args, self.output, self.environment, self.result = args, output, environment, result
        self.counter = 0

    def run(self, action, *, scenario="baseline", start=0, events=0, required=True):
        self.counter += 1
        stem = f"{self.counter:04}-{action}-{scenario}-{events}"
        log, stats = self.output / f"{stem}.log", self.output / f"{stem}.time"
        command = ["/usr/bin/time", "-v", "-o", str(stats), str(self.args.binary),
                   "--action", action, "--application", self.result["application"],
                   "--scenario", scenario, "--start", str(start), "--events", str(events),
                   "--samples", str(self.args.samples), "--warmups", str(self.args.warmups)]

        def limits():
            resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
            ceiling = self.args.memory_gib * 1024 ** 3
            resource.setrlimit(resource.RLIMIT_AS, (ceiling, ceiling))

        started = time.monotonic()
        with log.open("w") as output:
            child = subprocess.Popen(command, cwd=ROOT, env=self.environment, stdout=output,
                                     stderr=subprocess.STDOUT, preexec_fn=limits, start_new_session=True)
            try:
                code = child.wait(timeout=self.args.operation_timeout)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()
                code = 124
            except BaseException:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()
                raise
        text = log.read_text()
        records = []
        for line in text.splitlines():
            try:
                records.append(json.loads(line))
            except ValueError:
                pass
        peak = re.search(r"Maximum resident set size \(kbytes\): (\d+)", stats.read_text()) if stats.exists() else None
        record = {"action": action, "scenario": scenario, "events": events, "start": start,
                  "exit_code": code, "wall_seconds": time.monotonic() - started,
                  "peak_rss_bytes": int(peak[1]) * 1024 if peak else None,
                  "log": log.name, "result": records[-1] if records else None}
        if code:
            record["error"] = text[-4000:]
        self.result["operations"].append(record)
        atomic_json(self.output / "results.json", self.result)
        if action != "seed" or code or events % 500_000 == 0:
            print(f"{action} {scenario} {events:,}: exit={code}, {record['wall_seconds']:.2f}s, peak RSS={record['peak_rss_bytes']}", flush=True)
        if code and required:
            raise RuntimeError(f"{action} failed: {log}")
        return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", type=Path, default=Path(os.environ.get("ROSTFREI_NATS_SERVER", "/tmp/opencode/nats-server")))
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/snapshot-scaling")
    parser.add_argument("--events", type=int, nargs="+", default=[100_000, 1_000_000, 10_000_000])
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--warmups", type=int, default=1)
    parser.add_argument("--memory-gib", type=int, default=16)
    parser.add_argument("--operation-timeout", type=int, default=600)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not args.events or any(n <= 0 or n % 50_000 for n in args.events) or sorted(set(args.events)) != args.events:
        parser.error("--events must be increasing positive multiples of 50000")
    if args.samples < 1 or args.warmups < 0 or args.memory_gib < 1:
        parser.error("invalid sample/warmup/memory budget")
    args.server, args.binary = args.server.resolve(), args.binary.resolve()
    output = (args.output or ROOT / "target/benchmarks" / f"snapshot-scaling-{time.strftime('%Y%m%d-%H%M%S')}").resolve()
    output.mkdir(parents=True, exist_ok=False)
    data_parent = ROOT / "target/benchmarks"
    data_parent.mkdir(parents=True, exist_ok=True)
    if shutil.disk_usage(data_parent).free < 64 * 1024 ** 3:
        raise RuntimeError("need at least 64 GiB of free disk for the owned benchmark")
    result = {"schema_version": 1, "application": f"snapshot-scale-{uuid4().hex[:12]}",
              "settings": {**vars(args), "server": str(args.server), "binary": str(args.binary), "output": str(output)},
              "intervals": INTERVALS, "history_kind": "direct atomic 100-event batches; import-only setup",
              "events_retained": True, "operations": [], "milestones": [], "cleanup": {"complete": False}}
    result["environment"] = {"platform": platform.platform(), "cpus": os.cpu_count(),
                             "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
                             "git_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                             "git_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip()),
                             "disk_free_before_bytes": shutil.disk_usage(data_parent).free}
    server = None
    data_path = None
    runner = None
    opener = build_opener(ProxyHandler({}))

    def cancelled(signum, _frame):
        raise SystemExit(128 + signum)

    previous = {s: signal.signal(s, cancelled) for s in (signal.SIGTERM, signal.SIGINT)}
    print(f"Evidence directory: {output}", flush=True)
    try:
        with tempfile.TemporaryDirectory(prefix="snapshot-scale-data-", dir=data_parent) as directory:
            data_path = Path(directory)
            port, monitor = unused_port(), unused_port()
            config = data_path / "nats.conf"
            config.write_text(f'host: "127.0.0.1"\nport: {port}\nhttp: "127.0.0.1:{monitor}"\nmax_payload: 4194304\njetstream {{\nstore_dir: "{data_path / "store"}"\nmax_file_store: 274877906944\nmax_memory_store: 268435456\n}}\n')
            environment = {**os.environ, "ROSTFREI_NATS_URL": f"nats://127.0.0.1:{port}"}
            runner = Runner(args, output, environment, result)
            with (output / "broker.log").open("w") as broker_log:
                server = subprocess.Popen([str(args.server), "--config", str(config)], stdout=broker_log, stderr=subprocess.STDOUT)
                try:
                    for attempt in range(150):
                        try:
                            with opener.open(f"http://127.0.0.1:{monitor}/healthz?js-enabled-only=true", timeout=1) as response:
                                if response.status == 200:
                                    break
                        except OSError:
                            if server.poll() is not None:
                                raise RuntimeError("benchmark broker exited")
                            time.sleep(0.2)
                    else:
                        raise RuntimeError("benchmark broker readiness timed out")
                    runner.run("init")
                    runner.run("seed", events=200)
                    runner.run("audit", events=200)
                    runner.run("read", events=200)
                    runner.run("capture", scenario="a", events=100)
                    runner.run("read", scenario="a", events=200)
                    runner.run("cleanup")
                    result["preflight_complete"] = True
                    runner.run("init")
                    head, captures = 0, {s: 0 for s in INTERVALS}
                    disabled = set()
                    for boundary in range(50_000, max(args.events) + 1, 50_000):
                        runner.run("seed", start=head, events=boundary)
                        head = boundary
                        for scenario, interval in INTERVALS.items():
                            if boundary % interval == 0 and scenario not in disabled:
                                operation = runner.run("capture", scenario=scenario, events=boundary, required=False)
                                if operation["exit_code"]:
                                    disabled.add(scenario)
                                else:
                                    captures[scenario] += 1
                        if boundary not in args.events:
                            continue
                        # Storage/read comparison at the identical immutable event head.
                        storage = runner.run("storage", events=boundary)
                        for stream in storage["result"]["streams"]:
                            directories = [p for p in (data_path / "store").rglob(stream["stream"]) if p.is_dir()]
                            stream["disk"] = disk_usage(directories[0]) if len(directories) == 1 else None
                        milestone = {"events": boundary, "captures_completed": captures.copy(),
                                     "disabled_capture_scenarios": sorted(disabled),
                                     "storage": storage["result"], "broker_disk": disk_usage(data_path / "store")}
                        result["milestones"].append(milestone)
                        for scenario in ["baseline", "a", "b", "c"]:
                            runner.run("read", scenario=scenario, events=boundary, required=False)
                        # Writes start at a recorded, slightly growing head; no rollback.
                        for scenario in ["baseline", "a", "b", "c"]:
                            operation = runner.run("command", scenario=scenario, events=head, required=False)
                            if operation["result"]:
                                head = operation["result"]["head_after"]
                            elif operation["exit_code"]:
                                # An aborted command could have persisted. Read its native
                                # committed head instead of guessing or rerunning the command.
                                head = runner.run("head")["result"]["head_after"]
                        atomic_json(output / "results.json", result)
                    result["measurements_complete"] = True
                finally:
                    result["cleanup"]["disk_before_removal"] = disk_usage(data_path / "store") if (data_path / "store").exists() else None
                    try:
                        if server.poll() is None:
                            cleanup = runner.run("cleanup", required=False)
                            result["cleanup"]["streams_removed"] = cleanup["exit_code"] == 0
                    finally:
                        if server.poll() is None:
                            server.terminate()
                            try:
                                server.wait(timeout=15)
                            except subprocess.TimeoutExpired:
                                server.kill()
                                server.wait()
                        result["cleanup"]["broker_stopped"] = server.poll() is not None
        result["cleanup"]["data_directory_removed"] = not data_path.exists()
        result["cleanup"]["complete"] = result["cleanup"].get("broker_stopped", False) and not data_path.exists()
    finally:
        if data_path is not None:
            result["cleanup"]["data_directory_removed"] = not data_path.exists()
            result["cleanup"]["complete"] = result["cleanup"].get("broker_stopped", False) and not data_path.exists()
        result["cleanup"]["disk_free_after_bytes"] = shutil.disk_usage(data_parent).free
        atomic_json(output / "results.json", result)
        for signum, handler in previous.items():
            signal.signal(signum, handler)
        print(f"Cleanup: {result['cleanup']}", flush=True)


if __name__ == "__main__":
    main()
