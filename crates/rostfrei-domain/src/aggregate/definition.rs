use super::{AggregateEventSet, AggregateType};
use crate::{BoundedContextType, EntityType};

/// Supplies the executable relationships of a modeled aggregate.
pub trait AggregateDefinition: AggregateType {
    type Context: BoundedContextType;
    type Root: EntityType<Owner = Self>;
    type Event: AggregateEventSet<Self>;
}
