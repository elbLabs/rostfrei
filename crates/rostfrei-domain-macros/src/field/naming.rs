use syn::{Fields, LitStr, Result};

use super::{ir::Field, role, scalar, shape};

pub fn extract(fields: &Fields) -> Result<Vec<Field>> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let name = field.ident.as_ref().map_or_else(
                || LitStr::new(&index.to_string(), field.ty.span()),
                |ident| LitStr::new(ident.to_string().trim_start_matches("r#"), ident.span()),
            );
            let (wrappers, base) = shape::parse(&field.ty)?;
            let role = match role::parse(&field.attrs)? {
                Some(role) => role,
                None => super::ir::Role::Scalar(scalar::classify(&base).ok_or_else(|| {
                    syn::Error::new_spanned(
                        &field.ty,
                        "custom domain fields require an explicit domain role",
                    )
                })?),
            };
            Ok(Field {
                name,
                base,
                wrappers,
                role,
            })
        })
        .collect()
}

use syn::spanned::Spanned;
