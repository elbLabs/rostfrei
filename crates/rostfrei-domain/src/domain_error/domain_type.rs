use super::{DomainErrorDescriptor, DomainErrorId};
use crate::FieldDescriptor;

pub trait DomainError: 'static {
    const LOCAL_ID: &'static str;
    const LABEL: &'static str;
    const CODE: &'static str;
    const MESSAGE: &'static str;
    const FIELDS: &'static [FieldDescriptor];
    const DESCRIPTOR: DomainErrorDescriptor = DomainErrorDescriptor {
        id: DomainErrorId(Self::LOCAL_ID),
        label: Self::LABEL,
        code: Self::CODE,
        message: Self::MESSAGE,
        fields: Self::FIELDS,
    };
}
