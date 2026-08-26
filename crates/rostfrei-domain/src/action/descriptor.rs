use crate::DomainErrorId;

use super::{ActionId, ActionInputDescriptor, ActionOutputDescriptor};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionDescriptor {
    pub id: ActionId,
    pub label: &'static str,
    pub input: Option<ActionInputDescriptor>,
    pub output: Option<ActionOutputDescriptor>,
    pub error: Option<DomainErrorId>,
}
