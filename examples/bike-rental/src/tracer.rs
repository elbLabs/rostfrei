use std::sync::Arc;

use rostfrei::nats::NatsConnection;
use rostfrei::tracer::{
    ApplicationBindingError, ApplicationBindings, NatsApplication, NatsApplicationBuilder,
    NatsRuntime, NatsRuntimeConfig, TracerConfig,
};
use rostfrei::tracer::{CommandInputField, CommandInputOption, CommandInputOptions, TracerBuilder};
use rostfrei::{
    CommandRejectionClassification, EventHistory, JsonDomainRejectionMapper, TrafficScope,
};

use crate::payments::{
    ChargeRecordedPayment, CollectionPaymentConflictIntegrationMapper, CollectionPaymentDecided,
    CollectionPaymentDecisionIntegrationMapper, CollectionPaymentIntegrationMapper,
    CollectionPaymentRequested, PaymentProvider, ProcessRequestedPayment,
    RentalPaymentChargeIntegrationMapper, RentalPaymentChargeReady, RentalReturnAssessed,
    RentalReturnIntegrationMapper, ResolveDecidedCollection, SandboxPaymentProvider,
    SettleReturnedRental,
};
use crate::rental_fleet::{
    BicycleCollectionRequested, CollectReservedBicycle, CollectReservedBicycleHandler,
    RegisterBicycle, RegisterBicycleHandler, RentalFleetAggregate, ReserveBicycleCategory,
    ReserveBicycleCategoryHandler, ReservedBicycleReturned, ResolveBicycleCollection,
    ResolveBicycleCollectionHandler, ReturnBicycle, ReturnBicycleHandler, TransferBicycle,
    TransferBicycleHandler,
};
use crate::rental_payment::{
    AssessRentalReturn, AssessRentalReturnHandler, CollectRentalPayment,
    CollectRentalPaymentHandler, RecordRentalPaymentRequest, RecordRentalPaymentRequestHandler,
    RefundEligibleRentalDeposit, RefundEligibleRentalDepositHandler, RentalPaymentAggregate,
    RentalPaymentApproved, RentalPaymentDeclined, RentalPaymentIdentityConflictRecorded,
    RentalPaymentRequested,
};
use crate::rental_tariff::{
    ConfigureRentalTariff, ConfigureRentalTariffHandler, RentalTariffAggregate,
};
use crate::repair_job::{OpenRepairJob, OpenRepairJobHandler, RepairJobAggregate};
use crate::workshop::{
    BicycleDamageIntegrationMapper, BicycleMaintenanceRequested, OpenRepairJobOnMaintenanceRequest,
};
use crate::{
    BikeRentalNatsConfig, BikeRentalNatsError, BikeRentalNatsResourceLimits, FINANCE_CONTEXT,
    WORKSHOP_CONTEXT, bike_rental_nats::runtime_config,
};

pub struct RegisterBicycleInputOptions;

impl CommandInputOptions<RegisterBicycle> for RegisterBicycleInputOptions {
    fn fields(&self) -> Vec<CommandInputField> {
        vec![CommandInputField::select(
            "condition",
            "Condition",
            vec![
                CommandInputOption::new("serviceable", "Serviceable"),
                CommandInputOption::new("maintenance-required", "Maintenance required"),
            ],
        )]
    }
}

/// Rental runtime bindings with the example's domain model for Tracer metadata.
pub fn bindings() -> Result<ApplicationBindings, Box<dyn std::error::Error + Send + Sync>> {
    Ok(command_bindings()?.with_domain_model(crate::domain_model()?))
}

