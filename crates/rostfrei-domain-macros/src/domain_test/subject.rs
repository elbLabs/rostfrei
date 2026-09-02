use proc_macro2::{Ident, Span, TokenStream};
use quote::quote_spanned;
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Expr, ExprPath, Path, PathArguments, Type, TypePath};

use super::DomainTestKind;

pub(super) enum DomainTestSubjectInput {
    Action(Expr),
    Decision(DecisionSubject),
    Invariant(TypedSubject),
    Lifecycle(TypePath),
}

pub(super) struct TypedSubject {
    owner: Box<Type>,
    trait_path: Path,
    reference: Ident,
    span: Span,
}

pub(super) struct DecisionSubject {
    owner: TypePath,
    reference: Ident,
    span: Span,
}

impl DomainTestSubjectInput {
    pub(super) fn parse(kind: DomainTestKind, args: TokenStream) -> syn::Result<Self> {
        match kind {
            DomainTestKind::Action => parse_action(args).map(Self::Action),
            DomainTestKind::Decision => parse_decision(args).map(Self::Decision),
            DomainTestKind::Invariant => parse_typed(kind, args).map(Self::Invariant),
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
            Self::Decision(subject) => subject.assemble(domain_path),
            Self::Invariant(subject) => subject.assemble(domain_path),
            Self::Lifecycle(lifecycle) => quote_spanned! {lifecycle.path.span()=>
                #domain_path::DomainTestSubject::Lifecycle(
                    <#lifecycle as #domain_path::EntityLifecycleType>::DESCRIPTOR.id
                )
            },
        }
    }
}

fn parse_action(args: TokenStream) -> syn::Result<Expr> {
    syn::parse2(args).map_err(|error| {
        syn::Error::new(
            error.span(),
            "action tests require exactly one action descriptor expression",
        )
    })
}

impl TypedSubject {
    fn assemble(&self, domain_path: &Path) -> TokenStream {
        let owner = &self.owner;
        let trait_path = &self.trait_path;
        let hidden_reference = hidden_reference("INVARIANT", &self.reference);
        let span = self.span;
        quote_spanned! {span=>
            {
                let _: &'static [#domain_path::InvariantDescriptor] =
                    <#owner as #trait_path>::__DOMAIN_INVARIANTS;
                let reference: #domain_path::InvariantReference =
                    <#owner as #trait_path>::#hidden_reference;
                #domain_path::DomainTestSubject::Invariant(reference.id())
            }
        }
    }
}

impl DecisionSubject {
    fn assemble(&self, domain_path: &Path) -> TokenStream {
        let owner = &self.owner;
        let hidden_reference = hidden_reference("DECISION", &self.reference);
        let span = self.span;
        quote_spanned! {span=>
            {
                let reference = #owner::#hidden_reference;
                #domain_path::DomainTestSubject::Decision(reference.__attached_id())
            }
        }
    }
}

fn parse_typed(kind: DomainTestKind, args: TokenStream) -> syn::Result<TypedSubject> {
    let path: ExprPath = syn::parse2(args)
        .map_err(|error| syn::Error::new(error.span(), typed_reference_message(kind, true)))?;
    if !path.attrs.is_empty() {
        return Err(syn::Error::new_spanned(
            &path,
            format!("{} test references cannot have attributes", kind.name()),
        ));
    }
    let Some(qself) = path.qself else {
        return Err(reference_shape_error(kind, &path.path));
    };
    if qself.as_token.is_none()
        || qself.position == 0
        || path.path.segments.len().checked_sub(1) != Some(qself.position)
    {
        return Err(reference_shape_error(kind, &path.path));
    }
    let Some(reference_segment) = path.path.segments.last() else {
        return Err(reference_shape_error(kind, &path.path));
    };
    if !matches!(reference_segment.arguments, PathArguments::None) {
        return Err(reference_shape_error(kind, reference_segment));
    }
    validate_canonical_reference(kind, &reference_segment.ident)?;
    let reference = reference_segment.ident.clone();
    let span = reference.span();
    let trait_path = Path {
        leading_colon: path.path.leading_colon,
        segments: path
            .path
            .segments
            .iter()
            .take(qself.position)
            .cloned()
            .collect(),
    };
    Ok(TypedSubject {
        owner: qself.ty,
        trait_path,
        reference,
        span,
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

fn parse_decision(args: TokenStream) -> syn::Result<DecisionSubject> {
    let path: ExprPath = syn::parse2(args).map_err(|error| {
        syn::Error::new(
            error.span(),
            "decision tests require exactly one owner-qualified reference in the form `Owner::REFERENCE`",
        )
    })?;
    if !path.attrs.is_empty() {
        return Err(syn::Error::new_spanned(
            &path,
            "decision test references cannot have attributes",
        ));
    }
    if path.qself.is_some() || path.path.segments.len() < 2 {
        return Err(syn::Error::new_spanned(
            path,
            "decision tests require an owner-qualified reference in the form `Owner::REFERENCE`",
        ));
    }
    let Some(reference) = path.path.segments.last() else {
        return Err(syn::Error::new_spanned(
            path,
            "decision tests require an owner-qualified reference in the form `Owner::REFERENCE`",
        ));
    };
    if !matches!(reference.arguments, PathArguments::None) {
        return Err(syn::Error::new_spanned(
            reference,
            "decision tests require an owner-qualified reference in the form `Owner::REFERENCE`",
        ));
    }
    validate_canonical_reference(DomainTestKind::Decision, &reference.ident)?;
    let owner_segment_count = path.path.segments.len().saturating_sub(1);
    let owner = TypePath {
        qself: None,
        path: Path {
            leading_colon: path.path.leading_colon,
            segments: path
                .path
                .segments
                .iter()
                .take(owner_segment_count)
                .cloned()
                .collect(),
        },
    };
    Ok(DecisionSubject {
        owner,
        reference: reference.ident.clone(),
        span: reference.ident.span(),
    })
}

fn reference_shape_error(kind: DomainTestKind, tokens: impl quote::ToTokens) -> syn::Error {
    syn::Error::new_spanned(tokens, typed_reference_message(kind, false))
}

fn typed_reference_message(kind: DomainTestKind, exactly: bool) -> String {
    let amount = if exactly { "exactly one " } else { "an " };
    format!(
        "{} tests require {amount}implementor-qualified reference in the form `<Type as TraitPath>::CANONICAL_REFERENCE`",
        kind.name()
    )
}

fn validate_canonical_reference(kind: DomainTestKind, reference: &Ident) -> syn::Result<()> {
    let name = reference.unraw().to_string();
    let normalized = if let Some(numeric) = name.strip_prefix('_') {
        if !numeric.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return Err(canonical_reference_error(kind, reference));
        }
        numeric
    } else {
        if !name.as_bytes().first().is_some_and(u8::is_ascii_uppercase) {
            return Err(canonical_reference_error(kind, reference));
        }
        name.as_str()
    };
    if normalized.split('_').all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    }) {
        Ok(())
    } else {
        Err(canonical_reference_error(kind, reference))
    }
}

fn canonical_reference_error(kind: DomainTestKind, reference: &Ident) -> syn::Error {
    syn::Error::new(
        reference.span(),
        format!(
            "{} test references must use canonical uppercase names such as `CREATE` or `_2FA_START`",
            kind.name()
        ),
    )
}

fn hidden_reference(subject: &str, reference: &Ident) -> Ident {
    Ident::new(
        &format!("__DOMAIN_{subject}_REFERENCE_{}", reference.unraw()),
        reference.span(),
    )
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
}
