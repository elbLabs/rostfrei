use domain::{DomainEvent, ScalarType, SemanticScalar, SemanticScalarDescriptor};

struct U64Scalar;

impl SemanticScalar for U64Scalar {
    type Value = u64;

    const DESCRIPTOR: SemanticScalarDescriptor = SemanticScalarDescriptor {
        id: "u64",
        label: "U64",
        representation: ScalarType::U64,
    };
}

#[derive(DomainEvent)]
#[domain(id = "mismatch", label = "Mismatch")]
struct Mismatch(#[domain(scalar = U64Scalar)] String);

fn main() {}
