use rostfrei_domain::{
    Aggregate, BoundedContext, DomainEvent, DomainIdentity, Entity, ValueObject, domain_actions,
};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct Id(u8);

#[derive(Entity)]
#[domain(id = "root", label = "Root", owner = Owner)]
struct Root {
    #[domain(identity)]
    id: Id,
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner", context = Context, root = Root)]
struct Owner;

#[derive(DomainEvent)]
#[domain(id = "renamed", label = "Renamed", owner = Owner)]
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
