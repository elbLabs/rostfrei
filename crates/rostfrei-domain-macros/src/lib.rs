use proc_macro::TokenStream;
use syn::{DeriveInput, Error, parse_macro_input};

#[doc(hidden)]
#[proc_macro]
pub fn __install_test_macro_support(input: TokenStream) -> TokenStream {
    if !input.is_empty() {
        return Error::new(
            proc_macro2::Span::call_site(),
            "internal macro support installer does not accept arguments",
        )
        .into_compile_error()
        .into();
    }
    quote::quote! {
        #[doc(hidden)]
        pub mod __rostfrei_macro_support {
            pub use ::domain::*;

            pub mod __private {
                pub use ::domain::__private::*;
            }

            macro_rules! __runtime {
                ($($tokens:tt)*) => {};
            }
            pub(crate) use __runtime;
        }
    }
    .into()
}

mod action;
mod aggregate;
mod aggregate_events;
mod bounded_context;
mod command;
mod decision;
mod decision_outcome;
mod domain_error;
mod domain_event;
mod domain_identity;
mod domain_service;
mod domain_test;
mod entity;
mod entity_lifecycle;
mod field;
mod helper;
mod invariant;
mod query;
mod value_object;

#[proc_macro_attribute]
pub fn domain_action(args: TokenStream, input: TokenStream) -> TokenStream {
    action::expand(&args.into(), input.into())
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn domain_decision(args: TokenStream, input: TokenStream) -> TokenStream {
    decision::expand(&args.into(), input.into())
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn domain_invariant(args: TokenStream, input: TokenStream) -> TokenStream {
    invariant::expand(&args.into(), input.into())
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn domain_query(args: TokenStream, input: TokenStream) -> TokenStream {
    query::expand(&args.into(), input.into())
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn domain_action_test(args: TokenStream, input: TokenStream) -> TokenStream {
    domain_test::expand(
        domain_test::DomainTestKind::Action,
        args.into(),
        input.into(),
    )
    .unwrap_or_else(Error::into_compile_error)
    .into()
}

#[proc_macro_attribute]
pub fn domain_decision_test(args: TokenStream, input: TokenStream) -> TokenStream {
    domain_test::expand(
        domain_test::DomainTestKind::Decision,
        args.into(),
        input.into(),
    )
    .unwrap_or_else(Error::into_compile_error)
    .into()
}

#[proc_macro_attribute]
pub fn domain_invariant_test(args: TokenStream, input: TokenStream) -> TokenStream {
    domain_test::expand(
        domain_test::DomainTestKind::Invariant,
        args.into(),
        input.into(),
    )
    .unwrap_or_else(Error::into_compile_error)
    .into()
}

#[proc_macro_attribute]
pub fn domain_lifecycle_test(args: TokenStream, input: TokenStream) -> TokenStream {
    domain_test::expand(
        domain_test::DomainTestKind::Lifecycle,
        args.into(),
        input.into(),
    )
    .unwrap_or_else(Error::into_compile_error)
    .into()
}

#[proc_macro_derive(BoundedContext, attributes(domain, rostfrei))]
pub fn derive_bounded_context(input: TokenStream) -> TokenStream {
    bounded_context::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(Aggregate, attributes(domain, rostfrei))]
pub fn derive_aggregate(input: TokenStream) -> TokenStream {
    aggregate::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(AggregateEvents)]
pub fn derive_aggregate_events(input: TokenStream) -> TokenStream {
    aggregate_events::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(DomainIdentity)]
pub fn derive_domain_identity(input: TokenStream) -> TokenStream {
    domain_identity::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(DecisionOutcome, attributes(outcome))]
pub fn derive_decision_outcome(input: TokenStream) -> TokenStream {
    decision_outcome::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(DomainEvent, attributes(domain, rostfrei))]
pub fn derive_domain_event(input: TokenStream) -> TokenStream {
    domain_event::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(Command, attributes(domain, rostfrei))]
pub fn derive_command(input: TokenStream) -> TokenStream {
    command::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(DomainError, attributes(domain, rostfrei))]
pub fn derive_domain_error(input: TokenStream) -> TokenStream {
    domain_error::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(DomainService, attributes(domain, rostfrei))]
pub fn derive_domain_service(input: TokenStream) -> TokenStream {
    domain_service::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(Entity, attributes(domain, rostfrei))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    entity::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(EntityLifecycle, attributes(domain, rostfrei, state))]
pub fn derive_entity_lifecycle(input: TokenStream) -> TokenStream {
    entity_lifecycle::expand(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(ValueObject, attributes(domain, rostfrei))]
pub fn derive_value_object(input: TokenStream) -> TokenStream {
    value_object::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}
