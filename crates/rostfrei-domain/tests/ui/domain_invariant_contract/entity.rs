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

#[domain_invariants(entity)]
trait Invariants {
    #[invariant(id = "valid", label = "Valid")]
    fn valid(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;
}

#[derive(Entity)]
#[domain(
    id = "root",
    label = "Root",
    owner = Owner,
    invariants = [Invariants]
)]
struct Root {
    #[domain(identity)]
    id: RootId,
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner", context = Context, root = Root)]
struct Owner;

impl Invariants for Root {
    fn valid(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation> {
        let _ = candidate;
        None
    }
}

fn main() {}
