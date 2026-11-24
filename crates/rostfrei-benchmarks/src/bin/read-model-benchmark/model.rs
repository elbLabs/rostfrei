use std::collections::BTreeSet;

use rostfrei_core::{RecordedEvent, StreamId};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::BenchResult;

/// The same response is returned and JSON-encoded by every measured query path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FleetDockCapacity {
    pub fleet_id: String,
    pub station_id: String,
    pub bicycle_count: u64,
    pub dock_count: u64,
    pub unallocated_docks: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Snapshot {
    pub view: FleetDockCapacity,
    pub fleet_version: u64,
    pub station_version: u64,
}

#[derive(Deserialize, Serialize)]
pub struct Payload<T> {
    pub change: T,
    pub note: String,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum FleetEvent {
    Created { station_id: String },
    BicycleRegistered { bicycle_id: String },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum StationEvent {
    Opened { fleet_id: String },
    DocksInstalled { docks: u64 },
}

/// Rehydrated fleet state includes bicycle identities, while the read
/// model only needs the count. Its first event establishes the aggregate link.
pub struct FleetState {
    pub station_id: String,
    pub bicycles: BTreeSet<String>,
    pub version: u64,
}

pub struct StationState {
    pub fleet_id: String,
    pub dock_count: u64,
    pub version: u64,
}

fn decode<T: DeserializeOwned>(record: &RecordedEvent, event_type: &str) -> BenchResult<T> {
    if record.schema_version() != 1 || record.event_type() != event_type {
        return Err("unexpected benchmark source event schema/type".into());
    }
    let payload: Payload<T> = serde_json::from_slice(record.payload())?;
    Ok(payload.change)
}

pub fn replay_fleet(records: &[RecordedEvent]) -> BenchResult<FleetState> {
    let mut station_id = None;
    let mut bicycles = BTreeSet::new();
    for record in records {
        match decode(record, "fleet-changed")? {
            FleetEvent::Created { station_id: id } => {
                if station_id.replace(id).is_some() {
                    return Err("fleet created twice".into());
                }
            }
            FleetEvent::BicycleRegistered { bicycle_id } => {
                if station_id.is_none() || !bicycles.insert(bicycle_id) {
                    return Err("invalid fleet registration history".into());
                }
            }
        }
    }
    Ok(FleetState {
        station_id: station_id.ok_or("fleet not created")?,
        bicycles,
        version: records
            .last()
            .ok_or("empty fleet history")?
            .stream_version()
            .value(),
    })
}

pub fn replay_station(records: &[RecordedEvent]) -> BenchResult<StationState> {
    let mut fleet_id = None;
    let mut dock_count = 0_u64;
    for record in records {
        match decode(record, "station-changed")? {
            StationEvent::Opened { fleet_id: id } => {
                if fleet_id.replace(id).is_some() {
                    return Err("station opened twice".into());
                }
            }
            StationEvent::DocksInstalled { docks } => {
                if fleet_id.is_none() {
                    return Err("station not opened".into());
                }
                dock_count = dock_count.checked_add(docks).ok_or("dock count overflow")?;
            }
        }
    }
    Ok(StationState {
        fleet_id: fleet_id.ok_or("station not opened")?,
        dock_count,
        version: records
            .last()
            .ok_or("empty station history")?
            .stream_version()
            .value(),
    })
}

pub fn join(fleet: &StreamId, state: FleetState, station: &StationState) -> BenchResult<Snapshot> {
    if fleet.aggregate_id().as_str() != station.fleet_id {
        return Err("station belongs to another fleet".into());
    }
    let bicycle_count = u64::try_from(state.bicycles.len())?;
    Ok(Snapshot {
        view: FleetDockCapacity {
            fleet_id: station.fleet_id.clone(),
            station_id: state.station_id,
            bicycle_count,
            dock_count: station.dock_count,
            unallocated_docks: station.dock_count.saturating_sub(bicycle_count),
        },
        fleet_version: state.version,
        station_version: station.version,
    })
}

pub fn replay(
    fleet: &StreamId,
    fleet_events: &[RecordedEvent],
    station_events: &[RecordedEvent],
) -> BenchResult<Snapshot> {
    join(
        fleet,
        replay_fleet(fleet_events)?,
        &replay_station(station_events)?,
    )
}
