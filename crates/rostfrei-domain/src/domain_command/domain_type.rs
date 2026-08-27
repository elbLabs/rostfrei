use super::{DomainCommandDescriptor, DomainCommandOwnerType};

pub trait DomainCommandType: 'static {
    type Owner: DomainCommandOwnerType;
    type Rejection: 'static;

    const LOCAL_ID: &'static str;
    const SCHEMA_VERSION: u32;
    const DESCRIPTOR: DomainCommandDescriptor;
}
