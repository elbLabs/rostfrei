use rostfrei::{Entity, EntityDefinition};

use super::{CollectionPayment, PaymentId, PaymentOutcome, RentalPaymentAggregate};
use crate::domain::rental_fleet::FleetId;

#[derive(Entity, Debug)]
#[domain(id = "rental-payment-root", label = "Rental payment")]
pub struct RentalPayment {
    pub(super) payment_id: PaymentId,
    pub(super) details: Option<CollectionPayment>,
    pub(super) outcome: Option<PaymentOutcome>,
    pub(super) provider_reference: Option<String>,
    pub(super) refund_reference: Option<String>,
    pub(super) return_eligible: Option<bool>,
    pub(super) conflicting_fleets: Vec<FleetId>,
}

impl EntityDefinition for RentalPayment {
    type Owner = RentalPaymentAggregate;
    type Identity = PaymentId;
    fn identity(&self) -> &Self::Identity {
        &self.payment_id
    }
}

impl RentalPayment {
    pub const fn return_eligible(&self) -> Option<bool> {
        self.return_eligible
    }
    pub fn refund_reference(&self) -> Option<&str> {
        self.refund_reference.as_deref()
    }
    pub const fn outcome(&self) -> Option<PaymentOutcome> {
        self.outcome
    }
    pub fn provider_reference(&self) -> Option<&str> {
        self.provider_reference.as_deref()
    }
    pub const fn payment_id(&self) -> &PaymentId {
        &self.payment_id
    }
    pub const fn details(&self) -> Option<&CollectionPayment> {
        self.details.as_ref()
    }
}
