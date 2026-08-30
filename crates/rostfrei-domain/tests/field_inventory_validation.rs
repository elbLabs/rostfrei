use std::fmt::Debug;

use domain::__private::DomainModelBuilder;
use domain::{
    ActionDescriptor, ActionId, ActionOwnerId, AggregateDescriptor, AggregateId, BoundedContext,
    BoundedContextId, CommandDescriptor, CommandId, CommandOwnerId, DomainErrorDescriptor,
    DomainErrorId, DomainErrorOwnerId, DomainEventDescriptor, DomainEventId,
    DomainIdentityDescriptor, DomainIdentityId, DomainModelError, DomainModelReference,
    EntityDescriptor, EntityId, FieldDescriptor, FieldKind, FieldValue, FieldWrapper,
    IdentityDescriptor, ScalarType, SemanticScalarDescriptor, ValueObjectDescriptor, ValueObjectId,
    ValueObjectOwnerId, ValueObjectShapeDescriptor, ValueObjectType, ValueObjectVariantDescriptor,
    ValueObjectVariantShapeDescriptor,
};

const CONTEXT_ID: BoundedContextId = BoundedContextId("field-inventory");
const AGGREGATE_ID: AggregateId = AggregateId {
    context: CONTEXT_ID,
    local: "registered-aggregate",
};
const MISSING_AGGREGATE_ID: AggregateId = AggregateId {
    context: CONTEXT_ID,
    local: "missing-aggregate",
};
const ENTITY_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "registered-entity",
};
const MISSING_ENTITY_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "missing-entity",
};
const ENTITY_SOURCE_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "entity-source",
};
const IDENTITY_ID: DomainIdentityId = DomainIdentityId { owner: ENTITY_ID };
const MISSING_IDENTITY_ID: DomainIdentityId = DomainIdentityId {
    owner: MISSING_ENTITY_ID,
};
const SOURCE_VALUE_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "source-value",
};
const CYCLE_VALUE_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "cycle-value",
};
const MISSING_VALUE_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "missing-value",
};
const CONTRACT_REJECTED_VALUE_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "contract-rejected-value",
};
const COMMAND_ID: CommandId = CommandId {
    owner: CommandOwnerId::Aggregate(AGGREGATE_ID),
    local: "inventory-command",
};
const EVENT_ID: DomainEventId = DomainEventId {
    aggregate: AGGREGATE_ID,
    local: "inventory-event",
};
const ERROR_ID: DomainErrorId = DomainErrorId {
    owner: DomainErrorOwnerId::Aggregate(AGGREGATE_ID),
    local: "inventory-error",
};
const SEMANTIC_SCALAR: SemanticScalarDescriptor = SemanticScalarDescriptor {
    id: "inventory-code",
    label: "Inventory code",
    representation: ScalarType::String,
};
const DUPLICATE_VALUE_ACTIONS: &[ActionDescriptor] = &[
    ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::ValueObject(CONTRACT_REJECTED_VALUE_ID),
            local: "duplicate",
        },
        label: "First duplicate",
        input: None,
        output: None,
        raises: &[],
        error: None,
    },
    ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::ValueObject(CONTRACT_REJECTED_VALUE_ID),
            local: "duplicate",
        },
        label: "Second duplicate",
        input: None,
        output: None,
        raises: &[],
        error: None,
    },
];

#[derive(BoundedContext)]
#[domain(id = "field-inventory", label = "Field inventory")]
struct FieldInventoryContext;

struct ContractRejectedValue;

impl ValueObjectType for ContractRejectedValue {
    type Owner = FieldInventoryContext;

    const LOCAL_ID: &'static str = "contract-rejected-value";
    const DESCRIPTOR: ValueObjectDescriptor = ValueObjectDescriptor {
        id: CONTRACT_REJECTED_VALUE_ID,
        label: "Contract rejected value",
        shape: ValueObjectShapeDescriptor::Struct {
            fields: &[FieldDescriptor {
                name: "aggregate",
                value: FieldValue {
                    kind: FieldKind::AggregateReference(MISSING_AGGREGATE_ID),
                    wrappers: &[],
                },
            }],
        },
    };
    const ACTION_CONTRACTS: &'static [&'static [ActionDescriptor]] = &[DUPLICATE_VALUE_ACTIONS];
}

