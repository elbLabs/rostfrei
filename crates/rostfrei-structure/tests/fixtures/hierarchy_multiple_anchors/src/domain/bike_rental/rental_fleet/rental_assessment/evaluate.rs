impl RentalAssessmentPolicy for RentalFleetAggregate {
    fn assess_rental(&self) -> bool {
        true
    }
}
