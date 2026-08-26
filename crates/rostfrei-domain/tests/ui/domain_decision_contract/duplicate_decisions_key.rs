use domain::DomainService;

struct Context;
trait Decisions {}

#[derive(DomainService)]
#[domain(
    id = "service",
    label = "Service",
    context = Context,
    decisions = [Decisions],
    decisions = [Decisions]
)]
struct Service;

fn main() {}
