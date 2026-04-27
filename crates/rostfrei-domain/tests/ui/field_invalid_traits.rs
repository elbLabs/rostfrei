use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, ValueObject};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct Id(u64);

#[derive(DomainIdentity)]
#[domain(owner = OtherRoot)]
struct OtherId(u64);

#[derive(DomainIdentity)]
#[domain(owner = Wrong)]
struct WrongId(u64);

struct Plain;

#[derive(Entity)]
#[domain(id = "root", label = "Root")]
struct Root {
    #[domain(identity)]
    id: Id,
}

impl domain::EntityDefinition for Root {
    type Owner = First;
    type Identity = Id;
}

#[derive(Aggregate)]
#[domain(id = "first", label = "First")]
struct First;

impl domain::AggregateDefinition for First {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

#[derive(Entity)]
#[domain(id = "other-root", label = "Other root")]
struct OtherRoot {
    #[domain(identity)]
    id: OtherId,
}

impl domain::EntityDefinition for OtherRoot {
    type Owner = Second;
    type Identity = OtherId;
}

#[derive(Aggregate)]
#[domain(id = "second", label = "Second")]
struct Second;

impl domain::AggregateDefinition for Second {
    type Context = Context;
    type Root = OtherRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(Entity)]
#[domain(id = "wrong", label = "Wrong")]
struct Wrong {
    #[domain(identity)]
    id: WrongId,
    #[domain(entity)]
    other: OtherRoot,
    #[domain(value_object)]
    plain_value: Plain,
    #[domain(aggregate_ref = Plain)]
    reference: Plain,
}

impl domain::EntityDefinition for Wrong {
    type Owner = First;
    type Identity = WrongId;
}

#[derive(ValueObject)]
#[domain(id = "wrong-value", label = "Wrong value", owner = Context)]
struct WrongValue(#[domain(value_object)] Plain);

fn main() {}
