#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DirectoryRole {
    BoundedContext,
    Aggregate,
    DomainService,
    Entity,
    ValueObject,
    Action,
    Decision,
    Query,
    Invariant,
    Lifecycle,
}

const CONTEXT_CHILDREN: &[DirectoryRole] = &[
    DirectoryRole::Aggregate,
    DirectoryRole::DomainService,
    DirectoryRole::ValueObject,
];
const DOMAIN_SERVICE_CHILDREN: &[DirectoryRole] = &[DirectoryRole::Action];
const AGGREGATE_CHILDREN: &[DirectoryRole] = &[
    DirectoryRole::Entity,
    DirectoryRole::Action,
    DirectoryRole::Decision,
    DirectoryRole::Query,
    DirectoryRole::Invariant,
    DirectoryRole::ValueObject,
];
const ENTITY_CHILDREN: &[DirectoryRole] = &[
    DirectoryRole::Action,
    DirectoryRole::Decision,
    DirectoryRole::Invariant,
    DirectoryRole::Lifecycle,
    DirectoryRole::ValueObject,
];
const VALUE_OBJECT_CHILDREN: &[DirectoryRole] = &[
    DirectoryRole::Action,
    DirectoryRole::Decision,
    DirectoryRole::Invariant,
];

impl DirectoryRole {
    pub(super) const ALL: [Self; 10] = [
        Self::BoundedContext,
        Self::Aggregate,
        Self::DomainService,
        Self::Entity,
        Self::ValueObject,
        Self::Action,
        Self::Decision,
        Self::Query,
        Self::Invariant,
        Self::Lifecycle,
    ];

    pub(super) const fn anchor(self) -> &'static str {
        match self {
            Self::BoundedContext => "context.rs",
            Self::Aggregate => "aggregate.rs",
            Self::DomainService => "service.rs",
            Self::Entity => "entity.rs",
            Self::ValueObject => "value.rs",
            Self::Action => "action.rs",
            Self::Decision => "decision.rs",
            Self::Query => "query.rs",
            Self::Invariant => "contract.rs",
            Self::Lifecycle => "lifecycle.rs",
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::BoundedContext => "bounded context",
            Self::Aggregate => "aggregate",
            Self::DomainService => "domain service",
            Self::Entity => "entity",
            Self::ValueObject => "value object",
            Self::Action => "action",
            Self::Decision => "decision",
            Self::Query => "query",
            Self::Invariant => "invariant",
            Self::Lifecycle => "lifecycle",
        }
    }

    pub(super) const fn allowed_children(self) -> &'static [Self] {
        match self {
            Self::BoundedContext => CONTEXT_CHILDREN,
            Self::Aggregate => AGGREGATE_CHILDREN,
            Self::DomainService => DOMAIN_SERVICE_CHILDREN,
            Self::Entity => ENTITY_CHILDREN,
            Self::ValueObject => VALUE_OBJECT_CHILDREN,
            Self::Action | Self::Decision | Self::Query | Self::Invariant | Self::Lifecycle => &[],
        }
    }
}
