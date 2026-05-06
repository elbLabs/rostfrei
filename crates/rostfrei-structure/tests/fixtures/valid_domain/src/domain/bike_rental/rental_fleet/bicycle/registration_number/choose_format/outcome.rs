#[derive(PolicyOutcome)]
pub enum RegistrationFormat {
    #[outcome(id = "compact", label = "Compact")]
    Compact,
    #[outcome(id = "spaced", label = "Spaced")]
    Spaced,
}
