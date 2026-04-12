use super::EntityDescriptor;
use crate::{
    ActionDescriptor, AggregateType, DecisionDescriptor, DomainIdentityType,
    EntityLifecycleDescriptor, InvariantDescriptor,
};

pub trait EntityType: 'static {
    type Owner: AggregateType;
    type Identity: DomainIdentityType<Owner = Self>;

    const LOCAL_ID: &'static str;
    const DESCRIPTOR: EntityDescriptor;
    const LIFECYCLE: Option<EntityLifecycleDescriptor> = None;
    const ACTION_CONTRACTS: &'static [&'static [ActionDescriptor]] = &[];
    const DECISION_CONTRACTS: &'static [&'static [DecisionDescriptor]] = &[];
    const INVARIANT_CONTRACTS: &'static [&'static [InvariantDescriptor]] = &[];
}
