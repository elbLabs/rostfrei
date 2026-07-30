use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use async_nats::jetstream;
use futures_util::{StreamExt as _, stream::FuturesUnordered};
use rostfrei::{ReadModel, ReadModelRuntime};
use rostfrei_messaging_core::{
    ConsumeError, ConsumerConfig, IntegrationEventAddress, MessageConsumer, MessageConsumerFactory,
    MessageHandler, RetryDelay,
};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::watch;

use crate::{
    DomainEventConsumerError, MessagingTopology, NatsConsumerFactory, NatsDomainEventConsumer,
    NatsDomainEventConsumerConfig, NatsError, NatsEventStoreConfig,
    provision_domain_event_consumer, provision_durable_consumer,
};

/// Delivery policy shared by one model's independently checkpointed consumers.
#[derive(Clone, Debug)]
pub struct NatsReadModelConsumerOptions {
    pub ack_wait: Duration,
    pub processing_timeout: Duration,
    pub domain_retry_delay: Duration,
    pub maximum_integration_attempts: u32,
}

impl Default for NatsReadModelConsumerOptions {
    fn default() -> Self {
        Self {
            ack_wait: Duration::from_secs(30),
            processing_timeout: Duration::from_secs(10),
            domain_retry_delay: Duration::from_millis(100),
            maximum_integration_attempts: 10,
        }
    }
}

