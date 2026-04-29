use crate::DomainErrorId;

use super::ActionId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionDescriptor {
    pub id: ActionId,
    pub label: &'static str,
    pub error: Option<DomainErrorId>,
}
