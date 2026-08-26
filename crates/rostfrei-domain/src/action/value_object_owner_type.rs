use super::InternalActionOwnerType;
use crate::ValueObjectType;

pub trait ValueObjectActionOwnerType: InternalActionOwnerType + ValueObjectType {}
