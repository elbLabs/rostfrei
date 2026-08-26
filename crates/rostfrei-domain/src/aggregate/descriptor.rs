use super::AggregateId;
use crate::EntityId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AggregateDescriptor {
    pub id: AggregateId,
    pub label: &'static str,
    pub root: EntityId,
}
