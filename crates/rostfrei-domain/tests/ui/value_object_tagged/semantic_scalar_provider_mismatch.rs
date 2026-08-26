use rostfrei_domain::{
    BoundedContext, ScalarType, SemanticScalar, SemanticScalarDescriptor, ValueObject,
};

struct StringScalar;

impl SemanticScalar for StringScalar {
    type Value = String;

    const DESCRIPTOR: SemanticScalarDescriptor = SemanticScalarDescriptor {
        id: "string",
        label: "String",
        representation: ScalarType::String,
    };
}

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(ValueObject)]
#[domain(id = "invalid", label = "Invalid", owner = Context)]
enum Invalid {
    Value(#[domain(scalar = StringScalar)] u64),
}

fn main() {}
