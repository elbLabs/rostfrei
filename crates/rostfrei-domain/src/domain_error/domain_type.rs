use super::{DomainErrorDescriptor, DomainErrorOwnerType};

pub trait DomainErrorType: 'static {
    type Owner: DomainErrorOwnerType;

    const LOCAL_ID: &'static str;
    const DESCRIPTOR: DomainErrorDescriptor;
}
