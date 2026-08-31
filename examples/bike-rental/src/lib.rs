mod bike_rental_nats;
pub mod demo;
mod domain;
pub mod tracer;

pub use bike_rental_nats::{
    BICYCLE_RENTAL_STARTED_EVENT_NAME, BOUNDED_CONTEXT_NAME, BicycleRentalStarted,
    BicycleRentalStartedHandler, BicycleRentedIntegrationMapper, BikeRentalCommand,
    BikeRentalCommandRoute, BikeRentalIntegrationEventRoute, BikeRentalNatsConfig,
    BikeRentalNatsError, BikeRentalNatsRuntime,
};
pub use domain::{BikeRental, domain_model, rental_fleet};
