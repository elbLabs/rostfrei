use crate::{EntityDefinition, SemanticScalarDescriptor};

use super::DomainIdentityDescriptor;

pub trait DomainIdentityType: 'static + Sized {
    type Owner: EntityDefinition<Identity = Self>;

    const DESCRIPTOR: DomainIdentityDescriptor;
    const SEMANTIC_SCALAR: Option<SemanticScalarDescriptor> = None;
}
