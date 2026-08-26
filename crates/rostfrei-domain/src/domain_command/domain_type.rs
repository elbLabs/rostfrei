use super::{DomainCommandDescriptor, DomainCommandOwnerType};

pub trait DomainCommandType: 'static {
    type Owner: DomainCommandOwnerType;

    const LOCAL_ID: &'static str;
    const DESCRIPTOR: DomainCommandDescriptor;
}
