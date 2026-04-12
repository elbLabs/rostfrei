#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainCommand, DomainCommandType, DomainError, DomainErrorType,
    DomainEvent, DomainEventDefinitionType, DomainIdentity, DomainIdentityDescriptor,
    DomainIdentityId, DomainIdentityType, Entity, EntityType, FieldKind, FieldWrapper, ScalarType,
    SemanticScalar, SemanticScalarDescriptor, ValueObject, ValueObjectType, domain_model,
};
use serde_json::json;

mod foreign {
    #[derive(Clone, Copy)]
    pub struct Uuid(pub [u8; 16]);
}

struct UuidScalar;

impl SemanticScalar for UuidScalar {
    type Value = foreign::Uuid;

    const DESCRIPTOR: SemanticScalarDescriptor = SemanticScalarDescriptor {
        id: "uuid",
        label: "UUID",
        representation: ScalarType::String,
    };
}

#[derive(BoundedContext)]
#[domain(id = "semantic-scalars", label = "Semantic scalars")]
struct SemanticScalars;

#[derive(DomainIdentity)]
#[domain(owner = DocumentRoot, scalar = UuidScalar)]
struct DocumentId(foreign::Uuid);

#[derive(DomainIdentity)]
#[domain(owner = Revision)]
struct RevisionId(u64);

#[derive(Entity)]
#[domain(id = "document-root", label = "Document", owner = Documents)]
struct DocumentRoot {
    #[domain(identity)]
    id: DocumentId,
    #[domain(scalar = UuidScalar)]
    correlation_id: foreign::Uuid,
    #[domain(scalar = UuidScalar)]
    related_ids: Option<Vec<foreign::Uuid>>,
    title: String,
}

#[derive(Entity)]
#[domain(id = "revision", label = "Revision", owner = Documents)]
struct Revision {
    #[domain(identity)]
    id: RevisionId,
}

#[derive(Aggregate)]
#[domain(
    id = "documents",
    label = "Documents",
    context = SemanticScalars,
    root = DocumentRoot,
    events = [DocumentCorrelated]
)]
struct Documents;

#[derive(ValueObject)]
#[domain(id = "external-reference", label = "External reference", owner = SemanticScalars)]
struct ExternalReference {
    #[domain(scalar = UuidScalar)]
    value: foreign::Uuid,
}

#[derive(DomainCommand)]
#[domain(id = "correlate-document", label = "Correlate document", owner = Documents)]
struct CorrelateDocument {
    #[domain(scalar = UuidScalar)]
    correlation_id: foreign::Uuid,
}

#[derive(DomainEvent)]
#[domain(id = "document-correlated", label = "Document correlated")]
struct DocumentCorrelated {
    #[domain(scalar = UuidScalar)]
    correlation_id: foreign::Uuid,
}

#[derive(DomainError)]
#[domain(
    id = "document-correlation-rejected",
    label = "Document correlation rejected",
    owner = Documents,
    code = "DOCUMENT_CORRELATION_REJECTED",
    message = "The document correlation was rejected."
)]
struct DocumentCorrelationRejected {
    #[domain(scalar = UuidScalar)]
    correlation_id: foreign::Uuid,
}

struct ContradictoryId(foreign::Uuid);

#[derive(Entity)]
#[domain(id = "contradictory-root", label = "Contradictory", owner = ContradictoryDocuments)]
struct ContradictoryRoot {
    #[domain(identity)]
    id: ContradictoryId,
}

#[derive(Aggregate)]
#[domain(
    id = "contradictory-documents",
    label = "Contradictory documents",
    context = SemanticScalars,
    root = ContradictoryRoot
)]
struct ContradictoryDocuments;

impl DomainIdentityType for ContradictoryId {
    type Owner = ContradictoryRoot;

    const DESCRIPTOR: DomainIdentityDescriptor = DomainIdentityDescriptor {
        id: DomainIdentityId {
            owner: domain::EntityId {
                aggregate: domain::AggregateId {
                    context: domain::BoundedContextId("semantic-scalars"),
                    local: "contradictory-documents",
                },
                local: "contradictory-root",
            },
        },
        scalar: ScalarType::U64,
    };
    const SEMANTIC_SCALAR: Option<SemanticScalarDescriptor> = Some(UuidScalar::DESCRIPTOR);
}

