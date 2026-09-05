#[derive(EntityLifecycle, Clone, Copy, Eq, PartialEq)]
#[domain(id = "fleet-status", label = "Fleet status")]
#[lifecycle(initial = Active)]
pub enum FleetStatus {
    #[state(id = "active", label = "Active")]
    Active,
    #[state(id = "retired", label = "Retired")]
    Retired,
}
