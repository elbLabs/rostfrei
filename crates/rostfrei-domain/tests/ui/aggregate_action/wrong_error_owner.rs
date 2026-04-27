use domain::{
    Aggregate, BoundedContext, DomainError, DomainIdentity, Entity, domain_actions,
};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct Id(u8);

#[derive(Entity)]
#[domain(id = "root", label = "Root", owner = Owner)]
pub struct Root {
    #[domain(identity)]
    id: Id,
}

#[derive(DomainIdentity)]
#[domain(owner = OtherRoot)]
struct OtherId(u8);

#[derive(Entity)]
#[domain(id = "other-root", label = "Other root", owner = Other)]
struct OtherRoot {
    #[domain(identity)]
    id: OtherId,
}

#[derive(Aggregate)]
#[domain(id = "other", label = "Other")]
struct Other;

impl domain::AggregateDefinition for Other {
    type Context = Context;
    type Root = OtherRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(DomainError)]
#[domain(
    id = "denied",
    label = "Denied",
    owner = Other,
    code = "DENIED",
    message = "Denied."
)]
pub struct Denied;

#[domain_actions(aggregate)]
pub trait Actions {
    #[action(id = "change", label = "Change")]
    fn change(root: &mut Root) -> Result<(), Denied>;
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
    fn change(root: &mut Root) -> Result<(), Denied> {
        let _ = root;
        Ok(())
    }
}

fn main() {}