#[test]
fn describes_semantic_fields_and_nested_wrappers() {
    let fields = DocumentRoot::DESCRIPTOR.fields;

    assert_eq!(
        fields[1].value.kind,
        FieldKind::SemanticScalar(UuidScalar::DESCRIPTOR)
    );
    assert_eq!(fields[1].value.wrappers, &[]);
    assert_eq!(
        fields[2].value.kind,
        FieldKind::SemanticScalar(UuidScalar::DESCRIPTOR)
    );
    assert_eq!(
        fields[2].value.wrappers,
        &[FieldWrapper::Optional, FieldWrapper::List]
    );
    assert_eq!(fields[3].value.kind, FieldKind::Scalar(ScalarType::String));

    let semantic_kinds = [
        match ExternalReference::DESCRIPTOR.shape {
            domain::ValueObjectShapeDescriptor::Struct { fields } => fields[0].value.kind,
            _ => panic!(),
        },
        CorrelateDocument::DESCRIPTOR.fields[0].value.kind,
        DocumentCorrelated::DEFINITION.fields[0].value.kind,
        DocumentCorrelationRejected::DESCRIPTOR.fields[0].value.kind,
    ];
    assert!(
        semantic_kinds
            .iter()
            .all(|kind| *kind == FieldKind::SemanticScalar(UuidScalar::DESCRIPTOR))
    );
}

#[test]
fn describes_semantic_identity_representation_without_changing_identity_descriptor() {
    assert_eq!(DocumentId::DESCRIPTOR.scalar, ScalarType::String);
    assert_eq!(DocumentId::SEMANTIC_SCALAR, Some(UuidScalar::DESCRIPTOR));

    assert_eq!(RevisionId::DESCRIPTOR.scalar, ScalarType::U64);
    assert_eq!(RevisionId::SEMANTIC_SCALAR, None);
}

#[test]
fn projects_semantic_scalars_and_canonical_regressions_to_exact_json() {
    let model = domain_model! {
        contexts: [SemanticScalars],
        aggregates: [Documents],
        entities: [DocumentRoot, Revision],
        identities: [DocumentId, RevisionId],
        value_objects: [ExternalReference],
        services: [],
        commands: [CorrelateDocument],
        errors: [DocumentCorrelationRejected],
        query_groups: [],
    };

    assert_eq!(
        model["entities"][0]["fields"],
        json!([{
            "name": "id",
            "value": {
                "kind": "identity",
                "id": {
                    "owner": {
                        "aggregate": {
                            "context": "semantic-scalars",
                            "local": "documents",
                        },
                        "local": "document-root",
                    },
                },
            },
        }, {
            "name": "correlation_id",
            "value": {
                "kind": "scalar",
                "scalar": {
                    "kind": "semantic",
                    "id": "uuid",
                    "label": "UUID",
                    "representation": "string",
                },
            },
        }, {
            "name": "related_ids",
            "value": {
                "kind": "optional",
                "value": {
                    "kind": "list",
                    "element": {
                        "kind": "scalar",
                        "scalar": {
                            "kind": "semantic",
                            "id": "uuid",
                            "label": "UUID",
                            "representation": "string",
                        },
                    },
                },
            },
        }, {
            "name": "title",
            "value": {
                "kind": "scalar",
                "scalar": "string",
            },
        }])
    );

    let semantic_field = json!({
        "name": "correlation_id",
        "value": {
            "kind": "scalar",
            "scalar": {
                "kind": "semantic",
                "id": "uuid",
                "label": "UUID",
                "representation": "string",
            },
        },
    });
    assert_eq!(model["domainCommands"][0]["fields"][0], semantic_field);
    assert_eq!(model["domainEvents"][0]["fields"][0], semantic_field);
    assert_eq!(model["domainErrors"][0]["fields"][0], semantic_field);

    assert_eq!(
        model["domainIdentities"],
        json!([{
            "id": {
                "owner": {
                    "aggregate": {
                        "context": "semantic-scalars",
                        "local": "documents",
                    },
                    "local": "document-root",
                },
            },
            "scalar": {
                "kind": "semantic",
                "id": "uuid",
                "label": "UUID",
                "representation": "string",
            },
        }, {
            "id": {
                "owner": {
                    "aggregate": {
                        "context": "semantic-scalars",
                        "local": "documents",
                    },
                    "local": "revision",
                },
            },
            "scalar": "u64",
        }])
    );
}

#[test]
#[should_panic(
    expected = "DomainIdentity semantic scalar representation must match its canonical scalar descriptor"
)]
fn rejects_contradictory_manual_identity_scalar_metadata() {
    let _ = domain_model! {
        contexts: [SemanticScalars],
        aggregates: [ContradictoryDocuments],
        entities: [ContradictoryRoot],
        identities: [ContradictoryId],
        value_objects: [],
        services: [],
        commands: [],
        errors: [],
        query_groups: [],
    };
}
