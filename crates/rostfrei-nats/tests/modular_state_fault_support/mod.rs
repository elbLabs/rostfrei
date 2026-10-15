//! Fault only owned NATS frames; never fabricate product state or a server response.
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
    pub block_state_reads: AtomicBool,
    pub lose_expiry_acks: AtomicBool,
    pub block_old_sequence: AtomicBool,
    pub dropped_reads: AtomicUsize,
    pub dropped_acks: AtomicUsize,
    replies: Mutex<HashSet<String>>,
}

pub struct StateProxy {
    _task: TaskGuard,
    pub faults: Arc<Faults>,
    pub js: async_nats::jetstream::Context,
}
struct TaskGuard(tokio::task::JoinHandle<TestResult>);
impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl StateProxy {
    pub async fn start(stream: &str, state_subject: &str, old_sequence: u64) -> TestResult<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let upstream = std::env::var("ROSTFREI_NATS_URL")?
            .rsplit('@')
            .next()
            .ok_or("owned broker address missing")?
            .to_owned();
        let state_get = format!("$JS.API.STREAM.MSG.GET.{stream}");
        let state_subject = state_subject.to_owned();
        let faults = Arc::new(Faults::default());
        let shared = faults.clone();
        let task = tokio::spawn(async move {
            let (client, _) = listener.accept().await?;
            let upstream = tokio::net::TcpStream::connect(upstream).await?;
            let (client_read, mut client_write) = client.into_split();
            let (upstream_read, mut upstream_write) = upstream.into_split();
            let upload = async {
                let mut reader = BufReader::new(client_read);
                loop {
                    let mut line = Vec::new();
                    if reader.read_until(b'\n', &mut line).await? == 0 {
                        return TestResult::Ok(());
                    }
                    let fields: Vec<_> = std::str::from_utf8(&line)?.split_whitespace().collect();
                    if matches!(fields.first().copied(), Some("PUB" | "HPUB")) {
                        let length: usize = fields.last().ok_or("missing frame length")?.parse()?;
                        let mut body = vec![0; length.checked_add(2).ok_or("frame overflow")?];
                        reader.read_exact(&mut body).await?;
                        let header_length = if fields[0] == "HPUB" {
                            fields[fields.len() - 2].parse()?
                        } else {
                            0
                        };
                        let subject = fields[1];
                        let old_get = serde_json::from_slice::<serde_json::Value>(
                            &body[header_length..length],
                        )
                        .is_ok_and(|request| request["seq"].as_u64() == Some(old_sequence));
                        if subject == state_get
                            && (shared.block_state_reads.load(Ordering::SeqCst)
                                || (shared.block_old_sequence.load(Ordering::SeqCst) && old_get))
                        {
                            shared.dropped_reads.fetch_add(1, Ordering::SeqCst);
                            continue;
                        }
                        if subject == state_subject
                            && shared.lose_expiry_acks.load(Ordering::SeqCst)
                        {
                            // Armed only after Active was prepared. Record the actual unique
                            // reply inbox before forwarding the maintenance publish.
                            let has_reply = fields.len() == if fields[0] == "HPUB" { 5 } else { 4 };
                            if !has_reply {
                                return Err("expiry publish did not request a PubAck".into());
                            }
                            shared.replies.lock().unwrap().insert(fields[2].to_owned());
                        }
                        upstream_write.write_all(&line).await?;
                        upstream_write.write_all(&body).await?;
                    } else {
                        upstream_write.write_all(&line).await?;
                    }
                }
            };
            let download = async {
                let mut reader = BufReader::new(upstream_read);
                loop {
                    let mut line = Vec::new();
                    if reader.read_until(b'\n', &mut line).await? == 0 {
                        return TestResult::Ok(());
                    }
                    let fields: Vec<_> = std::str::from_utf8(&line)?.split_whitespace().collect();
                    if matches!(fields.first().copied(), Some("MSG" | "HMSG")) {
                        let length: usize = fields.last().ok_or("missing reply length")?.parse()?;
                        let mut body = vec![0; length.checked_add(2).ok_or("reply overflow")?];
                        reader.read_exact(&mut body).await?;
                        if shared.replies.lock().unwrap().remove(fields[1]) {
                            shared.dropped_acks.fetch_add(1, Ordering::SeqCst);
                            continue;
                        }
                        client_write.write_all(&line).await?;
                        client_write.write_all(&body).await?;
                    } else {
                        client_write.write_all(&line).await?;
                    }
                }
            };
            tokio::select! { result = upload => result, result = download => result }
        });
        // Abort on connection failure too: all tasks/sockets are fixture-owned.
        let task = TaskGuard(task);
        let client = async_nats::ConnectOptions::with_user_and_password(
            std::env::var("ROSTFREI_CONTRACT_NATS_USER")?,
            std::env::var("ROSTFREI_CONTRACT_NATS_PASSWORD")?,
        )
        // Context's timeout governs PubAck, while raw GET uses the client's timeout.
        .request_timeout(Some(Duration::from_millis(250)))
        .connect(format!("nats://{address}"))
        .await?;
        let mut js = async_nats::jetstream::new(client);
        js.set_timeout(Duration::from_millis(250));
        Ok(Self {
            _task: task,
            faults,
            js,
        })
    }
}
