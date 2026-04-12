use super::InvariantId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvariantDescriptor {
    pub id: InvariantId,
    pub label: &'static str,
}
