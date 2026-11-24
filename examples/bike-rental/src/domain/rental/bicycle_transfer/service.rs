use rostfrei::{DomainService, DomainServiceDefinition};

use crate::domain::Rental;

#[derive(DomainService)]
#[domain(id = "bicycle-transfer", label = "Bicycle transfer")]
pub struct BicycleTransfer;

impl DomainServiceDefinition for BicycleTransfer {
    type Context = Rental;
}
