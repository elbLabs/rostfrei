use super::FieldValue;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FieldDescriptor {
    pub name: &'static str,
    pub value: FieldValue,
}

/// The Rust type as written in a field declaration, not a modeled role or wire schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclaredFieldType {
    pub name: &'static str,
    pub rust_type: &'static str,
}
