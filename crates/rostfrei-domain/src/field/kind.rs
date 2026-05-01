use crate::{AggregateId, EntityId};

use super::{ScalarType, SemanticScalarDescriptor};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum FieldKind {
    Scalar(ScalarType),
    SemanticScalar(SemanticScalarDescriptor),
    Entity(EntityId),
    AggregateReference(AggregateId),
    Opaque,
}
