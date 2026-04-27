use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote_spanned};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{ExprPath, Path, PathArguments, Type, TypePath};

use super::DomainTestKind;

pub(super) enum DomainTestSubjectInput {
    Action(TypedSubject),
    Decision(DecisionSubject),
    Invariant(TypedSubject),
    Lifecycle(TypePath),
}

#[derive(Clone, Copy)]
enum TypedSubjectKind {
    Action,
    Invariant,
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
            DomainTestKind::Action => parse_typed(kind, args).map(Self::Action),
            DomainTestKind::Decision => parse_decision(args).map(Self::Decision),
            DomainTestKind::Invariant => parse_typed(kind, args).map(Self::Invariant),
            DomainTestKind::Lifecycle => parse_lifecycle(args).map(Self::Lifecycle),
        }
    }

    pub(super) fn assemble(&self, domain_path: &Path) -> TokenStream {
        match self {
            Self::Action(subject) => subject.assemble(domain_path, TypedSubjectKind::Action),
            Self::Decision(subject) => subject.assemble(domain_path),
            Self::Invariant(subject) => subject.assemble(domain_path, TypedSubjectKind::Invariant),
            Self::Lifecycle(lifecycle) => quote_spanned! {lifecycle.path.span()=>
                #domain_path::DomainTestSubject::Lifecycle(
                    <#lifecycle as #domain_path::EntityLifecycleType>::DESCRIPTOR.id
                )
            },
        }
    }
}

impl TypedSubject {
    fn assemble(&self, domain_path: &Path, kind: TypedSubjectKind) -> TokenStream {
        if matches!(kind, TypedSubjectKind::Invariant) {
            return self.assemble_invariant(domain_path);
        }
        let owner = &self.owner;
        let trait_path = &self.trait_path;
        let span = self.span;
        let marker = format_ident!("__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE");
        let descriptor = format_ident!("ActionDescriptor");
        let reference = format_ident!("ActionReference");
        let variant = format_ident!("Action");
        let subject = "ACTION";
        let hidden_reference = hidden_reference(subject, &self.reference);

        quote_spanned! {span=>
            {
                let _: &'static [#domain_path::#descriptor] =
                    <#owner as #trait_path>::#marker;
                let reference: #domain_path::#reference<#owner> =
                    <#owner as #trait_path>::#hidden_reference;
                #domain_path::DomainTestSubject::#variant(reference.id())
            }
        }
    }

    fn assemble_invariant(&self, domain_path: &Path) -> TokenStream {
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

fn parse_lifecycle(args: TokenStream) -> syn::Result<TypePath> {
    let lifecycle: TypePath = syn::parse2(args).map_err(|error| {
        syn::Error::new(
            error.span(),
            "lifecycle tests require exactly one lifecycle type path",
        )
    })?;
    if lifecycle.qself.is_some() {
        return Err(syn::Error::new_spanned(
            lifecycle,
            "lifecycle tests require an unqualified lifecycle type path",
        ));
    }
    Ok(lifecycle)
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
    match kind {
        DomainTestKind::Invariant => format!(
            "invariant tests require {amount}implementor-qualified reference in the form `<Type as TraitPath>::CANONICAL_REFERENCE`"
        ),
        _ => format!(
            "{} tests require {amount}owner-qualified reference in the form `<Owner as TraitPath>::CANONICAL_REFERENCE`",
            kind.name(),
        ),
    }
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
