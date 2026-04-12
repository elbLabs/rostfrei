use super::ActionOwnerId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ActionId {
    pub owner: ActionOwnerId,
    pub local: &'static str,
}
