use rostfrei::InvariantViolation;

use super::FleetConsistency;
use crate::domain::rental_fleet::{RentalFleet, RentalFleetAggregate};

impl FleetConsistency for RentalFleetAggregate {
    fn unique_bicycle_identities(candidate: &RentalFleet) -> Option<InvariantViolation> {
        candidate
            .bicycles()
            .iter()
            .enumerate()
            .any(|(position, bicycle)| {
                candidate
                    .bicycles()
                    .iter()
                    .skip(position.saturating_add(1))
                    .any(|other| other.bicycle_id() == bicycle.bicycle_id())
            })
            .then(|| InvariantViolation::new("bicycles", "bicycle identities must be unique"))
    }
}
