use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::ParseStream;
use syn::punctuated::Punctuated;
use syn::{
    Attribute, DeriveInput, Error, LitInt, LitStr, Path, Token, Type, parenthesized,
    parse_macro_input,
};

#[proc_macro_derive(CommandDefinition, attributes(rostfrei))]
pub fn derive_command_definition(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_command(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(QueryDefinition, attributes(rostfrei))]
pub fn derive_query_definition(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_query(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(Module, attributes(rostfrei))]
pub fn derive_module(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_module(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn expand_command(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let attributes = CommandAttributes::parse(&input.attrs, input)?;
    let registry = registry_path()?;
    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let name = attributes.name;
    let version = attributes.version;
    let aggregate = attributes.aggregate;

    Ok(quote! {
        impl #impl_generics #registry::CommandDefinition for #ident #type_generics #where_clause {
            type Aggregate = #aggregate;

            const COMMAND_NAME: &'static str = #name;
            const SCHEMA_VERSION: u32 = #version;
        }
    })
}

fn expand_query(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let attributes = QueryAttributes::parse(&input.attrs, input)?;
    let registry = registry_path()?;
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

fn expand_module(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let attributes = ModuleAttributes::parse(&input.attrs, input)?;
    let registry = registry_path()?;
    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let name = attributes.name;
    let commands = attributes.commands;
    let queries = attributes.queries;

    Ok(quote! {
        impl #impl_generics #registry::DomainModule for #ident #type_generics #where_clause {
            const MODULE_NAME: &'static str = #name;

            fn descriptor() -> #registry::ModuleDescriptor {
                #registry::ModuleDescriptor {
                    module_name: Self::MODULE_NAME,
                    commands: ::std::vec![
                        #(<#commands as #registry::CommandDefinition>::descriptor()),*
                    ],
                    queries: ::std::vec![
                        #(<#queries as #registry::QueryDefinition>::descriptor()),*
                    ],
                }
            }
        }
    })
}

struct QueryAttributes {
    context: LitStr,
    name: LitStr,
    version: u32,
    response: Type,
}

impl QueryAttributes {
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

        if let Some(context) = &context
            && context.value().trim().is_empty()
        {
            errors.push(Error::new(
                context.span(),
                "query bounded context must not be empty",
            ));
        }
        if let Some(name) = &name
            && name.value().trim().is_empty()
        {
            errors.push(Error::new(name.span(), "query name must not be empty"));
        }
        if let Some(version) = &version {
            match version.base10_parse::<u32>() {
                Ok(0) => errors.push(Error::new(
                    version.span(),
                    "query version must be greater than zero",
                )),
                Ok(_) => {}
                Err(error) => errors.push(error),
            }
        }

        errors.finish()?;
        Ok(Self {
            context: context.ok_or_else(|| {
                Error::new(
                    input.ident.span(),
                    "missing `context` in `rostfrei` attribute",
                )
            })?,
            name: name.ok_or_else(|| {
                Error::new(input.ident.span(), "missing `name` in `rostfrei` attribute")
            })?,
            version: version
                .ok_or_else(|| {
                    Error::new(
                        input.ident.span(),
                        "missing `version` in `rostfrei` attribute",
                    )
                })?
                .base10_parse()?,
            response: response.ok_or_else(|| {
                Error::new(
                    input.ident.span(),
                    "missing `response` in `rostfrei` attribute",
                )
            })?,
        })
    }
}

struct CommandAttributes {
    name: LitStr,
    version: u32,
    aggregate: Type,
}

impl CommandAttributes {
    fn parse(attributes: &[Attribute], input: &DeriveInput) -> syn::Result<Self> {
        let mut name: Option<LitStr> = None;
        let mut version: Option<LitInt> = None;
        let mut aggregate: Option<Type> = None;
        let mut errors = Errors::default();

        for attribute in rostfrei_attributes(attributes) {
            if let Err(error) = attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    set_once(&mut name, meta.value()?.parse()?, &meta.path, "name")
                } else if meta.path.is_ident("version") {
                    set_once(&mut version, meta.value()?.parse()?, &meta.path, "version")
                } else if meta.path.is_ident("aggregate") {
                    set_once(
                        &mut aggregate,
                        meta.value()?.parse()?,
                        &meta.path,
                        "aggregate",
                    )
                } else {
                    Err(meta.error(
                        "unknown `rostfrei` attribute; expected `name`, `version`, or `aggregate`",
                    ))
                }
            }) {
                errors.push(error);
            }
        }

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
        let aggregate = required(
            aggregate,
            input,
            "missing `aggregate` in `rostfrei` attribute",
            &mut errors,
        );

        if let Some(name) = &name
            && name.value().trim().is_empty()
        {
            errors.push(Error::new(name.span(), "command name must not be empty"));
        }

        if let Some(version) = &version {
            match version.base10_parse::<u32>() {
                Ok(0) => errors.push(Error::new(
                    version.span(),
                    "command version must be greater than zero",
                )),
                Ok(_) => {}
                Err(error) => errors.push(error),
            }
        }

        errors.finish()?;

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
        let Some(aggregate) = aggregate else {
            return Err(Error::new(
                input.ident.span(),
                "missing `aggregate` in `rostfrei` attribute",
            ));
        };

        Ok(Self {
            name,
            version: version.base10_parse()?,
            aggregate,
        })
    }
}

