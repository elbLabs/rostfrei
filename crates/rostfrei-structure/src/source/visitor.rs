use proc_macro2::Span;
use syn::Attribute;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use super::facts::{
    AssociatedTypeReference, PrimaryDeclaration, PrimaryKind, TraitImplementation, TypeReference,
};
use super::recognize::{attribute_primaries, is_cfg_test, is_domain_test, known_final_segment};

#[derive(Default)]
pub(super) struct FactVisitor {
    pub(super) primaries: Vec<PrimaryDeclaration>,
    pub(super) trait_implementations: Vec<TraitImplementation>,
    pub(super) test_lines: Vec<usize>,
    pub(super) include_lines: Vec<usize>,
}

impl<'ast> Visit<'ast> for FactVisitor {
    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        if attribute.path().is_ident("test")
            || is_domain_test(attribute.path())
            || is_cfg_test(attribute)
        {
            self.test_lines.push(line(attribute.span()));
        }
        self.primaries
            .extend(
                attribute_primaries(attribute)
                    .into_iter()
                    .map(|kind| PrimaryDeclaration {
                        kind,
                        line: line(attribute.span()),
                    }),
            );
        visit::visit_attribute(self, attribute);
    }

    fn visit_item_impl(&mut self, implementation: &'ast syn::ItemImpl) {
        self.trait_implementations.push(TraitImplementation {
            trait_name: implementation
                .trait_
                .as_ref()
                .and_then(|(_, path, _)| path.segments.last())
                .map(|segment| segment.ident.to_string()),
            trait_is_direct: implementation
                .trait_
                .as_ref()
                .is_some_and(|(negation, path, _)| {
                    negation.is_none() && direct_path_identifier(path).is_some()
                }),
            implementor: type_reference(&implementation.self_ty),
            associated_event_types: implementation
                .items
                .iter()
                .filter_map(|item| {
                    let syn::ImplItem::Type(item) = item else {
                        return None;
                    };
                    (item.ident == "Event").then(|| AssociatedTypeReference {
                        name: direct_type_identifier(&item.ty),
                        line: line(item.ty.span()),
                    })
                })
                .collect(),
            associated_root_types: implementation
                .items
                .iter()
                .filter_map(|item| {
                    let syn::ImplItem::Type(item) = item else {
                        return None;
                    };
                    (item.ident == "Root").then(|| AssociatedTypeReference {
                        name: direct_type_identifier(&item.ty),
                        line: line(item.ty.span()),
                    })
                })
                .collect(),
            line: line(implementation.span()),
        });
        visit::visit_item_impl(self, implementation);
    }

    fn visit_macro(&mut self, item_macro: &'ast syn::Macro) {
        match known_final_segment(&item_macro.path) {
            Some("domain_model") => self.primaries.push(PrimaryDeclaration {
                kind: PrimaryKind::Model,
                line: line(item_macro.span()),
            }),
            Some("include") => self.include_lines.push(line(item_macro.span())),
            _ => {}
        }
        visit::visit_macro(self, item_macro);
    }
}

fn direct_type_identifier(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(ty) = ty else {
        return None;
    };
    if ty.qself.is_some() || ty.path.leading_colon.is_some() || ty.path.segments.len() != 1 {
        return None;
    }
    let segment = ty.path.segments.first()?;
    matches!(segment.arguments, syn::PathArguments::None).then(|| segment.ident.to_string())
}

fn direct_path_identifier(path: &syn::Path) -> Option<String> {
    if path.leading_colon.is_some() || path.segments.len() != 1 {
        return None;
    }
    let segment = path.segments.first()?;
    matches!(segment.arguments, syn::PathArguments::None).then(|| segment.ident.to_string())
}

fn type_reference(ty: &syn::Type) -> TypeReference {
    let syn::Type::Path(ty) = ty else {
        return TypeReference::Unsupported;
    };
    if ty.qself.is_some() || ty.path.leading_colon.is_some() || ty.path.segments.len() != 1 {
        return TypeReference::Unsupported;
    }
    let Some(segment) = ty.path.segments.first() else {
        return TypeReference::Unsupported;
    };
    match &segment.arguments {
        syn::PathArguments::None => TypeReference::Direct(segment.ident.to_string()),
        syn::PathArguments::AngleBracketed(arguments)
            if arguments.colon2_token.is_none() && arguments.args.len() == 1 =>
        {
            let Some(syn::GenericArgument::Type(argument)) = arguments.args.first() else {
                return TypeReference::Unsupported;
            };
            let Some(argument) = direct_type_identifier(argument) else {
                return TypeReference::Unsupported;
            };
            TypeReference::SingleGeneric {
                constructor: segment.ident.to_string(),
                argument,
            }
        }
        _ => TypeReference::Unsupported,
    }
}

pub(super) fn line(span: Span) -> usize {
    span.start().line
}
