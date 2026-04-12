use crate::BoundedContextId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DomainServiceId {
    pub context: BoundedContextId,
    pub local: &'static str,
}
