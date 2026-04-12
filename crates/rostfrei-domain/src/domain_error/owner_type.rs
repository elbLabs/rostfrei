use super::DomainErrorOwnerId;

pub trait DomainErrorOwnerType: 'static {
    const DOMAIN_ERROR_OWNER_ID: DomainErrorOwnerId;
}
