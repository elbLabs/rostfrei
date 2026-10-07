use std::collections::BTreeSet;

use rostfrei_core::{RecordedEvent, StreamId};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::BenchResult;

/// The same response is returned and JSON-encoded by every measured query path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccessView {
    pub organization_id: String,
    pub billing_account_id: String,
    pub member_count: u64,
    pub purchased_seats: u64,
    pub available_seats: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Snapshot {
    pub view: AccessView,
    pub organization_version: u64,
    pub billing_version: u64,
}

#[derive(Deserialize, Serialize)]
pub struct Payload<T> {
    pub change: T,
    pub note: String,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum OrganizationEvent {
    Created { billing_account_id: String },
    MemberJoined { member_id: String },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum BillingEvent {
    Opened { organization_id: String },
    SeatsPurchased { seats: u64 },
}

/// Rehydrated organization state includes member identities, while the read
/// model only needs the count. Its first event establishes the aggregate link.
pub struct OrganizationState {
    pub billing_account_id: String,
    pub members: BTreeSet<String>,
    pub version: u64,
}

pub struct BillingState {
    pub organization_id: String,
    pub purchased_seats: u64,
    pub version: u64,
}

fn decode<T: DeserializeOwned>(record: &RecordedEvent, event_type: &str) -> BenchResult<T> {
    if record.schema_version() != 1 || record.event_type() != event_type {
        return Err("unexpected benchmark source event schema/type".into());
    }
    let payload: Payload<T> = serde_json::from_slice(record.payload())?;
    Ok(payload.change)
}

pub fn replay_organization(records: &[RecordedEvent]) -> BenchResult<OrganizationState> {
    let mut billing_account_id = None;
    let mut members = BTreeSet::new();
    for record in records {
        match decode(record, "organization-changed")? {
            OrganizationEvent::Created {
                billing_account_id: id,
            } => {
                if billing_account_id.replace(id).is_some() {
                    return Err("organization created twice".into());
                }
            }
            OrganizationEvent::MemberJoined { member_id } => {
                if billing_account_id.is_none() || !members.insert(member_id) {
                    return Err("invalid organization membership history".into());
                }
            }
        }
    }
    Ok(OrganizationState {
        billing_account_id: billing_account_id.ok_or("organization not created")?,
        members,
        version: records
            .last()
            .ok_or("empty organization history")?
            .stream_version()
            .value(),
    })
}

pub fn replay_billing(records: &[RecordedEvent]) -> BenchResult<BillingState> {
    let mut organization_id = None;
    let mut purchased_seats = 0_u64;
    for record in records {
        match decode(record, "billing-changed")? {
            BillingEvent::Opened {
                organization_id: id,
            } => {
                if organization_id.replace(id).is_some() {
                    return Err("billing account opened twice".into());
                }
            }
            BillingEvent::SeatsPurchased { seats } => {
                if organization_id.is_none() {
                    return Err("billing account not opened".into());
                }
                purchased_seats = purchased_seats
                    .checked_add(seats)
                    .ok_or("seat count overflow")?;
            }
        }
    }
    Ok(BillingState {
        organization_id: organization_id.ok_or("billing account not opened")?,
        purchased_seats,
        version: records
            .last()
            .ok_or("empty billing history")?
            .stream_version()
            .value(),
    })
}

pub fn join(
    organization: &StreamId,
    state: OrganizationState,
    billing: &BillingState,
) -> BenchResult<Snapshot> {
    if organization.aggregate_id().as_str() != billing.organization_id {
        return Err("billing account belongs to another organization".into());
    }
    let member_count = u64::try_from(state.members.len())?;
    Ok(Snapshot {
        view: AccessView {
            organization_id: billing.organization_id.clone(),
            billing_account_id: state.billing_account_id,
            member_count,
            purchased_seats: billing.purchased_seats,
            available_seats: billing.purchased_seats.saturating_sub(member_count),
        },
        organization_version: state.version,
        billing_version: billing.version,
    })
}

pub fn replay(
    organization: &StreamId,
    organization_events: &[RecordedEvent],
    billing_events: &[RecordedEvent],
) -> BenchResult<Snapshot> {
    join(
        organization,
        replay_organization(organization_events)?,
        &replay_billing(billing_events)?,
    )
}
