//! Catalog-driven acceptance against an owned, disposable application only.
use crate::{Environment, array, nats, process, save_json, text};
use anyhow::{Context, Result, ensure};
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, Read},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

pub struct Capabilities {
    pub origin: String,
    pub application: String,
    pub output: PathBuf,
    pub catalogs: BTreeMap<String, Value>,
    feeds: BTreeMap<String, Value>,
    tokens: BTreeMap<String, String>,
    normal_root: Value,
    normal_snapshot: Value,
}
impl Capabilities {
    pub fn new(env: &Environment, output: &Path) -> Result<Self> {
        let get = |name: &str| {
            env.get(name)
                .cloned()
                .with_context(|| format!("missing {name}"))
        };
        let mut tokens = BTreeMap::from([
            ("test".into(), get("ROSTFREI_API_TOKEN")?),
            ("production".into(), get("ROSTFREI_INSPECTION_TOKEN")?),
        ]);
        if let Some(token) = env.get("ROSTFREI_DISPATCH_TOKEN") {
            tokens.insert("dispatch".into(), token.clone());
        }
        Ok(Self {
            origin: get("ROSTFREI_TRACER_URL")?,
            application: get("ROSTFREI_APPLICATION")?,
            output: output.into(),
            catalogs: BTreeMap::new(),
            feeds: BTreeMap::new(),
            tokens,
            normal_root: Value::Null,
            normal_snapshot: Value::Null,
        })
    }
    pub fn url(&self, href: &str) -> Result<String> {
        ensure!(
            href.starts_with('/')
                && !href.starts_with("//")
                && !href
                    .chars()
                    .any(|c| c <= ' ' || c == '\u{7f}' || "\\{}#".contains(c)),
            "Host advertised an unsafe or unresolved href"
        );
        Ok(format!("{}{href}", self.origin))
    }
    pub fn save(&self, name: &str, value: &Value) -> Result<()> {
        save_json(&self.output.join(format!("{name}.json")), value)
    }
    pub fn request(
        &self,
        href: &str,
        scope: &str,
        payload: Option<&Value>,
        expected: u16,
        key: Option<&str>,
        evidence: Option<&str>,
    ) -> Result<(Value, HeaderMap)> {
        let client = nats::client(Duration::from_secs(45))?;
        let url = self.url(href)?;
        let mut request = if payload.is_some() {
            client.post(url)
        } else {
            client.get(url)
        };
        request = request.bearer_auth(self.tokens.get(scope).context("missing capability token")?);
        if let Some(key) = key {
            request = request.header("Idempotency-Key", key);
        }
        if let Some(payload) = payload {
            request = request
                .header("Content-Type", "application/json")
                .body(serde_json::to_vec(payload)?);
        }
        let mut response = request.send()?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let mut body = Vec::new();
        let read = response.read_to_end(&mut body);
        if let Some(evidence) = evidence {
            self.save(evidence,&json!({"status":status,"location":headers.get("Location").and_then(|h|h.to_str().ok()),
                "contentType":headers.get("Content-Type").and_then(|h|h.to_str().ok()),"body":String::from_utf8_lossy(&body),
                "bodyComplete":read.is_ok(),"readError":read.as_ref().err().map(|_|"IncompleteRead")}))?;
        }
        read.context("Incomplete HTTP response; do not resubmit")?;
        ensure!(
            status == expected,
            "{scope} {href}: HTTP {status}, expected {expected}"
        );
        let value = if !body.is_empty()
            && headers
                .get("Content-Type")
                .is_some_and(|h| h.to_str().unwrap_or_default().contains("json"))
        {
            serde_json::from_slice(&body)?
        } else {
            Value::Null
        };
        Ok((value, headers))
    }
    pub fn get(&self, href: &str, scope: &str) -> Result<Value> {
        Ok(self.request(href, scope, None, 200, None, None)?.0)
    }
    pub fn snapshot(&self, scope: &str) -> Result<Value> {
        let snapshot = self.get(
            text(&self.feeds.get(scope).context("missing feed")?["listHref"])?,
            scope,
        )?;
        ensure!(
            snapshot["application"] == self.application
                && snapshot["scope"] == scope
                && snapshot["status"] == "live",
            "unavailable or mismatched observation"
        );
        ensure!(
            !text(&snapshot["generation"])?.is_empty(),
            "missing generation"
        );
        for name in ["revision", "evictedFlows", "discardedMessages"] {
            ensure!(
                !text(&snapshot[name])?.is_empty()
                    && text(&snapshot[name])?.chars().all(|c| c.is_ascii_digit()),
                "invalid observation {name}"
            );
        }
        array(&snapshot["items"])?;
        Ok(snapshot)
    }
    pub fn discover(&mut self) -> Result<()> {
        for scope in ["test", "production"] {
            let catalog = self.get("/catalog", scope)?;
            ensure!(catalog["catalogVersion"] == 1, "catalog version mismatch");
            let feeds = array(&catalog["observation"])?;
            ensure!(
                feeds.len() == 1
                    && feeds[0]["scope"] == scope
                    && feeds[0]["application"] == self.application,
                "scoped feed mismatch"
            );
            let feed = feeds[0].clone();
            self.feeds.insert(scope.into(), feed.clone());
            self.save(&format!("catalog-{scope}"), &catalog)?;
            self.save(
                &format!("observation-{scope}-initial"),
                &self.snapshot(scope)?,
            )?;
            let listing = self.get(text(&catalog["quarantine"][scope]["listHref"])?, scope)?;
            array(&listing["items"])?;
            self.save(&format!("quarantine-{scope}"), &listing)?;
            let response = nats::client(Duration::from_secs(2))?
                .get(self.url(text(&feed["eventsHref"])?)?)
                .bearer_auth(&self.tokens[scope])
                .send()?
                .error_for_status()?;
            let mut event = String::new();
            let mut data = String::new();
            let mut snapshot = None;
            for line in std::io::BufReader::new(response).lines() {
                let line = line?;
                if line.is_empty() {
                    if event == "observation" {
                        snapshot = Some(serde_json::from_str::<Value>(&data)?);
                        break;
                    }
                    event.clear();
                    data.clear();
                } else if let Some(value) = line.strip_prefix("event: ") {
                    event = value.into();
                } else if let Some(value) = line.strip_prefix("data: ") {
                    data.push_str(value);
                }
            }
            let snapshot = snapshot.context("SSE did not send initial observation")?;
            ensure!(
                snapshot["scope"] == scope && snapshot["status"] == "live",
                "SSE snapshot mismatch"
            );
            self.save(&format!("observation-{scope}-sse-initial"), &snapshot)?;
            self.catalogs.insert(scope.into(), catalog);
        }
        Ok(())
    }
    pub fn command_version(&self, root: &Value) -> Result<(Value, Value)> {
        let context = array(&self.catalogs.get("test").context("test catalog")?["contexts"])?
            .iter()
            .find(|c| c["id"] == root["context"])
            .context("command context")?;
        let command = array(&context["commands"])?
            .iter()
            .find(|c| c["id"] == root["name"])
            .context("command")?;
        let version = array(&command["versions"])?
            .iter()
            .find(|v| v["schemaVersion"] == root["schemaVersion"])
            .context("command version")?;
        Ok((context.clone(), version.clone()))
    }
    pub fn observed_run(&self, evidence: &Value, name: &str) -> Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let (snapshot, item) = loop {
            let snapshot = self.snapshot("test")?;
            if let Some(item) = array(&snapshot["items"])?
                .iter()
                .find(|i| i["correlationId"] == evidence["report"]["operation"]["correlationId"])
            {
                break (snapshot.clone(), item.clone());
            }
            ensure!(
                Instant::now() < deadline,
                "Suite committed events missing from observation"
            );
            process::cancelled()?;
            thread::sleep(Duration::from_millis(50));
        };
        let detail = self.get(text(&item["detailHref"])?, "test")?;
        ensure!(
            detail["partial"] == true
                && detail["fidelity"] == "grouped"
                && detail["generation"] == snapshot["generation"]
                && detail["truncated"] == false
                && detail["conflicted"] == false,
            "invalid observed flow"
        );
        let messages = array(&detail["messageSeries"]["messages"])?;
        ensure!(
            !messages.is_empty()
                && messages.iter().all(|m| m["kind"] != "command")
                && detail["messageSeries"]["commandOutcomes"] == json!([]),
            "observation must contain only observed events"
        );
        let commands = array(&evidence["messageSeries"]["messageSeries"]["messages"])?
            .iter()
            .filter(|m| m["kind"] == "command")
            .collect::<Vec<_>>();
        let events = messages
            .iter()
            .filter(|m| m["name"] == "bicycle-added")
            .collect::<Vec<_>>();
        ensure!(
            commands.len() == 1
                && events.len() == 1
                && events[0]["causationId"] == commands[0]["messageId"]
                && messages
                    .iter()
                    .all(|m| m["messageId"] != commands[0]["messageId"]),
            "observation causation mismatch"
        );
        self.save(&format!("observation-test-after-{name}"), &detail)?;
        Ok(detail)
    }
    pub fn normal_operation(
        &self,
        operation: &Value,
        root: &Value,
        identity: Option<&(String, String)>,
    ) -> Result<(String, String)> {
        ensure!(
            operation.is_object() && operation.get("operation").is_none(),
            "Expected a flattened Normal operation document; do not resubmit"
        );
        for field in ["operationId", "correlationId", "messageSeriesHref"] {
            ensure!(
                operation[field].as_str().is_some_and(|s| !s.is_empty()),
                "Normal operation missing {field}; do not resubmit"
            );
        }
        ensure!(
            operation["mode"] == "dispatch"
                && operation["context"] == root["context"]
                && operation["command"] == root["name"]
                && operation["schemaVersion"].is_u64()
                && operation["schemaVersion"] == root["schemaVersion"],
            "Normal command coordinates differ; do not resubmit"
        );
        let current = (
            text(&operation["operationId"])?.into(),
            text(&operation["correlationId"])?.into(),
        );
        ensure!(
            identity.is_none_or(|old| old == &current),
            "Normal operation identity changed while polling; do not resubmit"
        );
        self.url(text(&operation["messageSeriesHref"])?)?;
        ensure!(
            ["queued", "running", "completed", "failed", "indeterminate"]
                .contains(&text(&operation["status"])?),
            "Invalid Normal status; do not resubmit"
        );
        if operation["status"] == "completed" {
            ensure!(
                ["accepted", "rejected"].contains(&text(&operation["result"]["decision"])?)
                    && operation["result"]["published"] == true,
                "Completed Normal operation lacks a published business result; do not resubmit"
            );
        }
        Ok(current)
    }
    pub fn normal_command(&self, root: &Value, key: &str, decision: &str) -> Result<Value> {
        let (_, version) = self.command_version(root)?;
        let (mut operation, headers) = self.request(
            text(&version["dispatchHrefTemplate"])?,
            "dispatch",
            Some(&json!({"schemaVersion":root["schemaVersion"],"payload":root["payload"]})),
            202,
            Some(key),
            Some(&format!("{key}-submission")),
        )?;
        let identity = self.normal_operation(&operation, root, None)?;
        let location = headers
            .get("Location")
            .context("Normal submission missing Location; do not resubmit")?
            .to_str()?;
        self.url(location)?;
        let deadline = Instant::now() + Duration::from_secs(35);
        let mut poll = 0;
        while !["completed", "failed", "indeterminate"].contains(&text(&operation["status"])?) {
            ensure!(
                Instant::now() < deadline,
                "Normal command incomplete; do not resubmit"
            );
            process::cancelled()?;
            thread::sleep(Duration::from_millis(50));
            operation = self
                .request(
                    location,
                    "dispatch",
                    None,
                    200,
                    None,
                    Some(&format!("{key}-poll-{poll}")),
                )?
                .0;
            poll += 1;
            self.normal_operation(&operation, root, Some(&identity))?;
        }
        ensure!(
            operation["status"] == "completed",
            "Normal command {}; do not resubmit",
            operation["status"]
        );
        ensure!(
            operation["result"]["decision"] == decision,
            "unexpected Normal decision: {operation}"
        );
        let href = text(&operation["messageSeriesHref"])?;
        let capture = self
            .request(
                &format!(
                    "{href}{}within=10s&settleFor=500ms",
                    if href.contains('?') { "&" } else { "?" }
                ),
                "dispatch",
                None,
                200,
                None,
                Some(&format!("{key}-capture")),
            )?
            .0;
        ensure!(
            capture["operationId"] == identity.0
                && capture["correlationId"] == identity.1
                && capture["mode"] == "dispatch"
                && capture["messageSeries"].is_object()
                && capture["capture"]["settled"] == true
                && capture["capture"]["fidelity"] == "exact",
            "Normal capture invalid or incomplete; do not resubmit"
        );
        self.save(key, &json!({"operation":operation,"capture":capture}))?;
        Ok(operation)
    }
    pub fn seed_normal(&mut self, definition: &Value) -> Result<()> {
        let mut root = definition["expected"]["graphs"][0]["nodes"][0].clone();
        root["payload"]["fleet_id"] = json!("skills-normal-preserved");
        root["payload"]["bicycle_id"] = json!("skills-normal-bicycle");
        let operation = self.normal_command(&root, "normal-preservation-seed", "accepted")?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let snapshot = self.snapshot("production")?;
            if array(&snapshot["items"])?
                .iter()
                .any(|i| i["correlationId"] == operation["correlationId"])
            {
                self.normal_snapshot = snapshot;
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "Normal seed missing from observation"
            );
            thread::sleep(Duration::from_millis(50));
        }
        self.normal_root = root;
        self.save("normal-before-resets", &self.normal_snapshot)
    }
    pub fn normal_preserved(&self, name: &str) -> Result<()> {
        let current = self.snapshot("production")?;
        ensure!(
            current == self.normal_snapshot,
            "Test reset changed Normal observation"
        );
        self.save(&format!("normal-after-{name}"), &current)
    }
    pub fn verify_normal_history(&self) -> Result<()> {
        self.normal_command(
            &self.normal_root,
            "normal-preservation-after-resets",
            "rejected",
        )?;
        Ok(())
    }
    pub fn after_reset(&self, previous: &Value, name: &str) -> Result<()> {
        let error = self
            .request(
                text(&previous["detailHref"])?,
                "test",
                None,
                410,
                None,
                None,
            )?
            .0;
        ensure!(
            error["code"] == "observation-expired"
                && self.snapshot("test")?["generation"] != previous["generation"],
            "Test observation not expired after reset"
        );
        ensure!(
            self.snapshot("production")?["generation"]
                == crate::read_json(&self.output.join("observation-production-initial.json"))?["generation"],
            "Production generation changed"
        );
        self.save(&format!("observation-test-expired-{name}"), &error)
    }
    pub fn simulate_handler(&self, definition: &Value, required: bool) -> Result<String> {
        let root = &definition["expected"]["graphs"][0]["nodes"][0];
        let (context, version) = self.command_version(root)?;
        let Some(href) = version["simulateHandlerHrefTemplate"].as_str() else {
            self.save("handler-simulation",&json!({"status":"unavailable","reason":"No simulateHandlerHrefTemplate; legacy Preview is not a substitute"}))?;
            ensure!(
                !required,
                "Selected candidate required simulateHandlerHrefTemplate, but it is unavailable"
            );
            return Ok("unavailable".into());
        };
        for context in array(&self.catalogs["test"]["contexts"])? {
            for command in array(&context["commands"])? {
                for version in array(&command["versions"])? {
                    ensure!(
                        version.get("simulateHrefTemplate").is_none(),
                        "legacy preview advertised"
                    );
                }
            }
        }
        let encode = |s: &str| -> String {
            reqwest::Url::parse("http://localhost/")
                .map(|mut u| {
                    u.path_segments_mut().expect("base URL").push(s);
                    u.path().trim_start_matches('/').to_owned()
                })
                .unwrap_or_default()
        };
        self.request(
            &format!(
                "/contexts/{}/commands/{}/simulate",
                encode(text(&root["context"])?),
                encode(text(&root["name"])?)
            ),
            "test",
            Some(&json!({"schemaVersion":root["schemaVersion"],"payload":root["payload"]})),
            404,
            None,
            None,
        )?;
        let aggregate = array(&context["aggregates"])?
            .iter()
            .find(|a| a["aggregateType"] == "bike-rental/rental-fleet")
            .context("rental fleet aggregate")?;
        let before = self.get(text(&aggregate["testInstancesHref"])?, "test")?;
        let observation = self.snapshot("test")?;
        let mut payload = root["payload"].clone();
        payload["bicycle_id"] = json!("handler-only-bicycle");
        let (proposal, headers) = self.request(
            href,
            "test",
            Some(&json!({"schemaVersion":root["schemaVersion"],"payload":payload})),
            200,
            None,
            None,
        )?;
        ensure!(
            !headers.contains_key("Location")
                && proposal["outcome"]["decision"] == "accepted"
                && proposal["proposedEvents"][0]["payload"]["bicycle_id"] == "handler-only-bicycle",
            "invalid accepted simulation"
        );
        let streams = array(&proposal["streams"])?;
        ensure!(streams.len() == 1, "simulation streams");
        let current = array(&before["items"])?
            .iter()
            .find(|i| i["aggregateId"] == streams[0]["aggregateId"])
            .context("simulated stream")?;
        ensure!(
            streams[0]["baseStreamVersion"] == current["streamVersion"],
            "simulation base version"
        );
        let (rejection, headers) = self.request(
            href,
            "test",
            Some(&json!({"schemaVersion":root["schemaVersion"],"payload":root["payload"]})),
            200,
            None,
            None,
        )?;
        ensure!(
            !headers.contains_key("Location")
                && rejection["outcome"]["decision"] == "rejected"
                && rejection["proposedEvents"] == json!([]),
            "invalid rejected simulation"
        );
        for result in [&proposal, &rejection] {
            ensure!(
                result["kind"] == "handlerSimulation" && result["committed"] == false,
                "invalid simulation kind"
            );
            for field in [
                "operation",
                "operationId",
                "correlationId",
                "messageSeries",
                "capture",
                "receipt",
                "publication",
                "commandMessageId",
                "responseMessageId",
            ] {
                ensure!(result.get(field).is_none(), "invented simulation {field}");
            }
        }
        let after = self.get(text(&aggregate["testInstancesHref"])?, "test")?;
        let after_observation = self.snapshot("test")?;
        ensure!(
            before == after && observation == after_observation,
            "Handler diagnostics changed history or observation"
        );
        self.save("handler-simulation",&json!({"status":"passed","proposal":proposal,"rejection":rejection,"historyBefore":before,"historyAfter":after,
            "observationBefore":observation,"observationAfter":after_observation,"legacyPreviewHttpStatus":404,"evidenceScope":"HTTP history/observation and response shape; native no-publication boundary tested separately"}))?;
        Ok("passed".into())
    }
}
