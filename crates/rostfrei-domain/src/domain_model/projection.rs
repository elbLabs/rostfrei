use serde_json::{Value, json};

use crate::{
    ActionOwnerId, AggregateDescriptor, AggregateId, AggregateType, BoundedContextDescriptor,
    DecisionOwnerId, DomainCommandDescriptor, DomainCommandId, DomainErrorDescriptor,
    DomainErrorId, DomainEventDescriptor, DomainEventId, DomainIdentityDescriptor,
    DomainIdentityId, DomainIdentityType, DomainServiceDescriptor, DomainServiceType,
    EntityDescriptor, EntityType, InvariantOwnerId, QueryDescriptor, QueryId, QueryInputDescriptor,
    QueryOutputDescriptor, ValueObjectDescriptor, ValueObjectId, ValueObjectType,
    extension::ActionGroupType,
};

use super::{
    action_projection::ActionProjection,
    action_reference_validation::ActionReferenceInventory,
    decision_projection::DecisionProjection,
    decision_reference_validation::DecisionReferenceInventory,
    entity_projection::EntityProjection,
    field_projection,
    field_reference_collection::FieldReferenceCollection,
    field_reference_validation::{self, FieldReferenceInventory},
    id_projection::{
        aggregate as aggregate_id, domain_command as domain_command_id,
        domain_error_owner as domain_error_owner_id, domain_identity as domain_identity_id,
        entity as entity_id, query as query_id, value_object as value_object_id,
    },
    invariant_projection::InvariantProjection,
    lifecycle_action_validation::LifecycleActionInventory,
    value_object_projection,
};

pub struct DomainModelBuilder {
    bounded_contexts: Vec<Value>,
    aggregates: Vec<(AggregateId, Value)>,
    entities: EntityProjection,
    domain_identities: Vec<(DomainIdentityId, Value)>,
    value_objects: Vec<(ValueObjectId, Value)>,
    domain_services: Vec<Value>,
    domain_commands: Vec<(DomainCommandId, Value)>,
    domain_events: Vec<(DomainEventId, Value)>,
    domain_errors: Vec<(DomainErrorId, Value)>,
    actions: ActionProjection,
    decisions: DecisionProjection,
    queries: Vec<(QueryId, Value)>,
    invariants: InvariantProjection,
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
            domain_commands: Vec::new(),
            domain_events: Vec::new(),
            domain_errors: Vec::new(),
            actions: ActionProjection::new(),
            decisions: DecisionProjection::new(),
            queries: Vec::new(),
            invariants: InvariantProjection::new(),
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

    pub fn add_aggregate_type<A: AggregateType>(&mut self) {
        self.add_aggregate(A::DESCRIPTOR);
        let owner = ActionOwnerId::Aggregate(A::DESCRIPTOR.id);
        self.actions.register_owner(owner);
        for contract in A::ACTION_CONTRACTS {
            self.actions.add_group(owner, contract);
        }
        let owner = DecisionOwnerId::Aggregate(A::DESCRIPTOR.id);
        self.decisions.register_owner(owner);
        for contract in A::DECISION_CONTRACTS {
            self.decisions.add_group(owner, contract);
        }
        let owner = InvariantOwnerId::Aggregate(A::DESCRIPTOR.id);
        self.invariants.register_owner(owner);
        for contract in A::INVARIANT_CONTRACTS {
            self.invariants.add_group(owner, contract);
        }
        for event in A::DOMAIN_EVENTS {
            self.add_domain_event(*event);
        }
    }

    pub fn add_entity(&mut self, descriptor: EntityDescriptor) {
        self.entities.add(descriptor);
        self.field_references.add_entity(descriptor);
    }

    pub fn add_entity_type<E: EntityType>(&mut self) {
        self.entities
            .add_with_lifecycle(E::DESCRIPTOR, E::LIFECYCLE);
        self.field_references.add_entity(E::DESCRIPTOR);
        let owner = ActionOwnerId::Entity(E::DESCRIPTOR.id);
        self.actions.register_owner(owner);
        for contract in E::ACTION_CONTRACTS {
            self.actions.add_group(owner, contract);
        }
        let owner = DecisionOwnerId::Entity(E::DESCRIPTOR.id);
        self.decisions.register_owner(owner);
        for contract in E::DECISION_CONTRACTS {
            self.decisions.add_group(owner, contract);
        }
        let owner = InvariantOwnerId::Entity(E::DESCRIPTOR.id);
        self.invariants.register_owner(owner);
        for contract in E::INVARIANT_CONTRACTS {
            self.invariants.add_group(owner, contract);
        }
    }

