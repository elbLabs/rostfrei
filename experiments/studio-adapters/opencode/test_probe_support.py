import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
import contextlib
import io
import sys
from unittest.mock import patch

from probe_support import Outputs, stop_group
import probe
from terminal_screen import screen
from package_install import stage_core


class OutputBoundaryTests(unittest.TestCase):
    def setUp(self):
        scratch = probe.REPO / "artifacts/tmp"
        scratch.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="opencode-boundary-", dir=scratch)
        self.root = Path(self.temp.name)
        self.artifacts = self.root / "research"
        self.artifacts.mkdir()
        self.evidence = self.root / "capture.json"
        self.evidence.write_text('{"immutable":true}')
        self.before = self.evidence.read_bytes()

    def tearDown(self):
        self.assertEqual(self.evidence.read_bytes(), self.before)
        self.temp.cleanup()

    def test_report_sidecars_and_aliases_cannot_overwrite_inputs(self):
        for suffix in (".json", ".tui.ansi", ".tui.txt", ".tui.frames.json", ".integrity.json"):
            directory = self.artifacts / ("existing" + suffix.replace(".", "-"))
            directory.mkdir()
            alias = directory / ("report" + suffix)
            os.link(self.evidence, alias)
            with self.assertRaises((ValueError, FileExistsError)):
                Outputs(directory / "report.json", self.artifacts, [self.evidence])
        with self.assertRaises(ValueError):
            Outputs(self.evidence, self.artifacts, [self.evidence])
        (self.artifacts / "alias").symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(ValueError):
            Outputs(self.artifacts / "alias/new/report.json", self.artifacts, [self.evidence])
        (self.artifacts / "dangling").symlink_to(self.root / "absent")
        with self.assertRaises(ValueError):
            Outputs(self.artifacts / "dangling/report.json", self.artifacts, [self.evidence])

    def test_reserved_fd_refuses_replaced_symlink_and_hardlink(self):
        for kind in ("symlink", "hardlink"):
            output = Outputs(self.artifacts / kind / "report.json", self.artifacts, [self.evidence])
            output.paths["report"].unlink()
            if kind == "symlink":
                output.paths["report"].symlink_to(self.evidence)
            else:
                os.link(self.evidence, output.paths["report"])
            with self.assertRaises(ValueError):
                output.write("report", "overwrite")
            output.close()

    def test_integrity_read_happens_after_all_writes(self):
        output = Outputs(None, self.artifacts, [self.evidence])
        source = {"path": self.evidence, "sha256": hashlib.sha256(self.before).hexdigest()}
        seen = []
        from probe_support import digest
        def observed(path):
            seen.append(set(output.written))
            return digest(path)
        with patch("probe_support.digest", side_effect=observed):
            output.finish({"passed": False, "error": "primary failure"}, [source])
        self.assertEqual(seen[-1], set(output.paths))
        self.assertEqual(json.loads(output.paths["report"].read_text())["error"], "primary failure")
        output.close()

    def test_already_exited_group_cleanup_is_idempotent(self):
        class Exited:
            pid = 1234567
            def wait(self, **kwargs): return 1
            def poll(self): return 1
        with patch("probe_support.os.killpg", side_effect=ProcessLookupError):
            self.assertEqual(stop_group(Exited()), {"stopped": True, "errors": []})

    def test_cleanup_signals_descendants_even_after_leader_is_reaped(self):
        class Exited:
            pid = 1234567
            def wait(self, **kwargs): return 1
            def poll(self): return 1
        with patch("probe_support.os.killpg") as kill:
            stop_group(Exited())
        import signal
        self.assertEqual([call.args for call in kill.call_args_list], [(Exited.pid, signal.SIGTERM), (Exited.pid, signal.SIGKILL)])

    def test_actual_main_refuses_alias_before_any_process(self):
        with patch.object(probe, "RESEARCH", self.artifacts), patch.object(sys, "argv", ["probe.py", "--evidence", str(self.evidence), "--output", str(self.evidence)]), patch.object(probe.subprocess, "check_output") as version, patch.object(probe.subprocess, "Popen") as start:
            with self.assertRaises(ValueError): probe.main()
            version.assert_not_called()
            start.assert_not_called()

    def test_actual_main_preserves_early_exit_and_startup_log(self):
        (self.root / "packages/tracer-presentation").mkdir(parents=True)
        binary = self.root / "inert-binary"
        binary.write_bytes(b"not executed")
        report = self.artifacts / "new-owned-run/report.json"
        class Exited:
            pid = 1234567
            def wait(self, **kwargs): return 1
            def poll(self): return 1
        def start(argv, **kwargs):
            kwargs["stdout"].write("startup sentinel: controlled early exit\n")
            kwargs["stdout"].flush()
            return Exited()
        def check(argv, **kwargs):
            return "inert version\n" if argv[-1] == "--version" else "home " + kwargs["env"]["HOME"] + "\n"
        with patch.object(probe, "REPO", self.root), patch.object(probe, "RESEARCH", self.artifacts), \
             patch.object(probe.shutil, "which", return_value=str(binary)), \
             patch.object(probe.subprocess, "check_output", side_effect=check), \
             patch.object(probe.subprocess, "Popen", side_effect=start), \
             patch("probe_support.os.killpg", side_effect=ProcessLookupError), \
             patch.object(sys, "argv", ["probe.py", "--core-double", "--output", str(report)]), \
             contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as error: probe.main()
            self.assertEqual(error.exception.code, 1)
        result = json.loads(report.read_text())
        self.assertEqual(result["error"], "Owned server exited before readiness")
        self.assertIn("startup sentinel", result["startupLog"])
        self.assertTrue(result["ownedProcessStopped"])
        self.assertTrue(result["ownedDirectoryRemoved"])

    def test_cleanup_failure_does_not_replace_primary_error(self):
        result = {"passed": False, "error": "primary RPC failure"}
        class Log:
            def close(self): raise OSError("close failed")
        with patch.object(probe, "stop_group", return_value={"stopped": True, "errors": ["kill failed"]}):
            probe.cleanup_server(None, Log(), self.root, None, result)
        self.assertEqual(result["error"], "primary RPC failure")
        self.assertTrue(any("close failed" in message for message in result["cleanupErrors"]))

    def test_viewport_check_discards_erased_and_scrolled_text(self):
        raw = b"\x1b[2J\x1b[1;1Hhidden payload\x1b[1;1H\x1b[2KSelected message\x1b[3;1Hparent matched"
        current = "\n".join(screen(raw, 4, 30))
        self.assertNotIn("hidden payload", current)
        self.assertIn("Selected message", current)
        self.assertIn("parent matched", current)

    def test_package_staging_resolves_pnpm_style_transitive_dependencies(self):
        source = self.root / "core"
        (source / "dist").mkdir(parents=True)
        (source / "dist/index.js").write_text("export const fixture = true")
        (source / "README.md").write_text("Synthetic package-staging fixture")
        (source / "package.json").write_text(json.dumps({"dependencies": {"direct": "1"}}))
        (source / "node_modules").mkdir()
        installed = self.root / "store/direct/node_modules/direct"
        installed.mkdir(parents=True)
        (installed / "package.json").write_text(json.dumps({"dependencies": {"transitive": "1"}}))
        transitive = installed.parent / "transitive"
        transitive.mkdir()
        (transitive / "package.json").write_text("{}")
        (transitive / "index.js").write_text("original transitive content")
        (source / "node_modules/direct").symlink_to(installed, target_is_directory=True)
        output = self.root / "installation/node_modules/core"
        stage_core(source, output)
        self.assertEqual((output / "node_modules/direct/node_modules/transitive/index.js").read_text(), "original transitive content")
        self.assertFalse((output / "node_modules/direct").is_symlink())


if __name__ == "__main__":
    unittest.main()
