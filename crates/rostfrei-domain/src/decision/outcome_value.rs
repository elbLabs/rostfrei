use crate::{ScalarType, ValueObjectId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionOutcomeValueDescriptor {
    Scalar(ScalarType),
    ValueObject(ValueObjectId),
}

pub trait DecisionOutcomeValueType: 'static {
    const DESCRIPTOR: DecisionOutcomeValueDescriptor;
}

macro_rules! scalar_outcome_values {
    ($($ty:ty => $variant:ident),* $(,)?) => {$(
        impl DecisionOutcomeValueType for $ty {
            const DESCRIPTOR: DecisionOutcomeValueDescriptor =
                DecisionOutcomeValueDescriptor::Scalar(ScalarType::$variant);
        }
    )*};
}

scalar_outcome_values! {
    bool => Bool, String => String, char => Char, f32 => F32, f64 => F64,
    i8 => I8, i16 => I16, i32 => I32, i64 => I64, i128 => I128, isize => Isize,
    u8 => U8, u16 => U16, u32 => U32, u64 => U64, u128 => U128, usize => Usize,
}
