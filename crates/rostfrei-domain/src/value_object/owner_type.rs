use super::ValueObjectOwnerId;

pub trait ValueObjectOwnerType: 'static {
    const VALUE_OBJECT_OWNER_ID: ValueObjectOwnerId;
}
