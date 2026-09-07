#[async_trait]
impl CommandHandler<RentBicycle> for RentalFleetAggregate {
    type Rejection = BicycleUnavailable;

    async fn handle(
        command: &RentBicycle,
        primary: &mut AggregateInstance<Self>,
        _context: &mut CommandContext<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        match primary.rent_bicycle(command.bicycle_id) {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(rejection) => Ok(CommandDecision::Rejected(rejection)),
        }
    }
}
