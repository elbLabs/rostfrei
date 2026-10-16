"""Authenticated workspace fixture wiring; real NATS controls run through the CLI."""

from contextlib import redirect_stderr, redirect_stdout
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
from urllib.parse import urlsplit

import test_nats


class ContractFixtureTests(unittest.TestCase):
    def check_inherited_output(self, exit_code):
        with tempfile.TemporaryDirectory(prefix="rostfrei-output-regression-") as directory:
            marker = Path(directory) / "processes.json"
            secret = "fragmented-password-\N{LOCK}"
            child = (
                "import json, os, pathlib, subprocess, sys, time; "
                "descendant = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(300)']); "
                f"pathlib.Path({str(marker)!r}).write_text(json.dumps([os.getpid(), descendant.pid])); "
                f"data = {('start ' + secret + chr(10) + 'tail ' + secret).encode()!r}; "
                "[(os.write(1, bytes([byte])), time.sleep(0.001)) for byte in data]; "
                f"sys.exit({exit_code})"
            )
            runner = (
                "import os, sys; "
                f"sys.path.insert(0, {str(Path(test_nats.__file__).parent)!r}); "
                "import test_nats; "
                f"sys.exit(test_nats.run_command([sys.executable, '-c', {child!r}], "
                f"dict(os.environ), redactions=({secret!r},)))"
            )
            process = subprocess.Popen([sys.executable, "-c", runner],
                                       stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            try:
                output, _ = process.communicate(timeout=5)
                self.assertEqual(process.returncode, exit_code, output)
                self.assertEqual(output, "start [redacted]\ntail [redacted]")
                pids = json.loads(marker.read_text())

                def running(pid):
                    try:
                        return Path(f"/proc/{pid}/stat").read_text().split()[2] != "Z"
                    except FileNotFoundError:
                        return False

                deadline = time.monotonic() + 1
                while any(running(pid) for pid in pids) and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertFalse(any(running(pid) for pid in pids), pids)
            finally:
                if process.poll() is None:
                    process.send_signal(signal.SIGINT)
                    try:
                        process.communicate(timeout=15)
                    except subprocess.TimeoutExpired:
                        if marker.exists():
                            try:
                                os.killpg(json.loads(marker.read_text())[0], signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                        process.kill()
                        process.communicate(timeout=5)
                if marker.exists():
                    try:
                        os.killpg(json.loads(marker.read_text())[0], signal.SIGKILL)
                    except ProcessLookupError:
                        pass

    def test_failed_child_with_inherited_output_returns_and_cleans_descendant(self):
        self.check_inherited_output(7)

    def test_successful_child_with_inherited_output_returns_and_cleans_descendant(self):
        self.check_inherited_output(0)

    def check_fixture(self, command, **options):
        calls = []
        captured = {}
        output = io.StringIO()

        def docker(*arguments, **_kwargs):
            calls.append(arguments)
            if arguments[0] == "run":
                mount = arguments[arguments.index("--mount") + 1]
                captured["config"] = Path(mount.split("src=", 1)[1].split(",dst=", 1)[0])
            if arguments[0] == "inspect":
                return json.dumps({
                    f"{port}/tcp": [{"HostIp": "127.0.0.1", "HostPort": str(port + 10000)}]
                    for port in (4222, 8222)
                })
            if arguments[0] == "logs":
                return "broker diagnostic " + captured["password"]
            return ""

        def child(_command, environment, *, redactions=()):
            url = urlsplit(environment["ROSTFREI_NATS_URL"])
            user = environment["ROSTFREI_CONTRACT_NATS_USER"]
            password = environment["ROSTFREI_CONTRACT_NATS_PASSWORD"]
            captured["password"] = password
            self.assertEqual((url.username, url.password, url.hostname, url.port),
                             (user, password, "127.0.0.1", 14222))
            self.assertNotEqual(password, "inherited-password")
            config = captured["config"]
            self.assertIn(f'authorization {{ user: "{user}", password: "{password}" }}', config.read_text())
            self.assertEqual(config.stat().st_mode & 0o777, 0o600)
            captured["directory"] = Path(environment["ROSTFREI_CONTRACT_EVIDENCE_DIR"])
            self.assertTrue(captured["directory"].is_dir())
            self.assertNotEqual(str(captured["directory"]), "/do-not-use")
            self.assertEqual(environment["ROSTFREI_CONTRACT_NATS_BINARY"], str(Path(sys.executable).resolve()))
            self.assertIn(password, redactions)
            return 7

        with (
            patch.dict(os.environ, {
                "ROSTFREI_NATS_SERVER": sys.executable,
                "ROSTFREI_NATS_URL": "nats://external-do-not-use:4222",
                "ROSTFREI_CONTRACT_NATS_PASSWORD": "inherited-password",
                "ROSTFREI_CONTRACT_EVIDENCE_DIR": "/do-not-use",
            }),
            patch("test_nats.docker", side_effect=docker),
            patch("test_nats.wait_ready"),
            patch("test_nats.run_command", side_effect=child),
            redirect_stdout(output), redirect_stderr(output),
        ):
            self.assertEqual(test_nats.run(command, **options), 7)
            self.assertEqual(os.environ["ROSTFREI_CONTRACT_NATS_PASSWORD"], "inherited-password")
        self.assertFalse(captured["config"].exists())
        self.assertFalse(captured["directory"].exists())
        self.assertNotIn(captured["password"], output.getvalue())
        self.assertIn("[redacted]", output.getvalue())
        started = next(call for call in calls if call[0] == "run")
        name = started[started.index("--name") + 1]
        self.assertEqual(calls[-1], ("rm", "--force", "--volumes", name))

    def test_contract_environment_matches_private_config_and_cleans_on_failure(self):
        self.check_fixture(["cargo", "test"], contracts=True)

    def test_default_workspace_gets_the_authenticated_contract_fixture(self):
        self.check_fixture(test_nats.DEFAULT_COMMAND)

    def test_missing_native_binary_fails_before_starting_a_broker(self):
        with (
            patch.dict(os.environ, {"ROSTFREI_NATS_SERVER": "/missing/native-nats"}),
            patch("test_nats.docker") as docker,
        ):
            with self.assertRaisesRegex(RuntimeError, "native NATS"):
                test_nats.run(["cargo", "test"], contracts=True)
        docker.assert_not_called()

    def test_child_stdout_and_stderr_are_redacted_without_hiding_failure(self):
        output = io.StringIO()
        environment = {**os.environ, "FIXTURE_PASSWORD": "ephemeral-test-password"}
        with redirect_stdout(output):
            code = test_nats.run_command([
                sys.executable, "-c",
                "import os, sys; print(os.environ['FIXTURE_PASSWORD']); "
                "print(os.environ['FIXTURE_PASSWORD'], file=sys.stderr); sys.exit(7)",
            ], environment, redactions=(environment["FIXTURE_PASSWORD"],))
        self.assertEqual(code, 7)
        self.assertNotIn(environment["FIXTURE_PASSWORD"], output.getvalue())
        self.assertEqual(output.getvalue().count("[redacted]"), 2)


if __name__ == "__main__":
    unittest.main()
