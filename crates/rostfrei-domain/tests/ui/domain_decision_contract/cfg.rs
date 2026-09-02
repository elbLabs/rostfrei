#![allow(dead_code)]

use domain::DecisionOutcome;
use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, domain_decisions};

struct Decisions;

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
struct RootId(u8);

#[derive(Entity)]
#[domain(id = "root", label = "Root")]
struct Root {
    #[domain(identity)]
    id: RootId,
}

impl domain::EntityDefinition for Root {
    type Owner = Owner;
    type Identity = RootId;
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

#[derive(DecisionOutcome)]
enum Outcome {
    #[outcome(id = "enabled", label = "Enabled")]
    Enabled,
    #[cfg(any())]
    #[outcome(id = "disabled", label = "Disabled")]
    Disabled,
}

#[domain_decisions(aggregate, group = Decisions)]
impl Owner {
    #[decision(id = "enabled", label = "Enabled")]
    #[cfg_attr(all(), inline)]
    fn enabled() -> Outcome {
        Outcome::Enabled
    }

    #[cfg(any())]
    #[decision(id = "disabled", label = "Disabled")]
    fn disabled() -> Outcome {
        Outcome::Disabled
    }

    #[cfg_attr(all(), cfg(any()))]
    #[decision(id = "conditionally-disabled", label = "Conditionally disabled")]
    fn conditionally_disabled() -> Outcome {
        Outcome::Enabled
    }
}

#[cfg(any())]
struct DisabledDecisions;

#[cfg(any())]
struct DisabledOwner;

#[domain_decisions(aggregate, group = DisabledDecisions)]
#[cfg_attr(all(), cfg(any()))]
impl DisabledOwner {
    #[decision(id = "disabled", label = "Disabled")]
    fn disabled() -> Outcome {
        Outcome::Enabled
    }
}

fn main() {
    let _ = Owner::enabled();
}
