use std::{collections::BTreeMap, sync::Arc};

use async_nats::jetstream::{
    self,
    consumer::{self, PullConsumer},
};
use futures_util::TryStreamExt as _;
use rostfrei::{ReadModel, ReadModelRuntime};
use rostfrei_messaging_core::{
    ConsumeError, ConsumeErrorKind, ConsumerConfig, IntegrationEventAddress, MessageHandler,
};

use crate::{
    MessagingTopology, NatsError, NatsReadModelConsumerOptions, ReadModelWorkerError,
    consumer::{process_integration_message, verify_consumer_settings},
    provisioning::durable_consumer_config,
    read_model_worker::consumer_purpose,
};

struct Route {
    config: ConsumerConfig<IntegrationEventAddress>,
    handler: Arc<dyn MessageHandler<IntegrationEventAddress>>,
}

struct Configuration {
    durable: String,
    expected: consumer::pull::Config,
    routes: BTreeMap<String, Route>,
}

/// One ordered durable for all integration inputs of a read model.
pub struct ReadModelIntegrationConsumer {
    context: jetstream::Context,
    topology: MessagingTopology,
    configuration: Configuration,
}

impl ReadModelIntegrationConsumer {
    pub async fn connect<M: ReadModel>(
        context: jetstream::Context,
        model: &ReadModelRuntime<M>,
        topology: &MessagingTopology,
        options: &NatsReadModelConsumerOptions,
    ) -> Result<Option<Self>, ReadModelWorkerError> {
        let Some(configuration) = configuration(model, options)? else {
            return Ok(None);
        };
        let result = Self {
            context,
            topology: topology.clone(),
            configuration,
        };
        result.consumer().await?;
        Ok(Some(result))
    }

    async fn consumer(&self) -> Result<PullConsumer, ConsumeError> {
        let stream = self
            .context
            .get_stream(self.topology.integration_event_stream().as_str())
            .await
            .map_err(|_| ConsumeError::new(ConsumeErrorKind::Unavailable))?;
        let consumer: PullConsumer = stream
            .get_consumer(&self.configuration.durable)
            .await
            .map_err(|_| ConsumeError::new(ConsumeErrorKind::Unavailable))?;
        verify_consumer_settings(
            &consumer.cached_info().name,
            &consumer.cached_info().config,
            &self.configuration.expected,
        )?;
        Ok(consumer)
    }

    pub async fn run(self) -> Result<(), ConsumeError> {
        let consumer = self.consumer().await?;
        let mut messages = consumer
            .stream()
            .max_messages_per_batch(1)
            .messages()
            .await
            .map_err(|_| ConsumeError::new(ConsumeErrorKind::Unavailable))?;
        loop {
            let next = messages
                .try_next()
                .await
                .map_err(|_| ConsumeError::new(ConsumeErrorKind::Unavailable))?;
            let Some(message) = next else {
                break;
            };
            let route = self
                .configuration
                .routes
                .get(message.subject.as_str())
                .ok_or_else(|| ConsumeError::new(ConsumeErrorKind::InvalidConfiguration))?;
            process_integration_message(
                &self.context,
                &self.topology,
                &route.config,
                route.handler.clone(),
                message,
            )
            .await?;
        }
        Err(ConsumeError::new(ConsumeErrorKind::Ended))
    }
}

pub async fn provision<M: ReadModel>(
    context: &jetstream::Context,
    model: &ReadModelRuntime<M>,
    topology: &MessagingTopology,
    options: &NatsReadModelConsumerOptions,
) -> Result<(), ReadModelWorkerError> {
    let Some(configuration) = configuration(model, options)? else {
        return Ok(());
    };
    let consumer = context
        .create_consumer_on_stream(
            configuration.expected.clone(),
            topology.integration_event_stream().as_str(),
        )
        .await
        .map_err(|_| NatsError::Provisioning)?;
    verify_consumer_settings(
        &consumer.cached_info().name,
        &consumer.cached_info().config,
        &configuration.expected,
    )?;
    Ok(())
}

fn configuration<M: ReadModel>(
    model: &ReadModelRuntime<M>,
    options: &NatsReadModelConsumerOptions,
) -> Result<Option<Configuration>, ReadModelWorkerError> {
    let purpose = consumer_purpose(M::NAME, "ordered-integration", M::SCHEMA_VERSION);
    let invalid = |error: rostfrei_messaging_core::ContractError| {
        ReadModelWorkerError::Configuration(error.to_string())
    };
    let name = model
        .context()
        .consumer_name(&purpose, M::SCHEMA_VERSION)
        .map_err(invalid)?;
    let durable = model
        .context()
        .durable_name(&purpose, M::SCHEMA_VERSION)
        .map_err(invalid)?;
    let mut routes = BTreeMap::new();
    for binding in model.integration_bindings() {
        let config = ConsumerConfig::new(
            name.clone(),
            durable.clone(),
            binding.address().clone(),
            options.ack_wait,
            options.processing_timeout,
            1,
            options.maximum_integration_attempts,
        )
        .map_err(invalid)?;
        routes.insert(
            binding.address().as_str().to_owned(),
            Route {
                config,
                handler: binding.handler(),
            },
        );
    }
    let Some(first) = routes.values().next() else {
        return Ok(None);
    };
    let mut expected = durable_consumer_config(&first.config)?;
    if routes.len() > 1 {
        expected.filter_subject.clear();
        expected.filter_subjects = routes.keys().cloned().collect();
    }
    Ok(Some(Configuration {
        durable: durable.as_str().to_owned(),
        expected,
        routes,
    }))
}
