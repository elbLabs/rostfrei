use syn::{LitStr, TypePath};

#[derive(Clone)]
pub enum Wrapper {
    List,
    Optional,
}

#[derive(Clone)]
pub enum Scalar {
    Bool,
    String,
    Char,
    F32,
    F64,
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
}

#[derive(Clone)]
pub enum Role {
    Identity,
    Entity,
    ValueObject,
    AggregateReference(TypePath),
    SemanticScalar(TypePath),
    Scalar(Scalar),
}

#[derive(Clone)]
pub struct Field {
    pub name: LitStr,
    pub base: TypePath,
    pub wrappers: Vec<Wrapper>,
    pub role: Role,
}
