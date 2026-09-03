impl CommandHandler<RentBicycle> for RentalFleetAggregate {
    type Rejection = BicycleUnavailable;

    fn handle(
        instance: &mut AggregateInstance<Self>,
        command: RentBicycle,
    ) -> Result<(), Self::Rejection> {
        instance.rent_bicycle(command.bicycle_id)
    }
}
