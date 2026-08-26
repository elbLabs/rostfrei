use super::InvariantOwnerType;
use crate::ValueObjectType;

pub trait ValueObjectInvariantOwnerType:
    ValueObjectType + InvariantOwnerType<Candidate = Self> + Sized
{
}
