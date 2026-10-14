#![allow(
    clippy::panic_in_result_fn,
    reason = "test assertions report protocol failures while Result propagates fixture errors"
)]

use std::time::Duration;

use super::{End, Fixture, Result, acknowledge_processed, demonstrate_handoff, payloads};

async fn fixture(pin_ttl: Duration, ack_wait: Duration) -> Result<Fixture> {
    Fixture::create(
        &rostfrei_testing::integration::nats_url()?,
        pin_ttl,
        ack_wait,
    )
    .await
}

#[tokio::test]
async fn standby_preserves_split_pull_transaction_and_takes_over_existing_backlog() -> Result<()> {
    let fixture = fixture(Duration::from_secs(30), Duration::from_secs(10)).await?;
    let result = demonstrate_handoff(&fixture).await;
    fixture.cleanup().await?;
    result
}

#[tokio::test]
async fn explicit_ack_of_last_event_does_not_confirm_earlier_events() -> Result<()> {
    let fixture = fixture(Duration::from_secs(30), Duration::from_secs(10)).await?;
    fixture.publish(&["one", "two", "three"]).await?;
    let mut reader = fixture.reader().await?;
    let batch = reader.fetch(3, Duration::from_secs(1)).await?;
    assert_eq!(batch.end, End::Complete);
    let (last, prefix) = batch.messages.split_last().ok_or("missing batch")?;
    last.double_ack().await?;
    let info = fixture.info().await?;
    assert_eq!(info.num_ack_pending, 2);
    assert_eq!(info.ack_floor.stream_sequence, 0);
    acknowledge_processed(prefix).await?;
    let info = fixture.info().await?;
    assert_eq!(info.num_ack_pending, 0);
    assert_eq!(info.ack_floor.stream_sequence, 3);
    fixture.cleanup().await
}

#[tokio::test]
async fn takeover_redelivers_only_the_unacknowledged_suffix() -> Result<()> {
    let fixture = fixture(Duration::from_secs(30), Duration::from_millis(300)).await?;
    fixture.publish(&["one", "two", "three"]).await?;
    let mut old = fixture.reader().await?;
    let batch = old.fetch(3, Duration::from_secs(1)).await?;
    assert_eq!(batch.end, End::Complete);
    let (first, _) = batch.messages.split_first().ok_or("missing batch")?;
    first.double_ack().await?;
    assert_eq!(fixture.info().await?.ack_floor.stream_sequence, 1);
    drop(old);
    fixture.unpin().await?;

    let mut new = fixture.reader().await?;
    let replay = new.fetch(2, Duration::from_secs(2)).await?;
    assert_eq!(replay.end, End::Complete);
    assert_eq!(payloads(&replay.messages)?, ["two", "three"]);
    for message in &replay.messages {
        assert!(message.info()?.delivered > 1);
    }
    acknowledge_processed(&replay.messages).await?;
    assert_eq!(fixture.info().await?.ack_floor.stream_sequence, 3);
    fixture.cleanup().await
}

#[tokio::test]
async fn ttl_takeover_rejects_stale_pulls_but_does_not_fence_old_acks() -> Result<()> {
    let fixture = fixture(Duration::from_secs(1), Duration::from_secs(10)).await?;
    fixture.publish(&["one", "two"]).await?;
    let mut old = fixture.reader().await?;
    let first = old.fetch(1, Duration::from_millis(100)).await?;
    assert_eq!(first.end, End::Complete);
    let previous_pin = old.pin().ok_or("old reader has no pin")?.to_owned();

    let mut new = fixture.reader().await?;
    let next = new.fetch(1, Duration::from_secs(2)).await?;
    assert_eq!(next.end, End::Complete);
    assert_eq!(payloads(&next.messages)?, ["two"]);
    assert_ne!(new.pin(), Some(previous_pin.as_str()));
    assert_eq!(fixture.info().await?.num_ack_pending, 2);

    // Delivery of event two was allowed while event one remained unfinished.
    // A stale pull is rejected, but this does NOT fence an old ACK or an effect.
    fixture.publish(&["three"]).await?;
    let stale = old.fetch(1, Duration::from_millis(100)).await?;
    assert_eq!(stale.end, End::PinLost);
    assert!(stale.messages.is_empty());
    assert!(old.pin().is_none());
    acknowledge_processed(&first.messages).await?;
    assert_eq!(fixture.info().await?.ack_floor.stream_sequence, 1);

    acknowledge_processed(&next.messages).await?;
    let last = new.fetch(1, Duration::from_millis(100)).await?;
    assert_eq!(last.end, End::Complete);
    assert_eq!(payloads(&last.messages)?, ["three"]);
    acknowledge_processed(&last.messages).await?;

    // On a quiet stream, a cleared pin rejoins as a normal standby (408),
    // rather than turning the worker's lack of ownership into a fatal error.
    let standby = old.fetch(1, Duration::from_millis(50)).await?;
    assert_eq!(standby.end, End::Expired);
    assert!(standby.messages.is_empty());
    fixture.cleanup().await
}
