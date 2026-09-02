extern crate self as domain;

mod action;
mod aggregate;
mod bounded_context;
mod command;
mod decision;
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
mod json_wire;
mod value_object;

pub mod extension {
    pub use crate::action::ActionGroupType;
}

pub use action::{
    ActionDescriptor, ActionId, ActionOwnerId, ActionOwnerType, ActionReference,
    AggregateActionOwnerType, DomainServiceActionOwnerType, EntityActionOwnerType,
    InternalActionOwnerType, PublicActionOwnerType,
};
pub use aggregate::{
    AggregateDefinition, AggregateDescriptor, AggregateEventSet, AggregateId, AggregateType,
    NoDomainEvents,
};
pub use bounded_context::{BoundedContextDescriptor, BoundedContextId, BoundedContextType};
pub use command::{Command, CommandDescriptor};
#[doc(hidden)]
pub use decision::AttachedDecisionGroup;
pub use decision::{
    AggregateDecisionOwnerType, DecisionDescriptor, DecisionGroupType, DecisionId,
    DecisionImplementationDescriptor, DecisionOutcomeDescriptor, DecisionOutcomeId,
    DecisionOutcomeType, DecisionOwnerId, DecisionOwnerType, DecisionReference,
    EntityDecisionOwnerType,
};
pub use domain_error::{DomainError, DomainErrorDescriptor, DomainErrorId};
pub use domain_event::{
    DomainEventDefinition, DomainEventDefinitionType, DomainEventDescriptor, DomainEventId,
    DomainEventType,
};
pub use domain_identity::{DomainIdentity, DomainIdentityId};
#[doc(hidden)]
pub use domain_identity::{DomainIdentityDescriptor, DomainIdentityType};
pub use domain_model::{DomainModelError, DomainModelReference};
pub use domain_query::{QueryDescriptor, QueryGroupType, QueryId};
pub use domain_service::{DomainServiceDescriptor, DomainServiceId, DomainServiceType};
pub use domain_test::{DomainTestDescriptor, DomainTestSubject};
pub use entity::{EntityDefinition, EntityDescriptor, EntityId, EntityType, IdentityDescriptor};
pub use entity_lifecycle::{
    EntityLifecycleDescriptor, EntityLifecycleId, EntityLifecycleStateDescriptor,
    EntityLifecycleStateId, EntityLifecycleType,
};
pub use field::{
    FieldDescriptor, FieldKind, FieldValue, FieldWrapper, ScalarType, SemanticScalar,
    SemanticScalarDescriptor,
};
pub use invariant::{InvariantDescriptor, InvariantId, InvariantReference, InvariantViolation};
pub use json_wire::{JsonCommandPayload, JsonErrorPayload};
pub use rostfrei_domain_macros::{
    Aggregate, AggregateEvents, BoundedContext, Command, DecisionOutcome, DomainError, DomainEvent,
    DomainIdentity, DomainService, Entity, EntityLifecycle, ValueObject, domain_action_test,
    domain_actions, domain_decision_test, domain_decisions, domain_invariant_test,
    domain_invariants, domain_lifecycle_test, domain_queries,
};
pub use value_object::{ValueObject, ValueObjectDescriptor, ValueObjectId};

#[doc(hidden)]
pub mod __private {
    pub use crate::decision::AttachedDecisionGroup;
    pub use crate::domain_identity::{DomainIdentityDescriptor, DomainIdentityType};
    pub use crate::domain_model::{DomainModelBuilder, try_build};
    pub use crate::domain_test::emit_domain_test_metadata as emit_domain_test_descriptor;
    pub use serde;
    pub use serde_json;
}

#[macro_export]
macro_rules! domain_model {
    {
        contexts: [$($context:ty),* $(,)?],
        aggregates: [$($aggregate:ty),* $(,)?],
        entities: [$($entity:ty),* $(,)?],
        value_objects: [$($value_object:ty),* $(,)?],
        services: [$($service:ty),* $(,)?],
        errors: [$($error:ty),* $(,)?],
        $(action_extensions: [$($action_extension:ty),* $(,)?],)?
        query_groups: [$($query_group:ty),* $(,)?] $(,)?
    } => {{
        $crate::__private::try_build(|builder| {
            $(builder.add_bounded_context(<$context as $crate::BoundedContextType>::DESCRIPTOR);)*
            $(builder.add_aggregate_type::<$aggregate>()?;)*
            $(builder.add_entity_type::<$entity>()?;)*
            $(builder.add_value_object_type::<$value_object>()?;)*
            $(builder.add_domain_service_type::<$service>()?;)*
            $(builder.add_domain_error(<$error as $crate::DomainError>::DESCRIPTOR)?;)*
            $($(builder.add_action_extension::<$action_extension>()?;)*)?
            $(builder.add_queries(<$query_group as $crate::QueryGroupType>::QUERIES)?;)*
            Ok(())
        })
    }};
}