/// Bind just the Rental context, for single-runtime clients and regression tests.
pub fn command_bindings() -> Result<ApplicationBindings, ApplicationBindingError> {
    let mut bindings = ApplicationBindings::new();
    bindings.register_aggregate::<RentalFleetAggregate>()?;
    bindings.register_aggregate::<RentalTariffAggregate>()?;
    let rejections = JsonDomainRejectionMapper::new(CommandRejectionClassification::Conflict);
    bindings.register_command::<ReturnBicycle, _>(ReturnBicycleHandler, rejections)?;
    bindings.register_command::<RegisterBicycle, _>(RegisterBicycleHandler, rejections)?;
    bindings.register_command::<TransferBicycle, _>(TransferBicycleHandler, rejections)?;
    bindings
        .register_command::<ReserveBicycleCategory, _>(ReserveBicycleCategoryHandler, rejections)?;
    bindings
        .register_command::<ConfigureRentalTariff, _>(ConfigureRentalTariffHandler, rejections)?;
    bindings
        .register_command::<CollectReservedBicycle, _>(CollectReservedBicycleHandler, rejections)?;
    // Only integration deliveries may apply a Finance decision to a held bicycle.
    bindings.register_command::<ResolveBicycleCollection, _>(
        ResolveBicycleCollectionHandler,
        rejections,
    )?;
    bindings.register_input_options::<RegisterBicycle, _>(RegisterBicycleInputOptions)?;
    bindings
        .register_integration_event::<RentalFleetAggregate, crate::rental_fleet::BicycleRented, _>(
            crate::BicycleRentedIntegrationMapper,
        );
    bindings.register_integration_handler::<crate::BicycleRentalStarted, _>(
        crate::BicycleRentalStartedHandler,
    );
    bindings.register_integration_event::<RentalFleetAggregate, crate::rental_fleet::BicycleDamageReported, _>(
        BicycleDamageIntegrationMapper,
    );
    bindings.register_integration_event::<RentalFleetAggregate, BicycleCollectionRequested, _>(
        CollectionPaymentIntegrationMapper,
    );
    bindings.register_integration_command::<CollectionPaymentDecided, _>(ResolveDecidedCollection);
    bindings.register_integration_event::<RentalFleetAggregate, ReservedBicycleReturned, _>(
        RentalReturnIntegrationMapper,
    );
    Ok(bindings)
}

pub fn finance_bindings(
    provider: Arc<dyn PaymentProvider>,
) -> Result<ApplicationBindings, ApplicationBindingError> {
    let mut bindings = ApplicationBindings::new();
    bindings.register_aggregate::<RentalPaymentAggregate>()?;
    let rejections = JsonDomainRejectionMapper::new(CommandRejectionClassification::Conflict);
    bindings.register_command::<RecordRentalPaymentRequest, _>(
        RecordRentalPaymentRequestHandler,
        rejections,
    )?;
    bindings.register_command::<CollectRentalPayment, _>(
        CollectRentalPaymentHandler {
            provider: provider.clone(),
        },
        rejections,
    )?;
    bindings.register_command::<AssessRentalReturn, _>(
        AssessRentalReturnHandler {
            provider: provider.clone(),
        },
        rejections,
    )?;
    bindings.register_command::<RefundEligibleRentalDeposit, _>(
        RefundEligibleRentalDepositHandler { provider },
        rejections,
    )?;
    bindings.register_integration_command::<CollectionPaymentRequested, _>(ProcessRequestedPayment);
    bindings.register_integration_event::<RentalPaymentAggregate, RentalPaymentRequested, _>(
        RentalPaymentChargeIntegrationMapper,
    );
    bindings.register_integration_command::<RentalPaymentChargeReady, _>(ChargeRecordedPayment);
    bindings.register_integration_command::<RentalReturnAssessed, _>(SettleReturnedRental);
    bindings.register_integration_event::<RentalPaymentAggregate, RentalPaymentApproved, _>(
        CollectionPaymentDecisionIntegrationMapper,
    );
    bindings.register_integration_event::<RentalPaymentAggregate, RentalPaymentDeclined, _>(
        CollectionPaymentDecisionIntegrationMapper,
    );
    bindings.register_integration_event::<RentalPaymentAggregate, RentalPaymentIdentityConflictRecorded, _>(
        CollectionPaymentConflictIntegrationMapper,
    );
    Ok(bindings)
}

pub fn workshop_bindings() -> Result<ApplicationBindings, ApplicationBindingError> {
    let mut bindings = ApplicationBindings::new();
    bindings.register_aggregate::<RepairJobAggregate>()?;
    bindings.register_command::<OpenRepairJob, _>(
        OpenRepairJobHandler,
        JsonDomainRejectionMapper::new(CommandRejectionClassification::Conflict),
    )?;
    bindings.register_integration_command::<BicycleMaintenanceRequested, _>(
        OpenRepairJobOnMaintenanceRequest,
    );
    Ok(bindings)
}

// Low-level Rental tests deliberately replace registered handlers.
pub fn builder(history: Arc<dyn EventHistory>) -> Result<TracerBuilder, ApplicationBindingError> {
    Ok(TracerBuilder::new(
        history,
        command_bindings()?.registry().clone(),
    ))
}

/// One application and traffic scope, with separate bindings and persistence
/// for Rental, Finance and Workshop.
#[derive(Clone)]
pub struct ContextConfigurations {
    rental: BikeRentalNatsConfig,
    rental_bindings: Arc<ApplicationBindings>,
    others: [(NatsRuntimeConfig, Arc<ApplicationBindings>); 2],
}

