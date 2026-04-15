use super::DecisionOutputDescriptor;
use crate::ScalarType;

pub trait DecisionOutputType: 'static {
    const DESCRIPTOR: Option<DecisionOutputDescriptor>;
}

macro_rules! scalar_outputs {
    ($($ty:ty => $variant:ident),* $(,)?) => {$(
        impl DecisionOutputType for $ty {
            const DESCRIPTOR: Option<DecisionOutputDescriptor> =
                Some(DecisionOutputDescriptor::Scalar(ScalarType::$variant));
        }
    )*};
}

scalar_outputs! {
    bool => Bool, String => String, char => Char, f32 => F32, f64 => F64,
    i8 => I8, i16 => I16, i32 => I32, i64 => I64, i128 => I128, isize => Isize,
    u8 => U8, u16 => U16, u32 => U32, u64 => U64, u128 => U128, usize => Usize,
}

impl DecisionOutputType for () {
    const DESCRIPTOR: Option<DecisionOutputDescriptor> = None;
}
