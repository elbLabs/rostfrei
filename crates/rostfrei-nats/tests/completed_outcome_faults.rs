//! Actual authoritative-store `PubAck` loss (not a discarded Rust return value).
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
mod terminal_puback_support;
use release_contract_support::*;
use terminal_puback_support::TerminalPubAckLossProxy;
rostfrei_runtime::install_macro_support!();

async fn uncertain(accepted: bool) -> TestResult {
    let fixture = Fixture::new(if accepted {
        "92-accepted-outcome-puback"
    } else {
        "92-rejected-outcome-puback"
    })
    .await?;
    let durable = fixture.split_event_store().await?;
    let proxy = TerminalPubAckLossProxy::start(durable.config().stream_name().to_owned()).await?;
    let client = async_nats::ConnectOptions::with_user_and_password(
        std::env::var("ROSTFREI_CONTRACT_NATS_USER")?,
        std::env::var("ROSTFREI_CONTRACT_NATS_PASSWORD")?,
    )
    .connect(format!("nats://{}", proxy.address))
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
        proxy.dropped.load(Ordering::SeqCst),
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
