use crate::{DomainEventId, ScalarType, ValueObjectId};
use std::marker::PhantomData;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionOutputDescriptor {
    Scalar(ScalarType),
    ValueObject(ValueObjectId),
    DomainEvent(DomainEventId),
    Optional(&'static Self),
    List(&'static Self),
}

#[doc(hidden)]
pub struct AggregateActionOutput<Owner>(PhantomData<fn() -> Owner>);

#[doc(hidden)]
pub struct DomainServiceActionOutput<Owner>(PhantomData<fn() -> Owner>);

#[doc(hidden)]
pub struct EntityActionOutput<Owner>(PhantomData<fn() -> Owner>);

#[doc(hidden)]
pub struct ValueObjectActionOutput<Owner>(PhantomData<fn() -> Owner>);

#[doc(hidden)]
pub trait SameType {
    type Type;
}

impl<T> SameType for T {
    type Type = T;
}

pub trait ActionOutputType<Contract>: 'static {
    const DESCRIPTOR: Option<ActionOutputDescriptor>;
}

macro_rules! scalar_outputs {
    ($($ty:ty => $variant:ident),* $(,)?) => {$(
        impl<Contract> ActionOutputType<Contract> for $ty {
            const DESCRIPTOR: Option<ActionOutputDescriptor> =
                Some(ActionOutputDescriptor::Scalar(ScalarType::$variant));
        }
    )*};
}

scalar_outputs! {
    bool => Bool, String => String, char => Char, f32 => F32, f64 => F64,
    i8 => I8, i16 => I16, i32 => I32, i64 => I64, i128 => I128, isize => Isize,
    u8 => U8, u16 => U16, u32 => U32, u64 => U64, u128 => U128, usize => Usize,
}

impl<Contract> ActionOutputType<Contract> for () {
    const DESCRIPTOR: Option<ActionOutputDescriptor> = None;
}

impl<T: ActionOutputType<Contract>, Contract> ActionOutputType<Contract> for Option<T> {
    const DESCRIPTOR: Option<ActionOutputDescriptor> = match T::DESCRIPTOR {
        Some(ref descriptor) => Some(ActionOutputDescriptor::Optional(descriptor)),
        None => None,
    };
}

impl<T: ActionOutputType<Contract>, Contract> ActionOutputType<Contract> for Vec<T> {
    const DESCRIPTOR: Option<ActionOutputDescriptor> = match T::DESCRIPTOR {
        Some(ref descriptor) => Some(ActionOutputDescriptor::List(descriptor)),
        None => None,
    };
}