const SOURCE_VALUE: ValueObjectDescriptor = ValueObjectDescriptor {
    id: SOURCE_VALUE_ID,
    label: "Source value",
    shape: ValueObjectShapeDescriptor::Struct {
        fields: &[
            FieldDescriptor {
                name: "identity",
                value: FieldValue {
                    kind: FieldKind::DomainIdentity(IDENTITY_ID),
                    wrappers: &[FieldWrapper::Optional, FieldWrapper::List],
                },
            },
            FieldDescriptor {
                name: "entity",
                value: FieldValue {
                    kind: FieldKind::Entity(ENTITY_ID),
                    wrappers: &[FieldWrapper::List],
                },
            },
            FieldDescriptor {
                name: "value",
                value: FieldValue {
                    kind: FieldKind::ValueObject(CYCLE_VALUE_ID),
                    wrappers: &[FieldWrapper::Optional],
                },
            },
            FieldDescriptor {
                name: "aggregate",
                value: FieldValue {
                    kind: FieldKind::AggregateReference(AGGREGATE_ID),
                    wrappers: &[],
                },
            },
            FieldDescriptor {
                name: "scalar",
                value: FieldValue {
                    kind: FieldKind::Scalar(ScalarType::U64),
                    wrappers: &[],
                },
            },
            FieldDescriptor {
                name: "semantic",
                value: FieldValue {
                    kind: FieldKind::SemanticScalar(SEMANTIC_SCALAR),
                    wrappers: &[],
                },
            },
        ],
    },
};
const CYCLE_VALUE: ValueObjectDescriptor = ValueObjectDescriptor {
    id: CYCLE_VALUE_ID,
    label: "Cycle value",
    shape: ValueObjectShapeDescriptor::Struct {
        fields: &[FieldDescriptor {
            name: "source",
            value: FieldValue {
                kind: FieldKind::ValueObject(SOURCE_VALUE_ID),
                wrappers: &[],
            },
        }],
    },
};
const REGISTERED_ENTITY: EntityDescriptor = EntityDescriptor {
    id: ENTITY_ID,
    label: "Registered entity",
    identity: IdentityDescriptor {
        field: "id",
        identity: IDENTITY_ID,
    },
    fields: &[FieldDescriptor {
        name: "id",
        value: FieldValue {
            kind: FieldKind::DomainIdentity(IDENTITY_ID),
            wrappers: &[],
        },
    }],
};
const REGISTERED_IDENTITY: DomainIdentityDescriptor = DomainIdentityDescriptor {
    id: IDENTITY_ID,
    scalar: ScalarType::U64,
};
const REGISTERED_AGGREGATE: AggregateDescriptor = AggregateDescriptor {
    id: AGGREGATE_ID,
    label: "Registered aggregate",
    root: ENTITY_ID,
};

