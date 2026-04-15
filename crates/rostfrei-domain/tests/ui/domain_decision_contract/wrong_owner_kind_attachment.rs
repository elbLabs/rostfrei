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
#[domain(id = "owner", label = "Owner", context = Context, root = Root)]
struct Owner;

#[domain_decisions(entity)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide() -> Result<(), ()> {
        Ok(())
    }
}

fn main() {}
