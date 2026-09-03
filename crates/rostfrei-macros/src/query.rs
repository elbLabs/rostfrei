use proc_macro2::TokenStream;
use quote::quote;
use syn::{Attribute, DeriveInput, Error, LitInt, LitStr, Type};

use crate::support::{Errors, registry_path, required, rostfrei_attributes, set_once};

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let attributes = Attributes::parse(&input.attrs, input)?;
    let registry = registry_path();
    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let context = attributes.context;
    let name = attributes.name;
    let version = attributes.version;
    let response = attributes.response;

    Ok(quote! {
        impl #impl_generics #registry::QueryDefinition for #ident #type_generics #where_clause {
            type Response = #response;

            const BOUNDED_CONTEXT: &'static str = #context;
            const QUERY_NAME: &'static str = #name;
            const SCHEMA_VERSION: u32 = #version;
        }
    })
}

struct Attributes {
    context: LitStr,
    name: LitStr,
    version: u32,
    response: Type,
}

impl Attributes {
    #[allow(
        clippy::too_many_lines,
        reason = "query attribute diagnostics are kept together to report all invalid fields"
    )]
    fn parse(attributes: &[Attribute], input: &DeriveInput) -> syn::Result<Self> {
        let mut context: Option<LitStr> = None;
        let mut name: Option<LitStr> = None;
        let mut version: Option<LitInt> = None;
        let mut response: Option<Type> = None;
        let mut errors = Errors::default();

        for attribute in rostfrei_attributes(attributes) {
            if let Err(error) = attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("context") {
                    set_once(&mut context, meta.value()?.parse()?, &meta.path, "context")
                } else if meta.path.is_ident("name") {
                    set_once(&mut name, meta.value()?.parse()?, &meta.path, "name")
                } else if meta.path.is_ident("version") {
                    set_once(&mut version, meta.value()?.parse()?, &meta.path, "version")
                } else if meta.path.is_ident("response") {
                    set_once(
                        &mut response,
                        meta.value()?.parse()?,
                        &meta.path,
                        "response",
                    )
                } else {
                    Err(meta.error(
                        "unknown `rostfrei` attribute; expected `context`, `name`, `version`, or `response`",
                    ))
                }
            }) {
                errors.push(error);
            }
        }

        let context = required(
            context,
            input,
            "missing `context` in `rostfrei` attribute",
            &mut errors,
        );
        let name = required(
            name,
            input,
            "missing `name` in `rostfrei` attribute",
            &mut errors,
        );
        let version = required(
            version,
            input,
            "missing `version` in `rostfrei` attribute",
            &mut errors,
        );
        let response = required(
            response,
            input,
            "missing `response` in `rostfrei` attribute",
            &mut errors,
        );

        validate(
            context.as_ref(),
            name.as_ref(),
            version.as_ref(),
            &mut errors,
        );
        errors.finish()?;

        let Some(context) = context else {
            return Err(Error::new(
                input.ident.span(),
                "missing `context` in `rostfrei` attribute",
            ));
        };
        let Some(name) = name else {
            return Err(Error::new(
                input.ident.span(),
                "missing `name` in `rostfrei` attribute",
            ));
        };
        let Some(version) = version else {
            return Err(Error::new(
                input.ident.span(),
                "missing `version` in `rostfrei` attribute",
            ));
        };
        let Some(response) = response else {
            return Err(Error::new(
                input.ident.span(),
                "missing `response` in `rostfrei` attribute",
            ));
        };

        Ok(Self {
            context,
            name,
            version: version.base10_parse()?,
            response,
        })
    }
}

fn validate(
    context: Option<&LitStr>,
    name: Option<&LitStr>,
    version: Option<&LitInt>,
    errors: &mut Errors,
) {
    if let Some(context) = context
        && context.value().trim().is_empty()
    {
        errors.push(Error::new(
            context.span(),
            "query bounded context must not be empty",
        ));
    }
    if let Some(name) = name
        && name.value().trim().is_empty()
    {
        errors.push(Error::new(name.span(), "query name must not be empty"));
    }
    if let Some(version) = version {
        match version.base10_parse::<u32>() {
            Ok(0) => errors.push(Error::new(
                version.span(),
                "query version must be greater than zero",
            )),
            Ok(_) => {}
            Err(error) => errors.push(error),
        }
    }
}
