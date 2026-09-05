//! Owned protocol relay for independent malformed-readback and post-commit probes.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::too_many_lines)]
#![allow(
    clippy::items_after_statements,
    clippy::arithmetic_side_effects,
    reason = "fixture-local guard and bounded frames from the owned broker"
)]
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Default)]
pub struct Faults {
    pub mutation: Mutex<Option<&'static str>>,
    pub after_commit: AtomicBool,
    pub altered: AtomicUsize,
    replies: Mutex<HashSet<String>>,
}
pub struct Proxy {
    task: tokio::task::JoinHandle<TestResult>,
    pub js: async_nats::jetstream::Context,
    pub faults: Arc<Faults>,
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Proxy {
    pub async fn stop(mut self) {
        self.task.abort();
        let result = (&mut self.task).await;
        assert!(result.is_ok() || result.unwrap_err().is_cancelled());
    }
    pub async fn start(state: &str, journal: &str) -> TestResult<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let upstream = std::env::var("ROSTFREI_NATS_URL")?
            .rsplit('@')
            .next()
            .unwrap()
            .to_owned();
        let state_get = format!("$JS.API.STREAM.MSG.GET.{state}");
        let journal = journal.to_owned();
        let faults = Arc::new(Faults::default());
        let f = faults.clone();
        let task = tokio::spawn(async move {
            let (client, _) = listener.accept().await?;
            let upstream = tokio::net::TcpStream::connect(upstream).await?;
            client.set_nodelay(true)?;
            upstream.set_nodelay(true)?;
            let (cr, mut cw) = client.into_split();
            let (ur, mut uw) = upstream.into_split();
            let upload = async {
                let mut reader = BufReader::new(cr);
                loop {
                    let mut line = Vec::new();
                    if reader.read_until(b'\n', &mut line).await? == 0 {
                        return TestResult::Ok(());
                    }
                    let fields: Vec<_> = std::str::from_utf8(&line)?.split_whitespace().collect();
                    if matches!(fields.first().copied(), Some("PUB" | "HPUB")) {
                        let length: usize = fields.last().unwrap().parse()?;
                        let mut body = vec![0; length + 2];
                        reader.read_exact(&mut body).await?;
                        if fields[1] == state_get {
                            f.replies.lock().unwrap().insert(fields[2].to_owned());
                        }
                        uw.write_all(&line).await?;
                        uw.write_all(&body).await?;
                    } else {
                        uw.write_all(&line).await?;
                    }
                }
            };
            let download = async {
                let mut reader = BufReader::new(ur);
                loop {
                    let mut line = Vec::new();
                    if reader.read_until(b'\n', &mut line).await? == 0 {
                        return TestResult::Ok(());
                    }
                    let mut fields: Vec<String> = std::str::from_utf8(&line)?
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect();
                    if matches!(fields.first().map(String::as_str), Some("MSG" | "HMSG")) {
                        let length: usize = fields.last().unwrap().parse()?;
                        let header: usize = if fields[0] == "HMSG" {
                            fields[fields.len() - 2].parse()?
                        } else {
                            0
                        };
                        let mut body = vec![0; length + 2];
                        reader.read_exact(&mut body).await?;
                        let mut value =
                            serde_json::from_slice::<serde_json::Value>(&body[header..length]).ok();
                        if value
                            .as_ref()
                            .is_some_and(|v| v["stream"] == journal && v.get("batch").is_some())
                            && f.after_commit.swap(false, Ordering::SeqCst)
                        {
                            *f.mutation.lock().unwrap() = Some("detail-digest");
                        }
                        let state_reply = f.replies.lock().unwrap().remove(&fields[1]);
                        let mutation = *f.mutation.lock().unwrap();
                        if state_reply
                            && let (Some(value), Some(mutation)) = (&mut value, mutation)
                            && value.get("message").is_some()
                        {
                            alter(value, mutation)?;
                            f.altered.fetch_add(1, Ordering::SeqCst);
                            let payload = serde_json::to_vec(value)?;
                            let mut replacement = body[..header].to_vec();
                            replacement.extend(payload);
                            replacement.extend(b"\r\n");
                            *fields.last_mut().unwrap() = (replacement.len() - 2).to_string();
                            line = format!("{}\r\n", fields.join(" ")).into_bytes();
                            body = replacement;
                        }
                        cw.write_all(&line).await?;
                        cw.write_all(&body).await?;
                    } else {
                        cw.write_all(&line).await?;
                    }
                }
            };
            tokio::select! { result = upload => result, result = download => result }
        });
        // Keep the abort guard alive even if connecting fails.
        struct Starting(Option<tokio::task::JoinHandle<TestResult>>);
        impl Drop for Starting {
            fn drop(&mut self) {
                if let Some(t) = &self.0 {
                    t.abort();
                }
            }
        }
        let mut starting = Starting(Some(task));
        let client = async_nats::ConnectOptions::with_user_and_password(
            std::env::var("ROSTFREI_CONTRACT_NATS_USER")?,
            std::env::var("ROSTFREI_CONTRACT_NATS_PASSWORD")?,
        )
        .request_timeout(Some(Duration::from_secs(2)))
        .connect(format!("nats://{address}"))
        .await?;
        Ok(Self {
            task: starting.0.take().unwrap(),
            js: async_nats::jetstream::new(client),
            faults,
        })
    }
}
fn alter(reply: &mut serde_json::Value, mutation: &str) -> TestResult {
    let message = &mut reply["message"];
    match mutation {
        "subject" => message["subject"] = serde_json::json!("wrong.state.subject"),
        "sequence" => message["seq"] = serde_json::json!(0),
        "cas" | "expected-stream" | "message-id" => {
            let mut headers =
                String::from_utf8(STANDARD.decode(message["hdrs"].as_str().unwrap())?)?;
            match mutation {
                "cas" => {
                    headers = headers.replace(
                        "Nats-Expected-Last-Subject-Sequence: 0",
                        "Nats-Expected-Last-Subject-Sequence: 999",
                    );
                }
                "expected-stream" => {
                    headers = headers
                        .lines()
                        .filter(|l| !l.starts_with("Nats-Expected-Stream:"))
                        .collect::<Vec<_>>()
                        .join("\r\n")
                        + "\r\n";
                }
                _ => headers = headers.replace("\r\n\r\n", "\r\nNats-Msg-Id: forbidden-id\r\n\r\n"),
            }
            message["hdrs"] = serde_json::json!(STANDARD.encode(headers));
        }
        _ => {
            let mut state: serde_json::Value =
                serde_json::from_slice(&STANDARD.decode(message["data"].as_str().unwrap())?)?;
            match mutation {
                "version" => state["version"] = serde_json::json!(2),
                "binding" => {
                    state["binding"] = serde_json::json!(STANDARD.encode("wrong reference"));
                }
                "detail-digest" => {
                    state["state"]["bytes"] =
                        serde_json::json!(STANDARD.encode("incorrect immutable detail bytes"));
                }
                "unknown" => state["unknown"] = serde_json::json!(true),
                "missing-kind" => {
                    state["state"].as_object_mut().unwrap().remove("kind");
                }
                "expired-revision" => {
                    state["state"]["revision"] = serde_json::json!("00".repeat(32));
                }
                "expired-previous" => state["state"]["previous"] = serde_json::json!(0),
                _ => return Err(format!("unknown mutation {mutation}").into()),
            }
            message["data"] = serde_json::json!(STANDARD.encode(serde_json::to_vec(&state)?));
        }
    }
    Ok(())
}
