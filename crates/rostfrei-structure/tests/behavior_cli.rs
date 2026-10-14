#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::too_many_lines,
    reason = "bounded fault-server fixtures and assertions"
)]

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Command, Output},
    sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}},
    thread::{self, JoinHandle},
    time::Duration,
};

use serde_json::{Value, json};
use tempfile::TempDir;

const TOKEN: &str = "test-only-rust-client-capability";
const UNTRUSTED: &str = "untrusted-response-body";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode { Pass, Fail, PartialRun, PartialCapture, PartialFailedCapture, Malformed, Unknown, Redirect, UnsafeLink }

struct Server {
    url: String,
    submissions: Arc<AtomicUsize>,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn start(mode: Mode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let submissions = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let count = Arc::clone(&submissions);
        let total = Arc::clone(&requests);
        let stopping = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((socket, _)) => {
                        total.fetch_add(1, Ordering::Relaxed);
                        respond(socket, mode, &count);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(5)),
                    Err(error) => panic!("fault server: {error}"),
                }
            }
        });
        Self { url, submissions, requests, stop, thread: Some(thread) }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn respond(mut socket: TcpStream, mode: Mode, submissions: &AtomicUsize) {
    socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut reader = BufReader::new(&mut socket);
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    let path = first.split_whitespace().nth(1).unwrap().to_owned();
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" { break; }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap();
        }
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload).unwrap();
    if mode == Mode::Redirect {
        write!(socket, "HTTP/1.1 302 Found\r\nLocation: http://untrusted.invalid/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        return;
    }
    let verdict = if matches!(mode, Mode::Fail | Mode::PartialFailedCapture) { "failed" } else { "passed" };
    let value = match path.as_str() {
        "/catalog" => json!({"catalogVersion":1,"behavioralTest":{
            "schemaHref":if mode == Mode::UnsafeLink { "//untrusted.invalid/schema" } else { "/schema" },
            "validateHref":"/validate","runHref":"/run"},"testScenario":{"fixturesHref":"/fixtures"}}),
        "/schema" => json!({"type":"object"}),
        "/validate" => json!({"valid":true}),
        "/fixtures" => json!({"items":[{"id":"no-rental-fleets","fixtureHref":"/fixture"}]}),
        "/fixture" => json!({"id":"no-rental-fleets","revision":"1","messages":[]}),
        "/run" => {
            submissions.fetch_add(1, Ordering::Relaxed);
            json!({"status":if mode == Mode::Unknown { "unexpected" } else { verdict },
                "operation":{"messageSeriesHref":"/series"},"comparison":{"status":verdict}})
        }
        _ if path.starts_with("/series?") => json!({"capture":{"settled":true,"fidelity":"exact"}}),
        _ => panic!("unexpected request {path}"),
    };
    let partial = (path == "/run" && mode == Mode::PartialRun)
        || (path.starts_with("/series?") && matches!(mode, Mode::PartialCapture | Mode::PartialFailedCapture));
    let body = if partial || (path == "/run" && mode == Mode::Malformed) { UNTRUSTED.to_owned() } else { value.to_string() };
    let declared = body.len() + if partial { 100 } else { 0 };
    write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n{body}").unwrap();
}

fn invoke(server: &Server, directory: &Path) -> Output {
    let definition = directory.join("definition.json");
    fs::write(&definition, json!({"schemaVersion":2,"setup":{"fixture":"no-rental-fleets"}}).to_string()).unwrap();
    Command::new(env!("CARGO_BIN_EXE_cargo-rostfrei"))
        .args(["rostfrei", "test"]).arg(definition).arg("--output").arg(directory.join("evidence.json"))
        .current_dir(directory).env("PATH", "")
        .env("ROSTFREI_TRACER_URL", &server.url).env("ROSTFREI_API_TOKEN", TOKEN)
        .output().unwrap()
}

fn check(mode: Mode, code: i32) -> (Output, Value) {
    let temporary = TempDir::new().unwrap();
    let server = Server::start(mode);
    let result = invoke(&server, temporary.path());
    assert_eq!(result.status.code(), Some(code), "{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(server.submissions.load(Ordering::Relaxed), 1);
    let text = fs::read_to_string(temporary.path().join("evidence.json")).unwrap();
    for output in [text.as_str(), &String::from_utf8_lossy(&result.stderr), &String::from_utf8_lossy(&result.stdout)] {
        assert!(!output.contains(TOKEN));
        assert!(!output.contains(UNTRUSTED));
    }
    (result, serde_json::from_str(&text).unwrap())
}

#[test]
fn complete_reports_control_zero_and_one_without_python() {
    for (mode, code, status) in [(Mode::Pass, 0, "passed"), (Mode::Fail, 1, "failed")] {
        let (_, evidence) = check(mode, code);
        assert_eq!(evidence["report"]["status"], status);
        assert!(evidence.get("error").is_none());
    }
}

#[test]
fn truncated_run_is_typed_unknown_outcome_and_never_retried() {
    let (result, evidence) = check(Mode::PartialRun, 2);
    assert_eq!(evidence["runState"], "submission-outcome-unknown");
    assert!(evidence.get("report").is_none());
    assert_eq!(evidence["error"]["kind"], "incomplete_response");
    assert_eq!(evidence["error"]["phase"], "run");
    let stderr: Value = serde_json::from_slice(&result.stderr).unwrap();
    assert_eq!(stderr["error"], evidence["error"]);
}

#[test]
fn incomplete_capture_keeps_either_received_verdict() {
    for (mode, status) in [(Mode::PartialCapture, "passed"), (Mode::PartialFailedCapture, "failed")] {
        let (_, evidence) = check(mode, 2);
        assert_eq!(evidence["runState"], "reported");
        assert_eq!(evidence["captureState"], "incomplete");
        assert_eq!(evidence["report"]["status"], status);
        assert_eq!(evidence["error"]["kind"], "incomplete_response");
        assert_eq!(evidence["error"]["phase"], "capture");
    }
}

#[test]
fn malformed_or_unknown_report_is_not_behavioral_red() {
    for mode in [Mode::Malformed, Mode::Unknown] {
        let (_, evidence) = check(mode, 2);
        assert_eq!(evidence["error"]["kind"], "invalid_response");
    }
}

#[test]
fn redirects_and_cross_origin_links_are_not_followed() {
    for mode in [Mode::Redirect, Mode::UnsafeLink] {
        let directory = TempDir::new().unwrap();
        let server = Server::start(mode);
        let result = invoke(&server, directory.path());
        assert_eq!(result.status.code(), Some(2));
        assert_eq!(server.requests.load(Ordering::Relaxed), 1);
        assert_eq!(server.submissions.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn existing_evidence_is_not_overwritten_or_submitted() {
    let directory = TempDir::new().unwrap();
    let evidence = directory.path().join("evidence.json");
    fs::write(&evidence, "consumer work").unwrap();
    let server = Server::start(Mode::Pass);
    let result = invoke(&server, directory.path());
    assert_eq!(result.status.code(), Some(2));
    assert_eq!(server.submissions.load(Ordering::Relaxed), 0);
    assert_eq!(fs::read_to_string(evidence).unwrap(), "consumer work");
}
