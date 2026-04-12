#![allow(unused, non_snake_case)]

#![allow(dead_code)]

use domain::{
    BoundedContext, InvariantOwnerType, InvariantViolation, ValueObject, domain_invariants,
};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[domain_invariants(value_object)]
trait Invariants {
    #[invariant(id = "valid", label = "Valid")]
    fn valid(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;
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
    fn valid(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation> {
        let _ = candidate;
        None
    }
}

fn main() {}
