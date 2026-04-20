use std::sync::Arc;

use rostfrei::{DomainRegistry, EventHistory, RegistrationError, domain_module};
use rostfrei_tracer::{CommandInputField, CommandInputOption, CommandInputOptions, TracerBuilder};

use crate::rental_fleet::{
    AddBicycle, BicycleCondition, BicycleStatus, RentBicycle, RentalFleet, ReturnBicycle,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct RentBicycleInputOptions;

impl CommandInputOptions<RentBicycle> for RentBicycleInputOptions {
    fn fields(&self, state: &RentalFleet) -> Vec<CommandInputField> {
        let bicycles = state
            .bicycles()
            .iter()
            .filter(|bicycle| {
                bicycle.status() == BicycleStatus::Available
                    && bicycle.condition() == BicycleCondition::Serviceable
            })
            .map(|bicycle| {
                CommandInputOption::new(
                    bicycle.bicycle_id().as_str(),
                    bicycle.bicycle_id().as_str(),
                )
                .with_description("Available and serviceable")
            })
            .collect();
        vec![CommandInputField::select("bicycle_id", "Bicycle", bicycles)]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReturnBicycleInputOptions;

impl CommandInputOptions<ReturnBicycle> for ReturnBicycleInputOptions {
    fn fields(&self, state: &RentalFleet) -> Vec<CommandInputField> {
        let bicycles = state
            .bicycles()
            .iter()
            .filter(|bicycle| bicycle.status() == BicycleStatus::Rented)
            .map(|bicycle| {
                CommandInputOption::new(
                    bicycle.bicycle_id().as_str(),
                    bicycle.bicycle_id().as_str(),
                )
                .with_description("Currently rented")
            })
            .collect();
        vec![CommandInputField::select("bicycle_id", "Bicycle", bicycles)]
    }
}

domain_module! {
    pub struct BikeRentalRuntimeModule {
        commands: [RentBicycle, ReturnBicycle, AddBicycle],
    }
}

pub fn builder(history: Arc<dyn EventHistory>) -> Result<TracerBuilder, RegistrationError> {
    let mut registry = DomainRegistry::new();
    registry.register_module::<BikeRentalRuntimeModule>()?;
    Ok(TracerBuilder::new(history, registry))
}
