#![allow(unused, non_snake_case)]

use rostfrei_domain::DomainService;

struct Context;
trait Invariants {}

#[derive(DomainService)]
#[domain(
    id = "service",
    label = "Service",
    context = Context,
    invariants = [Invariants]
)]
struct Service;

fn main() {}
