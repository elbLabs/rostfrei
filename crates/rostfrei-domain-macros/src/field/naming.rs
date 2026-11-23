use quote::ToTokens as _;
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
            // The validated grammar is a direct path with at most single-type
            // wrapper arguments, so whitespace never carries meaning.
            let declared_type = field.ty.to_token_stream().to_string().replace(' ', "");
            let role = role::parse(&field.attrs)?.unwrap_or_else(|| {
                scalar::classify(&base).map_or(super::ir::Role::Opaque, super::ir::Role::Scalar)
            });
            Ok(Field {
                name,
                member,
                declared_type: LitStr::new(&declared_type, field.ty.span()),
                base,
                wrappers,
                role,
            })
        })
        .collect()
}

use syn::spanned::Spanned;
