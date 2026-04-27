#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, DomainIdentityType, Entity, ScalarType,
    SemanticScalar, SemanticScalarDescriptor,
};

mod uuid {
    pub struct Uuid;
}

struct UuidScalar;

impl SemanticScalar for UuidScalar {
    type Value = uuid::Uuid;

    const DESCRIPTOR: SemanticScalarDescriptor = SemanticScalarDescriptor {
        id: "uuid",
        label: "UUID",
        representation: ScalarType::String,
    };
}

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(DomainIdentity)]
#[domain(owner = Root, scalar = UuidScalar)]
struct RootId(uuid::Uuid);

#[derive(Entity)]
#[domain(id = "root", label = "Root")]
struct Root {
    #[domain(identity)]
    id: RootId,
}

impl domain::EntityDefinition for Root {
    type Owner = Owner;
    type Identity = RootId;
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Context;
    type Root = Root;
    type Event = domain::NoDomainEvents;
}

const _: Option<SemanticScalarDescriptor> = RootId::SEMANTIC_SCALAR;
const _: ScalarType = RootId::DESCRIPTOR.scalar;

fn main() {}
