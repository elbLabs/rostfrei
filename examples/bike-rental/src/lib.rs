mod bike_rental_nats;
pub mod demo;
mod domain;
pub mod payments;
pub mod tracer;
pub mod workshop;

pub use bike_rental_nats::{
    APPLICATION_NAME, BICYCLE_RENTAL_STARTED_EVENT_NAME, BOUNDED_CONTEXT_NAME,
    BicycleRentalStarted, BicycleRentalStartedHandler, BicycleRentedIntegrationMapper,
    BikeRentalCommand, BikeRentalCommandRoute, BikeRentalIntegrationEventRoute,
    BikeRentalNatsConfig, BikeRentalNatsError, BikeRentalNatsResourceLimits, BikeRentalNatsRuntime,
};
pub use domain::{
    BikeRental, domain_model, rental_fleet, rental_payment, rental_tariff, repair_job,
};
rostfrei::install_macro_support!();
