//! Experimental ADR-42 wire adapter. async-nats 0.50 models the policy but does
//! not implement the pin handshake or expose `priority_timeout` / UNPIN.
//!
//! A pin controls delivery, not ownership of effects or ACKs. This adapter must
//! not be used as a fencing mechanism for production domain-event handlers.

use std::{io, time::Duration};

use async_nats::{Request, jetstream};
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use tokio::time::{Instant, timeout_at};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum End {
    Complete,
    Expired,
    PinLost,
    Busy,
}

pub struct Batch {
    pub messages: Vec<jetstream::Message>,
    pub end: End,
}

#[derive(Serialize)]
struct Pull<'a> {
    batch: usize,
    expires: u64,
    group: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a str>,
}

/// One reader per application instance and durable. &mut self prevents
/// overlapping pulls from losing pin updates within the same instance.
pub struct PinnedReader {
    context: jetstream::Context,
    stream: String,
    consumer: String,
    group: String,
    pin: Option<String>,
}

impl PinnedReader {
    pub fn new(context: jetstream::Context, stream: &str, durable: &str, group: &str) -> Self {
        Self {
            context,
            stream: stream.to_owned(),
            consumer: durable.to_owned(),
            group: group.to_owned(),
            pin: None,
        }
    }

    pub fn pin(&self) -> Option<&str> {
        self.pin.as_deref()
    }

    /// Finite pulls deliberately avoid background prefetch. `PinLost` may follow
    /// partial delivery; callers must not treat such a batch as completed work.
    /// Cancelling this future does not instantly cancel its server-side pull.
    pub async fn fetch(&mut self, count: usize, expires: Duration) -> Result<Batch> {
        if count == 0 || expires.is_zero() {
            return Err(io::Error::other("a pull needs a nonzero count and expiry").into());
        }
        let deadline = Instant::now()
            .checked_add(expires)
            .and_then(|deadline| deadline.checked_add(Duration::from_secs(2)))
            .ok_or_else(|| io::Error::other("pull deadline overflow"))?;
        let client = self.context.client();
        let inbox = client.new_inbox();
        let mut subscription = client.subscribe(inbox.clone()).await?;
        let payload = serde_json::to_vec(&Pull {
            batch: count,
            expires: nanos(expires)?,
            group: &self.group,
            id: self.pin(),
        })?;
        self.context
            .send_request(
                format!("CONSUMER.MSG.NEXT.{}.{}", self.stream, self.consumer),
                Request::new().inbox(inbox).payload(payload.into()),
            )
            .await?;

        let mut messages = Vec::new();
        while messages.len() < count {
            let message = timeout_at(deadline, subscription.next())
                .await?
                .ok_or_else(|| io::Error::other("pull subscription ended"))?;
            if let Some(status) = message.status {
                let description = message.description.as_deref().unwrap_or_default();
                let end = match (status.as_u16(), description) {
                    (100, _) => continue,
                    (408, _) => End::Expired,
                    (423, "Nats-Pin-Id mismatch" | "Nats-Wrong-Pin-Id") => {
                        self.pin = None;
                        End::PinLost
                    }
                    (409, "Exceeded MaxWaiting") => End::Busy,
                    _ => {
                        return Err(io::Error::other(format!(
                            "unexpected pull status {status}: {description}"
                        ))
                        .into());
                    }
                };
                return Ok(Batch { messages, end });
            }
            let pin = message
                .headers
                .as_ref()
                .and_then(|headers| headers.get("Nats-Pin-Id"))
                .map(async_nats::HeaderValue::as_str)
                .filter(|pin| !pin.is_empty())
                .ok_or_else(|| io::Error::other("pinned delivery has no Nats-Pin-Id"))?;
            if self.pin().is_some_and(|previous| previous != pin) {
                return Err(io::Error::other("pin changed inside a delivery batch").into());
            }
            self.pin = Some(pin.to_owned());
            let message = jetstream::Message {
                message,
                context: self.context.clone(),
            };
            let info = message.info()?;
            if info.stream != self.stream || info.consumer != self.consumer {
                return Err(io::Error::other("delivery belongs to another consumer").into());
            }
            messages.push(message);
        }
        Ok(Batch {
            messages,
            end: End::Complete,
        })
    }
}

/// Invoke only after the complete transaction's handlers have succeeded.
/// Confirming in delivery order leaves a prefix, rather than ACK holes, if the
/// process stops halfway through the ACK phase.
pub async fn acknowledge_processed(messages: &[jetstream::Message]) -> Result<()> {
    for message in messages {
        message.double_ack().await?;
    }
    Ok(())
}

pub fn nanos(duration: Duration) -> Result<u64> {
    Ok(u64::try_from(duration.as_nanos())?)
}

/// Administrative, group-wide operation, NOT a conditional release of this
/// reader's pin. A stale worker could unpin its successor. The demo calls this
/// from its coordinator only after the active worker has finished all work.
pub async fn admin_unpin(
    context: &jetstream::Context,
    stream: &str,
    durable: &str,
    group: &str,
) -> Result<()> {
    #[derive(Deserialize)]
    struct Unpinned {
        #[serde(rename = "type")]
        kind: String,
    }

    let response: jetstream::response::Response<Unpinned> = context
        .request(
            format!("CONSUMER.UNPIN.{stream}.{durable}"),
            &serde_json::json!({ "group": group }),
        )
        .await?;
    match response {
        jetstream::response::Response::Ok(response)
            if response.kind == "io.nats.jetstream.api.v1.consumer_unpin_response" =>
        {
            Ok(())
        }
        jetstream::response::Response::Err { error } => Err(error.into()),
        jetstream::response::Response::Ok(_) => {
            Err(io::Error::other("unexpected UNPIN response type").into())
        }
    }
}
