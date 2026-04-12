use super::InternalActionOwnerType;
use crate::EntityType;

pub trait EntityActionOwnerType: InternalActionOwnerType + EntityType {}
