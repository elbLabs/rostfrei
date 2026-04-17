use proc_macro2::TokenStream;
use quote::quote;
use syn::{Fields, Ident, Index, Member, Path};

use crate::field::Field;

use super::attributes::Attributes;

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
    let code = &attributes.code;
    let message = &attributes.message;
    let assertions = crate::field::assemble_assertions_with_path(domain_path, name, None, fields);
    let encode_json = attributes
        .json
        .then(|| assemble_json_encoder(domain_path, name, syntax_fields));
    let fields = crate::field::assemble_descriptors_with_path(domain_path, fields);

    quote! {
        impl #domain_path::DomainErrorType for #name {
            type Owner = #owner;

            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: #domain_path::DomainErrorDescriptor =
                #domain_path::DomainErrorDescriptor {
                    id: #domain_path::DomainErrorId {
                        owner: <#owner as #domain_path::DomainErrorOwnerType>::DOMAIN_ERROR_OWNER_ID,
                        local: Self::LOCAL_ID,
                    },
                    label: #label,
                    code: #code,
                    message: #message,
                    fields: #fields,
                };
        }

        #assertions
        #encode_json
    }
}

fn assemble_json_encoder(domain_path: &Path, name: &Ident, fields: &Fields) -> TokenStream {
    let field_types: Vec<_> = fields.iter().map(|field| &field.ty).collect();
    let where_clause = (!field_types.is_empty()).then(|| {
        quote! {
            where
                #(#field_types: #domain_path::__private::serde::Serialize,)*
        }
    });
    let fields = fields.iter().enumerate().map(|(index, field)| {
        let member = field
            .ident
            .clone()
            .map_or_else(|| Member::Unnamed(Index::from(index)), Member::Named);
        let wire_name = field
            .ident
            .as_ref()
            .map_or_else(|| index.to_string(), std::string::ToString::to_string)
            .trim_start_matches("r#")
            .to_owned();
        quote! {
            object.insert(
                #wire_name.to_owned(),
                #domain_path::__private::serde_json::to_value(&self.#member)
                    .map_err(|error| ::std::format!(
                        "cannot encode domain error field `{}`: {error}",
                        #wire_name,
                    ))?,
            );
        }
    });

    quote! {
        impl #domain_path::JsonErrorPayload for #name
        #where_clause
        {
            fn encode_json(
                &self,
            ) -> ::core::result::Result<
                #domain_path::__private::serde_json::Value,
                ::std::string::String,
            > {
                let descriptor = <Self as #domain_path::DomainErrorType>::DESCRIPTOR;
                let mut object = #domain_path::__private::serde_json::Map::new();
                object.insert(
                    "code".to_owned(),
                    #domain_path::__private::serde_json::Value::String(descriptor.code.to_owned()),
                );
                object.insert(
                    "message".to_owned(),
                    #domain_path::__private::serde_json::Value::String(descriptor.message.to_owned()),
                );
                #(#fields)*
                ::core::result::Result::Ok(
                    #domain_path::__private::serde_json::Value::Object(object),
                )
            }
        }
    }
}
