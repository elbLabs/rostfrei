#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, Command, DomainError, DomainErrorType, DomainEvent,
    DomainEventDefinitionType, DomainIdentity, Entity, EntityType, FieldKind, FieldWrapper,
    ScalarType, SemanticScalar, SemanticScalarDescriptor, domain_model,
};
use serde_json::json;

mod foreign {
    #[derive(Clone, Copy, serde::Deserialize, serde::Serialize)]
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
struct DocumentId(foreign::Uuid);

#[derive(DomainIdentity)]
struct RevisionId(u64);

#[derive(Entity)]
#[domain(id = "document-root", label = "Document")]
struct DocumentRoot {
    #[domain(identity)]
    id: DocumentId,
    #[domain(scalar = UuidScalar)]
    correlation_id: foreign::Uuid,
    #[domain(scalar = UuidScalar)]
    related_ids: Option<Vec<foreign::Uuid>>,
    title: String,
}

impl domain::EntityDefinition for DocumentRoot {
    type Owner = Documents;
    type Identity = DocumentId;
}

#[derive(Entity)]
#[domain(id = "revision", label = "Revision")]
struct Revision {
    #[domain(identity)]
    id: RevisionId,
}

impl domain::EntityDefinition for Revision {
    type Owner = Documents;
    type Identity = RevisionId;
}

#[derive(Aggregate)]
#[domain(id = "documents", label = "Documents")]
struct Documents;

impl domain::AggregateDefinition for Documents {
    type Context = SemanticScalars;
    type Root = DocumentRoot;
    type Event = DocumentsEvents;
}

#[derive(domain::AggregateEvents)]
enum DocumentsEvents {
    Event0(DocumentCorrelated),
}

#[derive(Command)]
#[domain(id = "correlate-document", label = "Correlate document")]
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
#[allow(clippy::too_many_lines)]
fn projects_semantic_scalars_and_canonical_regressions_to_exact_json() {
    let model = domain_model! {
        contexts: [SemanticScalars],
        aggregates: [Documents],
        entities: [DocumentRoot, Revision],
        value_objects: [],
        services: [],
        errors: [DocumentCorrelationRejected],
        query_groups: [],
    }
    .expect("semantic scalar model projection should succeed");

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
        }])
    );
}
