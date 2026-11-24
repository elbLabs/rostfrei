#![allow(dead_code, unused_imports)]

// A minimal rental command for framework transport contracts, never an application binding.
mod command;
mod handler;
pub use command::RentBicycle;
pub use handler::RentBicycleHandler;

use bike_rental::{BikeRentalNatsConfig, BikeRentalNatsError};
use rostfrei::nats::NatsConnection;
use rostfrei::tracer::{NatsRuntime, NatsRuntimeConfig, TestScenarioReset, TestScenarioResetError};
use std::sync::Arc;

pub mod tracer {
    use super::{RentBicycle, RentBicycleHandler};
    use rostfrei::tracer::{ApplicationBindingError, ApplicationBindings, TracerBuilder};
    use rostfrei::{CommandRejectionClassification, EventHistory, JsonDomainRejectionMapper};
    use std::sync::Arc;

    pub fn command_bindings() -> Result<ApplicationBindings, ApplicationBindingError> {
        let mut bindings = bike_rental::tracer::command_bindings()?;
        bindings.register_command::<RentBicycle, _>(
            RentBicycleHandler,
            JsonDomainRejectionMapper::new(CommandRejectionClassification::Conflict),
        )?;
        Ok(bindings)
    }

    pub fn bindings() -> Result<ApplicationBindings, Box<dyn std::error::Error + Send + Sync>> {
        Ok(command_bindings()?.with_domain_model(bike_rental::domain_model()?))
    }

    pub fn builder(
        history: Arc<dyn EventHistory>,
    ) -> Result<TracerBuilder, ApplicationBindingError> {
        Ok(TracerBuilder::new(
            history,
            command_bindings()?.registry().clone(),
        ))
    }
}

/// Extend the Rental context only, preserving the application's isolated stores,
/// resource limits and bindings for Finance and Workshop.
pub fn rental_with_direct_command(
    config: &NatsRuntimeConfig,
) -> Result<
    (
        NatsRuntimeConfig,
        Arc<rostfrei::tracer::ApplicationBindings>,
    ),
    BikeRentalNatsError,
> {
    let bindings = Arc::new(
        tracer::bindings().map_err(|err| BikeRentalNatsError::Configuration(err.to_string()))?,
    );
    let mut runtime = config.clone();
    let address = config
        .context
        .command_address(<RentBicycle as rostfrei::Command>::LOCAL_ID)?;
    runtime.commands.extend(
        NatsRuntimeConfig::new(config.context.clone(), &bindings, 2)?
            .commands
            .into_iter()
            .filter(|route| route.address() == &address),
    );
    Ok((runtime, bindings))
}

pub fn application_with_direct_command(
    configs: &bike_rental::tracer::ContextConfigurations,
    connection: NatsConnection,
) -> Result<rostfrei::tracer::NatsApplicationBuilder, BikeRentalNatsError> {
    let (rental, bindings) = rental_with_direct_command(configs.rental().host_config())?;
    let mut builder =
        rostfrei::tracer::NatsApplication::builder(rental.context.clone(), connection, bindings)?
            .with_runtime_config(rental);
    for (config, bindings) in configs.contexts() {
        if config.context.name().as_str() != bike_rental::RENTAL_CONTEXT {
            builder = builder.with_context(config.clone(), bindings.clone());
        }
    }
    Ok(builder)
}

pub struct BikeRentalNatsRuntime {
    config: BikeRentalNatsConfig,
    inner: Arc<NatsRuntime>,
}

impl BikeRentalNatsRuntime {
    pub async fn provision_config(
        connection: NatsConnection,
        config: BikeRentalNatsConfig,
    ) -> Result<Self, BikeRentalNatsError> {
        let (runtime, bindings) = rental_with_direct_command(config.host_config())?;
        let inner =
            Arc::new(Box::pin(NatsRuntime::provision(connection, runtime, bindings)).await?);
        Ok(Self { config, inner })
    }

    pub const fn config(&self) -> &BikeRentalNatsConfig {
        &self.config
    }

    pub fn runtime_config(&self) -> &NatsRuntimeConfig {
        self.inner.config()
    }
}

impl std::ops::Deref for BikeRentalNatsRuntime {
    type Target = NatsRuntime;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[async_trait::async_trait]
impl TestScenarioReset for BikeRentalNatsRuntime {
    async fn reset(
        &self,
        fixture: &rostfrei::fixtures::Fixture,
    ) -> Result<(), TestScenarioResetError> {
        self.inner.reset(fixture).await
    }
}
