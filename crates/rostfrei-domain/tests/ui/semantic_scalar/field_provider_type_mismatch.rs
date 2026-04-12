use rostfrei_domain::{
    BoundedContext, ScalarType, SemanticScalar, SemanticScalarDescriptor, ValueObject,
};

struct U64Scalar;

impl SemanticScalar for U64Scalar {
    type Value = u64;

    const DESCRIPTOR: SemanticScalarDescriptor = SemanticScalarDescriptor {
        id: "u64",
        label: "U64",
        representation: ScalarType::U64,
    };
}

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(ValueObject)]
#[domain(id = "mismatch", label = "Mismatch", owner = Context)]
struct Mismatch(#[domain(scalar = U64Scalar)] String);

fn main() {}
