use super::EntityDescriptor;

pub trait EntityType: 'static {
    const LOCAL_ID: &'static str;
    const DESCRIPTOR: EntityDescriptor;
}
