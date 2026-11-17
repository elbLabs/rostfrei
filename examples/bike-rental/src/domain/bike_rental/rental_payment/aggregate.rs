use rostfrei::{Aggregate, AggregateDefinition};

use super::{RentalPayment, RentalPaymentEvent};
use crate::domain::BikeRental;

#[derive(Aggregate)]
#[domain(id = "rental-payment", label = "Rental payment")]
pub struct RentalPaymentAggregate;

impl AggregateDefinition for RentalPaymentAggregate {
    type Context = BikeRental;
    type Root = RentalPayment;
    type Event = RentalPaymentEvent;
}
