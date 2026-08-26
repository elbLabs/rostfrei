use domain::DomainService;

struct Context;
trait Actions {}

#[derive(DomainService)]
#[domain(
    id = "service",
    label = "Service",
    context = Context,
    actions = [Actions, Actions]
)]
struct Service;

fn main() {}
