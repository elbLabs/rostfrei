"""Broker-free contracts for the scaling benchmark's evidence and resource guards."""
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import benchmark_snapshots as suite


class ScalingBenchmarkTest(unittest.TestCase):
    def test_atomic_evidence_is_valid_json_and_leaves_no_temporary_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "results.json"
            suite.atomic_json(path, {"cleanup": {"complete": False}})
            suite.atomic_json(path, {"cleanup": {"complete": True}})
            self.assertTrue(json.loads(path.read_text())["cleanup"]["complete"])
            self.assertFalse(path.with_suffix(".tmp").exists())

    def test_disk_measurement_distinguishes_apparent_and_allocated_storage(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "one"
            path.write_bytes(b"test")
            usage = suite.disk_usage(root)
            self.assertEqual(usage["apparent_bytes"], 4)
            self.assertEqual(usage["allocated_bytes"], path.stat().st_blocks * 512)

    def test_failed_measurement_is_retained_without_inventing_a_result(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            args = SimpleNamespace(binary="/fake/snapshot-scaling", samples=3, warmups=1,
                                   memory_gib=16, operation_timeout=600)
            result = {"application": "isolated-benchmark", "operations": []}
            runner = suite.Runner(args, output, {}, result)
            with patch.object(suite.subprocess, "Popen") as popen:
                popen.return_value.wait.return_value = 134
                record = runner.run("read", events=10_000_000, required=False)
                self.assertEqual(record["exit_code"], 134)
                self.assertIsNone(record["result"])
                self.assertEqual(json.loads((output / "results.json").read_text())["operations"][0]["exit_code"], 134)
                self.assertTrue(popen.call_args.kwargs["start_new_session"])
                with patch.object(suite.resource, "setrlimit") as limit:
                    popen.call_args.kwargs["preexec_fn"]()
                    self.assertEqual(limit.call_args_list[-1].args, (suite.resource.RLIMIT_AS, (16 * 1024 ** 3, 16 * 1024 ** 3)))

    def test_required_fixture_failure_stops_instead_of_measuring_wrong_history(self):
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(binary="/fake/snapshot-scaling", samples=1, warmups=0,
                                   memory_gib=16, operation_timeout=600)
            runner = suite.Runner(args, Path(directory), {}, {"application": "isolated", "operations": []})
            with patch.object(suite.subprocess, "Popen") as popen:
                popen.return_value.wait.return_value = 1
                with self.assertRaises(RuntimeError):
                    runner.run("seed", events=100_000)


if __name__ == "__main__":
    unittest.main()
