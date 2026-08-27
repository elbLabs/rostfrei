use proc_macro2::TokenStream;
use quote::quote;
use syn::{Fields, Ident, Index, Path};

use super::attributes::Attributes;
use crate::field::{Field, Wrapper};

pub fn assemble(
    domain_path: &Path,
    name: &Ident,
    attributes: &Attributes,
    fields: &[Field],
    syntax_fields: &Fields,
) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let owner = &attributes.owner;
    let schema_version = &attributes.schema_version;
    let rejection = attributes.rejection.as_ref().map_or_else(
        || quote!(::core::convert::Infallible),
        |rejection| quote!(#rejection),
    );
    let descriptors = crate::field::assemble_descriptors_with_path(domain_path, fields);
    let assertions = crate::field::assemble_assertions_with_path(domain_path, name, None, fields);
    let decode_json = attributes
        .json
        .then(|| assemble_json_decoder(domain_path, name, fields, syntax_fields));
    quote! {
        impl #domain_path::DomainCommandType for #name {
            type Owner = #owner;
            type Rejection = #rejection;

            const LOCAL_ID: &'static str = #id;
            const SCHEMA_VERSION: u32 = #schema_version;
            const DESCRIPTOR: #domain_path::DomainCommandDescriptor =
                #domain_path::DomainCommandDescriptor {
                    id: #domain_path::DomainCommandId {
                        owner: <#owner as #domain_path::DomainCommandOwnerType>::DOMAIN_COMMAND_OWNER_ID,
                        local: Self::LOCAL_ID,
                    },
                    label: #label,
                    fields: #descriptors,
                };
        }

        #assertions
        #decode_json
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "keeps command JSON decoder token generation in one auditable block"
)]
fn assemble_json_decoder(
    domain_path: &Path,
    name: &Ident,
    parsed_fields: &[Field],
    fields: &Fields,
) -> TokenStream {
    let field_types: Vec<_> = fields.iter().map(|field| &field.ty).collect();
    let where_clause = (!field_types.is_empty()).then(|| {
        quote! {
            where
                #(#field_types: #domain_path::__private::serde::de::DeserializeOwned,)*
        }
    });
    let construct = match fields {
        Fields::Named(fields) => {
            let field_names: Vec<_> = fields
                .named
                .iter()
                .map(|field| {
                    field
                        .ident
                        .as_ref()
                        .expect("named field")
                        .to_string()
                        .trim_start_matches("r#")
                        .to_owned()
                })
                .collect();
            let fields = fields
                .named
                .iter()
                .zip(parsed_fields)
                .map(|(field, parsed_field)| {
                    let ident = field.ident.as_ref().expect("named field");
                    let wire_name = ident.to_string().trim_start_matches("r#").to_owned();
                    let value = if matches!(parsed_field.wrappers.first(), Some(Wrapper::Optional))
                    {
                        quote! {
                            object
                                .get(#wire_name)
                                .cloned()
                                .unwrap_or(#domain_path::__private::serde_json::Value::Null)
                        }
                    } else {
                        quote! {
                            object.get(#wire_name).cloned().ok_or_else(|| ::std::format!(
                                "missing command field `{}`",
                                #wire_name,
                            ))?
                        }
                    };
                    quote! {
                        #ident: #domain_path::__private::serde_json::from_value(#value)
                        .map_err(|error| ::std::format!(
                            "invalid command field `{}`: {error}",
                            #wire_name,
                        ))?
                    }
                });
            quote! {
                let object = payload.as_object().ok_or_else(||
                    "command payload must be a JSON object".to_owned()
                )?;
                const EXPECTED_FIELDS: &[&str] = &[#(#field_names),*];
                if let Some(field) = object
                    .keys()
                    .find(|field| !EXPECTED_FIELDS.contains(&field.as_str()))
                {
                    return ::core::result::Result::Err(::std::format!(
                        "unknown command field `{field}`",
                    ));
                }
                Self { #(#fields,)* }
            }
        }
        Fields::Unnamed(fields) => {
            let field_count = fields.unnamed.len();
            let fields = fields.unnamed.iter().enumerate().map(|(index, _)| {
                let index = Index::from(index);
                quote! {
                    #domain_path::__private::serde_json::from_value(
                        values
                            .get(#index)
                            .cloned()
                            .unwrap_or(#domain_path::__private::serde_json::Value::Null),
                    )
                    .map_err(|error| ::std::format!(
                        "invalid command field `{}`: {error}",
                        #index,
                    ))?
                }
            });
            quote! {
                let values = payload.as_array().ok_or_else(||
                    "tuple command payload must be a JSON array".to_owned()
                )?;
                if values.len() != #field_count {
                    return ::core::result::Result::Err(::std::format!(
                        "tuple command payload must contain exactly {} fields",
                        #field_count,
                    ));
                }
                Self(#(#fields,)*)
            }
        }
        Fields::Unit => quote! {
            if !payload.is_null()
                && !payload.as_object().is_some_and(|object| object.is_empty())
            {
                return ::core::result::Result::Err(
                    "unit command payload must be null or an empty JSON object".to_owned(),
                );
            }
            Self
        },
    };

    quote! {
        impl #domain_path::JsonCommandPayload for #name
        #where_clause
        {
            fn decode_json(
                payload: &#domain_path::__private::serde_json::Value,
            ) -> ::core::result::Result<Self, ::std::string::String> {
                ::core::result::Result::Ok({ #construct })
            }
        }
    }
}
