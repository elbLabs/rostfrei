use super::AggregateDefinition;
use crate::DomainEventDescriptor;

/// Describes the closed set of domain events owned by an aggregate.
pub trait AggregateEventSet<A>: 'static + Sized
where
    A: AggregateDefinition<Event = Self>,
{
    const DOMAIN_EVENTS: &'static [DomainEventDescriptor];
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
