use std::{collections::BTreeMap, sync::Arc};

use rostfrei_messaging_core::TrafficScope;
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsCorrelationHistory, NatsEventStore, NatsEventStoreConfig,
};

use super::{NatsHostError, NatsRuntime, NatsRuntimeConfig, TracerConfig};

/// Validate before starting workers. A context identifies exactly one retained
/// domain source; silently replacing a conflicting source would hide evidence.
pub(super) fn sources(
    tracer: &TracerConfig,
    test: &NatsRuntimeConfig,
) -> Result<Vec<NatsEventStoreConfig>, NatsHostError> {
    let mut sources = BTreeMap::new();
    for config in tracer.production_event_stores.iter().chain(
        tracer
            .dispatch
            .iter()
            .map(|runtime| &runtime.config.event_store),
    ) {
        if config.application() != test.context.application()
            || config.traffic_scope() != TrafficScope::Normal
        {
            return Err(NatsHostError::Configuration(
                "production inspection sources must belong to the host application in Normal scope"
                    .into(),
            ));
        }
        let name = config.bounded_context().as_str().to_owned();
        if let Some(previous) = sources.insert(name.clone(), config.clone())
            && previous != *config
        {
            return Err(NatsHostError::Configuration(format!(
                "conflicting production inspection sources for context `{name}`"
            )));
        }
    }
    Ok(sources.into_values().collect())
}

pub(super) async fn attach(
    mut builder: crate::TracerBuilder,
    test: &NatsRuntime,
    dispatch: Option<&NatsRuntime>,
    configs: Vec<NatsEventStoreConfig>,
) -> Result<crate::TracerBuilder, NatsHostError> {
    if configs.is_empty() {
        return Ok(builder);
    }
    let defaults = ApplicationMessagingConfig::new(test.config.context.application())?;
    let topology = dispatch.map_or_else(
        || defaults.topology(),
        |runtime| runtime.config.messaging.topology(),
    );
    let connection = dispatch.map_or(&test.connection, |runtime| &runtime.connection);
    let reader =
        NatsCorrelationHistory::connect(connection.jetstream().clone(), configs.clone(), topology)
            .await?;
    for config in configs {
        let context = config.bounded_context().as_str().to_owned();
        let history = NatsEventStore::connect(connection.jetstream().clone(), config).await?;
        builder = builder.with_production_history(context, Arc::new(history));
    }
    Ok(builder.with_correlation_history(Arc::new(reader)))
}
