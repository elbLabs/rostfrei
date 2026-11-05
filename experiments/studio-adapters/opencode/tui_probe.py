"""Actual private PTY interaction plus current-viewport assertions."""

import errno
import fcntl
import json
import os
import pty
import select
import struct
import subprocess
import termios
import time

from probe_support import stop_group
from terminal_screen import screen


def probe_tui(binary, base, root, env, rpc, captures, outputs, timeout=180, size=(28, 90)):
    rows, columns = size
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
    raw = bytearray()
    frames, steps = [], []
    result = {"attempted": True, "passed": False, "interactionDeadlineSeconds": timeout, "size": {"rows": rows, "columns": columns}, "viewportDecoder": "bounded VT cursor/erase decoder; raw ANSI retained"}
    deadline = time.monotonic() + timeout
    process = None

    def remaining():
        value = deadline - time.monotonic()
        if value <= 0: raise TimeoutError("Owned TUI interaction deadline exceeded")
        return value

    def call(method, value):
        return rpc(method, value, timeout=min(10, remaining()))

    def terminal_session():
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    def pump(seconds):
        end = min(deadline, time.monotonic() + seconds)
        while time.monotonic() < end:
            if not select.select([master], [], [], min(0.1, max(0, end - time.monotonic())))[0]: continue
            try: chunk = os.read(master, 65536)
            except OSError as error:
                if error.errno == errno.EIO: break
                raise
            if not chunk: break
            raw.extend(chunk)
            for query, response in ((b"\x1b[6n", b"\x1b[1;1R"), (b"\x1b[c", b"\x1b[?1;2c")):
                if query in chunk: os.write(master, response)

    def key(data, label):
        remaining()
        os.write(master, data)
        steps.append({"key": label, "outputOffset": len(raw)})
        pump(0.2)

    def visible(*needles, absent=()):
        screen_deadline = min(deadline, time.monotonic() + 15)
        while time.monotonic() < screen_deadline and remaining():
            pump(0.2)
            lines = screen(raw, rows, columns)
            current = "\n".join(lines)
            if all(needle in current for needle in needles):
                for needle in absent:
                    if needle in current: raise AssertionError(f"Unexpected visible content: {needle}")
                frames.append({"assertedVisible": list(needles), "assertedAbsent": list(absent), "lines": lines, "outputOffset": len(raw)})
                return
            if process.poll() is not None: raise RuntimeError(f"Owned TUI exited: {process.returncode}")
        raise TimeoutError(f"Viewport did not contain {needles!r}")

    def read_detail(identity, section, action):
        key(action.encode(), f"Search {action}")
        key(b"\r", f"Enter {action}")
        offset, text = 0, ""
        while True:
            detail = call("messageDetail", {**identity, "section": section, "offset": offset, "reveal": section in ("payload", "outcome")})
            chunks = [detail["text"][index:index + 40] for index in range(0, len(detail["text"]), 40)]
            title = "Payload revealed" if section == "payload" else section
            visible(title, f"page {offset // 160 + 1}", *chunks, absent=("Payload hidden",) if section in ("payload", "outcome") else ())
            text += detail["text"]
            if detail["nextOffset"] is None: break
            key(b"Next page", "Search Next page")
            key(b"\r", "Next bounded detail page")
            offset = detail["nextOffset"]
        key(b"\x1b", f"Escape {section} -> selected summary")
        visible("Selected:", "Payload hidden")
        return text

    try:
        process = subprocess.Popen([binary, "--server", base, str(root / "project")], cwd=root / "project",
            env={**env, "TERM": "xterm-256color", "COLORTERM": "truecolor", "LANG": "C.UTF-8"},
            stdin=slave, stdout=slave, stderr=slave, preexec_fn=terminal_session)
        os.close(slave); slave = None
        assert call("selectedMessage", {}) is None
        result["selectionBefore"] = None
        pump(5)
        result["captures"] = []
        for capture_index, capture in enumerate(captures):
            operation = {key: capture[key] for key in ("captureId", "snapshotId", "operationId")}
            messages = call("listMessages", {**operation, "offset": 0})["messages"]
            if not messages: raise RuntimeError("Native qualification requires an inspectable capture")
            message_index = min(1, len(messages) - 1)
            target = messages[message_index]
            identity = {"captureId": capture["captureId"], **target["selection"]}
            summary = call("inspectMessage", identity)
            key(b"\x07\x07", "rapid Ctrl+G twice (single interaction)")
            visible("Saved Tracer captures")
            key(capture["label"].encode(), "Search exact capture label")
            key(b"\r", "Enter capture")
            visible("Messages", target["name"])
            if message_index: key(b"\x1b[B", "Down to nondefault message")
            key(b"\r", "Enter message")
            visible("Selected:", target["name"], *summary["summary"], "Payload hidden", absent=("bicycle_id", "explicit-reveal-only"))
            assert call("selectedMessage", {}) == identity
            record = {"selectedIdentity": identity, "captureStatus": summary["captureStatus"], "nondefaultMessage": bool(message_index), "nondefaultCapture": bool(capture_index), "summary": summary["summary"]}
            result["captures"].append(record)
            exact = json.loads(read_detail(identity, "identity", "Exact IDs"))
            assert all(exact[key] == value for key, value in identity.items())
            record["exactIdentityTextReached"] = True
            record["payloadTextReached"] = read_detail(identity, "payload", "Reveal payload")
            visible("Selected:", "Payload hidden", absent=("bicycle_id", "explicit-reveal-only"))
            if summary["summary"][1].startswith("Outcome:"):
                record["outcomeTextReached"] = read_detail(identity, "outcome", "Reveal recorded outcome")
                assert json.loads(record["outcomeTextReached"])["outcome"]["status"] in summary["summary"][1]
            record["limitationsTextReached"] = read_detail(identity, "limitations", "Limitations")
            key(b"\x1b", "Escape summary -> messages")
            visible("Messages", "(selected)")
            key(b"\r", "Reopen remembered message")
            visible("Selected:", target["name"], summary["summary"][1])
            assert call("selectedMessage", {}) == identity
            key(b"\x1b", "Back to messages")
            visible("Messages", "(selected)")
            key(b"\x1b", "Cancel message picker -> captures")
            visible("Saved Tracer captures")
            key(b"\x1b", "Cancel capture picker")
            key(b"\x07", "Reopen inspector after cancellation")
            visible("Saved Tracer captures")
            key(b"\r", "Enter remembered capture")
            visible("Messages", "(selected)")
            key(b"\r", "Enter remembered message")
            visible("Selected:", target["name"])
            for _ in range(3): key(b"\x1b", "Back/cancel owned dialog")
            visible("Ask anything", absent=("Selected:", "Saved Tracer captures"))
            assert call("selectedMessage", {}) == identity
            record["backReopenCancelPassed"] = True
        visible("Ask anything", absent=("Selected:", "Saved Tracer captures"))
        key(b"tracer-focus-check", "Type unsent composer focus sentinel")
        visible("tracer-focus-check", absent=("Selected:", "Saved Tracer captures"))
        key(b"\x7f" * len("tracer-focus-check"), "Erase unsent focus sentinel")
        visible("Ask anything", absent=("tracer-focus-check",))
        result["composerFocusAfterCancel"] = True
        assert call("selectedMessage", {}) == identity
        result["passed"] = True
    except Exception as error:
        result["error"] = str(error)
        frames.append({"failure": str(error), "lines": screen(raw, rows, columns), "outputOffset": len(raw)})
    finally:
        cleanup = stop_group(process)
        result["ownedTuiStopped"] = cleanup["stopped"]
        result["cleanupErrors"] = cleanup["errors"]
        if cleanup["errors"] or not cleanup["stopped"]: result["passed"] = False
        for fd in (master, slave):
            if fd is not None:
                try: os.close(fd)
                except OSError: pass
        clean = bytes(raw).replace(env["OPENCODE_PASSWORD"].encode(), b"[redacted]").replace(str(root).encode(), b"<owned-temp>")
        artifacts = {"ansi": clean, "frames": json.dumps(frames, indent=2).replace(str(root), "<owned-temp>") + "\n",
                     "text": "\n\n".join("\n".join(frame["lines"]) for frame in frames).replace(str(root), "<owned-temp>")}
        for name, content in artifacts.items():
            try: outputs.write(name, content)
            except Exception as error:
                result["passed"] = False
                result["cleanupErrors"].append(f"write {name}: {error}")
                result.setdefault("error", "Native output retention failed")
        result["outputBytes"] = len(clean)
        result["steps"] = steps
        result["viewportAssertions"] = len([frame for frame in frames if "assertedVisible" in frame])
    return result
