use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use xtask::{capabilities::Capabilities, environment, read_json};

fn root() -> Value {
    json!({"context":"bike-rental","name":"add-bicycle","schemaVersion":2,"payload":{"fleet_id":"normal","bicycle_id":"one","condition":"serviceable"}})
}
fn operation(status: &str) -> Value {
    let mut operation = json!({"operationId":"owned-operation","correlationId":"owned-correlation","messageSeriesHref":"/returned-series","mode":"dispatch","status":status,"command":"add-bicycle","schemaVersion":2,"context":"bike-rental"});
    if status == "completed" {
        operation["result"] = json!({"decision":"accepted","published":true});
    }
    operation
}
struct Host {
    origin: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Host {
    fn new(submission: Vec<u8>, polled: Value, location: Option<&str>, truncated: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let location = location.map(str::to_owned);
        let worker = thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(_) => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    request.push(byte[0]);
                }
                let headers = String::from_utf8_lossy(&request);
                let first = headers.lines().next().unwrap_or("");
                let path = first.split_whitespace().nth(1).unwrap_or("");
                let post = first.starts_with("POST ");
                if let Some(length) = headers.lines().find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                }) {
                    let mut body = vec![0; length];
                    let _ = stream.read_exact(&mut body);
                }
                received
                    .lock()
                    .unwrap()
                    .push(format!("{} {path}", if post { "POST" } else { "GET" }));
                let body = if post {
                    submission.clone()
                } else if path == "/returned-operation" {
                    serde_json::to_vec(&polled).unwrap()
                } else {
                    serde_json::to_vec(&json!({"operationId":"owned-operation","correlationId":"owned-correlation","mode":"dispatch","messageSeries":{"messages":[],"commandOutcomes":[]},"capture":{"settled":true,"fidelity":"exact"}})).unwrap()
                };
                let location = if post {
                    location
                        .as_ref()
                        .map(|l| format!("Location: {l}\r\n"))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                write!(stream,"HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{location}Connection: close\r\n\r\n",if post{202}else{200},body.len()+if truncated{10}else{0}).unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        Self {
            origin: format!("http://{address}"),
            requests,
            stop,
            worker: Some(worker),
        }
    }
    fn client(&self, path: &std::path::Path) -> Capabilities {
        let mut env = environment();
        for (key, value) in [
            ("ROSTFREI_TRACER_URL", self.origin.as_str()),
            ("ROSTFREI_APPLICATION", "owned"),
            ("ROSTFREI_API_TOKEN", "test"),
            ("ROSTFREI_INSPECTION_TOKEN", "inspect"),
            ("ROSTFREI_DISPATCH_TOKEN", "dispatch"),
        ] {
            env.insert(key.into(), value.into());
        }
        let mut client = Capabilities::new(&env, path).unwrap();
        client.catalogs.insert("test".into(),json!({"contexts":[{"id":"bike-rental","commands":[{"id":"add-bicycle","versions":[{"schemaVersion":2,"dispatchHrefTemplate":"/advertised-dispatch"}]}]}]}));
        client
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
    }
}

