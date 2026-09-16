"""Runner lifecycle regressions; these tests do not require Docker."""

import json
import os
import signal
import subprocess
import sys
import unittest
from unittest.mock import Mock, patch

import test_nats


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.calls = []
        self.ports = {
            "4222/tcp": [{"HostIp": "127.0.0.1", "HostPort": "14222"}],
            "8222/tcp": [{"HostIp": "127.0.0.1", "HostPort": "18222"}],
        }

    def docker(self, *arguments, **_kwargs):
        self.calls.append(arguments)
        return json.dumps(self.ports) if arguments[0] == "inspect" else ""

    def assert_cleaned(self):
        started = next(call for call in self.calls if call[0] == "run")
        name = started[started.index("--name") + 1]
        self.assertEqual(self.calls[-1], ("rm", "--force", "--volumes", name))

    def test_success_overrides_inherited_url_and_cleans_only_its_container(self):
        with (
            patch.dict(os.environ, {"ROSTFREI_NATS_URL": "nats://do-not-use:4222"}),
            patch("test_nats.docker", side_effect=self.docker),
            patch("test_nats.wait_ready") as ready,
            patch("test_nats.run_command", return_value=0) as command,
        ):
            self.assertEqual(test_nats.run(["cargo", "test"]), 0)
        ready.assert_called_once_with(18222, 30)
        self.assertEqual(command.call_args.args[1]["ROSTFREI_NATS_URL"], "nats://127.0.0.1:14222")
        self.assert_cleaned()

    def test_test_failure_preserves_exit_status_and_collects_logs(self):
        with (
            patch("test_nats.docker", side_effect=self.docker),
            patch("test_nats.wait_ready"),
            patch("test_nats.run_command", return_value=7),
        ):
            self.assertEqual(test_nats.run(["cargo", "test"]), 7)
        self.assertTrue(any(call[0] == "logs" for call in self.calls))
        self.assert_cleaned()

    def test_readiness_failure_does_not_run_tests_and_still_cleans(self):
        with (
            patch("test_nats.docker", side_effect=self.docker),
            patch("test_nats.wait_ready", side_effect=RuntimeError("not ready")),
            patch("test_nats.run_command") as command,
        ):
            with self.assertRaisesRegex(RuntimeError, "not ready"):
                test_nats.run(["cargo", "test"])
        command.assert_not_called()
        self.assert_cleaned()

    def test_cancellation_cleans_the_broker(self):
        with (
            patch("test_nats.docker", side_effect=self.docker),
            patch("test_nats.wait_ready"),
            patch("test_nats.run_command", side_effect=KeyboardInterrupt),
        ):
            with self.assertRaises(KeyboardInterrupt):
                test_nats.run(["cargo", "test"])
        self.assert_cleaned()

    def test_non_loopback_port_binding_is_rejected(self):
        self.ports["4222/tcp"][0]["HostIp"] = "0.0.0.0"
        with self.assertRaisesRegex(RuntimeError, "loopback"):
            test_nats.mapped_port(self.ports, 4222)

    def test_readiness_wait_is_bounded(self):
        with self.assertRaisesRegex(RuntimeError, "did not become ready"):
            test_nats.wait_ready(18222, 0)

    def test_child_exit_code_is_preserved(self):
        self.assertEqual(
            test_nats.run_command([sys.executable, "-c", "raise SystemExit(7)"], dict(os.environ)),
            7,
        )

    def test_broker_stderr_is_included_in_failure_logs(self):
        result = subprocess.CompletedProcess([], 0, stdout="", stderr="NATS startup log\n")
        with patch("test_nats.subprocess.run", return_value=result):
            self.assertEqual(test_nats.docker("logs", "owned-container"), "NATS startup log")

    def test_cancellation_during_spawn_cannot_orphan_the_child(self):
        handlers = {}
        process = Mock(pid=12345)

        def install(signum, handler):
            previous = handlers.get(signum, signal.SIG_DFL)
            handlers[signum] = handler
            return previous

        def spawn(*_args, **_kwargs):
            handlers[signal.SIGTERM](signal.SIGTERM, None)
            return process

        with (
            patch("test_nats.signal.signal", side_effect=install),
            patch("test_nats.subprocess.Popen", side_effect=spawn),
            patch("test_nats.os.killpg") as kill,
        ):
            with self.assertRaises(SystemExit) as exit_status:
                test_nats.run_command(["cargo", "test"], {})
        self.assertEqual(exit_status.exception.code, 143)
        kill.assert_any_call(12345, signal.SIGTERM)
        self.assertEqual(handlers[signal.SIGTERM], signal.SIG_DFL)

    def test_cleanup_failure_cannot_report_success(self):
        def cleanup_fails(*arguments, **kwargs):
            if arguments[0] == "rm":
                raise RuntimeError("cleanup failed")
            return self.docker(*arguments, **kwargs)

        with (
            patch("test_nats.docker", side_effect=cleanup_fails),
            patch("test_nats.wait_ready"),
            patch("test_nats.run_command", return_value=0),
        ):
            with self.assertRaisesRegex(RuntimeError, "Could not remove test container"):
                test_nats.run(["cargo", "test"])

    def test_default_run_prefetches_the_offline_fixture_lockfile(self):
        with (
            patch("sys.argv", ["test_nats.py"]),
            patch("test_nats.run_command", return_value=0) as fetch,
            patch("test_nats.run", return_value=0) as run,
        ):
            self.assertEqual(test_nats.main(), 0)
        self.assertEqual(fetch.call_args.args[0], [
            "cargo", "fetch", "--locked", "--manifest-path", str(test_nats.FIXTURE_MANIFEST),
        ])
        run.assert_called_once_with(test_nats.DEFAULT_COMMAND, 30)

    def test_failed_dependency_preparation_does_not_start_a_broker(self):
        with (
            patch("sys.argv", ["test_nats.py"]),
            patch("test_nats.run_command", return_value=7),
            patch("test_nats.run") as run,
        ):
            self.assertEqual(test_nats.main(), 7)
        run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
