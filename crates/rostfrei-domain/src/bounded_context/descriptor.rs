use super::BoundedContextId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedContextDescriptor {
    pub id: BoundedContextId,
    pub label: &'static str,
}