const STRUCT_IDENTITY_REFERENCE: ValueObjectDescriptor = ValueObjectDescriptor {
    id: ValueObjectId {
        owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
        local: "struct-identity-source",
    },
    label: "Struct identity source",
    shape: ValueObjectShapeDescriptor::Struct {
        fields: &[FieldDescriptor {
            name: "identity",
            value: FieldValue {
                kind: FieldKind::DomainIdentity(MISSING_IDENTITY_ID),
                wrappers: &[FieldWrapper::List, FieldWrapper::Optional],
            },
        }],
    },
};
const STRUCT_ENTITY_REFERENCE: ValueObjectDescriptor = ValueObjectDescriptor {
    id: ValueObjectId {
        owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
        local: "struct-entity-source",
    },
    label: "Struct entity source",
    shape: ValueObjectShapeDescriptor::Struct {
        fields: &[FieldDescriptor {
            name: "entity",
            value: FieldValue {
                kind: FieldKind::Entity(MISSING_ENTITY_ID),
                wrappers: &[FieldWrapper::List, FieldWrapper::Optional],
            },
        }],
    },
};
const STRUCT_VALUE_REFERENCE: ValueObjectDescriptor = ValueObjectDescriptor {
    id: ValueObjectId {
        owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
        local: "struct-value-source",
    },
    label: "Struct value source",
    shape: ValueObjectShapeDescriptor::Struct {
        fields: &[FieldDescriptor {
            name: "value",
            value: FieldValue {
                kind: FieldKind::ValueObject(MISSING_VALUE_ID),
                wrappers: &[FieldWrapper::List, FieldWrapper::Optional],
            },
        }],
    },
};
const STRUCT_AGGREGATE_REFERENCE: ValueObjectDescriptor = ValueObjectDescriptor {
    id: ValueObjectId {
        owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
        local: "struct-aggregate-source",
    },
    label: "Struct aggregate source",
    shape: ValueObjectShapeDescriptor::Struct {
        fields: &[FieldDescriptor {
            name: "aggregate",
            value: FieldValue {
                kind: FieldKind::AggregateReference(MISSING_AGGREGATE_ID),
                wrappers: &[FieldWrapper::List, FieldWrapper::Optional],
            },
        }],
    },
};
const TAGGED_TUPLE_REFERENCE: ValueObjectDescriptor = ValueObjectDescriptor {
    id: SOURCE_VALUE_ID,
    label: "Tagged tuple source",
    shape: ValueObjectShapeDescriptor::TaggedEnum {
        variants: &[ValueObjectVariantDescriptor {
            name: "Tuple",
            shape: ValueObjectVariantShapeDescriptor::Tuple {
                fields: &[FieldDescriptor {
                    name: "0",
                    value: FieldValue {
                        kind: FieldKind::ValueObject(MISSING_VALUE_ID),
                        wrappers: &[FieldWrapper::Optional],
                    },
                }],
            },
        }],
    },
};
const TAGGED_STRUCT_REFERENCE: ValueObjectDescriptor = ValueObjectDescriptor {
    id: SOURCE_VALUE_ID,
    label: "Tagged struct source",
    shape: ValueObjectShapeDescriptor::TaggedEnum {
        variants: &[ValueObjectVariantDescriptor {
            name: "Struct",
            shape: ValueObjectVariantShapeDescriptor::Struct {
                fields: &[FieldDescriptor {
                    name: "identity",
                    value: FieldValue {
                        kind: FieldKind::DomainIdentity(MISSING_IDENTITY_ID),
                        wrappers: &[],
                    },
                }],
            },
        }],
    },
};
const ENTITY_REFERENCE: EntityDescriptor = EntityDescriptor {
    id: ENTITY_SOURCE_ID,
    label: "Entity source",
    identity: IdentityDescriptor {
        field: "unused",
        identity: IDENTITY_ID,
    },
    fields: &[FieldDescriptor {
        name: "related",
        value: FieldValue {
            kind: FieldKind::Entity(MISSING_ENTITY_ID),
            wrappers: &[],
        },
    }],
};
const ACCEPTED_COMMAND: CommandDescriptor = CommandDescriptor {
    id: COMMAND_ID,
    label: "Accepted command",
    fields: &[],
};
const COMMAND_REFERENCE: CommandDescriptor = CommandDescriptor {
    id: COMMAND_ID,
    label: "Command source",
    fields: &[FieldDescriptor {
        name: "aggregate",
        value: FieldValue {
            kind: FieldKind::AggregateReference(MISSING_AGGREGATE_ID),
            wrappers: &[],
        },
    }],
};
const EVENT_REFERENCE: DomainEventDescriptor = DomainEventDescriptor {
    id: EVENT_ID,
    label: "Event source",
    schema_version: 1,
    fields: &[FieldDescriptor {
        name: "value",
        value: FieldValue {
            kind: FieldKind::ValueObject(MISSING_VALUE_ID),
            wrappers: &[],
        },
    }],
};
const ERROR_REFERENCE: DomainErrorDescriptor = DomainErrorDescriptor {
    id: ERROR_ID,
    label: "Error source",
    code: "INVENTORY_ERROR",
    message: "Inventory error.",
    fields: &[FieldDescriptor {
        name: "identity",
        value: FieldValue {
            kind: FieldKind::DomainIdentity(MISSING_IDENTITY_ID),
            wrappers: &[],
        },
    }],
};

fn violation(missing_id: impl Debug, location: &str, inventory_key: &str) -> String {
    format!(
        "Field reference inventory violation: field references missing {missing_id:?} at descriptor location `{location}`; add it to domain_model! inventory key `{inventory_key}`"
    )
}

fn value_object_location(descriptor: ValueObjectDescriptor, field: &str) -> String {
    format!("value object {:?} field {field:?}", descriptor.id)
}

