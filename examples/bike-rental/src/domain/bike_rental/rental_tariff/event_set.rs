use rostfrei::AggregateEvents;

use super::RentalTariffConfigured;

#[derive(AggregateEvents)]
pub enum RentalTariffEvent {
    RentalTariffConfigured(RentalTariffConfigured),
}
