use domain::DecisionOutcome;
use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, domain_decision_test, domain_decisions,
};

struct AttachedDecisions;
struct UnattachedDecisions;

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
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

#[derive(DecisionOutcome)]
enum Outcome {
    #[outcome(id = "done", label = "Done")]
    Done,
}

#[domain_decisions(aggregate, group = AttachedDecisions)]
impl Owner {
    #[decision(id = "attached", label = "Attached")]
    fn attached() -> Outcome {
        Outcome::Done
    }
}

#[domain_decisions(aggregate, group = UnattachedDecisions)]
impl Owner {
    #[decision(id = "unattached", label = "Unattached")]
    fn unattached() -> Outcome {
        Outcome::Done
    }
}

#[domain_decision_test(Owner::UNATTACHED)]
fn rejects_the_exact_unattached_group() {}

fn main() {}
