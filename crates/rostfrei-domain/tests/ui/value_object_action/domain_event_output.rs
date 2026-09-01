use domain::{
    Aggregate, BoundedContext, DomainEvent, DomainIdentity, Entity, ValueObject, domain_actions,
};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct Id(u8);

#[derive(Entity)]
#[domain(id = "root", label = "Root")]
struct Root {
    #[domain(identity)]
    id: Id,
}

impl domain::EntityDefinition for Root {
    type Owner = Owner;
    type Identity = Id;
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = OwnerEvents;
}

#[derive(domain::AggregateEvents)]
enum OwnerEvents {
    Event0(Renamed),
}

#[derive(DomainEvent)]
#[domain(id = "renamed", label = "Renamed")]
struct Renamed;

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "rename", label = "Rename")]
    fn rename(self) -> Renamed;
}

#[derive(ValueObject)]
#[domain(id = "name", label = "Name", owner = Owner, actions = [Actions])]
struct Name(String);

impl Actions for Name {
    fn rename(self) -> Renamed {
        Renamed
    }
}

fn main() {}
