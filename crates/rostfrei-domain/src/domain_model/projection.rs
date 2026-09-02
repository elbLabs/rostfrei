use serde_json::{Value, json};

use crate::{
    ActionOwnerId, AggregateDefinition, AggregateDescriptor, AggregateEventSet, AggregateId,
    BoundedContextDescriptor, DecisionOwnerId, DomainErrorDescriptor, DomainErrorId,
    DomainEventDescriptor, DomainEventId, DomainIdentityId, DomainServiceDescriptor,
    DomainServiceType, EntityDefinition, EntityDescriptor, QueryDescriptor, QueryId, ValueObject,
    ValueObjectDescriptor, ValueObjectId, extension::ActionGroupType,
};

use super::{
    action_error_validation::ActionErrorInventory,
    action_projection::ActionProjection,
    decision_projection::DecisionProjection,
    entity_projection::EntityProjection,
    error::DomainModelError,
    field_projection,
    field_reference_collection::FieldReferenceCollection,
    field_reference_validation::{self, FieldReferenceInventory},
    id_projection::{
        aggregate as aggregate_id, domain_error as domain_error_id,
        domain_identity as domain_identity_id, entity as entity_id, query as query_id,
        value_object as value_object_id,
    },
};

pub struct DomainModelBuilder {
    bounded_contexts: Vec<Value>,
    aggregates: Vec<(AggregateId, Value)>,
    entities: EntityProjection,
    domain_identities: Vec<(DomainIdentityId, Value)>,
    value_objects: Vec<(ValueObjectId, Value)>,
    domain_services: Vec<Value>,
    domain_events: Vec<(DomainEventId, Value)>,
    domain_errors: Vec<(DomainErrorId, Value)>,
    actions: ActionProjection,
    decisions: DecisionProjection,
    queries: Vec<(QueryId, Value)>,
    field_references: FieldReferenceCollection,
}

impl DomainModelBuilder {
    pub const fn new() -> Self {
        Self {
            bounded_contexts: Vec::new(),
            aggregates: Vec::new(),
            entities: EntityProjection::new(),
            domain_identities: Vec::new(),
            value_objects: Vec::new(),
            domain_services: Vec::new(),
            domain_events: Vec::new(),
            domain_errors: Vec::new(),
            actions: ActionProjection::new(),
            decisions: DecisionProjection::new(),
            queries: Vec::new(),
            field_references: FieldReferenceCollection::new(),
        }
    }

    pub fn add_bounded_context(&mut self, descriptor: BoundedContextDescriptor) {
        self.bounded_contexts.push(json!({
            "id": descriptor.id.0,
            "label": descriptor.label,
        }));
    }

    pub fn add_aggregate(&mut self, descriptor: AggregateDescriptor) {
        self.aggregates.push((
            descriptor.id,
            json!({
                "id": aggregate_id(descriptor.id),
                "label": descriptor.label,
                "root": entity_id(descriptor.root),
            }),
        ));
    }

    pub fn add_aggregate_type<A: AggregateDefinition>(&mut self) -> Result<(), DomainModelError> {
        self.add_aggregate(A::DESCRIPTOR);
        let owner = ActionOwnerId::Aggregate(A::DESCRIPTOR.id);
        self.actions.register_owner(owner);
        for contract in A::ACTION_CONTRACTS {
            self.actions.add_group(owner, contract)?;
        }
        let owner = DecisionOwnerId::Aggregate(A::DESCRIPTOR.id);
        self.decisions.register_owner(owner);
        for group in A::DECISION_GROUPS {
            self.decisions.add_group(owner, group)?;
        }
        for event in <A::Event as AggregateEventSet<A>>::DOMAIN_EVENTS {
            self.add_domain_event(*event)?;
        }
        Ok(())
    }

    pub fn add_entity(&mut self, descriptor: EntityDescriptor) -> Result<(), DomainModelError> {
        self.add_domain_identity(descriptor.identity.identity)?;
        self.entities.add(descriptor);
        self.field_references.add_entity(descriptor);
        Ok(())
    }

    pub fn add_entity_type<E: EntityDefinition>(&mut self) -> Result<(), DomainModelError> {
        self.add_entity(E::DESCRIPTOR)?;
        self.actions
            .register_owner(ActionOwnerId::Entity(E::DESCRIPTOR.id));
        Ok(())
    }

    fn add_domain_identity(&mut self, id: DomainIdentityId) -> Result<(), DomainModelError> {
        if self
            .domain_identities
            .iter()
            .any(|(registered, _)| *registered == id)
        {
            return Err(DomainModelError::DuplicateDomainIdentityId { id: Box::new(id) });
        }
        self.domain_identities.push((
            id,
            json!({
                "id": domain_identity_id(id),
            }),
        ));
        Ok(())
    }

