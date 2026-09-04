use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimaryKind {
    Model,
    BoundedContext,
    Aggregate,
    AggregateEvents,
    Entity,
    Identity,
    ValueObject,
    DomainService,
    Action,
    Command,
    Event,
    Rejection,
    Decision,
    DecisionOutcome,
    Query,
    Invariant,
    Lifecycle,
    StateTransition,
}

impl PrimaryKind {
    pub const fn expected_file(self) -> Option<&'static str> {
        match self {
            Self::Model => Some("model.rs"),
            Self::BoundedContext => Some("context.rs"),
            Self::Aggregate => Some("aggregate.rs"),
            Self::AggregateEvents => Some("event_set.rs"),
            Self::Entity => None,
            Self::ValueObject => Some("value.rs"),
            Self::Identity => Some("identity.rs"),
            Self::DomainService => Some("service.rs"),
            Self::Action => Some("action.rs"),
            Self::Command => Some("command.rs"),
            Self::Event => Some("event.rs"),
            Self::Rejection => Some("rejection.rs"),
            Self::Decision => Some("decision.rs"),
            Self::DecisionOutcome => Some("outcome.rs"),
            Self::Query => Some("query.rs"),
            Self::Invariant => Some("contract.rs"),
            Self::Lifecycle => Some("lifecycle.rs"),
            Self::StateTransition => Some("transition.rs"),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Model => "domain_model!",
            Self::BoundedContext => "BoundedContext",
            Self::Aggregate => "Aggregate",
            Self::AggregateEvents => "AggregateEvents",
            Self::Entity => "Entity",
            Self::Identity => "DomainIdentity",
            Self::ValueObject => "ValueObject",
            Self::DomainService => "DomainService",
            Self::Action => "domain_action",
            Self::Command => "Command",
            Self::Event => "DomainEvent",
            Self::Rejection => "DomainError",
            Self::Decision => "domain_decision",
            Self::DecisionOutcome => "DecisionOutcome",
            Self::Query => "domain_query",
            Self::Invariant => "domain_invariant",
            Self::Lifecycle => "EntityLifecycle",
            Self::StateTransition => "StateTransition",
        }
    }
}

#[derive(Clone, Debug)]
pub struct PrimaryDeclaration {
    pub kind: PrimaryKind,
    pub line: usize,
}

#[derive(Clone, Debug)]
pub struct ModuleDeclaration {
    pub name: String,
    pub line: usize,
    pub is_inline: bool,
    pub has_path_override: bool,
    pub is_test_gate: bool,
}

#[derive(Clone, Debug)]
pub struct TraitImplementation {
    pub trait_name: Option<String>,
    pub trait_is_direct: bool,
    pub implementor: TypeReference,
    pub associated_event_types: Vec<AssociatedTypeReference>,
    pub associated_root_types: Vec<AssociatedTypeReference>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeReference {
    Direct(String),
    SingleGeneric {
        constructor: String,
        argument: String,
    },
    Unsupported,
}

#[derive(Clone, Debug)]
pub struct AssociatedTypeReference {
    pub name: Option<String>,
    pub line: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TopLevelItemKind {
    Import,
    Nominal,
    Trait,
    Implementation,
    Function,
    Macro,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NominalShape {
    Enum,
    UnitStruct,
    Other,
}

#[derive(Clone, Debug)]
pub struct TopLevelItem {
    pub kind: TopLevelItemKind,
    pub label: &'static str,
    pub name: Option<String>,
    pub primaries: Vec<PrimaryKind>,
    pub trait_name: Option<String>,
    pub self_type: Option<String>,
    pub line: usize,
    pub is_private: bool,
    pub nominal_shape: NominalShape,
    pub contains_domain_model: bool,
}

#[derive(Clone, Debug)]
pub struct SourceFileFacts {
    pub path: PathBuf,
    pub modules: Vec<ModuleDeclaration>,
    pub primaries: Vec<PrimaryDeclaration>,
    pub trait_implementations: Vec<TraitImplementation>,
    pub aliases: Vec<String>,
    pub glob_import_lines: Vec<usize>,
    pub top_level_items: Vec<TopLevelItem>,
    pub non_composition_items: Vec<(usize, &'static str)>,
    pub test_lines: Vec<usize>,
    pub include_lines: Vec<usize>,
}
