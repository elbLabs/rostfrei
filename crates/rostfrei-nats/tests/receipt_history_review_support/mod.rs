use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_nats::jetstream;
use futures_util::StreamExt;
use serde_json::Value;

use crate::correlation_history_support::TestResult;

/// Relay real `JetStream` APIs, changing only the consumer's reported pending
/// count. Delay forwarding pulls briefly so progress INFO also reports empty
/// before delivery; the actual messages still come directly from the broker.
pub struct PendingEmptyProxy {
    pub context: jetstream::Context,
    rewritten: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl PendingEmptyProxy {
    pub async fn start(client: async_nats::Client) -> TestResult<Self> {
        let prefix = format!("RECEIPT_REVIEW.{}", client.new_inbox());
        let mut requests = client.subscribe(format!("{prefix}.>")).await?;
        client.flush().await?;
        let context = jetstream::with_prefix(client.clone(), &prefix);
        let rewritten = Arc::new(AtomicUsize::new(0));
        let counter = rewritten.clone();
        let task = tokio::spawn(async move {
            let mut forwards = tokio::task::JoinSet::new();
            while let Some(request) = requests.next().await {
                let Some(suffix) = request.subject.as_str().strip_prefix(&format!("{prefix}."))
                else {
                    continue;
                };
                let Some(reply) = request.reply else { continue };
                let target = format!("$JS.API.{suffix}");
                let client = client.clone();
                let counter = counter.clone();
                let pull = suffix.starts_with("CONSUMER.MSG.NEXT.");
                let consumer_info =
                    suffix.starts_with("CONSUMER.CREATE.") || suffix.starts_with("CONSUMER.INFO.");
                forwards.spawn(async move {
                    if pull {
                        tokio::time::sleep(std::time::Duration::from_millis(650)).await;
                        client
                            .publish_with_reply(target, reply, request.payload)
                            .await?;
                    } else {
                        let response = client.request(target, request.payload).await?;
                        let mut payload = response.payload;
                        if consumer_info {
                            let mut info: Value = serde_json::from_slice(&payload)?;
                            if let Some(pending) = info.get_mut("num_pending") {
                                *pending = Value::from(0);
                                counter.fetch_add(1, Ordering::SeqCst);
                                payload = serde_json::to_vec(&info)?.into();
                            }
                        }
                        client.publish(reply, payload).await?;
                    }
                    TestResult::Ok(())
                });
                while forwards.try_join_next().is_some() {}
            }
        });
        Ok(Self {
            context,
            rewritten,
            task,
        })
    }

    pub fn rewritten(&self) -> usize {
        self.rewritten.load(Ordering::SeqCst)
    }
}

impl Drop for PendingEmptyProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}