#[test]
fn accepts_forward_references_registered_cycles_wrappers_and_non_reference_scalars() {
    let mut builder = DomainModelBuilder::new();
    builder.add_value_object(SOURCE_VALUE);
    builder.add_value_object(CYCLE_VALUE);
    builder.add_entity(REGISTERED_ENTITY);
    builder.add_domain_identity(REGISTERED_IDENTITY).unwrap();
    builder.add_aggregate(REGISTERED_AGGREGATE);

    let model = builder.finish().unwrap();

    assert_eq!(model["valueObjects"][0]["id"]["local"], "source-value");
    assert_eq!(model["valueObjects"][1]["id"]["local"], "cycle-value");
}

#[test]
fn rejected_duplicate_command_does_not_leave_field_reference_records() {
    let mut builder = DomainModelBuilder::new();
    builder.add_command(ACCEPTED_COMMAND).unwrap();

    let error = builder.add_command(COMMAND_REFERENCE).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("duplicate CommandId: {COMMAND_ID:?}")
    );
    assert_eq!(
        error,
        DomainModelError::DuplicateCommandId {
            id: Box::new(COMMAND_ID),
        }
    );

    let model = builder.finish().unwrap();
    assert_eq!(model["commands"].as_array().unwrap().len(), 1);
    assert_eq!(model["commands"][0]["fields"].as_array().unwrap().len(), 0);
}

#[test]
fn persisted_value_object_keeps_field_references_after_later_contract_rejection() {
    let mut builder = DomainModelBuilder::new();

    let registration_error = builder
        .add_value_object_type::<ContractRejectedValue>()
        .unwrap_err();
    assert!(
        registration_error
            .to_string()
            .starts_with("duplicate ActionId:")
    );
    assert_eq!(
        registration_error,
        DomainModelError::DuplicateActionId {
            id: Box::new(DUPLICATE_VALUE_ACTIONS.first().unwrap().id),
        }
    );

    let finish_error = builder.finish().unwrap_err();
    let location = format!(
        "value object {CONTRACT_REJECTED_VALUE_ID:?} field {:?}",
        "aggregate"
    );
    assert_eq!(
        finish_error.to_string(),
        violation(MISSING_AGGREGATE_ID, &location, "aggregates")
    );
    assert_eq!(
        finish_error,
        DomainModelError::FieldReferenceInventoryViolation {
            reference: DomainModelReference::Aggregate(Box::new(MISSING_AGGREGATE_ID)),
            location,
            inventory_key: "aggregates",
        }
    );
}

#[test]
fn reports_each_missing_reference_kind_from_struct_value_objects() {
    let cases = [
        (
            STRUCT_IDENTITY_REFERENCE,
            format!("{MISSING_IDENTITY_ID:?}"),
            DomainModelReference::DomainIdentity(Box::new(MISSING_IDENTITY_ID)),
            "identities",
        ),
        (
            STRUCT_ENTITY_REFERENCE,
            format!("{MISSING_ENTITY_ID:?}"),
            DomainModelReference::Entity(Box::new(MISSING_ENTITY_ID)),
            "entities",
        ),
        (
            STRUCT_VALUE_REFERENCE,
            format!("{MISSING_VALUE_ID:?}"),
            DomainModelReference::ValueObject(Box::new(MISSING_VALUE_ID)),
            "value_objects",
        ),
        (
            STRUCT_AGGREGATE_REFERENCE,
            format!("{MISSING_AGGREGATE_ID:?}"),
            DomainModelReference::Aggregate(Box::new(MISSING_AGGREGATE_ID)),
            "aggregates",
        ),
    ];

    for (descriptor, missing_id, reference, inventory_key) in cases {
        let mut builder = DomainModelBuilder::new();
        builder.add_value_object(descriptor);
        let error = builder.finish().unwrap_err();
        let field = descriptor.shape_fields().unwrap().first().unwrap();
        let location = value_object_location(descriptor, field.name);

        assert_eq!(
            error.to_string(),
            format!(
                "Field reference inventory violation: field references missing {missing_id} at descriptor location `{location}`; add it to domain_model! inventory key `{inventory_key}`"
            )
        );
        assert_eq!(
            error,
            DomainModelError::FieldReferenceInventoryViolation {
                reference,
                location,
                inventory_key,
            }
        );
    }
}

