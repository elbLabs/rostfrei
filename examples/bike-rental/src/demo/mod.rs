use rostfrei::{
    CommandExecutionMetadata, ContentFingerprint, EventStore, EventStoreError, OperationId,
    StreamId,
};
use rostfrei_core::{derive_commit_id, derive_event_id};
use rostfrei_fixtures::{
    Fixture, FixtureApplyError, FixtureApplyReport, FixtureCodecRegistrationError,
    MessageSeriesEngine,
};
use thiserror::Error;

use crate::rental_fleet::RentalFleetAggregate;

pub const DEMO_FLEET_ID: &str = "city-fleet";

const DEMO_FLEET_FIXTURE: &[u8] = include_bytes!("../../fixtures/demo-fleet.json");
const RENTED_DEMO_FLEET_FIXTURE: &[u8] = include_bytes!("../../fixtures/rented-demo-fleet.json");
const LEGACY_DEMO_SEED_OPERATION_ID: &str = "seed-city-fleet";
const LEGACY_DEMO_SEED_FINGERPRINT: &[u8] = b"bike-rental-demo-v1";
const LEGACY_DEMO_SEED_EVENT_TYPE: &str = "rental-fleet-imported";
const LEGACY_DEMO_SEED_SCHEMA_VERSION: u32 = 1;
const LEGACY_DEMO_SEED_PAYLOAD: &[u8] = br#"{"fleet_id":"city-fleet","bicycles":[{"bicycle_id":"bike-42","status":"available","condition":"serviceable"},{"bicycle_id":"bike-99","status":"available","condition":"maintenance-required"}]}"#;

#[allow(
    clippy::expect_used,
    reason = "the compiled demo aggregate and fixture identities are valid by construction"
)]
pub fn demo_stream() -> StreamId {
    crate::rental_fleet::stream_id(DEMO_FLEET_ID).expect("static demo stream identity is valid")
}

#[allow(
    clippy::expect_used,
    reason = "the frozen legacy operation identity is valid by construction"
)]
fn legacy_demo_seed_operation_id() -> OperationId {
    OperationId::new(LEGACY_DEMO_SEED_OPERATION_ID)
        .expect("static legacy demo operation identity is valid")
}

#[derive(Debug, Error)]
pub enum DemoFixtureError {
    #[error("bike-rental fixture document is invalid: {0}")]
    Document(#[from] serde_json::Error),
    #[error(transparent)]
    Registration(#[from] FixtureCodecRegistrationError),
    #[error(transparent)]
    Apply(#[from] FixtureApplyError),
    #[error("bike-rental fixture history could not be loaded: {0}")]
    History(#[from] EventStoreError),
}

pub fn demo_fixture() -> Result<Fixture, DemoFixtureError> {
    serde_json::from_slice(DEMO_FLEET_FIXTURE).map_err(Into::into)
}

pub fn rented_demo_fixture() -> Result<Fixture, DemoFixtureError> {
    serde_json::from_slice(RENTED_DEMO_FLEET_FIXTURE).map_err(Into::into)
}

pub fn message_series_engine() -> Result<MessageSeriesEngine, DemoFixtureError> {
    let mut engine = MessageSeriesEngine::new();
    engine.register_json::<RentalFleetAggregate>()?;
    Ok(engine)
}

pub async fn has_legacy_demo_seed(store: &dyn EventStore) -> Result<bool, DemoFixtureError> {
    let history = store.load(&demo_stream()).await?;
    let Some(existing) = history.first() else {
        return Ok(false);
    };
    let fingerprint = ContentFingerprint::digest(LEGACY_DEMO_SEED_FINGERPRINT);
    let metadata = CommandExecutionMetadata::new(legacy_demo_seed_operation_id(), fingerprint);
    let stream_id = demo_stream();
    let commit_id = derive_commit_id(&stream_id, metadata.operation_id());
    Ok(existing.operation_id() == metadata.operation_id()
        && existing.operation_fingerprint() == fingerprint
        && existing.event_id() == &derive_event_id(&commit_id, 0)
        && existing.commit_id() == &commit_id
        && existing.stream_id() == &stream_id
        && existing.stream_version().value() == 1
        && existing.commit_event_ordinal() == 0
        && existing.commit_event_count() == 1
        && existing.event_type() == LEGACY_DEMO_SEED_EVENT_TYPE
        && existing.schema_version() == LEGACY_DEMO_SEED_SCHEMA_VERSION
        && existing.correlation_id().is_none()
        && existing.causation_id().is_none()
        && existing.payload() == LEGACY_DEMO_SEED_PAYLOAD)
}

pub async fn apply_fixture(
    store: &dyn EventStore,
    fixture: &Fixture,
) -> Result<FixtureApplyReport, DemoFixtureError> {
    message_series_engine()?
        .apply(store, fixture)
        .await
        .map_err(Into::into)
}

pub async fn apply_demo_fixture(
    store: &dyn EventStore,
) -> Result<FixtureApplyReport, DemoFixtureError> {
    apply_fixture(store, &demo_fixture()?).await
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use rostfrei::{EventBatch, ExpectedVersion, InMemoryEventStore, NewEvent};

    use super::*;

    type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

    #[tokio::test]
    async fn recognizes_only_the_persisted_legacy_demo_seed() -> TestResult {
        let legacy_store = InMemoryEventStore::new();
        let fingerprint = ContentFingerprint::digest(LEGACY_DEMO_SEED_FINGERPRINT);
        let metadata = CommandExecutionMetadata::new(legacy_demo_seed_operation_id(), fingerprint);
        let stream_id = demo_stream();
        let commit_id = derive_commit_id(&stream_id, metadata.operation_id());
        let event = NewEvent::new(
            derive_event_id(&commit_id, 0),
            LEGACY_DEMO_SEED_EVENT_TYPE,
            LEGACY_DEMO_SEED_SCHEMA_VERSION,
            LEGACY_DEMO_SEED_PAYLOAD,
        )?;
        let batch = EventBatch::new(
            commit_id.clone(),
            metadata.operation_id().clone(),
            fingerprint,
            vec![event],
        )?;
        legacy_store
            .append(&stream_id, ExpectedVersion::NoStream, batch)
            .await?;

        if !has_legacy_demo_seed(&legacy_store).await? {
            return Err("legacy demo seed was not recognized".into());
        }

        let multi_event_store = InMemoryEventStore::new();
        let extra_event = NewEvent::new(
            derive_event_id(&commit_id, 1),
            "not-the-legacy-seed",
            1,
            b"{}",
        )?;
        let seed_event = NewEvent::new(
            derive_event_id(&commit_id, 0),
            LEGACY_DEMO_SEED_EVENT_TYPE,
            LEGACY_DEMO_SEED_SCHEMA_VERSION,
            LEGACY_DEMO_SEED_PAYLOAD,
        )?;
        let multi_event_batch = EventBatch::new(
            commit_id.clone(),
            metadata.operation_id().clone(),
            fingerprint,
            vec![seed_event, extra_event],
        )?;
        multi_event_store
            .append(&stream_id, ExpectedVersion::NoStream, multi_event_batch)
            .await?;
        if has_legacy_demo_seed(&multi_event_store).await? {
            return Err("multi-event history was mistaken for the legacy seed".into());
        }

        let fixture_store = InMemoryEventStore::new();
        apply_demo_fixture(&fixture_store).await?;
        if has_legacy_demo_seed(&fixture_store).await? {
            return Err("MessageSeries fixture was mistaken for the legacy seed".into());
        }
        Ok(())
    }
}