    pub fn add_value_object(&mut self, descriptor: ValueObjectDescriptor) {
        self.add_value_object_descriptor(descriptor);
    }

    fn add_value_object_descriptor(&mut self, descriptor: ValueObjectDescriptor) {
        let value = json!({
            "id": value_object_id(descriptor.id),
            "label": descriptor.label,
        });
        self.value_objects.push((descriptor.id, value));
    }

    pub fn add_value_object_type<V: ValueObject>(&mut self) -> Result<(), DomainModelError> {
        self.add_value_object(V::DESCRIPTOR);
        Ok(())
    }

    pub fn add_domain_service(&mut self, descriptor: DomainServiceDescriptor) {
        self.domain_services.push(json!({
            "id": {
                "context": descriptor.id.context.0,
                "local": descriptor.id.local,
            },
            "label": descriptor.label,
        }));
    }

    pub fn add_domain_service_type<S: DomainServiceType>(
        &mut self,
    ) -> Result<(), DomainModelError> {
        self.add_domain_service(S::DESCRIPTOR);
        let owner = ActionOwnerId::DomainService(S::DESCRIPTOR.id);
        self.actions.register_owner(owner);
        Ok(())
    }

    pub fn add_domain_event(
        &mut self,
        descriptor: DomainEventDescriptor,
    ) -> Result<(), DomainModelError> {
        if self
            .domain_events
            .iter()
            .any(|(id, _)| *id == descriptor.id)
        {
            return Err(DomainModelError::DuplicateDomainEventId {
                id: Box::new(descriptor.id),
            });
        }
        self.domain_events.push((
            descriptor.id,
            json!({
                "id": {
                    "aggregate": aggregate_id(descriptor.id.aggregate),
                    "local": descriptor.id.local,
                },
                "label": descriptor.label,
                "schemaVersion": descriptor.schema_version,
                "fields": field_projection::fields(descriptor.fields),
            }),
        ));
        self.field_references.add_domain_event(descriptor);
        Ok(())
    }

    pub fn add_domain_error(
        &mut self,
        descriptor: DomainErrorDescriptor,
    ) -> Result<(), DomainModelError> {
        if self
            .domain_errors
            .iter()
            .any(|(id, _)| *id == descriptor.id)
        {
            return Err(DomainModelError::DuplicateDomainErrorId {
                id: Box::new(descriptor.id),
            });
        }
        self.domain_errors.push((
            descriptor.id,
            json!({
                "id": domain_error_id(descriptor.id),
                "label": descriptor.label,
                "code": descriptor.code,
                "message": descriptor.message,
                "fields": field_projection::fields(descriptor.fields),
            }),
        ));
        self.field_references.add_domain_error(descriptor);
        Ok(())
    }

    pub fn add_action_extension<G: ActionGroupType>(&mut self) -> Result<(), DomainModelError> {
        let owner = <G::Owner as crate::ActionOwnerType>::ACTION_OWNER_ID;
        self.actions.add_extension(owner, G::ACTIONS)
    }

    pub fn add_queries(
        &mut self,
        descriptors: &'static [QueryDescriptor],
    ) -> Result<(), DomainModelError> {
        for descriptor in descriptors {
            if self.queries.iter().any(|(id, _)| *id == descriptor.id) {
                return Err(DomainModelError::DuplicateQueryId {
                    id: Box::new(descriptor.id),
                });
            }
            self.queries.push((
                descriptor.id,
                json!({
                    "id": query_id(descriptor.id),
                    "label": descriptor.label,
                }),
            ));
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Value, DomainModelError> {
        let inventory =
            ActionErrorInventory::new(self.domain_errors.iter().map(|(id, _)| *id).collect());
        self.actions.validate_errors(&inventory)?;
        let field_inventory = FieldReferenceInventory::new(
            self.domain_identities.iter().map(|(id, _)| *id).collect(),
            self.entities.ids().collect(),
            self.value_objects.iter().map(|(id, _)| *id).collect(),
            self.aggregates.iter().map(|(id, _)| *id).collect(),
        );
        field_reference_validation::validate(self.field_references.iter(), &field_inventory)?;

        Ok(json!({
            "boundedContexts": self.bounded_contexts,
            "aggregates": self.aggregates.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "entities": self.entities.into_values(),
            "domainIdentities": self.domain_identities.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "valueObjects": self.value_objects.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "domainServices": self.domain_services,
            "domainEvents": self.domain_events.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "domainErrors": self.domain_errors.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "actions": self.actions.into_values(),
            "decisions": self.decisions.into_values(),
            "queries": self.queries.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "invariants": [],
        }))
    }
}

impl Default for DomainModelBuilder {
    fn default() -> Self {
        Self::new()
    }
}
