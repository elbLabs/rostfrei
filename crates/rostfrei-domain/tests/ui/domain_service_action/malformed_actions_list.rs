use rostfrei_domain::DomainService;

struct Context;
trait Actions {}

#[derive(DomainService)]
#[domain(id = "service", label = "Service", context = Context, actions = Actions)]
struct Service;

fn main() {}
