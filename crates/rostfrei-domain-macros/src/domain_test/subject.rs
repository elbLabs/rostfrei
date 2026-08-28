use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote_spanned};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{ExprPath, Path, PathArguments, Type, TypePath};

use super::DomainTestKind;

pub(crate) enum DomainTestSubjectInput {
    Action(TypedSubject),
    Decision(TypedSubject),
    Invariant(TypedSubject),
    Lifecycle(TypePath),
}

#[derive(Clone, Copy)]
enum TypedSubjectKind {
    Action,
    Decision,
    Invariant,
}

pub(crate) struct TypedSubject {
    owner: Box<Type>,
    trait_path: Path,
    reference: Ident,
    span: Span,
}

impl DomainTestSubjectInput {
    pub(crate) fn parse(kind: DomainTestKind, args: TokenStream) -> syn::Result<Self> {
        match kind {
            DomainTestKind::Action => parse_typed(kind, args).map(Self::Action),
            DomainTestKind::Decision => parse_typed(kind, args).map(Self::Decision),
            DomainTestKind::Invariant => parse_typed(kind, args).map(Self::Invariant),
            DomainTestKind::Lifecycle => parse_lifecycle(args).map(Self::Lifecycle),
        }
    }

    pub(crate) fn assemble(&self, domain_path: &Path) -> TokenStream {
        match self {
            Self::Action(subject) => subject.assemble(domain_path, TypedSubjectKind::Action),
            Self::Decision(subject) => subject.assemble(domain_path, TypedSubjectKind::Decision),
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
        let owner = &self.owner;
        let trait_path = &self.trait_path;
        let hidden_reference = hidden_reference(kind, &self.reference);
        let span = self.span;
        let (marker, descriptor, reference, variant) = match kind {
            TypedSubjectKind::Action => (
                format_ident!("__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE"),
                format_ident!("ActionDescriptor"),
                format_ident!("ActionReference"),
                format_ident!("Action"),
            ),
            TypedSubjectKind::Decision => (
                format_ident!("__DOMAIN_DECISIONS_TRAIT_REQUIRES_DOMAIN_DECISIONS_ATTRIBUTE"),
                format_ident!("DecisionDescriptor"),
                format_ident!("DecisionReference"),
                format_ident!("Decision"),
            ),
            TypedSubjectKind::Invariant => (
                format_ident!("__DOMAIN_INVARIANTS_TRAIT_REQUIRES_DOMAIN_INVARIANTS_ATTRIBUTE"),
                format_ident!("InvariantDescriptor"),
                format_ident!("InvariantReference"),
                format_ident!("Invariant"),
            ),
        };

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
}

fn parse_typed(kind: DomainTestKind, args: TokenStream) -> syn::Result<TypedSubject> {
    let path: ExprPath = syn::parse2(args).map_err(|error| {
        syn::Error::new(
            error.span(),
            format!(
                "{} tests require exactly one owner-qualified reference in the form `<Owner as TraitPath>::CANONICAL_REFERENCE`",
                kind.name()
            ),
        )
    })?;
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
        || path.path.segments.len() != qself.position + 1
    {
        return Err(reference_shape_error(kind, &path.path));
    }
    let reference_segment = path.path.segments.last().unwrap();
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

fn reference_shape_error(kind: DomainTestKind, tokens: impl quote::ToTokens) -> syn::Error {
    syn::Error::new_spanned(
        tokens,
        format!(
            "{} tests require an owner-qualified reference in the form `<Owner as TraitPath>::CANONICAL_REFERENCE`",
            kind.name()
        ),
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

fn hidden_reference(kind: TypedSubjectKind, reference: &Ident) -> Ident {
    let subject = match kind {
        TypedSubjectKind::Action => "ACTION",
        TypedSubjectKind::Decision => "DECISION",
        TypedSubjectKind::Invariant => "INVARIANT",
    };
    Ident::new(
        &format!("__DOMAIN_{}_REFERENCE_{}", subject, reference.unraw()),
        reference.span(),
    )
}
