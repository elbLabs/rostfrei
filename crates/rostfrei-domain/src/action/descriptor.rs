use crate::{DomainErrorId, DomainEventId};

use super::{ActionId, ActionInputDescriptor, ActionOutputDescriptor};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionDescriptor {
    pub id: ActionId,
    pub label: &'static str,
    pub input: Option<ActionInputDescriptor>,
    pub output: Option<ActionOutputDescriptor>,
    pub raises: &'static [DomainEventId],
    pub error: Option<DomainErrorId>,
}
