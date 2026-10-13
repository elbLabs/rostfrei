#!/usr/bin/env python3
"""Validate and run one JSON v2 definition in an already authorized isolated Test scope.

Each run resets the host's complete Test scenario. No production capability is
used. Requires Python 3.11+, ROSTFREI_API_TOKEN and ROSTFREI_TRACER_URL (loopback
HTTP or HTTPS origin). Does not retry submissions or follow redirects.
"""

import argparse
from enum import Enum
from http.client import HTTPException, IncompleteRead
import ipaddress
import json
import os
from pathlib import Path
import subprocess
import sys
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, ProxyHandler, Request, build_opener


class ErrorKind(str, Enum):
    INCOMPLETE_RESPONSE = "incomplete_response"
    REQUEST_FAILED = "request_failed"
    INVALID_RESPONSE = "invalid_response"
    SETUP_FAILED = "setup_failed"


class RunnerError(RuntimeError):
    """A local client failure, distinct from a Tracer behavioral verdict."""

    def __init__(self, kind: ErrorKind, message: str, *, phase: str, **details):
        super().__init__(message)
        self.kind = kind
        self.phase = phase
        self.details = details

    def document(self):
        return {"kind": self.kind.value, "message": str(self), "phase": self.phase, "details": self.details}


def read_input(path, label):
    try:
        value = json.loads(path.read_text())
        if isinstance(value, dict):
            return value
    except (OSError, ValueError):
        pass
    raise RunnerError(ErrorKind.SETUP_FAILED, f"Could not read {label} as a JSON object", phase="preparation")


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Tracer:
    def __init__(self):
        self.origin = os.environ.get("ROSTFREI_TRACER_URL", "http://127.0.0.1:1309").rstrip("/")
        try:
            url = urlsplit(self.origin)
            url.port  # Validate before request construction; do not echo URL values.
        except ValueError:
            raise RunnerError(ErrorKind.SETUP_FAILED, "Invalid Tracer origin", phase="configuration") from None
        if (url.scheme not in ("http", "https") or not url.hostname or url.username
                or url.password or url.path or url.query or url.fragment):
            raise RunnerError(ErrorKind.SETUP_FAILED, "ROSTFREI_TRACER_URL must be an HTTP(S) origin without credentials or path", phase="configuration")
        if url.scheme == "http":
            try:
                loopback = ipaddress.ip_address(url.hostname).is_loopback
            except ValueError:
                loopback = url.hostname == "localhost"
            if not loopback:
                raise RunnerError(ErrorKind.SETUP_FAILED, "Use HTTPS for a non-loopback Tracer", phase="configuration")
        self.token = os.environ.get("ROSTFREI_API_TOKEN", "")
        if not self.token or any(ord(char) < 32 or ord(char) == 127 for char in self.token):
            raise RunnerError(ErrorKind.SETUP_FAILED, "ROSTFREI_API_TOKEN must be nonempty and contain no control characters", phase="configuration")
        self.opener = build_opener(ProxyHandler({}), NoRedirect())

    def request(self, href, payload=None, *, phase="request"):
        if (not isinstance(href, str) or not href.startswith("/") or href.startswith("//")
                or any(char in href for char in "\\#{}")
                or any(ord(char) <= 32 or ord(char) == 127 for char in href)):
            raise RunnerError(ErrorKind.INVALID_RESPONSE, "Tracer advertised an unsafe or unresolved href", phase=phase)
        try:
            parsed = urlsplit(href)
        except ValueError:
            raise RunnerError(ErrorKind.INVALID_RESPONSE, "Tracer advertised a malformed href", phase=phase) from None
        if parsed.scheme or parsed.netloc:
            raise RunnerError(ErrorKind.INVALID_RESPONSE, "Tracer advertised a cross-origin href", phase=phase)
        request = Request(
            self.origin + href,
            data=None if payload is None else json.dumps(payload).encode(),
            headers={"Authorization": f"Bearer {self.token}", "Content-Type": "application/json"},
        )
        try:
            with self.opener.open(request, timeout=90) as response:
                value = json.load(response)
                if not isinstance(value, dict):
                    raise RunnerError(ErrorKind.INVALID_RESPONSE, "Expected a JSON response object", phase=phase)
                return value
        except IncompleteRead as error:
            raise RunnerError(
                ErrorKind.INCOMPLETE_RESPONSE, "Tracer response ended before the declared body was received; no retry performed",
                phase=phase, method=request.get_method(), receivedBytes=len(error.partial),
                expectedAdditionalBytes=error.expected,
            ) from None
        except HTTPError as error:
            # Do not echo server bodies, redirect targets or credentials.
            raise RunnerError(ErrorKind.REQUEST_FAILED, "Tracer HTTP request failed; no retry performed",
                              phase=phase, method=request.get_method(), httpStatus=error.code) from None
        except (URLError, OSError, HTTPException):
            raise RunnerError(ErrorKind.REQUEST_FAILED, "Tracer transport failed or timed out; no retry performed",
                              phase=phase, method=request.get_method()) from None
        except ValueError:
            raise RunnerError(ErrorKind.INVALID_RESPONSE, "Tracer response was not valid JSON", phase=phase) from None


