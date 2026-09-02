use syn::{Fields, Index, LitStr, Member, Result};

use super::{ir::Field, role, scalar, shape};

pub fn extract(fields: &Fields) -> Result<Vec<Field>> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let (name, member) = field.ident.as_ref().map_or_else(
                || {
                    (
                        LitStr::new(&index.to_string(), field.ty.span()),
                        Member::Unnamed(Index::from(index)),
                    )
                },
                |ident| {
                    (
                        LitStr::new(ident.to_string().trim_start_matches("r#"), ident.span()),
                        Member::Named(ident.clone()),
                    )
                },
            );
            let (wrappers, base) = shape::parse(&field.ty)?;
            let role = role::parse(&field.attrs)?.unwrap_or_else(|| {
                scalar::classify(&base).map_or(super::ir::Role::Opaque, super::ir::Role::Scalar)
            });
            Ok(Field {
                name,
                member,
                base,
                wrappers,
                role,
            })
        })
        .collect()
}

use syn::spanned::Spanned;
