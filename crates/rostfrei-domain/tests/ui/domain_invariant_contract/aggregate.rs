#![allow(unused, non_snake_case)]

#![allow(dead_code)]

use rostfrei_domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, InvariantOwnerType, InvariantViolation,
    domain_invariants,
};

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

#[domain_invariants(aggregate)]
trait Invariants {
    #[invariant(id = "valid", label = "Valid")]
    fn valid(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;
}

#[derive(Aggregate)]
#[domain(
    id = "owner",
    label = "Owner",
    context = Context,
    root = Root,
    invariants = [Invariants]
)]
struct Owner;

impl Invariants for Owner {
    fn valid(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation> {
        let _ = candidate;
        None
    }
}

fn main() {}
