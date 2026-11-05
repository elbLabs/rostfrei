#!/usr/bin/env python3
"""Probe the installed OpenCode host in a disposable, credential-free HOME."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
from tui_probe import probe_tui
from probe_support import Outputs, digest, stop_group
from package_install import stage_core


HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
RESEARCH = REPO / "artifacts/studio-adapters/research"
RPC = "/api/rpc/rostfrei.presentation.spike/"


def cleanup_server(child, log, root, password, result):
    errors = result.setdefault("cleanupErrors", [])
    def attempt(label, callback):
        try:
            return callback()
        except Exception as error:
            errors.append(f"{label}: {error}")
    stopped = attempt("stop owned group", lambda: stop_group(child))
    if stopped:
        result["ownedProcessStopped"] = stopped["stopped"]
        errors.extend(stopped["errors"])
    attempt("close startup log", log.close)
    startup = attempt("read startup log", lambda: (root / "server.log").read_text())
    if startup is not None:
        result["startupLog"] = re.sub(r"server password \S+", "server password [redacted]", startup)[-4000:]
    def mcp_state():
        path = root / "mcp-initialize.pid"
        if path.exists():
            result["ownedMcpProcessStopped"] = not Path(f"/proc/{int(path.read_text())}").exists()
            if not result["ownedMcpProcessStopped"]: errors.append("Owned MCP process did not stop")
    attempt("MCP cleanup status", mcp_state)
    def collect_logs():
        for path in (root / "data").glob("**/opencode.log"):
            text = path.read_text()
            if password: text = text.replace(password.group(1), "[redacted]")
            result.setdefault("diagnosticLogs", []).extend(line for line in text.replace(str(root), "<owned-temp>").splitlines()
                if any(marker in line for marker in ('loading plugin', 'failed to load', 'level=ERROR', 'level=WARN')))
    attempt("read diagnostics", collect_logs)
    if errors or not result.get("ownedProcessStopped", False):
        result["passed"] = False
        result.setdefault("error", "Owned-resource cleanup failed; see cleanupErrors")


def run_probe(args, outputs, sources):
    binary = shutil.which("opencode")
    if not binary:
        raise SystemExit("opencode must be on PATH")
    preferred = Path("/tmp/opencode")
    parent = preferred if os.access(preferred, os.W_OK) else REPO / "artifacts/tmp"
    parent.mkdir(parents=True, exist_ok=True)
    result = {"temporaryDirectoryFallback": parent != preferred, "selectionScope": "single-owned-plugin-instance; not session-safe"}
    deadline = time.monotonic() + 300
    result["hostDeadlineSeconds"] = 300
    result["adapterSourceSha256"] = {path.name: digest(path) for path in sorted(HERE.iterdir()) if path.suffix in (".js", ".py", ".mjs", ".json")}
    with tempfile.TemporaryDirectory(prefix="rostfrei-opencode-", dir=parent, ignore_cleanup_errors=True) as temporary:
        root = Path(temporary)
        for name in ("home", "config", "data", "state", "cache", "project", "tmp"):
            (root / name).mkdir()
        # No inherited OpenCode session/server settings or provider credentials.
        env = {
            "PATH": os.environ["PATH"],
            "HOME": str(root / "home"),
            "XDG_CONFIG_HOME": str(root / "config"),
            "XDG_DATA_HOME": str(root / "data"),
            "XDG_STATE_HOME": str(root / "state"),
            "XDG_CACHE_HOME": str(root / "cache"),
            "TMPDIR": str(root / "tmp"),
            "OPENCODE_DB": str(root / "data/probe.db"),
        }
        # Project discovery walks ancestors; refuse unexpected external config.
        for ancestor in (root / "project").parents:
            for name in ("opencode.json", "opencode.jsonc", ".opencode/opencode.json", ".opencode/opencode.jsonc",
                         ".opencode/plugins", ".opencode/agents", ".opencode/commands", ".opencode/skills"):
                if (ancestor / name).exists():
                    raise RuntimeError(f"External configuration in {ancestor}; choose a clean temp parent")
        if Path("/etc/opencode").exists():
            raise RuntimeError("System OpenCode config exists; isolation requires explicit review")
        global_config = root / "config/opencode"
        global_config.mkdir()
        (global_config / "opencode.json").write_text(json.dumps({
            "$schema": "https://opencode.ai/config.json",
            "update": "disable",
            "policies": [{"action": "provider.use", "resource": "*", "effect": "deny"}],
        }))
        (global_config / "cli.json").write_text(json.dumps({
            "$schema": "https://opencode.ai/v2/cli.json", "highlights": False,
            "animations": False, "attention": {"sound": False, "notifications": False},
        }))
        # Exercise package exports from a real installed copy outside the workspace.
        plugin = root / "plugin"
        shutil.copytree(HERE, plugin, ignore=shutil.ignore_patterns("__pycache__", "node_modules", "host-evidence.json"))
        presentation = REPO / "packages/tracer-presentation"
        if not presentation.exists():
            raise RuntimeError("packages/tracer-presentation is required")
        dependency = plugin / "node_modules/@rostfrei/tracer-presentation"
        if args.core_double:
            dependency.mkdir(parents=True)
            (dependency / "package.json").write_text(json.dumps({"type": "module", "exports": "./index.js"}))
            shutil.copyfile(HERE / "host-core-double.js", dependency / "index.js")
            result["presentationLayer"] = "explicit synthetic core double; actual OpenCode host/TUI"
        else:
            stage_core(presentation, dependency)
            result["presentationLayer"] = "installed shared core JS"
            core_files = [presentation / "package.json", presentation / "tsconfig.build.json", *sorted((presentation / "src").rglob("*")), *sorted((presentation / "dist").rglob("*"))]
            result["presentationSourceSha256"] = {str(path.relative_to(presentation)): digest(path) for path in core_files if path.is_file()}
            result["presentationInstalledSha256"] = {str(path.relative_to(dependency)): digest(path) for path in sorted(dependency.rglob("*")) if path.is_file()}
            for name, expected in result["presentationSourceSha256"].items():
                if name.startswith("dist/") or name == "package.json":
                    assert result["presentationInstalledSha256"][name] == expected
            result["presentationPackage"] = {"exports": json.loads((dependency / "package.json").read_text())["exports"],
                "sourceDirectoryInstalled": (dependency / "src").exists(),
                "ajvVersion": json.loads((dependency / "node_modules/ajv/package.json").read_text())["version"],
                "checkoutRevision": subprocess.check_output(["git", "log", "-1", "--format=%H", "--", "packages/tracer-presentation"], cwd=REPO, env=env, text=True, timeout=10).strip(),
                "lockSha256": digest(REPO / "pnpm-lock.yaml")}
        staged = []
        for index, source in enumerate(sources):
            saved = root / f"capture-{index}.json"
            saved.write_bytes(source["bytes"])
            saved.chmod(0o400)
            staged.append({"path": str(saved), "label": source["path"].stem})
        (root / "project/opencode.json").write_text(json.dumps({
            "$schema": "https://opencode.ai/config.json",
            "plugins": [{"package": str(plugin), "options": {"captures": staged}}],
            "mcp": {"servers": {"synthetic": {
                "type": "local",
                "command": [sys.executable, str(plugin / "mcp_fixture.py"), str(root / "mcp-initialize.json")],
            }}},
        }))
        result["cliVersion"] = subprocess.check_output([binary, "--version"], text=True, env=env, timeout=10).strip()
        result["binarySha256"] = hashlib.sha256(Path(binary).read_bytes()).hexdigest()
        paths = subprocess.check_output([binary, "debug", "paths"], text=True, env=env, cwd=root / "project", timeout=10)
        assert all(str(root) in line for line in paths.splitlines()), paths
        result["isolatedPaths"] = paths.replace(str(root), "<owned-temp>").splitlines()
        log = (root / "server.log").open("w")
        child = None
        password = None
        try:
            child = subprocess.Popen(
                [binary, "serve", "--hostname", "127.0.0.1", "--port", "0"],
                cwd=root / "project", env=env, stdout=log, stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            for _ in range(300):
                text = (root / "server.log").read_text()
                url = re.search(r"http://127\.0\.0\.1:\d+", text)
                password = re.search(r"server password (\S+)", text)
                if url and password:
                    break
                if child.poll() is not None:
                    raise RuntimeError("Owned server exited before readiness")
                time.sleep(0.1)
            else:
                raise RuntimeError("Owned server startup timed out")
            base = url.group(0)
            env["OPENCODE_PASSWORD"] = password.group(1)

            def api(method, path, body=None, expected_error=False, raw_output=False, timeout=60):
                timeout = min(timeout, deadline - time.monotonic())
                if timeout <= 0: raise TimeoutError("Owned host qualification deadline exceeded")
                command = [binary, "api", "--server", base, method, path]
                if body is not None:
                    command.extend(["--data", json.dumps(body)])
                # Direct file output avoids this build's large piped-stdout truncation.
                with (root / "response.json").open("w") as output:
                    completed = subprocess.run(command, cwd=root / "project", env=env,
                                               stdout=output, stderr=subprocess.PIPE, text=True, timeout=timeout)
                raw = (root / "response.json").read_text()
                if expected_error:
                    assert completed.returncode != 0, "Expected host to reject invalid evidence selection"
                    return {"response": json.loads(raw), "status": completed.stderr.strip()}
                if completed.returncode:
                    raise RuntimeError((completed.stderr + raw).replace(password.group(1), "[redacted]"))
                return raw if raw_output else json.loads(raw)

            result["serverInfo"] = api("get", "/api/info")
            spec = api("get", "/openapi.json")
            result["rpcRoute"] = spec["paths"]["/api/rpc/{rpcID}/{method}"]
            result["hostSurfaces"] = {
                "pluginFeaturesSchema": spec["components"]["schemas"]["Plugin.Features"],
                "pluginRoutes": [path for path in spec["paths"] if path.startswith("/api/plugin")],
                "desktopBinaryOnPath": shutil.which("opencode-desktop") is not None,
                "displayAvailable": bool(os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")),
            }
            page = api("get", "/", raw_output=True)
            title = re.search(r"<title>(.*?)</title>", page)
            result["hostSurfaces"]["rootResponse"] = {
                "bytes": len(page.encode()), "html": "<!doctype html>" in page.lower(),
                "title": title.group(1) if title else None,
            }
            result["plugins"] = api("get", "/api/plugin")
            result["diagnostics"] = api("post", RPC + "diagnostics", {"input": {}})
            result["plugins"] = api("get", "/api/plugin")
            plugin_info = next(item for item in result["plugins"]["data"] if item["id"] == "rostfrei.presentation.spike")
            result["pluginInfo"] = plugin_info
            del result["plugins"]
            assert plugin_info["state"]["status"] == "active"
            assert plugin_info["features"] == {"server": True, "tui": True, "rpc": True}
            tool_ids = [item["id"] for item in result["diagnostics"]["output"]["tools"]]
            assert sorted(tool_ids) == sorted([
                "rostfrei_presentation_show_operation", "rostfrei_presentation_inspect_message",
                "rostfrei_presentation_selected_message",
            ])
            def rpc(method, value, timeout=60):
                return api("post", RPC + method, {"input": value}, timeout=timeout)["output"]

            captures = rpc("listCaptures", {})
            result["captures"] = captures
            assert rpc("selectedMessage", {}) is None
            if args.tui:
                columns, rows = map(int, args.tui_size.split("x"))
                result["nativeTui"] = probe_tui(binary, base, root, env, rpc, captures, outputs, size=(rows, columns))
                result["sessionsAfterTui"] = api("get", "/api/session")
                assert result["sessionsAfterTui"]["data"] == [], "Unexpected session creation during native dialogs"
            for index, capture in enumerate(captures):
                operation = {key: capture[key] for key in ("captureId", "snapshotId", "operationId")}
                before = rpc("showOperation", operation)
                if sources:
                    assert capture["captureId"] == "sha256:" + sources[index]["sha256"]
                    text, offset, pages = "", 0, 0
                    while True:
                        source_page = rpc("sourcePage", {**operation, "offset": offset, "reveal": True})
                        assert len(source_page["text"]) <= 4096
                        text += source_page["text"]
                        pages += 1
                        if source_page["nextOffset"] is None: break
                        offset = source_page["nextOffset"]
                    assert text.encode("utf8") == sources[index]["bytes"], "Original source text changed through HTTP transport"
                    result.setdefault("sourceTextChecks", []).append({"captureId": capture["captureId"], "pages": pages, "utf8Sha256": hashlib.sha256(text.encode()).hexdigest(), "exactOriginalBytes": True})
                messages = rpc("listMessages", {**operation, "offset": 0})["messages"]
                identity = {"captureId": capture["captureId"], **messages[min(1, len(messages) - 1)]["selection"]}
                assert rpc("selectMessage", identity) == identity
                assert rpc("selectedMessage", {}) == identity
                inspected = [rpc("inspectMessage", {**operation, "messageId": message["messageId"]}) for message in messages]
                assert [item["identity"]["messageId"] for item in inspected] == [item["messageId"] for item in messages]
                assert all(not item["payload"]["displayed"] for item in inspected)
                assert all(len(json.dumps(item).encode()) < 8192 for item in inspected)
                failures = {}
                for label, invalid in (
                    ("wrongCapture", {**identity, "captureId": "sha256:" + "0" * 64}),
                    ("staleSnapshot", {**identity, "snapshotId": "stale-snapshot"}),
                    ("wrongOperation", {**identity, "operationId": "unknown-operation"}),
                    ("wrongMessage", {**operation, "messageId": "unknown-message"}),
                    ("invalidType", {**operation, "messageId": 1}),
                ):
                    failures[label] = api("post", RPC + "selectMessage", {"input": invalid}, expected_error=True)
                assert rpc("selectedMessage", {}) == identity, "Rejected selection changed state"
                for method, invalid in (("sourcePage", {**operation, "offset": 0, "reveal": False}),
                                        ("messageDetail", {**identity, "section": "payload", "offset": 0, "reveal": False})):
                    failures[method + "RequiresReveal"] = api("post", RPC + method, {"input": invalid}, expected_error=True)
                assert rpc("showOperation", operation) == before, "Inspection changed evidence"
                rendered = rpc("renderOperation", operation)
                result.setdefault("inspections", []).append({
                    "capture": capture, "operationSummary": before, "selectedMessage": identity, "messages": messages, "summaries": inspected,
                    "text": rendered, "invalidSelections": failures,
                })
            # Cross-capture reads cannot use IDs from another capture, including
            # imported captures that happen to share one operation ID.
            result["crossCaptureChecks"] = []
            for index, capture in enumerate(captures):
                other = captures[(index + 1) % len(captures)]
                if other["captureId"] == capture["captureId"]:
                    continue
                own = result["inspections"][index]
                other_messages = {item["messageId"] for item in result["inspections"][(index + 1) % len(captures)]["messages"]}
                foreign = next((item for item in own["messages"] if item["messageId"] not in other_messages), None)
                if foreign:
                    invalid = {"captureId": other["captureId"], "snapshotId": other["snapshotId"], "operationId": other["operationId"], "messageId": foreign["messageId"]}
                    failure = api("post", RPC + "selectMessage", {"input": invalid}, expected_error=True)
                    result["crossCaptureChecks"].append({"sameOperationId": capture["operationId"] == other["operationId"], "rejected": invalid, **failure})
            result["mcp"] = api("get", "/api/mcp")
            result["mcpResources"] = api("get", "/api/mcp/resource")
            result["mcpClientInitialize"] = json.loads((root / "mcp-initialize.json").read_text())
            result["sessions"] = api("get", "/api/session")
            assert result["sessions"]["data"] == [], "Probe must not create model sessions"
            assert result["mcp"]["data"][0]["status"]["status"] == "connected"
            assert result["mcpResources"]["data"]["resources"][0]["uri"] == "ui://rostfrei/synthetic"
            result["checks"] = ["real host plugin loading", "three custom tools registered", "digest-bound bounded RPC summaries", "unknown capture/operation/message rejected", "invalid type rejected by host", "payloads hidden by default", "MCP negotiation recorded", "no model sessions"]
            if not args.core_double:
                result["coreFilesUnchanged"] = all(digest(presentation / name) == expected for name, expected in result["presentationSourceSha256"].items())
                assert result["coreFilesUnchanged"]
            result["rpcPassed"] = True
            result["passed"] = not args.tui or result["nativeTui"]["passed"]
        except Exception as error:
            result["passed"] = False
            result["error"] = str(error)
        finally:
            cleanup_server(child, log, root, password, result)
        result = json.loads(json.dumps(result).replace(str(root), "<owned-temp>"))
    result["ownedDirectoryRemoved"] = not root.exists()
    if not result["ownedDirectoryRemoved"]:
        result["passed"] = False
        result.setdefault("error", "Owned temporary directory cleanup incomplete")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="Report in a NEW directory under research artifacts; default creates a unique directory")
    parser.add_argument("--evidence", type=Path, action="append", default=[])
    parser.add_argument("--tui", action="store_true")
    parser.add_argument("--tui-size", choices=("90x28", "120x40"), default="90x28", help="Owned PTY columns x rows")
    parser.add_argument("--core-double", action="store_true", help="Synthetic UI qualification only, never real evidence")
    args = parser.parse_args()
    if args.core_double and args.evidence:
        raise SystemExit("--core-double cannot be used with evidence files")
    # All report/sidecar destinations are reserved before ANY subprocess call.
    outputs = Outputs(args.output, RESEARCH, args.evidence)
    sources = []
    result = {"passed": False}
    try:
        for path in args.evidence:
            data = path.read_bytes()
            sources.append({"path": path.resolve(), "bytes": data, "sha256": hashlib.sha256(data).hexdigest()})
        try:
            result = run_probe(args, outputs, sources)
        except Exception as error:
            result = {"passed": False, "error": str(error)}
        outputs.finish(result, sources)
    finally:
        outputs.close()
    print(json.dumps({"report": str(outputs.paths["report"]), "passed": result["passed"], "error": result.get("error"), "finalInputIntegrity": "verified after all output writes"}, indent=2))
    if not result["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
