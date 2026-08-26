#![allow(unused, non_snake_case)]

use rostfrei_domain::{BoundedContext, ValueObject};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

trait Invariants {
    fn valid(candidate: &Value) -> Option<rostfrei_domain::InvariantViolation>;

    fn __DOMAIN_INVARIANTS_APPEND_VIOLATIONS(
        candidate: &Value,
        violations: &mut Vec<rostfrei_domain::InvariantViolation>,
    ) {
        if let Some(violation) = Self::valid(candidate) {
            violations.push(violation);
        }
    }
}

#[derive(ValueObject)]
#[domain(
    id = "value",
    label = "Value",
    owner = Context,
    invariants = [Invariants]
)]
struct Value(u8);

impl Invariants for Value {
    fn valid(candidate: &Value) -> Option<rostfrei_domain::InvariantViolation> {
        let _ = candidate;
        None
    }
}

fn main() {}
