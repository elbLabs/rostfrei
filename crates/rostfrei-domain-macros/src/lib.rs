use proc_macro::TokenStream;
use syn::{DeriveInput, Error, parse_macro_input};

mod action;
mod aggregate;
mod aggregate_events;
mod bounded_context;
mod command;
mod decision_outcome;
mod domain_decisions;
mod domain_error;
mod domain_event;
mod domain_identity;
mod domain_invariants;
mod domain_queries;
mod domain_service;
mod domain_test;
mod entity;
mod entity_lifecycle;
mod field;
mod helper;
mod value_object;

#[proc_macro_attribute]
pub fn domain_action(args: TokenStream, input: TokenStream) -> TokenStream {
    action::expand(&args.into(), input.into())
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn domain_decisions(args: TokenStream, input: TokenStream) -> TokenStream {
    domain_decisions::expand(args.into(), input.into())
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn domain_invariants(args: TokenStream, input: TokenStream) -> TokenStream {
    domain_invariants::expand(args.into(), input.into())
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn domain_queries(args: TokenStream, input: TokenStream) -> TokenStream {
    domain_queries::expand(args.into(), input.into())
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
