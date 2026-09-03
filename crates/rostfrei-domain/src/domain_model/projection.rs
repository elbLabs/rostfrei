use serde_json::{Value, json};

use crate::{
    AggregateDefinition, AggregateDescriptor, AggregateEventSet, AggregateId,
    BoundedContextDescriptor, BoundedContextId, DomainErrorDescriptor, DomainErrorId,
    DomainEventDescriptor, DomainEventId, DomainIdentityId, DomainServiceDescriptor,
    DomainServiceId, DomainServiceType, EntityDefinition, EntityDescriptor, ValueObject,
    ValueObjectDescriptor, ValueObjectId,
};

use super::{
    entity_projection::EntityProjection,
    error::DomainModelError,
    field_projection,
    field_reference_collection::FieldReferenceCollection,
    field_reference_validation::{self, FieldReferenceInventory},
    id_projection::{
        aggregate as aggregate_id, domain_error as domain_error_id,
        domain_identity as domain_identity_id, entity as entity_id,
        value_object as value_object_id,
    },
};

pub struct DomainModelBuilder {
    bounded_contexts: Vec<(BoundedContextId, Value)>,
    aggregates: Vec<(AggregateId, Value)>,
    entities: EntityProjection,
    domain_identities: Vec<(DomainIdentityId, Value)>,
    value_objects: Vec<(ValueObjectId, Value)>,
    domain_services: Vec<(DomainServiceId, Value)>,
    domain_events: Vec<(DomainEventId, Value)>,
    domain_errors: Vec<(DomainErrorId, Value)>,
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
            field_references: FieldReferenceCollection::new(),
        }
    }

    pub fn add_bounded_context(
        &mut self,
        descriptor: BoundedContextDescriptor,
    ) -> Result<(), DomainModelError> {
        if contains_id(&self.bounded_contexts, descriptor.id) {
            return Err(DomainModelError::DuplicateBoundedContextId {
                id: Box::new(descriptor.id),
            });
        }
        self.bounded_contexts.push((
            descriptor.id,
            json!({
                "id": descriptor.id.0,
                "label": descriptor.label,
            }),
        ));
        Ok(())
    }

    pub fn add_aggregate(
        &mut self,
        descriptor: AggregateDescriptor,
    ) -> Result<(), DomainModelError> {
        self.validate_aggregate_id(descriptor.id)?;
        self.insert_aggregate(descriptor);
        Ok(())
    }

    fn validate_aggregate_id(&self, id: AggregateId) -> Result<(), DomainModelError> {
        if contains_id(&self.aggregates, id) {
            return Err(DomainModelError::DuplicateAggregateId { id: Box::new(id) });
        }
        Ok(())
    }

    fn insert_aggregate(&mut self, descriptor: AggregateDescriptor) {
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
        let descriptor = A::DESCRIPTOR;
        let events = <A::Event as AggregateEventSet<A>>::DOMAIN_EVENTS;
        self.validate_aggregate_id(descriptor.id)?;
        self.validate_domain_event_batch(events)?;

        self.insert_aggregate(descriptor);
        for event in events {
            self.insert_domain_event(*event);
        }
        Ok(())
    }

    pub fn add_entity(&mut self, descriptor: EntityDescriptor) -> Result<(), DomainModelError> {
        if self.entities.contains(descriptor.id) {
            return Err(DomainModelError::DuplicateEntityId {
                id: Box::new(descriptor.id),
            });
        }
        self.validate_domain_identity_id(descriptor.identity)?;

        self.insert_domain_identity(descriptor.identity);
        self.entities.add(descriptor);
        self.field_references.add_entity(descriptor);
        Ok(())
    }

    pub fn add_entity_type<E: EntityDefinition>(&mut self) -> Result<(), DomainModelError> {
        self.add_entity(E::DESCRIPTOR)
    }

    fn validate_domain_identity_id(&self, id: DomainIdentityId) -> Result<(), DomainModelError> {
        if contains_id(&self.domain_identities, id) {
            return Err(DomainModelError::DuplicateDomainIdentityId { id: Box::new(id) });
        }
        Ok(())
    }

    fn insert_domain_identity(&mut self, id: DomainIdentityId) {
        self.domain_identities.push((
            id,
            json!({
                "id": domain_identity_id(id),
            }),
        ));
    }

    pub fn add_value_object(
        &mut self,
        descriptor: ValueObjectDescriptor,
    ) -> Result<(), DomainModelError> {
        if contains_id(&self.value_objects, descriptor.id) {
            return Err(DomainModelError::DuplicateValueObjectId {
                id: Box::new(descriptor.id),
            });
        }
        let value = json!({
            "id": value_object_id(descriptor.id),
            "label": descriptor.label,
        });
        self.value_objects.push((descriptor.id, value));
        Ok(())
    }

    pub fn add_value_object_type<V: ValueObject>(&mut self) -> Result<(), DomainModelError> {
        self.add_value_object(V::DESCRIPTOR)
    }

    pub fn add_domain_service(
        &mut self,
        descriptor: DomainServiceDescriptor,
    ) -> Result<(), DomainModelError> {
        if contains_id(&self.domain_services, descriptor.id) {
            return Err(DomainModelError::DuplicateDomainServiceId {
                id: Box::new(descriptor.id),
            });
        }
        self.domain_services.push((
            descriptor.id,
            json!({
                "id": {
                    "context": descriptor.id.context.0,
                    "local": descriptor.id.local,
                },
                "label": descriptor.label,
            }),
        ));
        Ok(())
    }

    pub fn add_domain_service_type<S: DomainServiceType>(
        &mut self,
    ) -> Result<(), DomainModelError> {
        self.add_domain_service(S::DESCRIPTOR)
    }

    pub fn add_domain_event(
        &mut self,
        descriptor: DomainEventDescriptor,
    ) -> Result<(), DomainModelError> {
        self.validate_domain_event_id(descriptor.id)?;
        self.insert_domain_event(descriptor);
        Ok(())
    }

    fn validate_domain_event_id(&self, id: DomainEventId) -> Result<(), DomainModelError> {
        if contains_id(&self.domain_events, id) {
            return Err(DomainModelError::DuplicateDomainEventId { id: Box::new(id) });
        }
        Ok(())
    }

    fn validate_domain_event_batch(
        &self,
        descriptors: &[DomainEventDescriptor],
    ) -> Result<(), DomainModelError> {
        for (index, descriptor) in descriptors.iter().enumerate() {
            self.validate_domain_event_id(descriptor.id)?;
            if descriptors
                .iter()
                .take(index)
                .any(|registered| registered.id == descriptor.id)
            {
                return Err(DomainModelError::DuplicateDomainEventId {
                    id: Box::new(descriptor.id),
                });
            }
        }
        Ok(())
    }

    fn insert_domain_event(&mut self, descriptor: DomainEventDescriptor) {
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

    pub fn finish(self) -> Result<Value, DomainModelError> {
        let field_inventory = FieldReferenceInventory::new(
            self.entities.ids().collect(),
            self.aggregates.iter().map(|(id, _)| *id).collect(),
        );
        field_reference_validation::validate(self.field_references.iter(), &field_inventory)?;

        Ok(json!({
            "boundedContexts": into_values(self.bounded_contexts),
            "aggregates": self.aggregates.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "entities": self.entities.into_values(),
            "domainIdentities": self.domain_identities.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "valueObjects": self.value_objects.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "domainServices": into_values(self.domain_services),
            "domainEvents": self.domain_events.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "domainErrors": self.domain_errors.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "actions": [],
            "decisions": [],
            "queries": [],
            "invariants": [],
        }))
    }
}

fn contains_id<I: Copy + Eq>(entries: &[(I, Value)], id: I) -> bool {
    entries.iter().any(|(registered, _)| *registered == id)
}

fn into_values<I>(entries: Vec<(I, Value)>) -> Vec<Value> {
    entries.into_iter().map(|(_, value)| value).collect()
}

impl Default for DomainModelBuilder {
    fn default() -> Self {
        Self::new()
    }
}
