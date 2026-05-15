use rostfrei::{DomainService, DomainServiceDefinition};

use crate::domain::BikeRental;

#[derive(DomainService)]
#[domain(id = "bicycle-transfer", label = "Bicycle transfer")]
pub struct BicycleTransfer;

impl DomainServiceDefinition for BicycleTransfer {
    type Context = BikeRental;
}
