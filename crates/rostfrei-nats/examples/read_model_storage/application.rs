//! Low-level storage example with application-owned checkpoints and query port.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use rostfrei::{
    CommittedDomainEvent, DomainEventDispatcher, DomainEventHandler, DomainEventHandlerError,
    DomainEventHandlerErrorKind, EventHistory, QueryErrorPayload, QueryHandler,
    QueryHandlerRequest, ReadModelError, ReadModelErrorKind, ReadModelKey, ReadModelStore,
};
use rostfrei_core::{
    Aggregate, Event, EventCodecError, EventCodecErrorKind, EventVariant, RecordedEvent, StreamId,
};
use rostfrei_messaging_core::{
    DeliveryDisposition, IntegrationEventAddress, IntegrationEventEnvelope, MessageDelivery,
    MessageHandler, QuarantineReason, RetryDelay,
};
use serde::{Deserialize, Serialize};

pub type ExampleResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Entitlement {
    pub members: u64,
    pub demo: bool,
    pub paid: bool,
    pub organization_version: u64,
    pub billing_version: u64,
}

#[derive(Deserialize, Serialize)]
pub struct MemberJoined;

#[derive(Deserialize, Serialize)]
pub struct DemoChanged {
    pub active: bool,
}

#[derive(Deserialize, Serialize)]
pub enum OrganizationEvent {
    MemberJoined(MemberJoined),
    DemoChanged(DemoChanged),
}

// Minimal event-sourced source model for this storage example. Applications
// normally use their compiled domain model and generated aggregate event codec.
pub struct Organization;

impl Aggregate for Organization {
    type State = (u64, bool);
    type Event = OrganizationEvent;
    const BOUNDED_CONTEXT: &'static str = "access";
    const AGGREGATE_TYPE: &'static str = "organization";
    fn initial(_: &StreamId) -> Self::State {
        (0, false)
    }
    fn apply(state: &mut Self::State, event: &Self::Event) {
        match event {
            OrganizationEvent::MemberJoined(_) => state.0 = state.0.saturating_add(1),
            OrganizationEvent::DemoChanged(event) => state.1 = event.active,
        }
    }
}

impl Event for OrganizationEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::MemberJoined(_) => "member-joined",
            Self::DemoChanged(_) => "demo-changed",
        }
    }
    fn schema_version(&self) -> u32 {
        1
    }
    fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
        serde_json::to_vec(self).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::EncodingFailed, error.to_string())
        })
    }
    fn decode_json(event: &RecordedEvent) -> Result<Self, EventCodecError> {
        if event.schema_version() != 1 {
            return Err(EventCodecError::new(
                EventCodecErrorKind::UnsupportedSchemaVersion,
                "unsupported organization event",
            ));
        }
        let decoded: Self = serde_json::from_slice(event.payload()).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::MalformedPayload, error.to_string())
        })?;
        if decoded.event_type() != event.event_type() {
            return Err(EventCodecError::new(
                EventCodecErrorKind::InvalidEnvelope,
                "event type mismatch",
            ));
        }
        Ok(decoded)
    }
}

impl EventVariant<MemberJoined> for OrganizationEvent {
    fn event(&self) -> Option<&MemberJoined> {
        if let Self::MemberJoined(event) = self {
            Some(event)
        } else {
            None
        }
    }
    fn into_event(self) -> Option<MemberJoined> {
        if let Self::MemberJoined(event) = self {
            Some(event)
        } else {
            None
        }
    }
}

impl EventVariant<DemoChanged> for OrganizationEvent {
    fn event(&self) -> Option<&DemoChanged> {
        if let Self::DemoChanged(event) = self {
            Some(event)
        } else {
            None
        }
    }
    fn into_event(self) -> Option<DemoChanged> {
        if let Self::DemoChanged(event) = self {
            Some(event)
        } else {
            None
        }
    }
}

/// Public billing contract: a complete replacement fact with a monotonically
/// increasing per-organization source version, supplied by the billing owner.
/// This version is not the NATS consumer sequence or an unordered webhook ID.
#[derive(Deserialize, Serialize)]
pub struct BillingChanged {
    pub organization_id: String,
    pub source_version: u64,
    pub paid: bool,
}