pub fn configurations(
    application: &str,
    scope: TrafficScope,
    limits: BikeRentalNatsResourceLimits,
) -> Result<ContextConfigurations, BikeRentalNatsError> {
    configurations_with_payments(application, scope, limits, Arc::new(SandboxPaymentProvider))
}

pub fn configurations_with_payments(
    application: &str,
    scope: TrafficScope,
    limits: BikeRentalNatsResourceLimits,
    provider: Arc<dyn PaymentProvider>,
) -> Result<ContextConfigurations, BikeRentalNatsError> {
    let model = crate::domain_model()
        .map_err(|error| BikeRentalNatsError::Configuration(error.to_string()))?;
    let rental_bindings = Arc::new(command_bindings()?.with_domain_model(model));
    let rental = BikeRentalNatsConfig::for_bindings(application, scope, limits, &rental_bindings)?;
    let finance = Arc::new(finance_bindings(provider)?);
    let workshop = Arc::new(workshop_bindings()?);
    let finance_config = runtime_config(application, scope, FINANCE_CONTEXT, limits, &finance)?;
    let workshop_config = runtime_config(application, scope, WORKSHOP_CONTEXT, limits, &workshop)?;
    Ok(ContextConfigurations {
        rental,
        rental_bindings,
        others: [(finance_config, finance), (workshop_config, workshop)],
    })
}

impl ContextConfigurations {
    pub const fn rental(&self) -> &BikeRentalNatsConfig {
        &self.rental
    }

    pub fn contexts(
        &self,
    ) -> impl Iterator<Item = (&NatsRuntimeConfig, &Arc<ApplicationBindings>)> {
        std::iter::once((self.rental.host_config(), &self.rental_bindings)).chain(
            self.others
                .iter()
                .map(|(config, bindings)| (config, bindings)),
        )
    }

    pub fn runtime_config(&self, context: &str) -> Option<&NatsRuntimeConfig> {
        self.contexts()
            .find_map(|(config, _)| (config.context.name().as_str() == context).then_some(config))
    }

    pub fn bindings_for(&self, context: &str) -> Option<&Arc<ApplicationBindings>> {
        self.contexts().find_map(|(config, bindings)| {
            (config.context.name().as_str() == context).then_some(bindings)
        })
    }

    pub fn with_fresh_outcomes(mut self) -> Result<Self, BikeRentalNatsError> {
        self.rental = self.rental.with_fresh_outcomes()?;
        for (config, _) in &mut self.others {
            *config = config.clone().with_fresh_outcomes()?;
        }
        Ok(self)
    }

    pub fn with_fresh_completed_outcome_epoch(
        mut self,
        epoch: &str,
    ) -> Result<Self, BikeRentalNatsError> {
        self.rental = self.rental.with_fresh_completed_outcome_epoch(epoch)?;
        for (config, _) in &mut self.others {
            *config = config.clone().with_fresh_completed_outcome_epoch(epoch)?;
        }
        Ok(self)
    }

    pub fn application_builder(
        &self,
        connection: NatsConnection,
    ) -> Result<NatsApplicationBuilder, BikeRentalNatsError> {
        let mut builder = NatsApplication::builder(
            self.rental.context().clone(),
            connection,
            self.rental_bindings.clone(),
        )?
        .with_runtime_config(self.rental.host_config().clone());
        for (config, bindings) in &self.others {
            builder = builder.with_context(config.clone(), bindings.clone());
        }
        Ok(builder)
    }

    pub async fn provision(&self, connection: &NatsConnection) -> Result<(), BikeRentalNatsError> {
        for (config, _) in self.contexts() {
            Box::pin(config.provision(connection)).await?;
        }
        Ok(())
    }

    /// Provision the three Normal runtimes and attach them as a dispatch group.
    pub async fn with_dispatch_runtimes(
        &self,
        connection: &NatsConnection,
        mut tracer: TracerConfig,
    ) -> Result<TracerConfig, BikeRentalNatsError> {
        if self.rental.context().traffic_scope() != TrafficScope::Normal {
            return Err(BikeRentalNatsError::Configuration(
                "dispatch requires Normal scope".into(),
            ));
        }
        for (config, bindings) in self.contexts() {
            let runtime = Box::pin(NatsRuntime::provision(
                connection.clone(),
                config.clone(),
                bindings.clone(),
            ))
            .await?;
            tracer = tracer.with_dispatch_runtime(Arc::new(runtime));
        }
        Ok(tracer)
    }
}
