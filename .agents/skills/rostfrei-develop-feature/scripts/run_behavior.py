#!/usr/bin/env python3
"""Validate and run one JSON v2 definition in an already authorized isolated Test scope.

Each run resets the host's complete Test scenario. No production capability is
used. Requires Python 3.11+, ROSTFREI_API_TOKEN and ROSTFREI_TRACER_URL (loopback
HTTP or HTTPS origin). Does not retry submissions or follow redirects.
"""

import argparse
import ipaddress
import json
import os
from pathlib import Path
import sys
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, ProxyHandler, Request, build_opener


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Tracer:
    def __init__(self):
        self.origin = os.environ.get("ROSTFREI_TRACER_URL", "http://127.0.0.1:1309").rstrip("/")
        url = urlsplit(self.origin)
        if (url.scheme not in ("http", "https") or not url.hostname or url.username
                or url.password or url.path or url.query or url.fragment):
            raise ValueError("ROSTFREI_TRACER_URL must be an HTTP(S) origin without credentials or path")
        if url.scheme == "http":
            try:
                loopback = ipaddress.ip_address(url.hostname).is_loopback
            except ValueError:
                loopback = url.hostname == "localhost"
            if not loopback:
                raise ValueError("Use HTTPS for a non-loopback Tracer")
        self.token = os.environ["ROSTFREI_API_TOKEN"]
        if not self.token or any(ord(char) < 32 or ord(char) == 127 for char in self.token):
            raise ValueError("ROSTFREI_API_TOKEN must be nonempty and contain no control characters")
        self.opener = build_opener(ProxyHandler({}), NoRedirect())

    def request(self, href, payload=None):
        if (not isinstance(href, str) or not href.startswith("/") or href.startswith("//")
                or any(char in href for char in "\\#{}")
                or any(ord(char) <= 32 or ord(char) == 127 for char in href)):
            raise ValueError("Tracer advertised an unsafe or unresolved href")
        parsed = urlsplit(href)
        if parsed.scheme or parsed.netloc:
            raise ValueError("Tracer advertised a cross-origin href")
        request = Request(
            self.origin + href,
            data=None if payload is None else json.dumps(payload).encode(),
            headers={"Authorization": f"Bearer {self.token}", "Content-Type": "application/json"},
        )
        try:
            with self.opener.open(request, timeout=90) as response:
                return json.load(response)
        except HTTPError as error:
            # Do not echo server bodies, redirect targets or credentials.
            raise RuntimeError(f"Tracer HTTP {error.code}; no retry performed") from None
        except (URLError, TimeoutError) as error:
            raise RuntimeError("Tracer unreachable or timed out; a submitted run may have occurred; no retry performed") from error


def run(definition, output):
    skill = Path(__file__).resolve().parent.parent
    release = json.loads((skill / "references/release.json").read_text())
    tracer = Tracer()
    catalog = tracer.request("/catalog")
    if catalog["catalogVersion"] != release["contracts"]["catalog"]:
        raise ValueError("Catalog version differs from this installed kit; use matching framework/skills")
    if definition.get("schemaVersion") != 2:
        raise ValueError("Expected a canonical JSON v2 behavior definition")
    capability = catalog["behavioralTest"]
    schema = tracer.request(capability["schemaHref"])
    validation = tracer.request(capability["validateHref"], definition)
    if validation.get("valid") is not True:
        raise ValueError("Definition did not pass runtime validation")
    fixtures = tracer.request(catalog["testScenario"]["fixturesHref"])
    fixture = next(item for item in fixtures["items"] if item["id"] == definition["setup"]["fixture"])
    fixture_document = tracer.request(fixture["fixtureHref"])
    # Reserve the output before a destructive request; never clobber prior evidence.
    with output.open("x") as evidence:
        os.chmod(output, 0o600)
        initial = {"release": release, "validation": validation, "fixture": fixture_document,
                   "catalogVersion": catalog["catalogVersion"], "schema": schema,
                   "definition": definition, "runState": "submission-outcome-unknown"}
        json.dump(initial, evidence, indent=2)
        evidence.flush()
        print("Running isolated Test definition; resets the host's entire configured Test scenario.", flush=True)
        report = tracer.request(capability["runHref"], definition)
        initial.update(report=report, runState="reported")
        # Preserve the report even if subsequent observation fails.
        evidence.seek(0)
        json.dump(initial, evidence, indent=2)
        evidence.truncate()
        evidence.flush()
        series = tracer.request(report["operation"]["messageSeriesHref"] + "?within=10s&settleFor=500ms")
        initial["messageSeries"] = series
        evidence.seek(0)
        json.dump(initial, evidence, indent=2)
        evidence.truncate()
    print(f"Behavioral status: {report['status']}; evidence: {output}")
    return 0 if report["status"] == "passed" else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("definition", type=Path)
    parser.add_argument("--output", type=Path, required=True, help="new evidence file; may contain business payloads")
    args = parser.parse_args()
    try:
        return run(json.loads(args.definition.read_text()), args.output)
    except (OSError, ValueError, KeyError, RuntimeError, StopIteration) as error:
        print(f"Behavior run blocked/incomplete: {error}. Do not infer behavioral red or retry an ambiguous submission.", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
