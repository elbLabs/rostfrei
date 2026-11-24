use super::{
    RentalPaymentApproved, RentalPaymentDeclined, RentalPaymentIdentityConflictRecorded,
    RentalPaymentProcessed, RentalPaymentRequested, RentalReturnEligibilityRecorded,
    SecurityDepositRefunded,
};
use rostfrei::AggregateEvents;

#[derive(AggregateEvents)]
pub enum RentalPaymentEvent {
    SecurityDepositRefunded(SecurityDepositRefunded),
    RentalPaymentRequested(RentalPaymentRequested),
    RentalPaymentIdentityConflictRecorded(RentalPaymentIdentityConflictRecorded),
    RentalPaymentProcessed(RentalPaymentProcessed),
    RentalPaymentApproved(RentalPaymentApproved),
    RentalPaymentDeclined(RentalPaymentDeclined),
    RentalReturnEligibilityRecorded(RentalReturnEligibilityRecorded),
}