#[test]
fn reports_a_missing_reference_from_a_tagged_tuple_variant() {
    let mut builder = DomainModelBuilder::new();
    builder.add_value_object(TAGGED_TUPLE_REFERENCE);
    let error = builder.finish().unwrap_err();
    let location = format!(
        "value object {SOURCE_VALUE_ID:?} variant {:?} field {:?}",
        "Tuple", "0"
    );

    assert_eq!(
        error.to_string(),
        violation(MISSING_VALUE_ID, &location, "value_objects")
    );
    assert_eq!(
        error,
        DomainModelError::FieldReferenceInventoryViolation {
            reference: DomainModelReference::ValueObject(Box::new(MISSING_VALUE_ID)),
            location,
            inventory_key: "value_objects",
        }
    );
}

#[test]
fn reports_a_missing_reference_from_a_tagged_struct_variant() {
    let mut builder = DomainModelBuilder::new();
    builder.add_value_object(TAGGED_STRUCT_REFERENCE);
    let error = builder.finish().unwrap_err();
    let location = format!(
        "value object {SOURCE_VALUE_ID:?} variant {:?} field {:?}",
        "Struct", "identity"
    );

    assert_eq!(
        error.to_string(),
        violation(MISSING_IDENTITY_ID, &location, "identities")
    );
    assert_eq!(
        error,
        DomainModelError::FieldReferenceInventoryViolation {
            reference: DomainModelReference::DomainIdentity(Box::new(MISSING_IDENTITY_ID)),
            location,
            inventory_key: "identities",
        }
    );
}

#[test]
fn reports_a_missing_reference_from_an_entity_field() {
    let mut builder = DomainModelBuilder::new();
    builder.add_entity(ENTITY_REFERENCE);
    let error = builder.finish().unwrap_err();
    let location = format!("entity {ENTITY_SOURCE_ID:?} field {:?}", "related");

    assert_eq!(
        error.to_string(),
        violation(MISSING_ENTITY_ID, &location, "entities")
    );
    assert_eq!(
        error,
        DomainModelError::FieldReferenceInventoryViolation {
            reference: DomainModelReference::Entity(Box::new(MISSING_ENTITY_ID)),
            location,
            inventory_key: "entities",
        }
    );
}

#[test]
fn reports_a_missing_reference_from_a_command_field() {
    let mut builder = DomainModelBuilder::new();
    builder.add_command(COMMAND_REFERENCE).unwrap();
    let error = builder.finish().unwrap_err();
    let location = format!("command {COMMAND_ID:?} field {:?}", "aggregate");

    assert_eq!(
        error.to_string(),
        violation(MISSING_AGGREGATE_ID, &location, "aggregates")
    );
    assert_eq!(
        error,
        DomainModelError::FieldReferenceInventoryViolation {
            reference: DomainModelReference::Aggregate(Box::new(MISSING_AGGREGATE_ID)),
            location,
            inventory_key: "aggregates",
        }
    );
}

#[test]
fn reports_a_missing_reference_from_an_event_field() {
    let mut builder = DomainModelBuilder::new();
    builder.add_domain_event(EVENT_REFERENCE).unwrap();
    let error = builder.finish().unwrap_err();
    let location = format!("domain event {EVENT_ID:?} field {:?}", "value");

    assert_eq!(
        error.to_string(),
        violation(MISSING_VALUE_ID, &location, "value_objects")
    );
    assert_eq!(
        error,
        DomainModelError::FieldReferenceInventoryViolation {
            reference: DomainModelReference::ValueObject(Box::new(MISSING_VALUE_ID)),
            location,
            inventory_key: "value_objects",
        }
    );
}

#[test]
fn reports_a_missing_reference_from_an_error_field() {
    let mut builder = DomainModelBuilder::new();
    builder.add_domain_error(ERROR_REFERENCE);
    let error = builder.finish().unwrap_err();
    let location = format!("domain error {ERROR_ID:?} field {:?}", "identity");

    assert_eq!(
        error.to_string(),
        violation(MISSING_IDENTITY_ID, &location, "identities")
    );
    assert_eq!(
        error,
        DomainModelError::FieldReferenceInventoryViolation {
            reference: DomainModelReference::DomainIdentity(Box::new(MISSING_IDENTITY_ID)),
            location,
            inventory_key: "identities",
        }
    );
}

trait StructShapeFields {
    fn shape_fields(self) -> Option<&'static [FieldDescriptor]>;
}

impl StructShapeFields for ValueObjectDescriptor {
    fn shape_fields(self) -> Option<&'static [FieldDescriptor]> {
        match self.shape {
            ValueObjectShapeDescriptor::Struct { fields } => Some(fields),
            _ => None,
        }
    }
}
