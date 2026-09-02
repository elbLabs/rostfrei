use crate::{DomainErrorId, DomainEventId};

use super::ActionId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionDescriptor {
    pub id: ActionId,
    pub label: &'static str,
    pub raises: &'static [DomainEventId],
    pub error: Option<DomainErrorId>,
}
