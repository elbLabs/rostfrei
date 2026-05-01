use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, Fields, Item, Type, Visibility};

use super::facts::{NominalShape, PrimaryKind, TopLevelItem, TopLevelItemKind};
use super::recognize::{attribute_primaries, known_final_segment};
use super::visitor::line;

pub(super) fn top_level_fact(item: &Item) -> TopLevelItem {
    let mut fact = TopLevelItem {
        kind: kind(item),
        label: label(item),
        name: name(item),
        primaries: attributes(item)
            .iter()
            .flat_map(attribute_primaries)
            .collect(),
        trait_name: None,
        self_type: None,
        line: line(item.span()),
        is_private: visibility(item)
            .is_none_or(|visibility| matches!(visibility, Visibility::Inherited)),
        nominal_shape: match item {
            Item::Enum(_) => NominalShape::Enum,
            Item::Struct(item) if matches!(item.fields, Fields::Unit) => NominalShape::UnitStruct,
            _ => NominalShape::Other,
        },
        contains_domain_model: contains_domain_model(item),
    };

    if let Item::Impl(implementation) = item {
        fact.trait_name = implementation
            .trait_
            .as_ref()
            .and_then(|(_, path, _)| path.segments.last())
            .map(|segment| segment.ident.to_string());
        fact.self_type = type_name(&implementation.self_ty);
    }
    if matches!(item, Item::Macro(item) if known_final_segment(&item.mac.path) == Some("domain_model"))
    {
        fact.primaries.push(PrimaryKind::Model);
    }
    fact
}

fn attributes(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

const fn kind(item: &Item) -> TopLevelItemKind {
    match item {
        Item::Use(_) => TopLevelItemKind::Import,
        Item::Enum(_) | Item::Struct(_) | Item::Union(_) => TopLevelItemKind::Nominal,
        Item::Trait(_) => TopLevelItemKind::Trait,
        Item::Impl(_) => TopLevelItemKind::Implementation,
        Item::Fn(_) => TopLevelItemKind::Function,
        Item::Macro(_) => TopLevelItemKind::Macro,
        _ => TopLevelItemKind::Other,
    }
}

fn name(item: &Item) -> Option<String> {
    match item {
        Item::Enum(item) => Some(item.ident.to_string()),
        Item::Fn(item) => Some(item.sig.ident.to_string()),
        Item::Struct(item) => Some(item.ident.to_string()),
        Item::Trait(item) => Some(item.ident.to_string()),
        Item::Union(item) => Some(item.ident.to_string()),
        _ => None,
    }
}

const fn visibility(item: &Item) -> Option<&Visibility> {
    match item {
        Item::Enum(item) => Some(&item.vis),
        Item::Fn(item) => Some(&item.vis),
        Item::Struct(item) => Some(&item.vis),
        Item::Trait(item) => Some(&item.vis),
        Item::Union(item) => Some(&item.vis),
        _ => None,
    }
}

fn type_name(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else {
        return None;
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn contains_domain_model(item: &Item) -> bool {
    let mut finder = DomainModelFinder(false);
    finder.visit_item(item);
    finder.0
}

struct DomainModelFinder(bool);

impl<'ast> Visit<'ast> for DomainModelFinder {
    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        if known_final_segment(&item.path) == Some("domain_model") {
            self.0 = true;
        }
        visit::visit_macro(self, item);
    }
}

const fn label(item: &Item) -> &'static str {
    match item {
        Item::Const(_) => "const item",
        Item::Enum(_) => "enum",
        Item::ExternCrate(_) => "extern crate item",
        Item::Fn(_) => "function",
        Item::ForeignMod(_) => "extern block",
        Item::Impl(_) => "implementation",
        Item::Macro(_) => "macro invocation",
        Item::Mod(_) => "module declaration",
        Item::Static(_) => "static item",
        Item::Struct(_) => "struct",
        Item::Trait(_) => "trait",
        Item::TraitAlias(_) => "trait alias",
        Item::Type(_) => "type alias",
        Item::Union(_) => "union",
        Item::Use(_) => "import",
        _ => "item",
    }
}
