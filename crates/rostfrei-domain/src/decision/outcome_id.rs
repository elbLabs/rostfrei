use super::DecisionId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DecisionOutcomeId {
    pub decision: DecisionId,
    pub local: &'static str,
}
