use rostfrei::DecisionOutcome;

#[derive(DecisionOutcome, Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationNumberFormat {
    #[outcome(id = "compact", label = "Compact")]
    Compact,
    #[outcome(id = "segmented", label = "Segmented")]
    Segmented,
}
