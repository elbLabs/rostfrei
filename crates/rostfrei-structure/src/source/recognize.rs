use syn::punctuated::Punctuated;
use syn::{Attribute, Path, Token};

use super::facts::PrimaryKind;

pub(super) fn attribute_primaries(attribute: &Attribute) -> Vec<PrimaryKind> {
    let Some(name) = known_final_segment(attribute.path()) else {
        return Vec::new();
    };
    match name {
        "domain_action" => vec![PrimaryKind::Action],
        "domain_decision" => vec![PrimaryKind::Decision],
        "domain_query" => vec![PrimaryKind::Query],
        "domain_invariant" => vec![PrimaryKind::Invariant],
        "derive" => derive_primaries(attribute),
        _ => Vec::new(),
    }
}

fn derive_primaries(attribute: &Attribute) -> Vec<PrimaryKind> {
    let Ok(paths) = attribute.parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated)
    else {
        return Vec::new();
    };
    paths
        .iter()
        .filter_map(|path| match known_final_segment(path)? {
            "BoundedContext" => Some(PrimaryKind::BoundedContext),
            "Aggregate" => Some(PrimaryKind::Aggregate),
            "AggregateEvents" => Some(PrimaryKind::AggregateEvents),
            "Entity" => Some(PrimaryKind::Entity),
            "DomainIdentity" => Some(PrimaryKind::Identity),
            "ValueObject" => Some(PrimaryKind::ValueObject),
            "DomainService" => Some(PrimaryKind::DomainService),
            "Command" => Some(PrimaryKind::Command),
            "DomainEvent" => Some(PrimaryKind::Event),
            "DomainError" => Some(PrimaryKind::Rejection),
            "DecisionOutcome" => Some(PrimaryKind::DecisionOutcome),
            "EntityLifecycle" => Some(PrimaryKind::Lifecycle),
            "StateTransition" => Some(PrimaryKind::StateTransition),
            _ => None,
        })
        .collect()
}

pub(super) fn known_final_segment(path: &Path) -> Option<&str> {
    const NAMES: &[&str] = &[
        "domain_model",
        "include",
        "domain_action",
        "domain_decision",
        "domain_query",
        "domain_invariant",
        "derive",
        "BoundedContext",
        "Aggregate",
        "AggregateEvents",
        "Entity",
        "DomainIdentity",
        "ValueObject",
        "DomainService",
        "Command",
        "DomainEvent",
        "DomainError",
        "DecisionOutcome",
        "EntityLifecycle",
        "StateTransition",
    ];
    final_segment_from(path, NAMES)
}

pub(super) fn is_domain_test(path: &Path) -> bool {
    const TEST_NAMES: &[&str] = &[
        "domain_action_test",
        "domain_decision_test",
        "domain_invariant_test",
        "domain_lifecycle_test",
    ];
    final_segment_from(path, TEST_NAMES).is_some()
}

fn final_segment_from<'a>(path: &Path, names: &'a [&str]) -> Option<&'a str> {
    let segment = path.segments.last()?.ident.to_string();
    names.iter().copied().find(|name| *name == segment)
}

pub(super) fn is_cfg_test(attribute: &Attribute) -> bool {
    attribute.path().is_ident("cfg")
        && attribute.meta.require_list().is_ok_and(|list| {
            list.tokens
                .to_string()
                .split_whitespace()
                .collect::<String>()
                == "test"
        })
}
