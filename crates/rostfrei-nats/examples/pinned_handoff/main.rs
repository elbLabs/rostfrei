//! Runnable protocol prototype for overlapping domain-event workers.
//! Experimental pinned-consumer protocol; production uses the ordered worker pool.

mod fixture;
mod protocol;

#[cfg(test)]
mod tests;

use std::time::Duration;

use fixture::{Fixture, payloads, require};
use protocol::{End, Result, acknowledge_processed};

#[tokio::main]
async fn main() -> Result<()> {
    let url = rostfrei_testing::integration::nats_url()?;
    let fixture = Fixture::create(&url, Duration::from_secs(30), Duration::from_secs(10)).await?;
    let result = demonstrate_handoff(&fixture).await;
    fixture.cleanup().await?;
    result
}

/// Simulates the existing consumer's first-event pull followed by the rest of
/// its transaction. Payloads label illustrative commits; this is a wire-level
/// example, not Rostfrei's transaction validation or publication pipeline.
async fn demonstrate_handoff(fixture: &Fixture) -> Result<()> {
    fixture
        .publish(&["commit-a/1", "commit-a/2", "commit-a/3", "commit-b/1"])
        .await?;
    let mut old = fixture.reader().await?;
    let mut new = fixture.reader().await?;
    let mut first = old.fetch(1, Duration::from_secs(1)).await?;
    require(
        first.end == End::Complete,
        "old worker did not receive its first event",
    )?;

    // Keep the future in this scope so errors cancel the demo's standby too.
    let standby = new.fetch(1, Duration::from_secs(5));
    tokio::pin!(standby);
    tokio::select! {
        result = &mut standby => {
            result?;
            return Err(std::io::Error::other("standby completed before handoff").into());
        }
        result = async {
            fixture.wait_for_standby().await?;
            let rest = old.fetch(2, Duration::from_secs(1)).await?;
            require(rest.end == End::Complete, "old worker lost the transaction tail")?;
            first.messages.extend(rest.messages);
            require(
                payloads(&first.messages)? == ["commit-a/1", "commit-a/2", "commit-a/3"],
                "transaction was split or reordered",
            )?;
            println!("old worker processes {:?}", payloads(&first.messages)?);
            acknowledge_processed(&first.messages).await?;
            require(fixture.info().await?.num_ack_pending == 0, "ACKs still pending")
        } => result?,
    }
    // A coordinator performs the administrative switch in this demo. Do this
    // after the select: delivery may arrive before the UNPIN response itself.
    // Never copy this into an unconditional worker shutdown hook.
    fixture.unpin().await?;
    let next = tokio::time::timeout(Duration::from_secs(2), &mut standby).await??;
    require(next.end == End::Complete, "standby did not take over")?;
    require(
        payloads(&next.messages)? == ["commit-b/1"],
        "incorrect handoff position",
    )?;
    println!("new worker processes {:?}", payloads(&next.messages)?);
    acknowledge_processed(&next.messages).await?;
    println!(
        "handoff complete; ACK floor = {}",
        fixture.info().await?.ack_floor.stream_sequence
    );
    Ok(())
}
