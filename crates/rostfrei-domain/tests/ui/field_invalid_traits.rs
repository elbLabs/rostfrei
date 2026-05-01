use domain::{Aggregate, BoundedContext, DomainIdentity, Entity};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
struct Id(u64);

#[derive(DomainIdentity)]
struct OtherId(u64);

#[derive(DomainIdentity)]
struct WrongId(u64);

struct Plain;

#[derive(Entity)]
#[domain(id = "root", label = "Root")]
struct Root {
    id: Id,
}

impl domain::EntityDefinition for Root {
    type Owner = First;
    type Identity = Id;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
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
    id: OtherId,
}

impl domain::EntityDefinition for OtherRoot {
    type Owner = Second;
    type Identity = OtherId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
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
    id: WrongId,
    #[domain(entity)]
    other: OtherRoot,
    plain_value: Plain,
    #[domain(aggregate_ref = Plain)]
    reference: Plain,
}

impl domain::EntityDefinition for Wrong {
    type Owner = First;
    type Identity = WrongId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
