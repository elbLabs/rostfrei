#!/usr/bin/env python3
"""Compare preserved release benchmark binaries on one disposable NATS server.

Run through scripts/test_nats.py. Build/copy the before binary before changing
the adapter; both binaries must use the current benchmark harness (including
--transactional), with only the linked event-store implementation differing.
"""

import argparse
import json
import os
from pathlib import Path
import platform
import subprocess
import sys


WORKLOADS = [
    ("single_event_commits", ["--events-per-aggregate", "10,50,100", "--samples", "100", "--rounds", "3", "--warmup", "10"]),
    ("long_histories", ["--events-per-aggregate", "500,1000", "--events-per-commit", "99", "--samples", "20", "--rounds", "2", "--warmup", "5"]),
    ("transactional", ["--transactional", "--events-per-aggregate", "10,50,100", "--samples", "30", "--rounds", "2", "--warmup", "5"]),
]


def compare(before, after):
    if not before["release_build"] or not after["release_build"]:
        raise ValueError("both benchmark binaries must be release builds")
    for field in ("options", "nats_server_version", "runtime_workers", "architecture", "os"):
        if before[field] != after[field]:
            raise ValueError(f"benchmark {field} differs")
    if len(before["cases"]) != len(after["cases"]):
        raise ValueError("benchmark case counts differ")
    comparisons = []
    for old, new in zip(before["cases"], after["cases"]):
        for field in ("events_per_aggregate", "total_events", "source_payload_bytes", "encoded_snapshot_bytes", "encoded_response_bytes"):
            if old[field] != new[field]:
                raise ValueError(f"benchmark {field} differs")
        old_paths = {result["path"]: result for result in old["results"]}
        new_paths = {result["path"]: result for result in new["results"]}
        if old_paths.keys() != new_paths.keys():
            raise ValueError("benchmark paths differ")
        for path, old_result in old_paths.items():
            new_result = new_paths[path]
            if old_result["samples"] != new_result["samples"]:
                raise ValueError("benchmark sample counts differ")
            comparisons.append({
                "total_events": old["total_events"],
                "path": path,
                "p50_speedup": old_result["p50_us"] / new_result["p50_us"],
                "before_p50_us": old_result["p50_us"],
                "after_p50_us": new_result["p50_us"],
                "before_p95_us": old_result["p95_us"],
                "after_p95_us": new_result["p95_us"],
                "before_requests": old_result["published_messages_per_query"],
                "after_requests": new_result["published_messages_per_query"],
            })
    return comparisons


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--baseline-revision", required=True)
    args = parser.parse_args()
    if not os.environ.get("ROSTFREI_NATS_URL"):
        parser.error("run through scripts/test_nats.py with a disposable broker")
    binaries = {"before": args.before.resolve(strict=True), "after": args.after.resolve(strict=True)}
    report = {
        "baseline_revision": args.baseline_revision,
        "platform": platform.platform(),
        "logical_cpus": os.cpu_count(),
        "workloads": [],
    }
    for index, (name, options) in enumerate(WORKLOADS):
        order = ["before", "after"] if index % 2 == 0 else ["after", "before"]
        results = {}
        for version in order:
            print(f"Running {name}: {version}", file=sys.stderr, flush=True)
            completed = subprocess.run([str(binaries[version]), *options],
                                       stdout=subprocess.PIPE, text=True, check=True)
            results[version] = json.loads(completed.stdout)
        comparisons = compare(results["before"], results["after"])
        report["workloads"].append({"name": name, "execution_order": order,
                                    **results, "comparison": comparisons})
        # Preserve complete finished workloads even if a later workload fails.
        args.output.write_text(json.dumps(report, indent=2) + "\n")
        for result in comparisons:
            if result["path"] == "replay_sequential":
                print(f"{name}, {result['total_events']} events: "
                      f"{result['before_p50_us'] / 1000:.3f} -> "
                      f"{result['after_p50_us'] / 1000:.3f} ms "
                      f"({result['p50_speedup']:.2f}x), requests "
                      f"{result['before_requests']:g} -> {result['after_requests']:g}", flush=True)


if __name__ == "__main__":
    main()
