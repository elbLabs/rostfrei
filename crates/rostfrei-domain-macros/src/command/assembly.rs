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
    let schema_version = &attributes.schema_version;
    let descriptors = crate::field::assemble_descriptors_with_path(domain_path, fields);
    let assertions = crate::field::assemble_assertions_with_path(domain_path, name, None, fields);
    let json_codec = assemble_json_codec(domain_path, name, fields, syntax_fields);
    quote! {
        impl #domain_path::Command for #name {
            const LOCAL_ID: &'static str = #id;
            const LABEL: &'static str = #label;
            const FIELDS: &'static [#domain_path::FieldDescriptor] = #descriptors;
            const SCHEMA_VERSION: u32 = #schema_version;
        }

        #assertions
        #json_codec
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "keeps command JSON decoder token generation in one auditable block"
)]
fn assemble_json_codec(
    domain_path: &Path,
    name: &Ident,
    parsed_fields: &[Field],
    fields: &Fields,
) -> TokenStream {
    let field_types: Vec<_> = fields.iter().map(|field| &field.ty).collect();
    let where_clause = (!field_types.is_empty()).then(|| {
        quote! {
            where
                #(#field_types: #domain_path::__private::serde::de::DeserializeOwned
                    + #domain_path::__private::serde::Serialize,)*
        }
    });
    let encode = match fields {
        Fields::Named(_) => {
            let fields = parsed_fields.iter().map(|parsed_field| {
                let member = &parsed_field.member;
                let wire_name = &parsed_field.name;
                quote! {
                    object.insert(
                        #wire_name.to_owned(),
                        #domain_path::__private::serde_json::to_value(&self.#member)
                            .map_err(|error| ::std::format!(
                                "command field `{}` could not be encoded: {error}",
                                #wire_name,
                            ))?,
                    );
                }
            });
            quote! {
                let mut object = #domain_path::__private::serde_json::Map::new();
                #(#fields)*
                #domain_path::__private::serde_json::Value::Object(object)
            }
        }
        Fields::Unnamed(fields) => {
            let fields = fields.unnamed.iter().enumerate().map(|(index, _)| {
                let index = Index::from(index);
                quote! {
                    #domain_path::__private::serde_json::to_value(&self.#index)
                        .map_err(|error| ::std::format!(
                            "command field `{}` could not be encoded: {error}",
                            #index,
                        ))?
                }
            });
            quote! {
                #domain_path::__private::serde_json::Value::Array(::std::vec![#(#fields),*])
            }
        }
        Fields::Unit => quote!(#domain_path::__private::serde_json::Value::Null),
    };
    let construct = match fields {
        Fields::Named(_) => {
            let field_names = parsed_fields.iter().map(|field| &field.name);
            let fields = parsed_fields.iter().map(|parsed_field| {
                let member = &parsed_field.member;
                let wire_name = &parsed_field.name;
                let value = if matches!(parsed_field.wrappers.first(), Some(Wrapper::Optional)) {
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
                    #member: #domain_path::__private::serde_json::from_value(#value)
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
            fn encode_json(
                &self,
            ) -> ::core::result::Result<
                #domain_path::__private::serde_json::Value,
                ::std::string::String,
            > {
                ::core::result::Result::Ok({ #encode })
            }

            fn decode_json(
                payload: &#domain_path::__private::serde_json::Value,
            ) -> ::core::result::Result<Self, ::std::string::String> {
                ::core::result::Result::Ok({ #construct })
            }
        }
    }
}
