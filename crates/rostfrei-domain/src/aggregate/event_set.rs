use super::AggregateDefinition;
use crate::{DeclaredFieldType, DomainEventDescriptor, DomainEventId};

/// Describes the closed set of domain events owned by an aggregate.
pub trait AggregateEventSet<A>: 'static + Sized
where
    A: AggregateDefinition<Event = Self>,
{
    const DOMAIN_EVENTS: &'static [DomainEventDescriptor];

    /// Declaration evidence for an owned event, if supplied by the event set.
    fn declared_field_types(_id: DomainEventId) -> &'static [DeclaredFieldType] {
        &[]
    }
}

/// Empty event set for aggregate definitions used only by the compiled model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoDomainEvents {}

impl<A> AggregateEventSet<A> for NoDomainEvents
where
    A: AggregateDefinition<Event = Self>,
{
    const DOMAIN_EVENTS: &'static [DomainEventDescriptor] = &[];
}
