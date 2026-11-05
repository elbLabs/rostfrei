"""Owned subreaper regressions, with an independent cleanup owner for failures."""

import ctypes
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import unittest
from unittest.mock import patch, Mock


NODE = sys.argv[1]
HERE = Path(__file__).resolve().parent
# Exact fixtureSource from the independent closure review (including readiness
# synchronization). This is a synthetic process fault, not a Claude observation.
FIXTURE = 'import json, os, signal, time\nread, write = os.pipe()\nchild = os.fork()\nif child == 0:\n    os.close(read)\n    os.setsid()\n    signal.signal(signal.SIGTERM, signal.SIG_IGN)\n    print(json.dumps({"role":"detached-child","pid":os.getpid(),"group":os.getpgrp()}),flush=True)\n    os.write(write,b"ready")\n    os.close(write)\n    while True: time.sleep(1)\nos.close(write)\nos.read(read,5)\nos.close(read)\nprint(json.dumps({"role":"leader","pid":os.getpid(),"group":os.getpgrp()}),flush=True)\n'
spec = importlib.util.spec_from_file_location("supervisor", HERE / "supervise.py")
supervisor = importlib.util.module_from_spec(spec)
sys.dont_write_bytecode = True
spec.loader.exec_module(supervisor)


class SupervisorTests(unittest.TestCase):
    def test_exact_review_fixture(self):
        # This test process is an independent subreaper, so even the old broken
        # helper's detached child is ours to kill AND reap in the finally block.
        self.assertEqual(ctypes.CDLL(None).prctl(36, 1, 0, 0, 0), 0)
        driver = '''
          import { runProcess } from %s;
          const result = await runProcess(['/usr/bin/python3', '-c', %s], {
            cwd: process.cwd(), env: {PATH: '/usr/bin:/bin', HOME: process.cwd()},
            output: process.cwd() + '/detached', timeout: 300, grace: 50,
          });
          console.log(JSON.stringify(result));
        ''' % (json.dumps((HERE / "process.mjs").as_uri()), json.dumps(FIXTURE))
        try:
            run = subprocess.run([NODE, "--input-type=module", "-e", driver],
                                 capture_output=True, text=True, timeout=5, check=True)
            result = json.loads(run.stdout)
            Path("review-repro.json").write_text(json.dumps({"fixtureSource": FIXTURE, "result": result}, indent=2) + "\n")
            self.assertEqual(result["exit"], 0)
            self.assertEqual(result["supervisorExit"], 0)
            self.assertFalse(result["timedOut"])
            self.assertTrue(result["escalated"])
            self.assertTrue(result["childrenCleaned"])
            self.assertEqual(result["remainingChildren"], [])
            self.assertLess(result["durationMs"], 3000)
            rows = [json.loads(line) for line in result["stdout"].splitlines()]
            self.assertEqual({row["role"] for row in rows}, {"leader", "detached-child"})
            self.assertNotEqual(rows[0]["group"], rows[1]["group"])
            for row in rows:
                with self.assertRaises(ProcessLookupError):
                    os.kill(row["pid"], 0)  # Must be reaped, not merely dead/zombie.
        finally:
            # Only the kernel's children of THIS test owner; no global PID scan.
            deadline = time.monotonic() + 2
            own = Path(f"/proc/self/task/{os.getpid()}/children")
            while own.read_text().split() and time.monotonic() < deadline:
                for pid in map(int, own.read_text().split()):
                    if not os.waitpid(pid, os.WNOHANG)[0]:
                        os.kill(pid, signal.SIGKILL)
                time.sleep(0.01)
            self.assertEqual(own.read_text().split(), [], "Independent test cleanup incomplete")

    def test_unreapable_owned_child_fails_even_with_clean_group_and_zero_exit(self):
        # Inject failed reaping; never create an unkillable real process or signal
        # this illustrative PID. The child's waitpid ownership remains intact.
        child = Mock(pid=123, returncode=0)
        with patch.object(supervisor, "owned_children", return_value=[456]), \
             patch.object(supervisor.os, "waitpid", return_value=(0, 0)), \
             patch.object(supervisor.os, "kill") as kill, \
             patch.object(supervisor.os, "killpg", side_effect=ProcessLookupError):
            cleanup = supervisor.cleanup(child, 0, reap_timeout=0)
        self.assertTrue(cleanup["groupCleaned"])
        self.assertFalse(cleanup["childrenCleaned"])
        self.assertEqual(cleanup["remainingChildren"], [456])
        self.assertEqual([call.args for call in kill.call_args_list],
                         [(456, signal.SIGTERM), (456, signal.SIGKILL)])
        handlers = {number: signal.getsignal(number) for number in (signal.SIGINT, signal.SIGTERM)}
        try:
            with patch.object(supervisor, "cleanup", return_value=cleanup), \
                 patch.object(sys, "argv", ["supervise.py", "--timeout", "1", "--output", "incomplete", "--", "/usr/bin/true"]):
                self.assertEqual(supervisor.main(), 1)
            result = json.loads(Path("incomplete.process.json").read_text())
            self.assertEqual(result["exit"], 0)
            self.assertFalse(result["childrenCleaned"])
        finally:
            for number, handler in handlers.items():
                signal.signal(number, handler)


if __name__ == "__main__":
    os.umask(0o077)
    unittest.main(argv=[sys.argv[0]], verbosity=2)