    pub fn add_domain_identity(&mut self, descriptor: DomainIdentityDescriptor) {
        self.add_domain_identity_descriptor(descriptor, None);
    }

    pub fn add_domain_identity_type<I: DomainIdentityType>(&mut self) {
        if let Some(semantic_scalar) = I::SEMANTIC_SCALAR {
            assert_eq!(
                I::DESCRIPTOR.scalar,
                semantic_scalar.representation,
                "DomainIdentity semantic scalar representation must match its canonical scalar descriptor"
            );
        }
        self.add_domain_identity_descriptor(I::DESCRIPTOR, I::SEMANTIC_SCALAR);
    }

    fn add_domain_identity_descriptor(
        &mut self,
        descriptor: DomainIdentityDescriptor,
        semantic_scalar: Option<crate::SemanticScalarDescriptor>,
    ) {
        if self
            .domain_identities
            .iter()
            .any(|(id, _)| *id == descriptor.id)
        {
            panic!("duplicate DomainIdentityId: {:?}", descriptor.id);
        }
        let scalar = match semantic_scalar {
            Some(descriptor) => field_projection::semantic_scalar_value(descriptor),
            None => scalar_value(descriptor.scalar),
        };
        self.domain_identities.push((
            descriptor.id,
            json!({
                "id": domain_identity_id(descriptor.id),
                "scalar": scalar,
            }),
        ));
    }

    pub fn add_value_object(&mut self, descriptor: ValueObjectDescriptor) {
        self.add_value_object_descriptor(descriptor);
        self.field_references.add_value_object(descriptor);
    }

    fn add_value_object_descriptor(&mut self, descriptor: ValueObjectDescriptor) {
        let mut value = json!({
            "id": value_object_id(descriptor.id),
            "label": descriptor.label,
        });
        value_object_projection::apply_shape(&mut value, descriptor.shape);
        self.value_objects.push((descriptor.id, value));
    }

