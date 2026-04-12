use crate::{EntityType, SemanticScalarDescriptor};

use super::DomainIdentityDescriptor;

pub trait DomainIdentityType: 'static + Sized {
    type Owner: EntityType<Identity = Self>;

    const DESCRIPTOR: DomainIdentityDescriptor;
    const SEMANTIC_SCALAR: Option<SemanticScalarDescriptor> = None;
}