/// Application-owned query port: query handlers do not know about KV or codecs.
#[async_trait]
pub trait EntitlementLookup: Send + Sync {
    async fn lookup(&self, organization_id: &str) -> Result<Option<Entitlement>, ReadModelError>;
}

pub struct Entitlements {
    store: Arc<dyn ReadModelStore<Entitlement>>,
    retry_delay: RetryDelay,
    invalid_message: QuarantineReason,
    materialization_failed: QuarantineReason,
}

impl Entitlements {
    pub fn new(store: Arc<dyn ReadModelStore<Entitlement>>) -> ExampleResult<Self> {
        Ok(Self {
            store,
            retry_delay: RetryDelay::new(Duration::from_millis(100))?,
            invalid_message: QuarantineReason::new(
                "invalid billing read-model input or configuration",
            )?,
            materialization_failed: QuarantineReason::new(
                "billing snapshot not materialized; repair storage/configuration, then replay or refresh billing",
            )?,
        })
    }

    pub fn dispatcher(self: &Arc<Self>) -> ExampleResult<DomainEventDispatcher> {
        let mut dispatcher = DomainEventDispatcher::new();
        dispatcher.register::<Organization, MemberJoined, _>("member-joined", self.clone())?;
        dispatcher.register::<Organization, DemoChanged, _>("demo-changed", self.clone())?;
        Ok(dispatcher)
    }

    async fn change(
        &self,
        organization_id: &str,
        apply: impl Fn(&mut Entitlement) -> Result<bool, DomainEventHandlerError> + Send + Sync,
    ) -> Result<(), DomainEventHandlerError> {
        let key = ReadModelKey::new(organization_id).map_err(|error| storage_error(&error))?;
        for _ in 0..8 {
            let current = self
                .store
                .read(&key)
                .await
                .map_err(|error| storage_error(&error))?;
            let (mut value, revision) = current.map_or_else(
                || (Entitlement::default(), None),
                |entry| (entry.value, Some(entry.revision)),
            );
            if !apply(&mut value)? {
                return Ok(());
            }
            let result = match revision {
                Some(revision) => self.store.update(&key, &revision, &value).await,
                None => self.store.create(&key, &value).await,
            };
            match result {
                Ok(_) => return Ok(()),
                // Reload, re-check source position, and re-apply. Never write a
                // snapshot calculated from the losing revision a second time.
                Err(error) if error.kind() == ReadModelErrorKind::Conflict => {
                    tokio::task::yield_now().await;
                }
                Err(error) => return Err(storage_error(&error)),
            }
        }
        Err(DomainEventHandlerError::new(
            DomainEventHandlerErrorKind::Retryable,
            "read-model contention; redeliver later",
        ))
    }

    /// A complete billing fact can skip versions, unlike organization deltas.
    ///
    /// Also use this path with the latest fact fetched from the billing authority
    /// to reconcile a quarantined update. Fetch/retry scheduling and recovery
    /// tracking belong to the application; storage recovery alone does not redrive
    /// an integration event that the consumer has already quarantined.
    pub async fn billing_changed(
        &self,
        event: &BillingChanged,
    ) -> Result<(), DomainEventHandlerError> {
        if event.source_version == 0 {
            return Err(DomainEventHandlerError::new(
                DomainEventHandlerErrorKind::InvalidCommittedEvent,
                "billing source version must be positive",
            ));
        }
        self.change(&event.organization_id, |value| {
            if event.source_version <= value.billing_version {
                return Ok(false);
            }
            value.paid = event.paid;
            value.billing_version = event.source_version;
            Ok(true)
        })
        .await
    }
}

fn storage_error(error: &ReadModelError) -> DomainEventHandlerError {
    let kind = match error.kind() {
        ReadModelErrorKind::Conflict | ReadModelErrorKind::Unavailable => {
            DomainEventHandlerErrorKind::Retryable
        }
        _ => DomainEventHandlerErrorKind::OperatorBlocking,
    };
    DomainEventHandlerError::new(kind, error.to_string())
}

