use domain::{Aggregate, BoundedContext, DomainIdentity, Entity};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox;

struct PlainRoot;

#[derive(Aggregate)]
#[domain(id = "plain-root", label = "Plain Root")]
struct PlainRootAggregate;

impl domain::AggregateDefinition for PlainRootAggregate {
    type Context = Inbox;
    type Root = PlainRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(DomainIdentity)]
struct Id(u64);

#[derive(Entity)]
#[domain(id = "other-root", label = "Other")]
struct OtherRoot {
    id: Id,
}

impl domain::EntityDefinition for OtherRoot {
    type Owner = OtherAggregate;
    type Identity = Id;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Aggregate)]
#[domain(id = "wrong-owner", label = "Wrong Owner")]
struct WrongOwner;

impl domain::AggregateDefinition for WrongOwner {
    type Context = Inbox;
    type Root = OtherRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(Aggregate)]
#[domain(id = "other", label = "Other")]
struct OtherAggregate;

impl domain::AggregateDefinition for OtherAggregate {
    type Context = Inbox;
    type Root = OtherRoot;
    type Event = domain::NoDomainEvents;
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
