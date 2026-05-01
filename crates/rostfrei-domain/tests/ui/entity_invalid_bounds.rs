use domain::{Aggregate, BoundedContext, DomainIdentity, Entity};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox;

struct PlainId(u64);

#[derive(Entity)]
#[domain(id = "plain-id-root", label = "Plain Id")]
struct PlainIdRoot {
    id: PlainId,
}

impl domain::EntityDefinition for PlainIdRoot {
    type Owner = PlainIdAggregate;
    type Identity = PlainId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
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
struct ValidId(u64);

#[derive(Entity)]
#[domain(id = "missing-owner-root", label = "Missing Owner")]
struct MissingOwnerRoot {
    id: ValidId,
}

impl domain::EntityDefinition for MissingOwnerRoot {
    type Owner = MissingAggregate;
    type Identity = ValidId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

fn main() {}
