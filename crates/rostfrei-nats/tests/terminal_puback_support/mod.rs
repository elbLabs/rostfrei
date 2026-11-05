//! Lose one real FINAL atomic-batch acknowledgement while forwarding every other frame.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct TerminalPubAckLossProxy {
    task: tokio::task::JoinHandle<TestResult>,
    pub address: std::net::SocketAddr,
    pub dropped: Arc<AtomicU64>,
}

impl Drop for TerminalPubAckLossProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TerminalPubAckLossProxy {
    #[allow(
        clippy::panic_in_result_fn,
        reason = "assert the intercepted acknowledgement is at the named FINAL boundary"
    )]
    pub async fn start(stream: String) -> TestResult<Self> {
        let upstream: async_nats::ServerAddr =
            rostfrei_testing::integration::nats_url()?.parse()?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let dropped = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&dropped);
        let task = tokio::spawn(async move {
            let (client, _) = listener.accept().await?;
            let upstream =
                tokio::net::TcpStream::connect((upstream.host(), upstream.port())).await?;
            client.set_nodelay(true)?;
            upstream.set_nodelay(true)?;
            let (mut client_read, mut client_write) = client.into_split();
            let (upstream_read, mut upstream_write) = upstream.into_split();
            let upload = async {
                tokio::io::copy(&mut client_read, &mut upstream_write).await?;
                TestResult::Ok(())
            };
            let download = async {
                let mut reader = BufReader::new(upstream_read);
                let ack = format!("\"stream\":\"{stream}\"");
                loop {
                    let mut line = Vec::new();
                    if reader.read_until(b'\n', &mut line).await? == 0 {
                        return TestResult::Ok(());
                    }
                    let text = std::str::from_utf8(&line)?;
                    if text.starts_with("MSG ") || text.starts_with("HMSG ") {
                        let length: usize = text
                            .split_whitespace()
                            .last()
                            .ok_or("missing frame length")?
                            .parse()?;
                        let mut body =
                            vec![0; length.checked_add(2).ok_or("frame length overflow")?];
                        reader.read_exact(&mut body).await?;
                        if body
                            .windows(ack.len())
                            .any(|window| window == ack.as_bytes())
                            && counter.load(Ordering::SeqCst) == 0
                        {
                            assert!(
                                body.windows(b"\"batch\"".len())
                                    .any(|window| window == b"\"batch\""),
                                "must drop the FINAL atomic-batch PubAck, not an intermediate reply"
                            );
                            counter.fetch_add(1, Ordering::SeqCst);
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
        Ok(Self {
            task,
            address,
            dropped,
        })
    }
}
