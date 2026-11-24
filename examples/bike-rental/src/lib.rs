mod bike_rental_nats;
pub mod demo;
mod domain;
pub mod payments;
pub mod tracer;
pub mod workshop;

use rostfrei::BoundedContextType;

pub use bike_rental_nats::{
    APPLICATION_NAME, BICYCLE_RENTAL_STARTED_EVENT_NAME, BOUNDED_CONTEXT_NAME,
    BicycleRentalStarted, BicycleRentalStartedHandler, BicycleRentedIntegrationMapper,
    BikeRentalCommand, BikeRentalCommandRoute, BikeRentalIntegrationEventRoute,
    BikeRentalNatsConfig, BikeRentalNatsError, BikeRentalNatsResourceLimits, BikeRentalNatsRuntime,
};
pub use domain::{
    Finance, Rental, Workshop, bicycle_transfer, domain_model, rental_fleet, rental_payment,
    rental_tariff, repair_job,
};

pub const RENTAL_CONTEXT: &str = Rental::DESCRIPTOR.id.0;
pub const FINANCE_CONTEXT: &str = Finance::DESCRIPTOR.id.0;
pub const WORKSHOP_CONTEXT: &str = Workshop::DESCRIPTOR.id.0;

rostfrei::install_macro_support!();
