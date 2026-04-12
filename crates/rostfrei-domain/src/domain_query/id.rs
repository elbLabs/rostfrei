use crate::AggregateId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct QueryId {
    pub aggregate: AggregateId,
    pub local: &'static str,
}
