use rostfrei::{Aggregate, AggregateDefinition};

use super::{RentalPayment, RentalPaymentEvent};
use crate::domain::Finance;

#[derive(Aggregate)]
#[domain(id = "rental-payment", label = "Rental payment")]
pub struct RentalPaymentAggregate;

impl AggregateDefinition for RentalPaymentAggregate {
    type Context = Finance;
    type Root = RentalPayment;
    type Event = RentalPaymentEvent;
}
