use super::ValueObjectOwnerId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ValueObjectId {
    pub owner: ValueObjectOwnerId,
    pub local: &'static str,
}
