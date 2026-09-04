use rostfrei::DecisionOutcome;

#[derive(DecisionOutcome, Clone, Copy, Debug, Eq, PartialEq)]
pub enum RentalEligibilityOutcome {
    #[outcome(id = "eligible", label = "Eligible")]
    Eligible,
    #[outcome(id = "already-rented", label = "Already rented")]
    AlreadyRented,
    #[outcome(id = "maintenance-required", label = "Maintenance required")]
    MaintenanceRequired,
}
