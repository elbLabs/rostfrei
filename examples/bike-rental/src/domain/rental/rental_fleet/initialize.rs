use rostfrei::{Initialize, StreamId};

use super::{FleetId, RentalFleet, RentalFleetAggregate};

impl Initialize<RentalFleetAggregate> for RentalFleet {
    fn initialize(stream_id: &StreamId) -> Self {
        Self::new(FleetId::from(stream_id.aggregate_id()), Vec::new())
    }
}