    pub fn add_value_object_type<V: ValueObjectType>(&mut self) {
        self.add_value_object(V::DESCRIPTOR);
        let owner = ActionOwnerId::ValueObject(V::DESCRIPTOR.id);
        self.actions.register_owner(owner);
        for contract in V::ACTION_CONTRACTS {
            self.actions.add_group(owner, contract);
        }
        let owner = DecisionOwnerId::ValueObject(V::DESCRIPTOR.id);
        self.decisions.register_owner(owner);
        for contract in V::DECISION_CONTRACTS {
            self.decisions.add_group(owner, contract);
        }
        let owner = InvariantOwnerId::ValueObject(V::DESCRIPTOR.id);
        self.invariants.register_owner(owner);
        for contract in V::INVARIANT_CONTRACTS {
            self.invariants.add_group(owner, contract);
        }
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

    pub fn add_domain_service_type<S: DomainServiceType>(&mut self) {
        self.add_domain_service(S::DESCRIPTOR);
        let owner = ActionOwnerId::DomainService(S::DESCRIPTOR.id);
        self.actions.register_owner(owner);
        for contract in S::ACTION_CONTRACTS {
            self.actions.add_group(owner, contract);
        }
        let owner = DecisionOwnerId::DomainService(S::DESCRIPTOR.id);
        self.decisions.register_owner(owner);
        for contract in S::DECISION_CONTRACTS {
            self.decisions.add_group(owner, contract);
        }
    }

    pub fn add_domain_event(&mut self, descriptor: DomainEventDescriptor) {
        if self
            .domain_events
            .iter()
            .any(|(id, _)| *id == descriptor.id)
        {
            panic!("duplicate DomainEventId: {:?}", descriptor.id);
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
    }

    pub fn add_domain_command(&mut self, descriptor: DomainCommandDescriptor) {
        if self
            .domain_commands
            .iter()
            .any(|(id, _)| *id == descriptor.id)
        {
            panic!("duplicate DomainCommandId: {:?}", descriptor.id);
        }
        self.domain_commands.push((
            descriptor.id,
            json!({
                "id": domain_command_id(descriptor.id),
                "label": descriptor.label,
                "fields": field_projection::fields(descriptor.fields),
            }),
        ));
        self.field_references.add_domain_command(descriptor);
    }

    pub fn add_domain_error(&mut self, descriptor: DomainErrorDescriptor) {
        self.domain_errors.push((
            descriptor.id,
            json!({
                "id": {
                    "owner": domain_error_owner_id(descriptor.id.owner),
                    "local": descriptor.id.local,
                },
                "label": descriptor.label,
                "code": descriptor.code,
                "message": descriptor.message,
                "fields": field_projection::fields(descriptor.fields),
            }),
        ));
        self.field_references.add_domain_error(descriptor);
    }

    pub fn add_action_extension<G: ActionGroupType>(&mut self) {
        let owner = <G::Owner as crate::ActionOwnerType>::ACTION_OWNER_ID;
        self.actions.add_extension(owner, G::ACTIONS);
    }

    pub fn add_queries(&mut self, descriptors: &'static [QueryDescriptor]) {
        for descriptor in descriptors {
            if self.queries.iter().any(|(id, _)| *id == descriptor.id) {
                panic!("duplicate QueryId: {:?}", descriptor.id);
            }
            self.queries.push((
                descriptor.id,
                json!({
                    "id": query_id(descriptor.id),
                    "label": descriptor.label,
                    "input": descriptor.input.map(query_input),
                    "output": query_output(descriptor.output),
                }),
            ));
        }
    }

    pub fn finish(self) -> Value {
        let inventory = ActionReferenceInventory::new(
            self.domain_identities.iter().map(|(id, _)| *id).collect(),
            self.domain_events.iter().map(|(id, _)| *id).collect(),
            self.domain_errors.iter().map(|(id, _)| *id).collect(),
            self.value_objects.iter().map(|(id, _)| *id).collect(),
        );
        self.actions.validate_references(&inventory);
        let lifecycle_action_inventory = LifecycleActionInventory::new(
            self.actions.attached_ids().collect(),
            self.actions.extension_ids().collect(),
        );
        self.entities
            .validate_lifecycle_actions(&lifecycle_action_inventory);
        let decision_inventory =
            DecisionReferenceInventory::new(self.value_objects.iter().map(|(id, _)| *id).collect());
        self.decisions.validate_references(&decision_inventory);
        let field_inventory = FieldReferenceInventory::new(
            self.domain_identities.iter().map(|(id, _)| *id).collect(),
            self.entities.ids().collect(),
            self.value_objects.iter().map(|(id, _)| *id).collect(),
            self.aggregates.iter().map(|(id, _)| *id).collect(),
        );
        field_reference_validation::validate(self.field_references.iter(), &field_inventory);

        json!({
            "boundedContexts": self.bounded_contexts,
            "aggregates": self.aggregates.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "entities": self.entities.into_values(),
            "domainIdentities": self.domain_identities.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "valueObjects": self.value_objects.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "domainServices": self.domain_services,
            "domainCommands": self.domain_commands.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "domainEvents": self.domain_events.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "domainErrors": self.domain_errors.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "actions": self.actions.into_values(),
            "decisions": self.decisions.into_values(),
            "queries": self.queries.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            "invariants": self.invariants.into_values(),
        })
    }
}

fn query_input(descriptor: QueryInputDescriptor) -> Value {
    match descriptor {
        QueryInputDescriptor::Scalar(scalar) => field_projection::scalar(scalar),
        QueryInputDescriptor::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": value_object_id(id) })
        }
        QueryInputDescriptor::DomainIdentity(id) => {
            json!({ "kind": "domainIdentity", "id": domain_identity_id(id) })
        }
    }
}

fn query_output(descriptor: QueryOutputDescriptor) -> Value {
    match descriptor {
        QueryOutputDescriptor::Scalar(scalar) => field_projection::scalar(scalar),
        QueryOutputDescriptor::ValueObject(id) => {
            json!({ "kind": "valueObject", "id": value_object_id(id) })
        }
        QueryOutputDescriptor::DomainIdentity(id) => {
            json!({ "kind": "domainIdentity", "id": domain_identity_id(id) })
        }
        QueryOutputDescriptor::Optional(value) => {
            json!({ "kind": "optional", "value": query_output(*value) })
        }
        QueryOutputDescriptor::List(element) => {
            json!({ "kind": "list", "element": query_output(*element) })
        }
    }
}

fn scalar_value(scalar: crate::ScalarType) -> Value {
    Value::String(field_projection::scalar_name(scalar).to_owned())
}

impl Default for DomainModelBuilder {
    fn default() -> Self {
        Self::new()
    }
}
