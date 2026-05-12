impl RentalAssessmentPolicy for RentalFleetAggregate {
    fn assess_rental(&self) -> RentalAssessment {
        RentalAssessment::Eligible
    }
}
