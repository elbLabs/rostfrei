#![allow(unused, non_snake_case)]

#[derive(rostfrei_domain::BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(rostfrei_domain::DomainIdentity)]
#[domain(owner = Root)]
struct RootId(u8);

#[derive(rostfrei_domain::Entity)]
#[domain(id = "root", label = "Root", owner = shadowed::Owner)]
struct Root {
    #[domain(identity)]
    id: RootId,
}

mod shadowed {
    use super::{Context, Root};

    struct Result;
    struct Vec;
    struct Some;
    struct Ok;
    struct Err;

    #[rostfrei_domain::domain_invariants(aggregate)]
    pub(crate) trait Invariants {
        #[invariant(id = "valid", label = "Valid")]
        fn valid(
            candidate: &<Self as rostfrei_domain::InvariantOwnerType>::Candidate,
        ) -> ::core::option::Option<rostfrei_domain::InvariantViolation>;
    }

    #[derive(rostfrei_domain::Aggregate)]
    #[domain(
        id = "owner",
        label = "Owner",
        context = Context,
        root = Root,
        invariants = [Invariants]
    )]
    pub(crate) struct Owner;

    impl Invariants for Owner {
        fn valid(
            candidate: &<Self as rostfrei_domain::InvariantOwnerType>::Candidate,
        ) -> ::core::option::Option<rostfrei_domain::InvariantViolation> {
            let _ = candidate;
            ::core::option::Option::None
        }
    }

    pub(crate) fn validate(candidate: &Root) {
        let validation =
            <Owner as rostfrei_domain::InvariantOwnerType>::validate_invariants(candidate);
        match validation {
            ::core::result::Result::Ok(()) => {}
            ::core::result::Result::Err(_) => {}
        }
    }
}

fn main() {
    shadowed::validate(&Root { id: RootId(1) });
}
