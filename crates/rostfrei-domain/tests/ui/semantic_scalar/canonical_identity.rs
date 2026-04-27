#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, DomainIdentityType, Entity, ScalarType,
};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root)]
struct RootId(u64);

#[derive(Entity)]
#[domain(id = "root", label = "Root", owner = Owner)]
struct Root {
    #[domain(identity)]
    id: RootId,
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

const _: () = assert!(matches!(RootId::DESCRIPTOR.scalar, ScalarType::U64));
const _: () = assert!(RootId::SEMANTIC_SCALAR.is_none());

fn main() {}
