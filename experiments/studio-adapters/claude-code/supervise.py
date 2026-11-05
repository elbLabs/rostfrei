#!/usr/bin/env python3
"""Linux-only bounded child/subreaper cleanup; output files are owner-private."""

import argparse
import ctypes
import json
import os
from pathlib import Path
import signal
import subprocess
import time


def owned_children():
    # This single-threaded supervisor's kernel-maintained child list includes
    # adopted orphans. Do not scan other tasks or infer ownership from PIDs/PGIDs.
    return [int(pid) for pid in Path(f"/proc/self/task/{os.getpid()}/children").read_text().split()]


def cleanup(child, grace, reap_timeout=2):
    escalated = False
    for number, duration in ((signal.SIGTERM, grace), (signal.SIGKILL, reap_timeout)):
        deadline = time.monotonic() + duration
        escalated = number == signal.SIGKILL
        while True:
            child.poll()  # Popen alone reaps its leader and retains its status.
            for index, pid in enumerate(owned_children()):
                if index and time.monotonic() >= deadline:
                    break
                if pid != child.pid:
                    try:
                        if os.waitpid(pid, os.WNOHANG)[0]:
                            continue
                    except ChildProcessError:
                        continue  # No longer ours: never signal a stale identity.
                # A live direct/adopted child cannot have its PID reused before
                # we reap it. Signal only these children, including setsid ones.
                try:
                    os.kill(pid, number)
                except ProcessLookupError:
                    pass
                if time.monotonic() >= deadline:
                    break
            child.poll()
            remaining = owned_children()
            if not remaining or time.monotonic() >= deadline:
                break
            time.sleep(min(0.01, max(0, deadline - time.monotonic())))
        if not remaining:
            break
        # Killing a parent makes its children ours on the next bounded sweep.
    try:
        os.killpg(child.pid, 0)  # Observation only; never signal a possibly reused PGID.
        group_cleaned = False
    except ProcessLookupError:
        group_cleaned = True
    return {"groupCleaned": group_cleaned, "childrenCleaned": not remaining,
            "remainingChildren": remaining, "escalated": escalated}


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--timeout", type=float, required=True)
    parser.add_argument("--grace", type=float, default=0.25)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if args.timeout <= 0 or args.grace < 0 or not command:
        parser.error("positive timeout, nonnegative grace and a command are required")
    # PR_SET_CHILD_SUBREAPER: orphaned descendants are ours to waitpid(), rather
    # than leaving zombies to a container's potentially non-reaping PID 1.
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(36, 1, 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), "Cannot enable child subreaper")
    interrupted = None

    def interrupt(number, _frame):
        nonlocal interrupted
        interrupted = number

    for number in (signal.SIGINT, signal.SIGTERM):
        signal.signal(number, interrupt)
    started = time.monotonic()
    result = {"timedOut": False, "groupCleaned": False, "childrenCleaned": False}
    child = None

    try:
        with open(str(args.output) + ".stdout.txt", "xb") as stdout, open(
            str(args.output) + ".stderr.txt", "xb"
        ) as stderr:
            child = subprocess.Popen(command, stdin=subprocess.DEVNULL,
                                     stdout=stdout, stderr=stderr, start_new_session=True)
            result["pid"] = child.pid
            deadline = started + args.timeout
            while child.poll() is None and interrupted is None:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    result["timedOut"] = True
                    break
                time.sleep(min(remaining, 0.01))
    except Exception as error:
        result["error"] = str(error)
    finally:
        if child is not None:
            try:
                result.update(cleanup(child, args.grace))
            except Exception as error:
                result["cleanupError"] = str(error)
            result["exit"] = child.returncode
        else:
            result["groupCleaned"] = True
            result["childrenCleaned"] = True
        result.update(interruptedSignal=interrupted, durationMs=round((time.monotonic() - started) * 1000))
        with open(str(args.output) + ".process.json", "x", encoding="utf8") as output:
            json.dump(result, output, indent=2)
            output.write("\n")
    return 0 if (result.get("exit") == 0 and result["groupCleaned"] and result["childrenCleaned"]
                 and not result["timedOut"] and interrupted is None
                 and "error" not in result) else 1


if __name__ == "__main__":
    raise SystemExit(main())
