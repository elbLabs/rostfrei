use crate::rental_payment::{
    CollectionPayment, PaymentId, PaymentOutcome, ProcessRentalPayment, RentalPaymentRequested,
};
use async_trait::async_trait;
use rostfrei::{
    CommittedDomainEvent, IntegrationCommandMapper, IntegrationEvent, IntegrationEventMapper,
};
use serde::{Deserialize, Serialize};

pub struct PaymentResult {
    pub outcome: PaymentOutcome,
    pub reference: String,
}

#[async_trait]
pub trait PaymentProvider: Send + Sync {
    // The payment identity is the provider's idempotency key across handler retries.
    async fn charge(&self, id: &PaymentId, details: &CollectionPayment) -> PaymentResult;
    // Refund keys must use a separate provider operation namespace from charges.
    async fn refund_deposit(
        &self,
        id: &PaymentId,
        charge_reference: &str,
        currency: &str,
        amount_minor: u64,
    ) -> PaymentResult;
}

pub struct SandboxPaymentProvider;

#[async_trait]
impl PaymentProvider for SandboxPaymentProvider {
    async fn refund_deposit(
        &self,
        id: &PaymentId,
        _charge_reference: &str,
        _currency: &str,
        _amount_minor: u64,
    ) -> PaymentResult {
        PaymentResult {
            outcome: PaymentOutcome::Approved,
            reference: format!("sandbox:deposit-refund:{}", id.as_str()),
        }
    }
    async fn charge(&self, id: &PaymentId, details: &CollectionPayment) -> PaymentResult {
        PaymentResult {
            outcome: if details.payment_method == "sandbox-approved" {
                PaymentOutcome::Approved
            } else {
                PaymentOutcome::Declined
            },
            reference: format!("sandbox:{}", id.as_str()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RentalReturnRecorded {
    pub payment_id: PaymentId,
}

impl IntegrationEvent for RentalReturnRecorded {
    const EVENT_NAME: &'static str = "rental-return-recorded";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some(crate::BOUNDED_CONTEXT_NAME);
}

pub struct RentalReturnIntegrationMapper;

impl IntegrationEventMapper<crate::rental_fleet::ReservedBicycleReturned>
    for RentalReturnIntegrationMapper
{
    type Output = RentalReturnRecorded;
    fn map(
        &self,
        event: &CommittedDomainEvent<'_, crate::rental_fleet::ReservedBicycleReturned>,
    ) -> Self::Output {
        RentalReturnRecorded {
            payment_id: event.event().payment_id.clone(),
        }
    }
}

pub struct SettleReturnedRental;

impl IntegrationCommandMapper<RentalReturnRecorded> for SettleReturnedRental {
    type Command = crate::rental_payment::SettleRentalDeposit;
    type Error = &'static str;
    fn map(&self, event: &RentalReturnRecorded) -> Result<Self::Command, Self::Error> {
        Ok(crate::rental_payment::SettleRentalDeposit {
            payment_id: event.payment_id.clone(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CollectionPaymentRequested {
    pub payment_id: PaymentId,
}

impl IntegrationEvent for CollectionPaymentRequested {
    const EVENT_NAME: &'static str = "collection-payment-requested";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some(crate::BOUNDED_CONTEXT_NAME);
}

pub struct CollectionPaymentIntegrationMapper;

impl IntegrationEventMapper<RentalPaymentRequested> for CollectionPaymentIntegrationMapper {
    type Output = CollectionPaymentRequested;
    fn map(&self, event: &CommittedDomainEvent<'_, RentalPaymentRequested>) -> Self::Output {
        CollectionPaymentRequested {
            payment_id: event.event().payment_id.clone(),
        }
    }
}

pub struct ProcessRequestedPayment;

impl IntegrationCommandMapper<CollectionPaymentRequested> for ProcessRequestedPayment {
    type Command = ProcessRentalPayment;
    type Error = &'static str;
    fn map(&self, event: &CollectionPaymentRequested) -> Result<Self::Command, Self::Error> {
        Ok(ProcessRentalPayment {
            payment_id: event.payment_id.clone(),
        })
    }
}
