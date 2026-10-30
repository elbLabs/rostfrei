use super::{AggregateEventSet, AggregateType};
use crate::{BoundedContextType, EntityDefinition};

/// Supplies the executable relationships of a modeled aggregate.
pub trait AggregateDefinition: AggregateType {
    type Context: BoundedContextType;
    type Root: EntityDefinition<Owner = Self>;
    type Event: AggregateEventSet<Self>;

    /// Makes this aggregate a correlation-inspection anchor without changing execution.
    const ANCHOR: bool = false;
}
