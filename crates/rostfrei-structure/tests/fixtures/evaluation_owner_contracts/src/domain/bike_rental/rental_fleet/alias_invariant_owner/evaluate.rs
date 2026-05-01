use model::OtherAggregate as RentalFleetAggregate;

impl AliasInvariantOwner for RentalFleetAggregate {
    fn validate(&self) {}
}
