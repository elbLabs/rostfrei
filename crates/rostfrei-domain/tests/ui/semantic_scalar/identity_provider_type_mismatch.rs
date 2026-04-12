use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, ScalarType, SemanticScalar,
    SemanticScalarDescriptor,
};

mod uuid {
    pub struct Uuid;
}

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

#[derive(DomainIdentity)]
#[domain(owner = Root, scalar = U64Scalar)]
struct RootId(uuid::Uuid);

#[derive(Entity)]
#[domain(id = "root", label = "Root", owner = Owner)]
struct Root {
    #[domain(identity)]
    id: RootId,
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner", context = Context, root = Root)]
struct Owner;

fn main() {}
