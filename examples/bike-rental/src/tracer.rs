use std::sync::Arc;

use rostfrei::tracer::{ApplicationBindingError, ApplicationBindings};
use rostfrei::tracer::{CommandInputField, CommandInputOption, CommandInputOptions, TracerBuilder};
use rostfrei::{CommandRejectionClassification, EventHistory, JsonDomainRejectionMapper};

use crate::rental_fleet::{
    AddBicycle, AddBicycleHandler, RentBicycle, RentBicycleHandler, RentalFleetAggregate,
    ReturnBicycle, ReturnBicycleHandler, TransferBicycle, TransferBicycleHandler,
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
    let mut bindings = ApplicationBindings::new();
    bindings.register_aggregate::<RentalFleetAggregate>()?;
    let rejections = JsonDomainRejectionMapper::new(CommandRejectionClassification::Conflict);
    bindings.register_command::<RentBicycle, _>(RentBicycleHandler, rejections)?;
    bindings.register_command::<ReturnBicycle, _>(ReturnBicycleHandler, rejections)?;
    bindings.register_command::<AddBicycle, _>(AddBicycleHandler, rejections)?;
    bindings.register_command::<TransferBicycle, _>(TransferBicycleHandler, rejections)?;
    bindings.register_input_options::<AddBicycle, _>(AddBicycleInputOptions)?;
    bindings
        .register_integration_event::<RentalFleetAggregate, crate::rental_fleet::BicycleRented, _>(
            crate::BicycleRentedIntegrationMapper,
        );
    bindings.register_integration_handler::<crate::BicycleRentalStarted, _>(
        crate::BicycleRentalStartedHandler,
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
