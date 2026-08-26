use super::InvariantOwnerId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InvariantId {
    pub owner: InvariantOwnerId,
    pub local: &'static str,
}
