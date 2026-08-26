extern crate self as domain;

mod action;
mod aggregate;
mod bounded_context;
mod decision;
mod domain_command;
mod domain_error;
mod domain_event;
mod domain_identity;
mod domain_model;
mod domain_query;
mod domain_service;
mod domain_test;
mod entity;
mod entity_lifecycle;
mod field;
mod invariant;
mod value_object;

pub mod extension {
    pub use crate::action::ActionGroupType;
}

pub use action::{
    ActionDescriptor, ActionId, ActionInputDescriptor, ActionInputType, ActionOutputDescriptor,
    ActionOutputType, ActionOwnerId, ActionOwnerType, ActionReference, AggregateActionOwnerType,
    DomainServiceActionOwnerType, EntityActionOwnerType, InternalActionOwnerType,
    PublicActionOwnerType, ValueObjectActionOwnerType,
};
pub use aggregate::{AggregateDescriptor, AggregateId, AggregateType};
pub use bounded_context::{BoundedContextDescriptor, BoundedContextId, BoundedContextType};
pub use decision::{
    AggregateDecisionOwnerType, DecisionDescriptor, DecisionId, DecisionImplementationDescriptor,
    DecisionInputDescriptor, DecisionInputType, DecisionOutputDescriptor, DecisionOutputType,
    DecisionOwnerId, DecisionOwnerType, DecisionReference, DomainServiceDecisionOwnerType,
    EntityDecisionOwnerType, ValueObjectDecisionOwnerType,
};
pub use domain_command::{
    DomainCommandDescriptor, DomainCommandId, DomainCommandOwnerId, DomainCommandOwnerType,
    DomainCommandType,
};
pub use domain_error::{
    DomainErrorDescriptor, DomainErrorId, DomainErrorOwnerId, DomainErrorOwnerType, DomainErrorType,
};
pub use domain_event::{
    DomainEventDefinition, DomainEventDefinitionType, DomainEventDescriptor, DomainEventId,
    DomainEventType,
};
pub use domain_identity::{DomainIdentityDescriptor, DomainIdentityId, DomainIdentityType};
pub use domain_query::{
    QueryDescriptor, QueryGroupType, QueryId, QueryInputDescriptor, QueryInputType,
    QueryOutputDescriptor, QueryOutputType,
};
pub use domain_service::{DomainServiceDescriptor, DomainServiceId, DomainServiceType};
pub use domain_test::{DomainTestDescriptor, DomainTestSubject};
pub use entity::{EntityDescriptor, EntityId, EntityType, IdentityDescriptor};
pub use entity_lifecycle::{
    EntityLifecycleDescriptor, EntityLifecycleId, EntityLifecycleStateDescriptor,
    EntityLifecycleStateId, EntityLifecycleTransitionDescriptor, EntityLifecycleType,
};
pub use field::{
    FieldDescriptor, FieldKind, FieldValue, FieldWrapper, ScalarType, SemanticScalar,
    SemanticScalarDescriptor,
};
pub use invariant::{
    AggregateInvariantOwnerType, EntityInvariantOwnerType, InvariantDescriptor, InvariantId,
    InvariantOwnerId, InvariantOwnerType, InvariantReference, InvariantViolation,
    ValueObjectInvariantOwnerType,
};
pub use rostfrei_domain_macros::{
    Aggregate, BoundedContext, DomainCommand, DomainError, DomainEvent, DomainIdentity,
    DomainService, Entity, EntityLifecycle, ValueObject, domain_action_test, domain_actions,
    domain_decision_test, domain_decisions, domain_invariant_test, domain_invariants,
    domain_lifecycle_test, domain_queries,
};
pub use value_object::{
    ValueObjectDescriptor, ValueObjectId, ValueObjectOwnerId, ValueObjectOwnerType,
    ValueObjectShapeDescriptor, ValueObjectType, ValueObjectVariantDescriptor,
    ValueObjectVariantShapeDescriptor,
};

#[doc(hidden)]
pub mod __private {
    pub use crate::action::output::{
        AggregateActionOutput, DomainServiceActionOutput, EntityActionOutput, SameType,
        ValueObjectActionOutput,
    };
    pub use crate::domain_model::DomainModelBuilder;
    pub use crate::domain_test::emit_domain_test_metadata as emit_domain_test_descriptor;
}

#[macro_export]
macro_rules! domain_model {
    {
        contexts: [$($context:ty),* $(,)?],
        aggregates: [$($aggregate:ty),* $(,)?],
        entities: [$($entity:ty),* $(,)?],
        identities: [$($identity:ty),* $(,)?],
        value_objects: [$($value_object:ty),* $(,)?],
        services: [$($service:ty),* $(,)?],
        commands: [$($command:ty),* $(,)?],
        errors: [$($error:ty),* $(,)?],
        $(action_extensions: [$($action_extension:ty),* $(,)?],)?
        query_groups: [$($query_group:ty),* $(,)?] $(,)?
    } => {{
        let mut builder = $crate::__private::DomainModelBuilder::new();
        $(builder.add_bounded_context(<$context as $crate::BoundedContextType>::DESCRIPTOR);)*
        $(builder.add_aggregate_type::<$aggregate>();)*
        $(builder.add_entity_type::<$entity>();)*
        $(builder.add_domain_identity_type::<$identity>();)*
        $(builder.add_value_object_type::<$value_object>();)*
        $(builder.add_domain_service_type::<$service>();)*
        $(builder.add_domain_command(<$command as $crate::DomainCommandType>::DESCRIPTOR);)*
        $(builder.add_domain_error(<$error as $crate::DomainErrorType>::DESCRIPTOR);)*
        $($(builder.add_action_extension::<$action_extension>();)*)?
        $(builder.add_queries(<$query_group as $crate::QueryGroupType>::QUERIES);)*
        builder.finish()
    }};
}
