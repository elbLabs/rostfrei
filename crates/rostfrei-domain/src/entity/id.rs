use crate::AggregateId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EntityId {
    pub aggregate: AggregateId,
    pub local: &'static str,
}
