#![allow(dead_code)]

use domain::{DomainEvent, ScalarType, SemanticScalar, SemanticScalarDescriptor};

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

#[derive(DomainEvent)]
#[domain(id = "custom-fields", label = "Custom fields")]
struct CustomFields {
    #[domain(scalar = UuidScalar)]
    values: Option<Vec<uuid::Uuid>>,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
