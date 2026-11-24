use super::{RentalPaymentRequested, RequestRentalPaymentAction};
use crate::domain::rental_payment::record_payment_identity_conflict::RecordPaymentIdentityConflictAction as _;
use crate::domain::rental_payment::{CollectionPayment, PaymentRejected, RentalPaymentAggregate};
use rostfrei::AggregateInstance;

impl RequestRentalPaymentAction for AggregateInstance<RentalPaymentAggregate> {
    fn request_rental_payment(
        &mut self,
        details: CollectionPayment,
    ) -> Result<(), PaymentRejected> {
        if let Some(existing) = self.state().details() {
            if existing.fleet_id != details.fleet_id {
                // Rental can reserve a payment ID in its own fleet only. Finance
                // owns the global identity and releases a conflicting hold without
                // overwriting the original request or making another provider call.
                self.record_payment_identity_conflict(
                    details.fleet_id,
                    details.reservation_id,
                    details.bicycle_id,
                );
                return Ok(());
            }
            return if existing == &details {
                Ok(())
            } else {
                Err(PaymentRejected {
                    payment_id: self.state().payment_id().clone(),
                    reason: "payment-identity-conflict".to_owned(),
                })
            };
        }
        if details.quote.total_due_minor().is_none() || details.payment_method.trim().is_empty() {
            return Err(PaymentRejected {
                payment_id: self.state().payment_id().clone(),
                reason: "invalid-payment-details".to_owned(),
            });
        }
        self.raise(RentalPaymentRequested {
            payment_id: self.state().payment_id().clone(),
            details,
        });
        Ok(())
    }
}
