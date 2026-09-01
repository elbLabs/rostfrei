use domain::{Aggregate, BoundedContext, DomainIdentity, Entity};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox;

struct PlainId(u64);

#[derive(Entity)]
#[domain(id = "plain-id-root", label = "Plain Id", owner = PlainIdAggregate)]
struct PlainIdRoot {
    #[domain(identity)]
    id: PlainId,
}

#[derive(Aggregate)]
#[domain(id = "plain-id", label = "Plain Id")]
struct PlainIdAggregate;

impl domain::AggregateDefinition for PlainIdAggregate {
    type Context = Inbox;
    type Root = PlainIdRoot;
    type Event = domain::NoDomainEvents;
}

struct MissingAggregate;

#[derive(DomainIdentity)]
#[domain(owner = MissingOwnerRoot)]
struct ValidId(u64);

#[derive(Entity)]
#[domain(id = "missing-owner-root", label = "Missing Owner", owner = MissingAggregate)]
struct MissingOwnerRoot {
    #[domain(identity)]
    id: ValidId,
}

fn main() {}
