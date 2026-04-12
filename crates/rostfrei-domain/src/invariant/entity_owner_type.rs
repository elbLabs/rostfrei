use super::InvariantOwnerType;
use crate::EntityType;

pub trait EntityInvariantOwnerType:
    EntityType + InvariantOwnerType<Candidate = Self> + Sized
{
}
