use std::fs;
use std::path::Path;

use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Item, ItemMod};

use super::facts::{ModuleDeclaration, SourceFileFacts};
use super::item::top_level_fact;
use super::recognize::is_cfg_test;
use super::visitor::{FactVisitor, line};

pub fn parse(path: &Path) -> Result<SourceFileFacts, syn::Error> {
    let source = fs::read_to_string(path).map_err(|error| {
        syn::Error::new(
            Span::call_site(),
            format!("could not read {}: {error}", path.display()),
        )
    })?;
    let file = syn::parse_file(&source)?;
    let mut visitor = FactVisitor::default();
    visitor.visit_file(&file);

    let modules = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Mod(module) => Some(module_fact(module)),
            _ => None,
        })
        .collect();
    let non_composition_items = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Mod(_) | Item::Use(_) => None,
            _ => Some((line(item.span()), item_label(item))),
        })
        .collect();
    let top_level_items = file.items.iter().map(top_level_fact).collect();
    let aliases = file.items.iter().flat_map(item_aliases).collect();
    let glob_import_lines = file.items.iter().flat_map(item_glob_import_lines).collect();

    Ok(SourceFileFacts {
        path: path.to_path_buf(),
        modules,
        primaries: visitor.primaries,
        trait_implementations: visitor.trait_implementations,
        aliases,
        glob_import_lines,
        top_level_items,
        non_composition_items,
        test_lines: visitor.test_lines,
        include_lines: visitor.include_lines,
    })
}

fn item_aliases(item: &Item) -> Vec<String> {
    match item {
        Item::Type(item) => vec![item.ident.to_string()],
        Item::Use(item) => {
            let mut aliases = Vec::new();
            collect_use_aliases(&item.tree, &mut aliases);
            aliases
        }
        _ => Vec::new(),
    }
}

fn collect_use_aliases(tree: &syn::UseTree, aliases: &mut Vec<String>) {
    match tree {
        syn::UseTree::Rename(rename) if rename.rename != "_" => {
            aliases.push(rename.rename.to_string());
        }
        syn::UseTree::Path(path) => collect_use_aliases(&path.tree, aliases),
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_aliases(item, aliases);
            }
        }
        _ => {}
    }
}

fn item_glob_import_lines(item: &Item) -> Vec<usize> {
    let Item::Use(item) = item else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    collect_glob_import_lines(&item.tree, &mut lines);
    lines
}

fn collect_glob_import_lines(tree: &syn::UseTree, lines: &mut Vec<usize>) {
    match tree {
        syn::UseTree::Glob(glob) => lines.push(line(glob.span())),
        syn::UseTree::Path(path) => collect_glob_import_lines(&path.tree, lines),
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_glob_import_lines(item, lines);
            }
        }
        _ => {}
    }
}

fn module_fact(module: &ItemMod) -> ModuleDeclaration {
    ModuleDeclaration {
        name: module.ident.to_string(),
        line: line(module.span()),
        is_inline: module.content.is_some(),
        has_path_override: module
            .attrs
            .iter()
            .any(|attribute| attribute.path().is_ident("path")),
        is_test_gate: module.attrs.iter().any(is_cfg_test),
    }
}

const fn item_label(item: &Item) -> &'static str {
    match item {
        Item::Const(_) => "const item",
        Item::Enum(_) => "enum",
        Item::ExternCrate(_) => "extern crate item",
        Item::Fn(_) => "function",
        Item::ForeignMod(_) => "extern block",
        Item::Impl(_) => "implementation",
        Item::Macro(_) => "macro invocation",
        Item::Static(_) => "static item",
        Item::Struct(_) => "struct",
        Item::Trait(_) => "trait",
        Item::TraitAlias(_) => "trait alias",
        Item::Type(_) => "type alias",
        Item::Union(_) => "union",
        _ => "item",
    }
}
