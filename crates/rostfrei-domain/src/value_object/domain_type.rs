use super::{ValueObjectDescriptor, ValueObjectOwnerType};
use crate::{ActionDescriptor, DecisionDescriptor, InvariantDescriptor};

pub trait ValueObjectType: 'static {
    type Owner: ValueObjectOwnerType;

    const LOCAL_ID: &'static str;
    const DESCRIPTOR: ValueObjectDescriptor;
    const ACTION_CONTRACTS: &'static [&'static [ActionDescriptor]] = &[];
    const DECISION_CONTRACTS: &'static [&'static [DecisionDescriptor]] = &[];
    const INVARIANT_CONTRACTS: &'static [&'static [InvariantDescriptor]] = &[];
}
