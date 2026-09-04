use super::InvalidRentalFleet;
use crate::domain::bike_rental::rental_fleet::fleet_consistency::FleetConsistency;
use crate::domain::rental_fleet::{RentalFleet, RentalFleetAggregate};

pub(super) fn validate(candidate: &RentalFleet) -> Result<(), InvalidRentalFleet> {
    RentalFleetAggregate::unique_bicycle_identities(candidate)
        .map_or(Ok(()), |violation| Err(violation.into()))
}