struct ModuleAttributes {
    name: LitStr,
    commands: Vec<Type>,
    queries: Vec<Type>,
}

impl ModuleAttributes {
    fn parse(attributes: &[Attribute], input: &DeriveInput) -> syn::Result<Self> {
        let mut name: Option<LitStr> = None;
        let mut commands: Option<Vec<Type>> = None;
        let mut queries: Option<Vec<Type>> = None;
        let mut errors = Errors::default();

        for attribute in rostfrei_attributes(attributes) {
            if let Err(error) = attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    set_once(&mut name, meta.value()?.parse()?, &meta.path, "name")
                } else if meta.path.is_ident("commands") {
                    let parsed = parse_commands(meta.input)?;
                    set_once(&mut commands, parsed, &meta.path, "commands")
                } else if meta.path.is_ident("queries") {
                    let parsed = parse_commands(meta.input)?;
                    set_once(&mut queries, parsed, &meta.path, "queries")
                } else {
                    Err(meta.error(
                        "unknown `rostfrei` attribute; expected `name`, `commands`, or `queries`",
                    ))
                }
            }) {
                errors.push(error);
            }
        }

        let name = required(
            name,
            input,
            "missing `name` in `rostfrei` attribute",
            &mut errors,
        );
        let commands = commands.unwrap_or_default();
        let queries = queries.unwrap_or_default();

        if let Some(name) = &name
            && name.value().trim().is_empty()
        {
            errors.push(Error::new(name.span(), "module name must not be empty"));
        }

        if commands.is_empty() && queries.is_empty() {
            errors.push(Error::new(
                input.ident.span(),
                "a module must contain at least one command or query",
            ));
        }

        errors.finish()?;

        let Some(name) = name else {
            return Err(Error::new(
                input.ident.span(),
                "missing `name` in `rostfrei` attribute",
            ));
        };
        Ok(Self {
            name,
            commands,
            queries,
        })
    }
}

fn rostfrei_attributes(attributes: &[Attribute]) -> impl Iterator<Item = &Attribute> {
    attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("rostfrei"))
}

fn set_once<T>(slot: &mut Option<T>, value: T, path: &Path, name: &str) -> syn::Result<()> {
    if slot.is_some() {
        return Err(Error::new_spanned(
            path,
            format!("duplicate `{name}` attribute"),
        ));
    }

    *slot = Some(value);
    Ok(())
}

fn required<T>(
    value: Option<T>,
    input: &DeriveInput,
    message: &str,
    errors: &mut Errors,
) -> Option<T> {
    if value.is_none() {
        errors.push(Error::new(input.ident.span(), message));
    }
    value
}

fn parse_commands(input: ParseStream<'_>) -> syn::Result<Vec<Type>> {
    let content;
    parenthesized!(content in input);
    let commands = Punctuated::<Type, Token![,]>::parse_terminated(&content)?;
    Ok(commands.into_iter().collect())
}

fn registry_path() -> syn::Result<Path> {
    if let Ok(found) = crate_name("rostfrei-registry") {
        found_crate_path("rostfrei-registry", found)
    } else {
        let facade = dependency_path("rostfrei")?;
        syn::parse2(quote!(#facade::__private::registry))
    }
}

fn dependency_path(package: &str) -> syn::Result<Path> {
    let found = crate_name(package).map_err(|error| {
        Error::new(
            proc_macro2::Span::call_site(),
            format!("could not resolve the `{package}` dependency: {error}"),
        )
    })?;

    found_crate_path(package, found)
}

fn found_crate_path(package: &str, found: FoundCrate) -> syn::Result<Path> {
    match found {
        FoundCrate::Itself if package == "rostfrei" => syn::parse_str("::rostfrei"),
        FoundCrate::Itself => syn::parse_str("crate"),
        FoundCrate::Name(name) => syn::parse_str(&format!("::{name}")),
    }
}

#[derive(Default)]
struct Errors(Option<Error>);

impl Errors {
    fn push(&mut self, error: Error) {
        if let Some(errors) = &mut self.0 {
            errors.combine(error);
        } else {
            self.0 = Some(error);
        }
    }

    fn finish(self) -> syn::Result<()> {
        self.0.map_or(Ok(()), Err)
    }
}
