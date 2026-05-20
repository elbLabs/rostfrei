use std::sync::Arc;

use rostfrei::{DomainRegistry, EventHistory, RegistrationError};
use rostfrei_tracer::{CommandInputField, CommandInputOption, CommandInputOptions, TracerBuilder};

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

pub fn builder(history: Arc<dyn EventHistory>) -> Result<TracerBuilder, RegistrationError> {
    let mut registry = DomainRegistry::new();
    registry.register_aggregate::<RentalFleetAggregate>()?;
    registry.register_command::<RentBicycle, RentBicycleHandler>()?;
    registry.register_command::<ReturnBicycle, ReturnBicycleHandler>()?;
    registry.register_command::<AddBicycle, AddBicycleHandler>()?;
    registry.register_command::<TransferBicycle, TransferBicycleHandler>()?;
    Ok(TracerBuilder::new(history, registry))
}
