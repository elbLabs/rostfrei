"""Repository-only live acceptance for the HTTP/SSE workflows taught by the skills.

Called by check_agent_skills_live.py on its owned bike-rental host. This is not
consumer tooling or an agent-usability trial; no extra command is submitted to
production. The host integration suite separately exercises external publishers.
"""

import json
import re
import subprocess
import time
from urllib.error import HTTPError
from urllib.parse import urlsplit
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

    def request(self, href, scope="test", payload=None, expected=200):
        request = Request(self.url(href), headers={"Authorization": "Bearer " + self.tokens[scope]})
        if payload is not None:
            request.data = json.dumps(payload).encode()
            request.add_header("Content-Type", "application/json")
        try:
            response = self.opener.open(request, timeout=15)
        except HTTPError as error:
            response = error
        with response:
            assert response.status == expected, f"{scope} {href}: HTTP {response.status}, expected {expected}"
            return json.load(response), response.headers

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
        for scope in self.tokens:
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

    def simulate_handler(self, definition):
        root = definition["expected"]["graphs"][0]["nodes"][0]
        context = next(item for item in self.catalogs["test"]["contexts"] if item["id"] == root["context"])
        command = next(item for item in context["commands"] if item["id"] == root["name"])
        version = next(item for item in command["versions"] if item["schemaVersion"] == root["schemaVersion"])
        href = version.get("simulateHandlerHrefTemplate")
        if href is None:
            result = {"status": "unavailable", "reason": "No simulateHandlerHrefTemplate; legacy Preview is not a substitute"}
            self.save("handler-simulation", result)
            return result["status"]
        assert "simulateHrefTemplate" not in version
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
        rejection, _ = self.request(href, payload={"schemaVersion": root["schemaVersion"], "payload": root["payload"]})
        assert rejection["outcome"]["decision"] == "rejected" and rejection["proposedEvents"] == []
        for result in (proposal, rejection):
            assert not any(field in result for field in ("operationId", "correlationId", "messageSeries", "capture", "receipt"))
        after, _ = self.request(aggregate["testInstancesHref"])
        assert after == before, "Handler diagnostics must not append history"
        assert self.snapshot("test")["items"] == observation["items"], "No invented observation"
        self.save("handler-simulation", {"status":"passed", "proposal":proposal, "rejection":rejection,
                                       "historyBefore":before, "historyAfter":after})
        return "passed"

    def after_reset(self, previous):
        error, _ = self.request(previous["detailHref"], expected=410)
        assert error["code"] == "observation-expired"
        assert self.snapshot("test")["generation"] != previous["generation"]
        production = self.snapshot("production")
        original = json.loads((self.output / "observation-production-initial.json").read_text())
        assert production["generation"] == original["generation"]
        self.save("observation-test-expired", error)
