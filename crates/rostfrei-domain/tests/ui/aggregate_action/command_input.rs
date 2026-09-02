use domain::{
    Aggregate, BoundedContext, Command, DomainIdentity, Entity, domain_actions,
};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
struct Id(u8);

#[derive(Entity)]
#[domain(id = "root", label = "Root")]
pub struct Root {
    #[domain(identity)]
    id: Id,
}

impl domain::EntityDefinition for Root {
    type Owner = Owner;
    type Identity = Id;
}

#[derive(DomainIdentity)]
struct OtherId(u8);

#[derive(Entity)]
#[domain(id = "other-root", label = "Other root")]
struct OtherRoot {
    #[domain(identity)]
    id: OtherId,
}

impl domain::EntityDefinition for OtherRoot {
    type Owner = Other;
    type Identity = OtherId;
}

#[derive(Aggregate)]
#[domain(id = "other", label = "Other")]
struct Other;

impl domain::AggregateDefinition for Other {
    type Context = Context;
    type Root = OtherRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(Command)]
#[domain(id = "change", label = "Change", owner = Other)]
pub struct Change;

#[domain_actions(aggregate)]
pub trait Actions {
    #[action(id = "change", label = "Change")]
    fn change(root: &mut Root, input: Change);
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

impl Actions for Owner {
    fn change(root: &mut Root, input: Change) {
        let _ = (root, input);
    }
}

fn main() {}
