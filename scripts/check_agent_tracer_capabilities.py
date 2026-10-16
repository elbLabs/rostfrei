"""Repository-only live acceptance for the HTTP/SSE workflows taught by the skills.

Called by check_agent_skills_live.py on its owned bike-rental host. This is not
consumer tooling or an agent-usability trial. Normal-state preservation submits
only to this driver's disposable application/broker, never an existing app.
"""

import json
import re
import subprocess
import time
from urllib.error import HTTPError
from urllib.parse import quote, urlsplit
from urllib.request import HTTPRedirectHandler, ProxyHandler, Request, build_opener


class NoRedirects(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Capabilities:
    def __init__(self, environment, output):
        self.origin = environment["ROSTFREI_TRACER_URL"]
        self.application = environment["ROSTFREI_APPLICATION"]
        self.tokens = {"test": environment["ROSTFREI_API_TOKEN"],
                       "production": environment["ROSTFREI_INSPECTION_TOKEN"]}
        if "ROSTFREI_DISPATCH_TOKEN" in environment:
            self.tokens["dispatch"] = environment["ROSTFREI_DISPATCH_TOKEN"]
        self.output = output
        self.opener = build_opener(ProxyHandler({}), NoRedirects())
        self.feeds = {}
        self.catalogs = {}

    def url(self, href):
        parsed = urlsplit(href)
        if (not href.startswith("/") or href.startswith("//") or parsed.scheme or parsed.netloc
                or parsed.fragment or re.search(r"[\\{}\x00-\x20\x7f]", href)):
            raise ValueError("Host advertised an unsafe or unresolved href")
        return self.origin + href

    def request(self, href, scope="test", payload=None, expected=200, idempotency_key=None):
        request = Request(self.url(href), headers={"Authorization": "Bearer " + self.tokens[scope]})
        if idempotency_key is not None:
            request.add_header("Idempotency-Key", idempotency_key)
        if payload is not None:
            request.data = json.dumps(payload).encode()
            request.add_header("Content-Type", "application/json")
        try:
            response = self.opener.open(request, timeout=15)
        except HTTPError as error:
            response = error
        with response:
            assert response.status == expected, f"{scope} {href}: HTTP {response.status}, expected {expected}"
            body = response.read()
            return (json.loads(body) if body and "json" in response.headers.get("Content-Type", "") else None), response.headers

    def save(self, name, value):
        (self.output / f"{name}.json").write_text(json.dumps(value, indent=2) + "\n")

    def snapshot(self, scope):
        snapshot, _ = self.request(self.feeds[scope]["listHref"], scope)
        assert snapshot["application"] == self.application
        assert snapshot["scope"] == scope
        assert snapshot["status"] == "live", "Unavailable coverage must not pass as healthy emptiness"
        assert isinstance(snapshot["generation"], str) and snapshot["generation"]
        for name in ("revision", "evictedFlows", "discardedMessages"):
            assert isinstance(snapshot[name], str) and snapshot[name].isdecimal(), name
        assert isinstance(snapshot["items"], list)
        return snapshot

    def discover(self):
        for scope in ("test", "production"):
            catalog, _ = self.request("/catalog", scope)
            self.catalogs[scope] = catalog
            assert catalog["catalogVersion"] == 1
            feed, = catalog["observation"]
            assert feed["scope"] == scope and feed["application"] == self.application
            self.feeds[scope] = feed
            self.save(f"catalog-{scope}", catalog)
            self.save(f"observation-{scope}-initial", self.snapshot(scope))
            listing, _ = self.request(catalog["quarantine"][scope]["listHref"], scope)
            assert isinstance(listing["items"], list)
            self.save(f"quarantine-{scope}", listing)
            # The skill's bounded curl transport, with the token supplied through
            # a pipe (not argv or a file). Timeout ends this deliberately short watch.
            result = subprocess.run([
                "curl", "-q", "--silent", "--show-error", "--proxy", "", "--noproxy", "*",
                "--max-redirs", "0", "--connect-timeout", "1", "--max-time", "1",
                "--no-buffer", "--header", "@-", self.url(feed["eventsHref"]),
            ], input=f"Authorization: Bearer {self.tokens[scope]}\n", capture_output=True, text=True, timeout=5)
            assert result.returncode in (0, 28), "Bounded SSE request failed"
            frame = next(frame for frame in result.stdout.split("\n\n") if "event: observation\n" in frame)
            snapshot = json.loads(next(line[6:] for line in frame.splitlines() if line.startswith("data: ")))
            assert snapshot["scope"] == scope and snapshot["status"] == "live"
            self.save(f"observation-{scope}-sse-initial", snapshot)

    def observed_run(self, evidence):
        correlation = evidence["report"]["operation"]["correlationId"]
        deadline = time.monotonic() + 10
        while True:
            snapshot = self.snapshot("test")
            found = [item for item in snapshot["items"] if item["correlationId"] == correlation]
            if found:
                break
            assert time.monotonic() < deadline, "Suite's committed events did not reach observation"
            time.sleep(0.05)
        detail, _ = self.request(found[0]["detailHref"])
        assert detail["partial"] is True and detail["fidelity"] == "grouped"
        assert detail["generation"] == snapshot["generation"]
        assert not detail["truncated"] and not detail["conflicted"]
        messages = detail["messageSeries"]["messages"]
        assert messages and all(message["kind"] != "command" for message in messages)
        assert detail["messageSeries"]["commandOutcomes"] == []
        finite = evidence["messageSeries"]["messageSeries"]["messages"]
        command, = [message for message in finite if message["kind"] == "command"]
        event, = [message for message in messages if message["name"] == "bicycle-added"]
        assert event["causationId"] == command["messageId"]
        assert all(message["messageId"] != command["messageId"] for message in messages)
        self.save("observation-test-after-run", detail)
        return detail

    def command_version(self, root):
        context = next(item for item in self.catalogs["test"]["contexts"] if item["id"] == root["context"])
        command = next(item for item in context["commands"] if item["id"] == root["name"])
        version = next(item for item in command["versions"] if item["schemaVersion"] == root["schemaVersion"])
        return context, version

    def simulate_handler(self, definition, required=False):
        root = definition["expected"]["graphs"][0]["nodes"][0]
        context, version = self.command_version(root)
        href = version.get("simulateHandlerHrefTemplate")
        if href is None:
            result = {"status": "unavailable", "reason": "No simulateHandlerHrefTemplate; legacy Preview is not a substitute"}
            self.save("handler-simulation", result)
            if required:
                raise ValueError("Selected candidate required simulateHandlerHrefTemplate, but it is unavailable")
            return result["status"]
        assert all("simulateHrefTemplate" not in item for context in self.catalogs["test"]["contexts"]
                   for command in context["commands"] for item in command["versions"])
        # Only this negative retirement check names an unadvertised legacy route.
        # Every supported action uses the current catalog's relation.
        retired = f'/contexts/{quote(root["context"], safe="")}/commands/{quote(root["name"], safe="")}/simulate'
        self.request(retired, payload={"schemaVersion": root["schemaVersion"], "payload": root["payload"]}, expected=404)
        aggregate = next(item for item in context["aggregates"] if item["aggregateType"] == "bike-rental/rental-fleet")
        before, _ = self.request(aggregate["testInstancesHref"])
        observation = self.snapshot("test")
        proposal_payload = {**root["payload"], "bicycle_id": "handler-only-bicycle"}
        proposal, headers = self.request(href, payload={"schemaVersion": root["schemaVersion"], "payload": proposal_payload})
        assert "Location" not in headers
        assert proposal["kind"] == "handlerSimulation" and proposal["committed"] is False
        assert proposal["outcome"]["decision"] == "accepted"
        assert proposal["proposedEvents"][0]["payload"]["bicycle_id"] == "handler-only-bicycle"
        stream, = proposal["streams"]
        current = next(item for item in before["items"] if item["aggregateId"] == stream["aggregateId"])
        assert stream["baseStreamVersion"] == current["streamVersion"]
        rejection, rejection_headers = self.request(href, payload={"schemaVersion": root["schemaVersion"], "payload": root["payload"]})
        assert "Location" not in rejection_headers
        assert rejection["outcome"]["decision"] == "rejected" and rejection["proposedEvents"] == []
        for result in (proposal, rejection):
            assert result["kind"] == "handlerSimulation" and result["committed"] is False
            assert not any(field in result for field in ("operation", "operationId", "correlationId", "messageSeries",
                "capture", "receipt", "publication", "commandMessageId", "responseMessageId"))
        after, _ = self.request(aggregate["testInstancesHref"])
        assert after == before, "Handler diagnostics must not append history"
        after_observation = self.snapshot("test")
        assert after_observation == observation, "No invented observation or observation generation change"
        self.save("handler-simulation", {"status":"passed", "proposal":proposal, "rejection":rejection,
                                       "historyBefore":before, "historyAfter":after,
                                       "observationBefore":observation, "observationAfter":after_observation,
                                       "legacyPreviewHttpStatus":404,
                                       "evidenceScope":"HTTP history/observation and response shape; native no-publication boundary tested separately"})
        return "passed"

    def normal_command(self, root, key, decision):
        _, version = self.command_version(root)
        submitted, headers = self.request(version["dispatchHrefTemplate"], "dispatch",
            payload={"schemaVersion": root["schemaVersion"], "payload": root["payload"]},
            expected=202, idempotency_key=key)
        operation = submitted["operation"]
        deadline = time.monotonic() + 35
        while operation["status"] not in ("completed", "failed", "indeterminate"):
            assert time.monotonic() < deadline, "Normal command incomplete; do not resubmit"
            time.sleep(0.05)
            operation, _ = self.request(headers["Location"], "dispatch")
        assert operation["status"] == "completed", operation
        assert operation["result"]["decision"] == decision, operation
        capture, _ = self.request(operation["messageSeriesHref"] + "?within=10s&settleFor=500ms", "dispatch")
        assert capture["capture"]["settled"] and capture["capture"]["fidelity"] == "exact"
        self.save(key, {"operation": operation, "capture": capture})
        return operation

    def seed_normal(self, definition):
        root = definition["expected"]["graphs"][0]["nodes"][0]
        self.normal_root = {**root, "payload": {**root["payload"],
            "fleet_id": "skills-normal-preserved", "bicycle_id": "skills-normal-bicycle"}}
        operation = self.normal_command(self.normal_root, "normal-preservation-seed", "accepted")
        deadline = time.monotonic() + 10
        while True:
            self.normal_snapshot = self.snapshot("production")
            if any(item["correlationId"] == operation["correlationId"] for item in self.normal_snapshot["items"]):
                break
            assert time.monotonic() < deadline, "Normal seed missing from observation"
            time.sleep(0.05)
        self.save("normal-before-resets", self.normal_snapshot)

    def normal_preserved(self, name):
        current = self.snapshot("production")
        assert current == self.normal_snapshot, "Test reset changed Normal observation"
        self.save(f"normal-after-{name}", current)

    def verify_normal_history(self):
        # A new command identity must still see the bicycle committed before all
        # three resets. This is a real business decision, not just a retained feed.
        self.normal_command(self.normal_root, "normal-preservation-after-resets", "rejected")

    def after_reset(self, previous, name="reset"):
        error, _ = self.request(previous["detailHref"], expected=410)
        assert error["code"] == "observation-expired"
        assert self.snapshot("test")["generation"] != previous["generation"]
        production = self.snapshot("production")
        original = json.loads((self.output / "observation-production-initial.json").read_text())
        assert production["generation"] == original["generation"]
        self.save(f"observation-test-expired-{name}", error)
