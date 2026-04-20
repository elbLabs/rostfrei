use domain::{Aggregate, BoundedContext, DomainIdentity, Entity, domain_decisions};
use domain::DecisionOutcome;

struct Decisions;

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

#[derive(DecisionOutcome)]
enum Outcome {
    #[outcome(id = "done", label = "Done")]
    Done,
}

#[domain_decisions(entity, group = Decisions)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide() -> Outcome {
        Outcome::Done
    }
}

fn main() {}