#[derive(Debug, Error)]
pub enum ReadModelWorkerError {
    #[error("invalid read-model consumer configuration: {0}")]
    Configuration(String),
    #[error(transparent)]
    Nats(#[from] NatsError),
    #[error(transparent)]
    Domain(#[from] DomainEventConsumerError),
    #[error(transparent)]
    Integration(#[from] ConsumeError),
    #[error("read-model consumer stopped before shutdown")]
    Ended,
}

type IntegrationWorker = (
    Arc<dyn MessageConsumer<IntegrationEventAddress>>,
    Arc<dyn MessageHandler<IntegrationEventAddress>>,
);

/// Runs the registered event subscriptions. Successful handlers have persisted
/// state before ACK; integration quarantine still requires explicit recovery.
pub struct NatsReadModelWorker {
    domain: Option<NatsDomainEventConsumer>,
    integrations: Vec<IntegrationWorker>,
}

impl NatsReadModelWorker {
    pub async fn connect<M: ReadModel>(
        context: jetstream::Context,
        model: &ReadModelRuntime<M>,
        history: &NatsEventStoreConfig,
        topology: &MessagingTopology,
        options: &NatsReadModelConsumerOptions,
    ) -> Result<Self, ReadModelWorkerError> {
        validate_scope(model, history, topology)?;
        let domain = if model.has_domain_events() {
            Some(
                NatsDomainEventConsumer::connect(
                    context.clone(),
                    history.clone(),
                    domain_config(model, options)?,
                    model.domain_dispatcher(),
                )
                .await?,
            )
        } else {
            None
        };
        let factory = NatsConsumerFactory::new(context, topology.clone());
        let mut integrations = Vec::new();
        for binding in model.integration_bindings() {
            let config =
                integration_config(model, binding.address(), binding.schema_version(), options)?;
            integrations.push((factory.create(config)?, binding.handler()));
        }
        Ok(Self {
            domain,
            integrations,
        })
    }

    pub async fn run_until_shutdown(
        self,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ReadModelWorkerError> {
        type Job = Pin<Box<dyn Future<Output = Result<(), ReadModelWorkerError>> + Send>>;
        if *shutdown.borrow() {
            return Ok(());
        }
        let mut jobs: FuturesUnordered<Job> = FuturesUnordered::new();
        if let Some(domain) = self.domain {
            let shutdown = shutdown.clone();
            jobs.push(Box::pin(async move {
                domain
                    .run_until_shutdown(shutdown)
                    .await
                    .map_err(Into::into)
            }));
        }
        for (consumer, handler) in self.integrations {
            jobs.push(Box::pin(async move {
                consumer.run(handler).await.map_err(Into::into)
            }));
        }
        loop {
            tokio::select! {
                biased;
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { return Ok(()); }
                }
                result = jobs.next() => {
                    if *shutdown.borrow() { return Ok(()); }
                    return match result { Some(Err(error)) => Err(error), _ => Err(ReadModelWorkerError::Ended) };
                }
            }
        }
    }
}

/// Explicitly provisions one domain durable and one durable per integration
/// registration. Each read model has independent delivery/recovery progress.
pub async fn provision_read_model_consumers<M: ReadModel>(
    context: &jetstream::Context,
    model: &ReadModelRuntime<M>,
    history: &NatsEventStoreConfig,
    topology: &MessagingTopology,
    options: &NatsReadModelConsumerOptions,
) -> Result<(), ReadModelWorkerError> {
    validate_scope(model, history, topology)?;
    if model.has_domain_events() {
        provision_domain_event_consumer(context, history, &domain_config(model, options)?).await?;
    }
    for binding in model.integration_bindings() {
        provision_durable_consumer(
            context,
            topology,
            &integration_config(model, binding.address(), binding.schema_version(), options)?,
        )
        .await?;
    }
    Ok(())
}

fn validate_scope<M: ReadModel>(
    model: &ReadModelRuntime<M>,
    history: &NatsEventStoreConfig,
    topology: &MessagingTopology,
) -> Result<(), ReadModelWorkerError> {
    let scope = model.context();
    let history_matches = (
        history.application(),
        history.bounded_context(),
        history.traffic_scope(),
    ) == (scope.application(), scope.name(), scope.traffic_scope());
    if topology.application() != scope.application()
        || topology.traffic_scope() != scope.traffic_scope()
        || !history_matches
    {
        return Err(ReadModelWorkerError::Configuration(
            "read-model worker must use the model's application, context and traffic scope"
                .to_owned(),
        ));
    }
    Ok(())
}

fn domain_config<M: ReadModel>(
    model: &ReadModelRuntime<M>,
    options: &NatsReadModelConsumerOptions,
) -> Result<NatsDomainEventConsumerConfig, ReadModelWorkerError> {
    let name = consumer_purpose(M::NAME, "domain", M::SCHEMA_VERSION);
    let invalid = |error: rostfrei_messaging_core::ContractError| {
        ReadModelWorkerError::Configuration(error.to_string())
    };
    Ok(NatsDomainEventConsumerConfig::new(
        model
            .context()
            .consumer_name(&name, M::SCHEMA_VERSION)
            .map_err(invalid)?,
        model
            .context()
            .durable_name(&name, M::SCHEMA_VERSION)
            .map_err(invalid)?,
        options.ack_wait,
        options.processing_timeout,
        RetryDelay::new(options.domain_retry_delay).map_err(invalid)?,
    )?)
}

fn integration_config<M: ReadModel>(
    model: &ReadModelRuntime<M>,
    address: &IntegrationEventAddress,
    schema: u32,
    options: &NatsReadModelConsumerOptions,
) -> Result<ConsumerConfig<IntegrationEventAddress>, ReadModelWorkerError> {
    let name = consumer_purpose(M::NAME, address.as_str(), schema);
    let invalid = |error: rostfrei_messaging_core::ContractError| {
        ReadModelWorkerError::Configuration(error.to_string())
    };
    ConsumerConfig::new(
        model
            .context()
            .consumer_name(&name, M::SCHEMA_VERSION)
            .map_err(invalid)?,
        model
            .context()
            .durable_name(&name, M::SCHEMA_VERSION)
            .map_err(invalid)?,
        address.clone(),
        options.ack_wait,
        options.processing_timeout,
        1,
        options.maximum_integration_attempts,
    )
    .map_err(invalid)
}

fn consumer_purpose(model: &str, source: &str, schema: u32) -> String {
    // A purpose is itself a <=64-byte scope segment. Hash the full identity so
    // truncating its human-readable prefix cannot alias two valid model names.
    let prefix: String = model.chars().take(19).collect();
    let prefix = prefix.trim_end_matches('-');
    let digest = crate::hex::encode_lower_hex(Sha256::digest(format!("{model}:{source}:{schema}")));
    let suffix: String = digest.chars().take(32).collect();
    format!("read-model-{prefix}-{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumer_names_accept_maximum_scope_names_without_aliasing_sources() {
        let application = rostfrei_messaging_core::ApplicationName::new("a".repeat(64)).unwrap();
        let context = application.test_bounded_context("b".repeat(64)).unwrap();
        let first = format!("{}-one", "c".repeat(60));
        let second = format!("{}-two", "c".repeat(60));
        let domain = consumer_purpose(&first, "domain", u32::MAX);
        context.consumer_name(&domain, u32::MAX).unwrap();
        let integration = consumer_purpose(&first, "app.integration.billing.changed", u32::MAX);
        context.durable_name(&integration, u32::MAX).unwrap();
        assert_ne!(domain, consumer_purpose(&second, "domain", u32::MAX));
        assert_ne!(domain, integration);
        assert_ne!(
            integration,
            consumer_purpose(&first, "app.integration.billing.changed", 1)
        );
    }
}
