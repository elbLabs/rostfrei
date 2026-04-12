#![allow(dead_code)]

use rostfrei_domain::{
    BoundedContext, ScalarType, SemanticScalar, SemanticScalarDescriptor, ValueObject,
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

#[derive(ValueObject)]
#[domain(id = "custom-fields", label = "Custom fields", owner = Context)]
struct CustomFields {
    #[domain(scalar = UuidScalar)]
    values: Option<Vec<uuid::Uuid>>,
}

fn main() {}
