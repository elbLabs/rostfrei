use std::sync::Arc;

use rostfrei::tracer::{ApplicationBindingError, ApplicationBindings};
use rostfrei::tracer::{CommandInputField, CommandInputOption, CommandInputOptions, TracerBuilder};
use rostfrei::{CommandRejectionClassification, EventHistory, JsonDomainRejectionMapper};

use crate::payments::{
    CollectionPaymentIntegrationMapper, CollectionPaymentRequested, PaymentProvider,
    ProcessRequestedPayment, SandboxPaymentProvider,
};
use crate::rental_fleet::{
    AddBicycle, AddBicycleHandler, CollectReservedBicycle, CollectReservedBicycleHandler,
    RentBicycle, RentBicycleHandler, RentalFleetAggregate, ReserveBicycleCategory,
    ReserveBicycleCategoryHandler, ReturnBicycle, ReturnBicycleHandler, TransferBicycle,
    TransferBicycleHandler,
};
use crate::rental_payment::{
    ProcessRentalPayment, ProcessRentalPaymentHandler, RentalPaymentAggregate,
    RentalPaymentRequested, SettleRentalDeposit, SettleRentalDepositHandler,
};
use crate::rental_tariff::{
    ConfigureRentalTariff, ConfigureRentalTariffHandler, RentalTariffAggregate,
};
use crate::repair_job::{OpenRepairJob, OpenRepairJobHandler, RepairJobAggregate};
use crate::workshop::{
    BicycleDamageIntegrationMapper, BicycleMaintenanceRequested, OpenRepairJobOnMaintenanceRequest,
};

pub struct AddBicycleInputOptions;

impl CommandInputOptions<AddBicycle> for AddBicycleInputOptions {
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

pub fn bindings() -> Result<ApplicationBindings, Box<dyn std::error::Error + Send + Sync>> {
    let mut bindings = command_bindings()?;
    bindings = bindings.with_domain_model(crate::domain_model()?);
    Ok(bindings)
}

pub fn command_bindings() -> Result<ApplicationBindings, ApplicationBindingError> {
    command_bindings_with_payments(Arc::new(SandboxPaymentProvider))
}

pub fn command_bindings_with_payments(
    provider: Arc<dyn PaymentProvider>,
) -> Result<ApplicationBindings, ApplicationBindingError> {
    let mut bindings = ApplicationBindings::new();
    bindings.register_aggregate::<RentalFleetAggregate>()?;
    bindings.register_aggregate::<RepairJobAggregate>()?;
    bindings.register_aggregate::<RentalTariffAggregate>()?;
    bindings.register_aggregate::<RentalPaymentAggregate>()?;
    let rejections = JsonDomainRejectionMapper::new(CommandRejectionClassification::Conflict);
    bindings.register_command::<RentBicycle, _>(RentBicycleHandler, rejections)?;
    bindings.register_command::<ReturnBicycle, _>(ReturnBicycleHandler, rejections)?;
    bindings.register_command::<AddBicycle, _>(AddBicycleHandler, rejections)?;
    bindings.register_command::<TransferBicycle, _>(TransferBicycleHandler, rejections)?;
    bindings.register_command::<OpenRepairJob, _>(OpenRepairJobHandler, rejections)?;
    bindings
        .register_command::<ReserveBicycleCategory, _>(ReserveBicycleCategoryHandler, rejections)?;
    bindings
        .register_command::<ConfigureRentalTariff, _>(ConfigureRentalTariffHandler, rejections)?;
    bindings
        .register_command::<CollectReservedBicycle, _>(CollectReservedBicycleHandler, rejections)?;
    bindings.register_command::<ProcessRentalPayment, _>(
        ProcessRentalPaymentHandler {
            provider: provider.clone(),
        },
        rejections,
    )?;
    bindings.register_command::<SettleRentalDeposit, _>(
        SettleRentalDepositHandler { provider },
        rejections,
    )?;
    bindings.register_input_options::<AddBicycle, _>(AddBicycleInputOptions)?;
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
    bindings.register_integration_command::<BicycleMaintenanceRequested, _>(
        OpenRepairJobOnMaintenanceRequest,
    );
    bindings.register_integration_event::<RentalPaymentAggregate, RentalPaymentRequested, _>(
        CollectionPaymentIntegrationMapper,
    );
    bindings.register_integration_command::<CollectionPaymentRequested, _>(ProcessRequestedPayment);
    bindings.register_integration_event::<RentalFleetAggregate, crate::rental_fleet::ReservedBicycleReturned, _>(crate::payments::RentalReturnIntegrationMapper);
    bindings.register_integration_command::<crate::payments::RentalReturnRecorded, _>(
        crate::payments::SettleReturnedRental,
    );
    Ok(bindings)
}

// Low-level test fixtures deliberately replace registered handlers. The runnable
// application uses `bindings()` directly and never repeats registrations.
pub fn builder(history: Arc<dyn EventHistory>) -> Result<TracerBuilder, ApplicationBindingError> {
    Ok(TracerBuilder::new(
        history,
        command_bindings()?.registry().clone(),
    ))
}