def prepare_source_resources(skill):
    # Installed kits have no source prerequisite. Source mode requires this exact
    # canonical runner and the packager to be tracked in the enclosing Git repo.
    if len(skill.parents) < 3:
        return
    root = skill.parents[2]
    packager = root / "scripts/package_agent_skills.py"
    if not (root / ".git").exists() or not packager.is_file():
        return
    tracked = subprocess.run([
        "git", "ls-files", "--error-unmatch", "--",
        str((skill / "scripts/run_behavior.py").relative_to(root)), "scripts/package_agent_skills.py",
    ], cwd=root, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if tracked.returncode == 0:
        result = subprocess.run([sys.executable, "-I", str(packager), "--prepare-source"], cwd=root,
                                capture_output=True, text=True)
        if result.returncode:
            raise RunnerError(ErrorKind.SETUP_FAILED,
                              "Source preparation failed; run scripts/package_agent_skills.py --prepare-source from the framework root to inspect conflicts",
                              phase="preparation", exitCode=result.returncode)


def write_evidence(file, state):
    try:
        file.seek(0)
        json.dump(state, file, indent=2)
        file.truncate()
        file.flush()
    except OSError:
        raise RunnerError(ErrorKind.SETUP_FAILED, "Could not retain execution evidence", phase="evidence_file") from None


def run(definition, output):
    skill = Path(__file__).resolve().parent.parent
    try:
        prepare_source_resources(skill)
        release = read_input(skill / "references/release.json", "release metadata")
        catalog_version = release["contracts"]["catalog"]
    except (OSError, KeyError, TypeError):
        raise RunnerError(ErrorKind.SETUP_FAILED, "Could not prepare skill release metadata", phase="preparation") from None
    if definition.get("schemaVersion") != 2:
        raise RunnerError(ErrorKind.SETUP_FAILED, "Expected a canonical JSON v2 behavior definition", phase="preparation")
    tracer = Tracer()
    phase = "catalog"
    try:
        catalog = tracer.request("/catalog", phase=phase)
        if catalog["catalogVersion"] != catalog_version:
            raise RunnerError(ErrorKind.INVALID_RESPONSE, "Catalog version differs from this kit; use matching framework/skills", phase=phase)
        capability = catalog["behavioralTest"]
        phase = "schema"
        schema = tracer.request(capability["schemaHref"], phase=phase)
        phase = "validation"
        validation = tracer.request(capability["validateHref"], definition, phase=phase)
        if validation.get("valid") is not True:
            raise RunnerError(ErrorKind.INVALID_RESPONSE, "Definition did not pass runtime validation", phase=phase)
        phase = "fixture"
        fixtures = tracer.request(catalog["testScenario"]["fixturesHref"], phase=phase)
        fixture = next(item for item in fixtures["items"] if item["id"] == definition["setup"]["fixture"])
        fixture_document = tracer.request(fixture["fixtureHref"], phase=phase)
        run_href = capability["runHref"]
    except (KeyError, TypeError, StopIteration):
        raise RunnerError(ErrorKind.INVALID_RESPONSE, "Missing or invalid discovery/fixture fields", phase=phase) from None
    # Reserve the output before a destructive request; never clobber prior evidence.
    try:
        evidence = os.fdopen(os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w")
    except OSError:
        raise RunnerError(ErrorKind.SETUP_FAILED, "Could not create a new evidence file; existing evidence is preserved", phase="evidence_file") from None
    with evidence:
        initial = {"release": release, "validation": validation, "fixture": fixture_document,
                   "catalogVersion": catalog["catalogVersion"], "schema": schema,
                   "definition": definition, "runState": "submission-outcome-unknown"}
        try:
            write_evidence(evidence, initial)
            print("Running isolated Test definition; resets the host's entire configured Test scenario.", flush=True)
            report = tracer.request(run_href, definition, phase="run")
            initial.update(report=report, runState="response-received")
            if report.get("status") not in ("passed", "failed"):
                raise RunnerError(ErrorKind.INVALID_RESPONSE, "Run response has no supported behavioral verdict", phase="run")
            initial.update(runState="reported", captureState="incomplete")
            # Preserve the complete verdict even if subsequent observation fails.
            write_evidence(evidence, initial)
            try:
                href = report["operation"]["messageSeriesHref"] + "?within=10s&settleFor=500ms"
            except (KeyError, TypeError):
                raise RunnerError(ErrorKind.INVALID_RESPONSE, "Report has no valid message-series link", phase="capture") from None
            initial["messageSeries"] = tracer.request(href, phase="capture")
            initial["captureState"] = "complete"
            write_evidence(evidence, initial)
        except RunnerError as error:
            if initial["runState"] == "reported":
                error.details["reportedVerdict"] = initial["report"]["status"]
            initial["error"] = error.document()
            try:
                write_evidence(evidence, initial)
            except RunnerError:
                error.details["evidenceWriteFailed"] = True
            raise
    print(f"Behavioral status: {report['status']}; evidence: {output}")
    return 0 if report["status"] == "passed" else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("definition", type=Path)
    parser.add_argument("--output", type=Path, required=True, help="new evidence file; may contain business payloads")
    args = parser.parse_args()
    try:
        return run(read_input(args.definition, "behavior definition"), args.output)
    except RunnerError as error:
        print(json.dumps({"error": error.document()}), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
