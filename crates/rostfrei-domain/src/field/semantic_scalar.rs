use super::ScalarType;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SemanticScalarDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub representation: ScalarType,
}

pub trait SemanticScalar: Sized + 'static {
    type Value: 'static;

    const DESCRIPTOR: SemanticScalarDescriptor;
}
