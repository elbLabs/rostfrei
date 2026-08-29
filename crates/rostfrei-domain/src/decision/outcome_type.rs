use super::DecisionOutcomeDescriptor;

pub trait DecisionOutcomeType: 'static {
    const OUTCOMES: &'static [DecisionOutcomeDescriptor];
}
