use super::{FieldKind, FieldWrapper};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FieldValue {
    pub kind: FieldKind,
    pub wrappers: &'static [FieldWrapper],
}
