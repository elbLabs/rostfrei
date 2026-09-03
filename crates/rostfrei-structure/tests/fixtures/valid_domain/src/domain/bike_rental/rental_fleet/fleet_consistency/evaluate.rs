impl FleetConsistency for RentalFleetAggregate {
    fn unique_bicycle_identities(candidate: &RentalFleet) -> Option<InvariantViolation> {
        candidate.find_duplicate_bicycle_identity()
    }
}
