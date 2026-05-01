use proc_macro2::TokenStream;
use quote::quote_spanned;
use syn::spanned::Spanned;
use syn::{Expr, Path, TypePath};

use super::DomainTestKind;

pub(super) enum DomainTestSubjectInput {
    Action(Expr),
    Decision(Expr),
    Invariant(Expr),
    Lifecycle(TypePath),
}

impl DomainTestSubjectInput {
    pub(super) fn parse(kind: DomainTestKind, args: TokenStream) -> syn::Result<Self> {
        match kind {
            DomainTestKind::Action => parse_action(args).map(Self::Action),
            DomainTestKind::Decision => parse_descriptor(kind, args).map(Self::Decision),
            DomainTestKind::Invariant => parse_descriptor(kind, args).map(Self::Invariant),
            DomainTestKind::Lifecycle => parse_type(args, "lifecycle").map(Self::Lifecycle),
        }
    }

    pub(super) fn assemble(&self, domain_path: &Path) -> TokenStream {
        match self {
            Self::Action(descriptor) => quote_spanned! {descriptor.span()=>
                #domain_path::DomainTestSubject::Action(
                    (#descriptor).id
                )
            },
            Self::Decision(descriptor) => quote_spanned! {descriptor.span()=>
                #domain_path::DomainTestSubject::Decision((#descriptor).id)
            },
            Self::Invariant(descriptor) => quote_spanned! {descriptor.span()=>
                #domain_path::DomainTestSubject::Invariant((#descriptor).id)
            },
            Self::Lifecycle(lifecycle) => quote_spanned! {lifecycle.path.span()=>
                #domain_path::DomainTestSubject::Lifecycle(
                    <#lifecycle as #domain_path::EntityLifecycleType>::DESCRIPTOR.id
                )
            },
        }
    }
}

fn parse_action(args: TokenStream) -> syn::Result<Expr> {
    parse_descriptor(DomainTestKind::Action, args)
}

fn parse_descriptor(kind: DomainTestKind, args: TokenStream) -> syn::Result<Expr> {
    syn::parse2(args).map_err(|error| {
        syn::Error::new(
            error.span(),
            format!(
                "{} tests require exactly one {} descriptor expression",
                kind.name(),
                kind.name()
            ),
        )
    })
}

fn parse_type(args: TokenStream, kind: &str) -> syn::Result<TypePath> {
    let subject: TypePath = syn::parse2(args).map_err(|error| {
        syn::Error::new(
            error.span(),
            format!("{kind} tests require exactly one {kind} type path"),
        )
    })?;
    if subject.qself.is_some() {
        return Err(syn::Error::new_spanned(
            subject,
            format!("{kind} tests require an unqualified {kind} type path"),
        ));
    }
    Ok(subject)
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::{DomainTestKind, DomainTestSubjectInput};

    #[test]
    fn action_subject_accepts_an_explicit_descriptor_expression() {
        let subject = DomainTestSubjectInput::parse(
            DomainTestKind::Action,
            quote!(<Actor as RentBicycleAction>::DESCRIPTOR),
        )
        .expect("descriptor expression");
        let output = subject.assemble(&syn::parse_quote!(::domain)).to_string();

        assert!(output.contains("< Actor as RentBicycleAction > :: DESCRIPTOR"));
        assert!(output.contains(". id"));
        assert!(!output.contains("ActionReference"));
    }

    #[test]
    fn action_subject_rejects_missing_descriptor_expression() {
        let error = DomainTestSubjectInput::parse(DomainTestKind::Action, quote!())
            .err()
            .expect("empty action subject");
        assert!(error.to_string().contains("descriptor expression"));
    }

    #[test]
    fn decision_and_invariant_subjects_accept_descriptor_expressions() {
        for (kind, descriptor, variant) in [
            (
                DomainTestKind::Decision,
                quote!(<Fleet as RentalEligibilityDecision>::DESCRIPTOR),
                "Decision",
            ),
            (
                DomainTestKind::Invariant,
                quote!(<Fleet as UniqueBicyclesInvariant>::DESCRIPTOR),
                "Invariant",
            ),
        ] {
            let subject = DomainTestSubjectInput::parse(kind, descriptor.clone())
                .expect("descriptor expression");
            let output = subject.assemble(&syn::parse_quote!(::domain)).to_string();
            assert!(output.contains(&descriptor.to_string()));
            assert!(output.contains(variant));
            assert!(output.contains(". id"));
            assert!(!output.contains("REFERENCE"));
        }
    }
}
