use super::{AggregateEventSet, AggregateType};
use crate::{BoundedContextType, EntityDefinition};

/// Supplies the executable relationships of a modeled aggregate.
pub trait AggregateDefinition: AggregateType {
    type Context: BoundedContextType;
    type Root: EntityDefinition<Owner = Self>;
    type Event: AggregateEventSet<Self>;

    /// Declares the aggregate as an anchor: a viewpoint from which inspection
    /// lists every correlation that touched one of its streams. A role, not a
    /// separate model element; it does not change execution.
    const ANCHOR: bool = false;
}
