use crate::{DomainIdentityId, ScalarType, ValueObjectId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryOutputDescriptor {
    Scalar(ScalarType),
    ValueObject(ValueObjectId),
    DomainIdentity(DomainIdentityId),
    Optional(&'static Self),
    List(&'static Self),
}

pub trait QueryOutputType<Aggregate>: 'static {
    const DESCRIPTOR: QueryOutputDescriptor;
}

macro_rules! scalar_outputs {
    ($($ty:ty => $variant:ident),* $(,)?) => {$ (
        impl<Aggregate> QueryOutputType<Aggregate> for $ty {
            const DESCRIPTOR: QueryOutputDescriptor = QueryOutputDescriptor::Scalar(ScalarType::$variant);
        }
    )* };
}

scalar_outputs! {
    bool => Bool, String => String, char => Char, f32 => F32, f64 => F64,
    i8 => I8, i16 => I16, i32 => I32, i64 => I64, i128 => I128, isize => Isize,
    u8 => U8, u16 => U16, u32 => U32, u64 => U64, u128 => U128, usize => Usize,
}

impl<Aggregate, T: QueryOutputType<Aggregate>> QueryOutputType<Aggregate> for Option<T> {
    const DESCRIPTOR: QueryOutputDescriptor = QueryOutputDescriptor::Optional(&T::DESCRIPTOR);
}

impl<Aggregate, T: QueryOutputType<Aggregate>> QueryOutputType<Aggregate> for Vec<T> {
    const DESCRIPTOR: QueryOutputDescriptor = QueryOutputDescriptor::List(&T::DESCRIPTOR);
}
