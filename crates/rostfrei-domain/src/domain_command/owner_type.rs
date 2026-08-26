use super::DomainCommandOwnerId;
use crate::PublicActionOwnerType;

pub trait DomainCommandOwnerType: PublicActionOwnerType {
    const DOMAIN_COMMAND_OWNER_ID: DomainCommandOwnerId;
}
