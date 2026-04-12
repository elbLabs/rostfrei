use crate::BoundedContextId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AggregateId {
    pub context: BoundedContextId,
    pub local: &'static str,
}
