use crate::rental_fleet::{
    BicycleCollectionRequested, BicycleId, FleetId, ReservationId, ReservedBicycleReturned,
    ResolveBicycleCollection,
};
use crate::rental_payment::{
    AssessRentalReturn, CollectRentalPayment, CollectionPayment, PaymentId, PaymentOutcome,
    RecordRentalPaymentRequest, RentalPaymentApproved, RentalPaymentDeclined,
    RentalPaymentIdentityConflictRecorded, RentalPaymentRequested,
};
use crate::{FINANCE_CONTEXT, RENTAL_CONTEXT};
use async_trait::async_trait;
use rostfrei::{
    CommandExecution, CommittedDomainEvent, IntegrationCommandMapper, IntegrationEvent,
    IntegrationEventMapper,
};
use serde::{Deserialize, Serialize};

pub struct PaymentResult {
    pub outcome: PaymentOutcome,
    pub reference: String,
}

#[async_trait]
pub trait PaymentProvider: Send + Sync {
    // Charge retries for this key must return the original result without charging again.
    async fn charge(&self, id: &PaymentId, details: &CollectionPayment) -> PaymentResult;
    // Refunds must also be idempotent, using a separate operation namespace from charges.
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

pub(crate) fn is_integration_command(execution: &CommandExecution<'_>) -> bool {
    // The mapped integration-command path sets this runtime marker; ordinary
    // authoring cannot set it with a payload, actor claim or caller causation.
    execution
        .metadata()
        .outcome_evidence()
        .is_some_and(|evidence| {
            evidence.events_caused_by_command && evidence.original_causation.is_some()
        })
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RentalReturnAssessed {
    pub payment_id: PaymentId,
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub bicycle_id: BicycleId,
    pub eligible: bool,
}

impl IntegrationEvent for RentalReturnAssessed {
    const EVENT_NAME: &'static str = "rental-return-assessed";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some(RENTAL_CONTEXT);
}

pub struct RentalReturnIntegrationMapper;

impl IntegrationEventMapper<ReservedBicycleReturned> for RentalReturnIntegrationMapper {
    type Output = RentalReturnAssessed;
    fn map(&self, event: &CommittedDomainEvent<'_, ReservedBicycleReturned>) -> Self::Output {
        let returned = event.event();
        RentalReturnAssessed {
            payment_id: returned.payment_id.clone(),
            fleet_id: returned.fleet_id.clone(),
            reservation_id: returned.reservation_id.clone(),
            bicycle_id: returned.bicycle_id.clone(),
            eligible: returned.eligible,
        }
    }
}

pub struct SettleReturnedRental;

impl IntegrationCommandMapper<RentalReturnAssessed> for SettleReturnedRental {
    type Command = AssessRentalReturn;
    type Error = &'static str;
    fn map(&self, event: &RentalReturnAssessed) -> Result<Self::Command, Self::Error> {
        Ok(AssessRentalReturn {
            payment_id: event.payment_id.clone(),
            fleet_id: event.fleet_id.clone(),
            reservation_id: event.reservation_id.clone(),
            bicycle_id: event.bicycle_id.clone(),
            eligible: event.eligible,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CollectionPaymentRequested {
    pub payment_id: PaymentId,
    pub details: CollectionPayment,
}

impl IntegrationEvent for CollectionPaymentRequested {
    const EVENT_NAME: &'static str = "collection-payment-requested";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some(RENTAL_CONTEXT);
}

pub struct CollectionPaymentIntegrationMapper;

impl IntegrationEventMapper<BicycleCollectionRequested> for CollectionPaymentIntegrationMapper {
    type Output = CollectionPaymentRequested;
    fn map(&self, event: &CommittedDomainEvent<'_, BicycleCollectionRequested>) -> Self::Output {
        CollectionPaymentRequested {
            payment_id: event.event().payment_id.clone(),
            details: event.event().payment_details(),
        }
    }
}

pub struct ProcessRequestedPayment;

impl IntegrationCommandMapper<CollectionPaymentRequested> for ProcessRequestedPayment {
    type Command = RecordRentalPaymentRequest;
    type Error = &'static str;
    fn map(&self, event: &CollectionPaymentRequested) -> Result<Self::Command, Self::Error> {
        Ok(RecordRentalPaymentRequest {
            payment_id: event.payment_id.clone(),
            details: event.details.clone(),
        })
    }
}

/// Finance-local continuation: the immutable request must commit before any provider call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RentalPaymentChargeReady {
    pub payment_id: PaymentId,
}

impl IntegrationEvent for RentalPaymentChargeReady {
    const EVENT_NAME: &'static str = "rental-payment-charge-ready";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some(FINANCE_CONTEXT);
}

pub struct RentalPaymentChargeIntegrationMapper;

impl IntegrationEventMapper<RentalPaymentRequested> for RentalPaymentChargeIntegrationMapper {
    type Output = RentalPaymentChargeReady;

    fn map(&self, event: &CommittedDomainEvent<'_, RentalPaymentRequested>) -> Self::Output {
        RentalPaymentChargeReady {
            payment_id: event.event().payment_id.clone(),
        }
    }
}

pub struct ChargeRecordedPayment;

impl IntegrationCommandMapper<RentalPaymentChargeReady> for ChargeRecordedPayment {
    type Command = CollectRentalPayment;
    type Error = &'static str;

    fn map(&self, event: &RentalPaymentChargeReady) -> Result<Self::Command, Self::Error> {
        Ok(CollectRentalPayment {
            payment_id: event.payment_id.clone(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CollectionPaymentDecided {
    pub payment_id: PaymentId,
    pub fleet_id: FleetId,
    pub reservation_id: ReservationId,
    pub bicycle_id: BicycleId,
    pub outcome: PaymentOutcome,
}

impl IntegrationEvent for CollectionPaymentDecided {
    const EVENT_NAME: &'static str = "collection-payment-decided";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some(FINANCE_CONTEXT);
}

pub struct CollectionPaymentDecisionIntegrationMapper;

impl IntegrationEventMapper<RentalPaymentApproved> for CollectionPaymentDecisionIntegrationMapper {
    type Output = CollectionPaymentDecided;

    fn map(&self, event: &CommittedDomainEvent<'_, RentalPaymentApproved>) -> Self::Output {
        let approved = event.event();
        CollectionPaymentDecided {
            payment_id: approved.payment_id.clone(),
            fleet_id: approved.fleet_id.clone(),
            reservation_id: approved.reservation_id.clone(),
            bicycle_id: approved.bicycle_id.clone(),
            outcome: PaymentOutcome::Approved,
        }
    }
}

impl IntegrationEventMapper<RentalPaymentDeclined> for CollectionPaymentDecisionIntegrationMapper {
    type Output = CollectionPaymentDecided;

    fn map(&self, event: &CommittedDomainEvent<'_, RentalPaymentDeclined>) -> Self::Output {
        let declined = event.event();
        CollectionPaymentDecided {
            payment_id: declined.payment_id.clone(),
            fleet_id: declined.fleet_id.clone(),
            reservation_id: declined.reservation_id.clone(),
            bicycle_id: declined.bicycle_id.clone(),
            outcome: PaymentOutcome::Declined,
        }
    }
}

pub struct CollectionPaymentConflictIntegrationMapper;

impl IntegrationEventMapper<RentalPaymentIdentityConflictRecorded>
    for CollectionPaymentConflictIntegrationMapper
{
    type Output = CollectionPaymentDecided;

    fn map(
        &self,
        event: &CommittedDomainEvent<'_, RentalPaymentIdentityConflictRecorded>,
    ) -> Self::Output {
        let conflict = event.event();
        CollectionPaymentDecided {
            payment_id: conflict.payment_id.clone(),
            fleet_id: conflict.fleet_id.clone(),
            reservation_id: conflict.reservation_id.clone(),
            bicycle_id: conflict.bicycle_id.clone(),
            outcome: PaymentOutcome::Declined,
        }
    }
}

pub struct ResolveDecidedCollection;

impl IntegrationCommandMapper<CollectionPaymentDecided> for ResolveDecidedCollection {
    type Command = ResolveBicycleCollection;
    type Error = &'static str;

    fn map(&self, event: &CollectionPaymentDecided) -> Result<Self::Command, Self::Error> {
        Ok(ResolveBicycleCollection {
            payment_id: event.payment_id.clone(),
            fleet_id: event.fleet_id.clone(),
            reservation_id: event.reservation_id.clone(),
            bicycle_id: event.bicycle_id.clone(),
            outcome: event.outcome,
        })
    }
}
