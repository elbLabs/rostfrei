use super::{
    RentalPaymentApproved, RentalPaymentDeclined, RentalPaymentProcessed, RentalPaymentRequested,
    SecurityDepositRefunded,
};
use rostfrei::AggregateEvents;

#[derive(AggregateEvents)]
pub enum RentalPaymentEvent {
    SecurityDepositRefunded(SecurityDepositRefunded),
    RentalPaymentRequested(RentalPaymentRequested),
    RentalPaymentProcessed(RentalPaymentProcessed),
    RentalPaymentApproved(RentalPaymentApproved),
    RentalPaymentDeclined(RentalPaymentDeclined),
}
