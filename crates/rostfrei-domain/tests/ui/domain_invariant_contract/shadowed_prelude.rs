#![allow(unused, non_snake_case)]

#[derive(domain::BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(domain::DomainIdentity)]
#[domain(owner = Root)]
struct RootId(u8);

#[derive(domain::Entity)]
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

    #[domain::domain_invariants(aggregate)]
    pub(crate) trait Invariants {
        #[invariant(id = "valid", label = "Valid")]
        fn valid(
            candidate: &<Self as domain::InvariantOwnerType>::Candidate,
        ) -> ::core::option::Option<domain::InvariantViolation>;
    }

    #[derive(domain::Aggregate)]
#[domain(id = "owner", label = "Owner")]
pub(crate) struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

    impl Invariants for Owner {
        fn valid(
            candidate: &<Self as domain::InvariantOwnerType>::Candidate,
        ) -> ::core::option::Option<domain::InvariantViolation> {
            let _ = candidate;
            ::core::option::Option::None
        }
    }

    pub(crate) fn validate(candidate: &Root) {
        let validation =
            <Owner as domain::InvariantOwnerType>::validate_invariants(candidate);
        match validation {
            ::core::result::Result::Ok(()) => {}
            ::core::result::Result::Err(_) => {}
        }
    }
}

fn main() {
    shadowed::validate(&Root { id: RootId(1) });
}
