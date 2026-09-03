use crate::source::{NominalShape, PrimaryKind, SourceFileFacts, TopLevelItem, TopLevelItemKind};

#[derive(Clone, Copy)]
pub(super) enum RolePolicy {
    Marker(PrimaryKind),
    Aggregate,
    DomainService,
    EventSet,
    Declaration(PrimaryKind),
    Contract(PrimaryKind),
    Execute,
    Implementation(Option<&'static str>),
    Model,
}

impl RolePolicy {
    pub(super) fn for_file(file: &SourceFileFacts) -> Option<Self> {
        let file_name = file.path.file_name()?.to_str()?;
        let policy = match file_name {
            "mod.rs" => return None,
            "model.rs" => Self::Model,
            "context.rs" => Self::Marker(PrimaryKind::BoundedContext),
            "aggregate.rs" => Self::Aggregate,
            "service.rs" => Self::DomainService,
            "event_set.rs" => Self::EventSet,
            "entity.rs" | "root.rs" => Self::Declaration(PrimaryKind::Entity),
            "identity.rs" => Self::Declaration(PrimaryKind::Identity),
            "action.rs" => Self::Contract(PrimaryKind::Action),
            "command.rs" => Self::Declaration(PrimaryKind::Command),
            "event.rs" => Self::Declaration(PrimaryKind::Event),
            "rejection.rs" => Self::Declaration(PrimaryKind::Rejection),
            "decision.rs" => Self::Contract(PrimaryKind::Decision),
            "outcome.rs" => Self::Declaration(PrimaryKind::DecisionOutcome),
            "query.rs" => Self::Contract(PrimaryKind::Query),
            "contract.rs" => Self::Contract(PrimaryKind::Invariant),
            "lifecycle.rs" => Self::Declaration(PrimaryKind::Lifecycle),
            "execute.rs" => Self::Execute,
            "handler.rs" => Self::Implementation(Some("CommandHandler")),
            "apply.rs" => Self::Implementation(Some("Apply")),
            "initialize.rs" => Self::Implementation(Some("Initialize")),
            "evaluate.rs" => Self::Implementation(None),
            _ if has_value_object(file) => Self::Declaration(PrimaryKind::ValueObject),
            _ => return None,
        };
        Some(policy)
    }

    pub(super) fn allows(self, item: &TopLevelItem, file: &SourceFileFacts) -> bool {
        if item.kind == TopLevelItemKind::Import {
            return true;
        }
        match self {
            Self::Marker(kind) => {
                item.kind == TopLevelItemKind::Nominal && item.primaries.contains(&kind)
            }
            Self::Aggregate => {
                item.kind == TopLevelItemKind::Nominal
                    && item.primaries.contains(&PrimaryKind::Aggregate)
                    || item.kind == TopLevelItemKind::Implementation
                        && item.trait_name.as_deref() == Some("AggregateDefinition")
                        && item.self_type.as_deref() == primary_name(file, PrimaryKind::Aggregate)
            }
            Self::DomainService => {
                item.kind == TopLevelItemKind::Nominal
                    && item.primaries.contains(&PrimaryKind::DomainService)
                    || item.kind == TopLevelItemKind::Implementation
                        && item.trait_name.as_deref() == Some("DomainServiceDefinition")
                        && item.self_type.as_deref()
                            == primary_name(file, PrimaryKind::DomainService)
            }
            Self::EventSet => {
                item.kind == TopLevelItemKind::Nominal
                    && item.nominal_shape == NominalShape::Enum
                    && item.primaries.contains(&PrimaryKind::AggregateEvents)
            }
            Self::Declaration(kind) => declaration_item(item, file, kind),
            Self::Contract(kind) => {
                item.kind == TopLevelItemKind::Trait && item.primaries.contains(&kind)
            }
            Self::Execute => {
                (item.kind == TopLevelItemKind::Implementation && item.trait_name.is_some())
                    || (item.kind == TopLevelItemKind::Function && item.is_private)
            }
            Self::Implementation(expected) => {
                item.kind == TopLevelItemKind::Implementation
                    && item.trait_name.is_some()
                    && expected.is_none_or(|name| item.trait_name.as_deref() == Some(name))
            }
            Self::Model => item.kind == TopLevelItemKind::Function && item.contains_domain_model,
        }
    }
}

fn declaration_item(item: &TopLevelItem, file: &SourceFileFacts, expected: PrimaryKind) -> bool {
    if item.kind == TopLevelItemKind::Nominal && item.primaries.contains(&expected) {
        return true;
    }
    if item.kind != TopLevelItemKind::Implementation {
        return false;
    }
    let Some(primary_name) = primary_name(file, expected) else {
        return false;
    };
    item.self_type.as_deref() == Some(primary_name)
}

fn primary_name(file: &SourceFileFacts, expected: PrimaryKind) -> Option<&str> {
    file.top_level_items
        .iter()
        .find(|item| item.primaries.contains(&expected))
        .and_then(|item| item.name.as_deref())
}

fn has_value_object(file: &SourceFileFacts) -> bool {
    file.primaries
        .iter()
        .any(|primary| primary.kind == PrimaryKind::ValueObject)
}
