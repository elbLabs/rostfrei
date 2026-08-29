use super::{ValueObjectDescriptor, ValueObjectOwnerType};
use crate::{ActionDescriptor, InvariantDescriptor};

pub trait ValueObjectType: 'static {
    type Owner: ValueObjectOwnerType;

    const LOCAL_ID: &'static str;
    const DESCRIPTOR: ValueObjectDescriptor;
    const ACTION_CONTRACTS: &'static [&'static [ActionDescriptor]] = &[];
    const INVARIANT_CONTRACTS: &'static [&'static [InvariantDescriptor]] = &[];
}
