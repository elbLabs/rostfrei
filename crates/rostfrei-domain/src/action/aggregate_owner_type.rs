use super::PublicActionOwnerType;
use crate::AggregateType;

pub trait AggregateActionOwnerType: PublicActionOwnerType + AggregateType {}
