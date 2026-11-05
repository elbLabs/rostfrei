"""Owned output reservation, integrity verification and idempotent process cleanup."""

import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import uuid


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class Outputs:
    """Reserve every destination before subprocess startup; retain exclusive FDs."""

    def __init__(self, requested, artifact_root, inputs):
        artifact_root = Path(artifact_root).absolute()
        path = Path(requested).absolute() if requested else artifact_root / f"opencode-{uuid.uuid4().hex}" / "report.json"
        path = Path(os.path.abspath(path))
        if not path.is_relative_to(artifact_root) or path.parent == artifact_root:
            raise ValueError("Output must be inside a NEW owned directory under the research artifact area")
        for candidate in (path, *path.parents):
            if candidate.is_symlink():
                raise ValueError("Symlinked output paths are not allowed")
        self.paths = {
            "report": path,
            "ansi": path.with_suffix(".tui.ansi"),
            "text": path.with_suffix(".tui.txt"),
            "frames": path.with_suffix(".tui.frames.json"),
            "integrity": path.with_suffix(".integrity.json"),
        }
        if len(set(self.paths.values())) != len(self.paths):
            raise ValueError("Output names collide")
        resolved_inputs = [Path(item).resolve(strict=True) for item in inputs]
        for destination in self.paths.values():
            if destination.exists() or destination.is_symlink():
                raise ValueError("Every report and sidecar must be new; existing files/aliases are refused")
            if destination.resolve() in resolved_inputs:
                raise ValueError("Output aliases an evidence input")
        # A new parent excludes existing hardlinks as well as source/evidence
        # paths. No recursive creation that could traverse an unreviewed alias.
        path.parent.mkdir(mode=0o700)
        self.files = {}
        try:
            for key, destination in self.paths.items():
                fd = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
                self.files[key] = os.fdopen(fd, "wb")
        except BaseException:
            self.close()
            raise
        self.written = set()

    def write(self, key, content):
        if key in self.written:
            raise ValueError(f"Output {key} is write-once")
        file = self.files[key]
        owned = os.fstat(file.fileno())
        current = self.paths[key].lstat()
        if (owned.st_dev, owned.st_ino) != (current.st_dev, current.st_ino) or owned.st_nlink != 1:
            raise ValueError("Reserved output identity changed")
        file.write(content.encode() if isinstance(content, str) else content)
        file.flush()
        os.fsync(file.fileno())
        self.written.add(key)

    def finish(self, result, sources):
        result["inputFiles"] = [{"path": str(source["path"]), "beforeSha256": source["sha256"]} for source in sources]
        result["integrityReport"] = self.paths["integrity"].name
        for key in ("ansi", "text", "frames"):
            if key not in self.written:
                self.write(key, "[]\n" if key == "frames" else "")
        self.write("report", json.dumps(result, indent=2) + "\n")
        integrity = [{**item, "afterWritesSha256": digest(item["path"])} for item in result["inputFiles"]]
        self.write("integrity", json.dumps({"afterReportAndSidecars": integrity}, indent=2) + "\n")
        # Final read is AFTER every output write, including the integrity report.
        if any(digest(source["path"]) != source["sha256"] for source in sources):
            raise RuntimeError("Evidence changed: final integrity check after all artifact writes failed")
        return integrity

    def close(self):
        for file in self.files.values():
            file.close()


def stop_group(process, timeout=5):
    """Even a reaped leader may have descendants; missing group means stopped."""
    errors = []
    if process is None:
        return {"stopped": True, "errors": errors}
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(process.pid, sig)
        except ProcessLookupError:
            pass
        except OSError as error:
            errors.append(str(error))
        try:
            process.wait(timeout=timeout)
            # A reaped leader does not establish that its group is empty. Still
            # attempt SIGKILL for descendants that ignored SIGTERM.
        except subprocess.TimeoutExpired:
            continue
        except OSError as error:
            errors.append(str(error))
            break
    return {"stopped": process.poll() is not None, "errors": errors}