#[test]
fn flat_submission_is_polled_to_terminal_result_before_capture() {
    let temp = tempfile::tempdir().unwrap();
    let submitted = operation("queued");
    let host = Host::new(
        serde_json::to_vec(&submitted).unwrap(),
        operation("completed"),
        Some("/returned-operation"),
        false,
    );
    let client = host.client(temp.path());
    assert_eq!(
        client.normal_command(&root(), "seed", "accepted").unwrap()["status"],
        "completed"
    );
    assert_eq!(
        *host.requests.lock().unwrap(),
        [
            "POST /advertised-dispatch",
            "GET /returned-operation",
            "GET /returned-series?within=10s&settleFor=500ms"
        ]
    );
    let retained = read_json(&temp.path().join("seed-submission.json")).unwrap();
    assert_eq!(retained["status"], 202);
    assert_eq!(
        serde_json::from_str::<Value>(retained["body"].as_str().unwrap()).unwrap(),
        submitted
    );
}
#[test]
fn invalid_submissions_stop_with_retained_evidence_without_retry_or_capture() {
    let mut missing = operation("queued");
    missing.as_object_mut().unwrap().remove("operationId");
    let mut no_result = operation("completed");
    no_result.as_object_mut().unwrap().remove("result");
    for (name, submission, location, truncated) in [
        (
            "nested",
            serde_json::to_vec(&json!({"operation":operation("queued")})).unwrap(),
            Some("/returned-operation"),
            false,
        ),
        (
            "missing-id",
            serde_json::to_vec(&missing).unwrap(),
            Some("/returned-operation"),
            false,
        ),
        (
            "missing-location",
            serde_json::to_vec(&operation("queued")).unwrap(),
            None,
            false,
        ),
        (
            "unsafe-location",
            serde_json::to_vec(&operation("queued")).unwrap(),
            Some("https://example.invalid/operation"),
            false,
        ),
        (
            "completed-without-result",
            serde_json::to_vec(&no_result).unwrap(),
            Some("/returned-operation"),
            false,
        ),
        (
            "broken-json",
            b"{\"operationId\":".to_vec(),
            Some("/returned-operation"),
            false,
        ),
        (
            "truncated-http",
            serde_json::to_vec(&operation("queued")).unwrap(),
            Some("/returned-operation"),
            true,
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let host = Host::new(submission, Value::Null, location, truncated);
        let client = host.client(temp.path());
        assert!(
            client
                .normal_command(&root(), "invalid", "accepted")
                .is_err(),
            "{name}"
        );
        assert_eq!(
            *host.requests.lock().unwrap(),
            ["POST /advertised-dispatch"],
            "{name}"
        );
        let retained = read_json(&temp.path().join("invalid-submission.json")).unwrap();
        assert_eq!(retained["status"], 202);
        assert_eq!(retained["location"], json!(location));
        assert!(!retained["body"].as_str().unwrap().is_empty());
        assert_eq!(retained["bodyComplete"], !truncated);
        assert!(!temp.path().join("invalid.json").exists());
    }
}
#[test]
fn changed_identity_or_indeterminate_poll_stops_without_capture_or_resubmission() {
    for uncertain in [false, true] {
        let mut polled = operation(if uncertain {
            "indeterminate"
        } else {
            "completed"
        });
        if !uncertain {
            polled["operationId"] = json!("different-operation");
        }
        let temp = tempfile::tempdir().unwrap();
        let host = Host::new(
            serde_json::to_vec(&operation("queued")).unwrap(),
            polled.clone(),
            Some("/returned-operation"),
            false,
        );
        let client = host.client(temp.path());
        let error = client
            .normal_command(&root(), "changed", "accepted")
            .unwrap_err()
            .to_string();
        assert!(error.contains(if uncertain {
            "indeterminate"
        } else {
            "identity"
        }));
        assert_eq!(
            *host.requests.lock().unwrap(),
            ["POST /advertised-dispatch", "GET /returned-operation"]
        );
        let retained = read_json(&temp.path().join("changed-poll-0.json")).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(retained["body"].as_str().unwrap()).unwrap(),
            polled
        );
    }
}
#[test]
fn selected_handler_capability_is_required_but_older_absence_is_recorded() {
    let temp = tempfile::tempdir().unwrap();
    let host = Host::new(Vec::new(), Value::Null, None, false);
    let client = host.client(temp.path());
    let definition = json!({"expected":{"graphs":[{"nodes":[root()]}]}});
    assert_eq!(
        client.simulate_handler(&definition, false).unwrap(),
        "unavailable"
    );
    assert!(
        client
            .simulate_handler(&definition, true)
            .unwrap_err()
            .to_string()
            .contains("required simulateHandlerHrefTemplate")
    );
    assert_eq!(
        read_json(&temp.path().join("handler-simulation.json")).unwrap()["status"],
        "unavailable"
    );
}
