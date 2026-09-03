use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::source::{PrimaryKind, SourceFileFacts, TypeReference};

pub(super) enum ExpectedOwner {
    Aggregate(String),
    Direct(String),
}

pub(super) fn expected_owner(
    directory: &Path,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
) -> Option<ExpectedOwner> {
    let candidates = [
        ("aggregate.rs", PrimaryKind::Aggregate),
        ("entity.rs", PrimaryKind::Entity),
        ("service.rs", PrimaryKind::DomainService),
        ("value.rs", PrimaryKind::ValueObject),
    ]
    .into_iter()
    .filter_map(|(file, kind)| {
        facts
            .get(&directory.join(file))
            .and_then(|facts| unique_primary_name(facts, kind))
            .map(|name| (kind, name.to_owned()))
    })
    .collect::<Vec<_>>();
    let [(kind, name)] = candidates.as_slice() else {
        return None;
    };
    Some(if *kind == PrimaryKind::Aggregate {
        ExpectedOwner::Aggregate(name.clone())
    } else {
        ExpectedOwner::Direct(name.clone())
    })
}

fn unique_primary_name(file: &SourceFileFacts, kind: PrimaryKind) -> Option<&str> {
    let declarations = file
        .top_level_items
        .iter()
        .filter(|item| item.primaries.contains(&kind))
        .collect::<Vec<_>>();
    let [declaration] = declarations.as_slice() else {
        return None;
    };
    declaration.name.as_deref()
}

impl ExpectedOwner {
    pub(super) fn matches(&self, actual: &TypeReference, aliases: &[String]) -> bool {
        if actual.references_alias(aliases) {
            return false;
        }
        match (self, actual) {
            (
                Self::Aggregate(expected),
                TypeReference::SingleGeneric {
                    constructor,
                    argument,
                },
            ) => constructor == "AggregateInstance" && argument == expected,
            (Self::Direct(expected), TypeReference::Direct(actual)) => actual == expected,
            _ => false,
        }
    }

    pub(super) fn display(&self) -> String {
        match self {
            Self::Aggregate(name) => format!("AggregateInstance<{name}>"),
            Self::Direct(name) => name.clone(),
        }
    }
}

impl TypeReference {
    pub(super) fn references_alias(&self, aliases: &[String]) -> bool {
        match self {
            Self::Direct(name) => aliases.contains(name),
            Self::SingleGeneric {
                constructor,
                argument,
            } => aliases.contains(constructor) || aliases.contains(argument),
            Self::Unsupported => false,
        }
    }

    pub(super) fn display(&self) -> Option<String> {
        match self {
            Self::Direct(name) => Some(name.clone()),
            Self::SingleGeneric {
                constructor,
                argument,
            } => Some(format!("{constructor}<{argument}>")),
            Self::Unsupported => None,
        }
    }
}
