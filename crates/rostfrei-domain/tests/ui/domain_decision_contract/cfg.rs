#![allow(dead_code)]

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
    #[decision(id = "enabled", label = "Enabled")]
    #[cfg_attr(all(), inline)]
    fn enabled() -> Result<(), ()> {
        Ok(())
    }

    #[cfg(any())]
    #[decision(id = "disabled", label = "Disabled")]
    fn disabled() -> Result<(), ()> {
        Ok(())
    }

    #[cfg_attr(all(), cfg(any()))]
    #[decision(id = "conditionally-disabled", label = "Conditionally disabled")]
    fn conditionally_disabled() -> Result<(), ()> {
        Ok(())
    }
}

#[domain_decisions(aggregate)]
#[cfg_attr(all(), cfg(any()))]
impl DisabledOwner {
    #[decision(id = "disabled", label = "Disabled")]
    fn disabled() -> Result<(), ()> {
        Ok(())
    }
}

fn main() {
    assert_eq!(Owner::enabled(), Ok(()));
}
