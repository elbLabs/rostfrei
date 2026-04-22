use domain::{
    Aggregate, BoundedContext, Command, DomainIdentity, Entity, domain_actions,
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
#[domain(id = "other", label = "Other", context = Context, root = OtherRoot)]
struct Other;

#[derive(Command)]
#[domain(id = "change", label = "Change", owner = Other)]
pub struct Change;

#[domain_actions(aggregate)]
pub trait Actions {
    #[action(id = "change", label = "Change")]
    fn change(root: &mut Root, input: Change);
}

#[derive(Aggregate)]
#[domain(
    id = "owner",
    label = "Owner",
    context = Context,
    root = Root,
    actions = [Actions]
)]
struct Owner;

impl Actions for Owner {
    fn change(root: &mut Root, input: Change) {
        let _ = (root, input);
    }
}

fn main() {}
