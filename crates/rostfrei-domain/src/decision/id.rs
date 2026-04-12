use super::DecisionOwnerId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DecisionId {
    pub owner: DecisionOwnerId,
    pub local: &'static str,
}
