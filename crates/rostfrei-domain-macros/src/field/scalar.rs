use syn::TypePath;

use super::ir::Scalar;

pub fn classify(path: &TypePath) -> Option<Scalar> {
    if path.qself.is_some() {
        return None;
    }
    let names: Vec<_> = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let name = match names.as_slice() {
        [name] => name.as_str(),
        [module, string, name]
            if (module == "std" || module == "alloc") && string == "string" && name == "String" =>
        {
            name.as_str()
        }
        _ => return None,
    };
    Some(match name {
        "bool" => Scalar::Bool,
        "String" => Scalar::String,
        "char" => Scalar::Char,
        "f32" => Scalar::F32,
        "f64" => Scalar::F64,
        "i8" => Scalar::I8,
        "i16" => Scalar::I16,
        "i32" => Scalar::I32,
        "i64" => Scalar::I64,
        "i128" => Scalar::I128,
        "isize" => Scalar::Isize,
        "u8" => Scalar::U8,
        "u16" => Scalar::U16,
        "u32" => Scalar::U32,
        "u64" => Scalar::U64,
        "u128" => Scalar::U128,
        "usize" => Scalar::Usize,
        _ => return None,
    })
}