fn next_organization_event(
    value: &Entitlement,
    version: u64,
) -> Result<bool, DomainEventHandlerError> {
    if version <= value.organization_version {
        return Ok(false);
    }
    if value.organization_version.checked_add(1) != Some(version) {
        // Block this source until the missing history is replayed. A filtered
        // consumer must also advance positions for source events it ignores.
        return Err(DomainEventHandlerError::new(
            DomainEventHandlerErrorKind::OperatorBlocking,
            "organization source gap: rebuild missing history before resuming",
        ));
    }
    Ok(true)
}

#[async_trait]
impl DomainEventHandler<MemberJoined> for Entitlements {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, MemberJoined>,
    ) -> Result<(), DomainEventHandlerError> {
        let version = event.recorded().stream_version().value();
        self.change(
            event.recorded().stream_id().aggregate_id().as_str(),
            |value| {
                if !next_organization_event(value, version)? {
                    return Ok(false);
                }
                value.members = value.members.checked_add(1).ok_or_else(|| {
                    DomainEventHandlerError::new(
                        DomainEventHandlerErrorKind::OperatorBlocking,
                        "member count overflow",
                    )
                })?;
                value.organization_version = version;
                Ok(true)
            },
        )
        .await
    }
}

#[async_trait]
impl DomainEventHandler<DemoChanged> for Entitlements {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, DemoChanged>,
    ) -> Result<(), DomainEventHandlerError> {
        let version = event.recorded().stream_version().value();
        self.change(
            event.recorded().stream_id().aggregate_id().as_str(),
            |value| {
                if !next_organization_event(value, version)? {
                    return Ok(false);
                }
                value.demo = event.event().active;
                value.organization_version = version;
                Ok(true)
            },
        )
        .await
    }
}

#[async_trait]
impl MessageHandler<IntegrationEventAddress> for Entitlements {
    async fn handle(
        &self,
        delivery: MessageDelivery<IntegrationEventAddress>,
    ) -> DeliveryDisposition {
        let Ok(envelope) =
            serde_json::from_slice::<IntegrationEventEnvelope<BillingChanged>>(delivery.payload())
        else {
            return DeliveryDisposition::Quarantine(self.invalid_message.clone());
        };
        if envelope.schema_version().get() != 1 {
            return DeliveryDisposition::Quarantine(self.invalid_message.clone());
        }
        match self.billing_changed(envelope.payload()).await {
            // The snapshot AND source position are durable before returning ACK.
            Ok(()) => DeliveryDisposition::Acknowledge,
            Err(error) if error.kind() == DomainEventHandlerErrorKind::Retryable => {
                // Retry exhaustion also ends in quarantine. The durable may
                // advance without updating billing_version: reconcile explicitly.
                DeliveryDisposition::RetryAfter(self.retry_delay)
            }
            Err(error) if error.kind() == DomainEventHandlerErrorKind::InvalidCommittedEvent => {
                DeliveryDisposition::Quarantine(self.invalid_message.clone())
            }
            // Quarantine persists the failed delivery, then terminally ACKs it.
            // Repair the cause and replay it or refresh the authoritative fact.
            Err(_) => DeliveryDisposition::Quarantine(self.materialization_failed.clone()),
        }
    }
}

#[async_trait]
impl EntitlementLookup for Entitlements {
    async fn lookup(&self, organization_id: &str) -> Result<Option<Entitlement>, ReadModelError> {
        Ok(self
            .store
            .read(&ReadModelKey::new(organization_id)?)
            .await?
            .map(|entry| entry.value))
    }
}

pub struct GetEntitlement {
    pub lookup: Arc<dyn EntitlementLookup>,
}

#[async_trait]
impl QueryHandler<String, Option<Entitlement>> for GetEntitlement {
    async fn handle(
        &self,
        request: QueryHandlerRequest<String>,
    ) -> Result<Option<Entitlement>, QueryErrorPayload> {
        self.lookup
            .lookup(request.payload())
            .await
            .map_err(|_| QueryErrorPayload::internal_error())
    }
}

/// Replays authoritative domain history into a new generation of the read
/// model, or fills a gap in an existing one. Duplicate replay is harmless.
/// External billing facts must separately be re-fetched from their authority.
pub async fn rebuild(
    history: &dyn EventHistory,
    stream: &StreamId,
    dispatcher: &DomainEventDispatcher,
) -> ExampleResult {
    for event in history.load(stream).await? {
        dispatcher.dispatch(&event).await?;
    }
    Ok(())
}
