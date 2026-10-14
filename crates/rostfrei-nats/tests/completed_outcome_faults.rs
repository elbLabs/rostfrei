//! Actual authoritative-store `PubAck` loss (not a discarded Rust return value).
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use release_contract_support::*;
rostfrei::install_macro_support!();

struct Proxy(tokio::task::JoinHandle<TestResult>);
impl Drop for Proxy {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn drop_outcome_puback(
    stream: String,
) -> TestResult<(Proxy, std::net::SocketAddr, Arc<AtomicUsize>)> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = std::env::var("ROSTFREI_NATS_URL")?
        .rsplit('@')
        .next()
        .ok_or("fixture URL missing")?
        .to_owned();
    let dropped = Arc::new(AtomicUsize::new(0));
    let count = dropped.clone();
    let task = tokio::spawn(async move {
        let (client, _) = listener.accept().await?;
        let upstream = tokio::net::TcpStream::connect(upstream).await?;
        let (mut client_read, mut client_write) = client.into_split();
        let (upstream_read, mut upstream_write) = upstream.into_split();
        let upload = async {
            tokio::io::copy(&mut client_read, &mut upstream_write).await?;
            TestResult::Ok(())
        };
        let download = async {
            let mut reader = BufReader::new(upstream_read);
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
                    let mut body = vec![0; length.checked_add(2).ok_or("frame overflow")?];
                    reader.read_exact(&mut body).await?;
                    let ack = format!("\"stream\":\"{stream}\"");
                    if body
                        .windows(ack.len())
                        .any(|window| window == ack.as_bytes())
                        && count.load(Ordering::SeqCst) == 0
                    {
                        assert!(
                            body.windows(b"\"batch\"".len())
                                .any(|window| window == b"\"batch\""),
                            "must drop the FINAL atomic decision acknowledgement, not an intermediate reply"
                        );
                        count.fetch_add(1, Ordering::SeqCst);
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
    Ok((Proxy(task), address, dropped))
}

async fn uncertain(accepted: bool) -> TestResult {
    let fixture = Fixture::new(if accepted {
        "92-accepted-outcome-puback"
    } else {
        "92-rejected-outcome-puback"
    })
    .await?;
    let durable = fixture.event_store().await?;
    let (proxy, address, dropped) =
        drop_outcome_puback(durable.config().stream_name().to_owned()).await?;
    let client = async_nats::ConnectOptions::with_user_and_password(
        std::env::var("ROSTFREI_CONTRACT_NATS_USER")?,
        std::env::var("ROSTFREI_CONTRACT_NATS_PASSWORD")?,
    )
    .connect(format!("nats://{address}"))
    .await?;
    let proxied = Arc::new(
        NatsEventStore::connect(
            async_nats::jetstream::new(client.clone()),
            durable
                .config()
                .clone()
                .with_puback_timeout(Duration::from_millis(200))?,
        )
        .await?,
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let first = processor(&fixture.context, proxied, calls.clone(), None)?;
    let original = local_bus(&fixture.context, first.clone()).encode(request(
        "uncertain-outcome",
        "target",
        if accepted { "create" } else { "add" },
    )?)?;
    let response = first.process(&original).await?;
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "actual atomic outcome PubAck was dropped"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let recreated = processor(&fixture.context, durable.clone(), calls.clone(), None)?;
    if !accepted {
        local_bus(&fixture.context, recreated.clone())
            .dispatch(request("repair-after-uncertainty", "target", "create")?)
            .await?;
    }
    let before = calls.load(Ordering::SeqCst);
    assert_eq!(recreated.process(&original).await?, response);
    let _worker = fixture.command_worker(recreated).await?;
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    assert_eq!(
        *remote.dispatch_encoded(original).await?.response(),
        response
    );
    assert_eq!(calls.load(Ordering::SeqCst), before);
    assert_eq!(durable.load(&stream("target")?).await?.len(), 1);
    client.drain().await?;
    drop(proxy);
    Ok(())
}

#[tokio::test]
async fn accepted_atomic_outcome_puback_loss_reconciles_without_second_effect() -> TestResult {
    uncertain(true).await
}
#[tokio::test]
async fn rejected_atomic_outcome_puback_loss_reconciles_without_reevaluation() -> TestResult {
    uncertain(false).await
}
