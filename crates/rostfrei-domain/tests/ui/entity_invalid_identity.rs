use domain::{Aggregate, BoundedContext, DomainIdentity, Entity};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
struct Id(u64);

#[derive(DomainIdentity)]
struct OtherId(u64);

#[derive(Entity)]
#[domain(id = "missing-accessor", label = "Missing accessor")]
struct MissingAccessor {
    id: Id,
}

impl domain::EntityDefinition for MissingAccessor {
    type Owner = MissingAccessorAggregate;
    type Identity = Id;
}

#[derive(Aggregate)]
#[domain(id = "missing-accessor", label = "Missing accessor")]
struct MissingAccessorAggregate;

impl domain::AggregateDefinition for MissingAccessorAggregate {
    type Context = Context;
    type Root = MissingAccessor;
    type Event = domain::NoDomainEvents;
}

#[derive(Entity)]
#[domain(id = "wrong-accessor", label = "Wrong accessor")]
struct WrongAccessor {
    id: Id,
    other_id: OtherId,
}

impl domain::EntityDefinition for WrongAccessor {
    type Owner = WrongAccessorAggregate;
    type Identity = Id;

    fn identity(&self) -> &Self::Identity {
        &self.other_id
    }
}

#[derive(Aggregate)]
#[domain(id = "wrong-accessor", label = "Wrong accessor")]
struct WrongAccessorAggregate;

impl domain::AggregateDefinition for WrongAccessorAggregate {
    type Context = Context;
    type Root = WrongAccessor;
    type Event = domain::NoDomainEvents;
}

fn main() {}
