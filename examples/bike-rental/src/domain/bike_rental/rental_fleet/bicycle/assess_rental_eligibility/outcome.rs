use rostfrei::DecisionOutcome;

#[derive(DecisionOutcome, Clone, Copy, Debug, Eq, PartialEq)]
pub enum RentalEligibilityOutcome {
    #[outcome(id = "eligible", label = "Eligible")]
    Eligible,
    #[outcome(id = "unavailable-status", label = "Unavailable status")]
    UnavailableStatus,
    #[outcome(id = "maintenance-required", label = "Maintenance required")]
    MaintenanceRequired,
}
