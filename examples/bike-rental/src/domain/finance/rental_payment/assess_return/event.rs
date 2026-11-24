use crate::domain::rental_payment::PaymentId;
use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "rental-return-eligibility-recorded",
    label = "Rental return eligibility recorded"
)]
pub struct RentalReturnEligibilityRecorded {
    pub payment_id: PaymentId,
    pub eligible: bool,
}
