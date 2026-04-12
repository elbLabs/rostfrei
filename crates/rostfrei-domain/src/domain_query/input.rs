use crate::{DomainIdentityId, ScalarType, ValueObjectId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryInputDescriptor {
    Scalar(ScalarType),
    ValueObject(ValueObjectId),
    DomainIdentity(DomainIdentityId),
}

pub trait QueryInputType<Aggregate>: 'static {
    const DESCRIPTOR: QueryInputDescriptor;
}

macro_rules! scalar_inputs {
    ($($ty:ty => $variant:ident),* $(,)?) => {$ (
        impl<Aggregate> QueryInputType<Aggregate> for $ty {
            const DESCRIPTOR: QueryInputDescriptor = QueryInputDescriptor::Scalar(ScalarType::$variant);
        }
    )* };
}

scalar_inputs! {
    bool => Bool, String => String, char => Char, f32 => F32, f64 => F64,
    i8 => I8, i16 => I16, i32 => I32, i64 => I64, i128 => I128, isize => Isize,
    u8 => U8, u16 => U16, u32 => U32, u64 => U64, u128 => U128, usize => Usize,
}
