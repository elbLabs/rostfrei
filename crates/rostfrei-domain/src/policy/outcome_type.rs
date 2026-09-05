use super::PolicyOutcomeDescriptor;

pub trait PolicyOutcomeType: 'static {
    const OUTCOMES: &'static [PolicyOutcomeDescriptor];
}
