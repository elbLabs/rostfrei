use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, domain_decisions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct RootId(u8);

#[derive(Entity)]
#[domain(id = "root", label = "Root", owner = Owner)]
struct Root {
    #[domain(identity)]
    id: RootId,
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner", context = Context, root = Root, decisions)]
struct Owner;

#[domain_decisions(aggregate)]
impl Owner {
    #[decision(id = "first", label = "First")]
    fn first() -> Result<(), ()> {
        Ok(())
    }
}

#[domain_decisions(aggregate)]
impl Owner {
    #[decision(id = "second", label = "Second")]
    fn second() -> Result<(), ()> {
        Ok(())
    }
}

fn main() {}
