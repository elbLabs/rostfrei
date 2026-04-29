use crate::{AggregateId, DomainIdentityId, EntityId, ValueObjectId};

use super::{ScalarType, SemanticScalarDescriptor};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum FieldKind {
    Scalar(ScalarType),
    SemanticScalar(SemanticScalarDescriptor),
    DomainIdentity(DomainIdentityId),
    Entity(EntityId),
    ValueObject(ValueObjectId),
    AggregateReference(AggregateId),
    Opaque,
}
