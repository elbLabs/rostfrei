#![allow(dead_code)]

use std::convert::Infallible;

use domain::{
    Aggregate, AggregateType, BoundedContext, Command, CommandOwnerId, CommandType, DomainIdentity,
    DomainIdentityType, DomainModelError, DomainService, Entity, FieldKind, FieldWrapper,
    JsonCommandPayload, JsonErrorPayload, ValueObject, ValueObjectType, domain_actions,
    domain_model,
};
use serde_json::{Value, json};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
pub struct Catalog;

#[derive(DomainIdentity)]
#[domain(owner = CatalogRoot)]
pub struct ProductId(u64);

#[derive(Entity)]
#[domain(id = "catalog-root", label = "Catalog", owner = CatalogAggregate)]
pub struct CatalogRoot {
    #[domain(identity)]
    id: ProductId,
}

#[derive(Aggregate)]
#[domain(id = "catalog", label = "Catalog")]
pub struct CatalogAggregate;

impl domain::AggregateDefinition for CatalogAggregate {
    type Context = Catalog;
    type Root = CatalogRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(ValueObject)]
#[domain(id = "status", label = "Status", owner = Catalog)]
pub struct Status(String);

#[derive(Command)]
#[domain(id = "change-status", label = "Change status", owner = CatalogAggregate)]
pub struct ChangeStatus {
    #[domain(identity)]
    target_id: ProductId,
    #[domain(value_object)]
    status: Status,
    #[domain(value_object)]
    alternatives: Option<Vec<Status>>,
}

#[derive(DomainService)]
#[domain(
    id = "catalog-sync",
    label = "Catalog sync",
    context = Catalog,
    actions = [CatalogSyncActions]
)]
pub struct CatalogSync;

#[derive(Command)]
#[domain(
    id = "sync-catalog",
    label = "Sync catalog",
    owner = CatalogSync,
    schema_version = 2
)]
pub struct SyncCatalog;

#[derive(Command, Debug, Eq, PartialEq)]
#[domain(
    id = "json-change",
    label = "JSON change",
    owner = CatalogAggregate,
    json
)]
struct JsonChange {
    value: String,
    optional: Option<u32>,
}

#[derive(Command, Debug, Eq, PartialEq)]
#[domain(
    id = "json-tuple",
    label = "JSON tuple",
    owner = CatalogAggregate,
    json
)]
struct JsonTuple(String, u32);

#[derive(Command, Debug, Eq, PartialEq)]
#[domain(
    id = "json-unit",
    label = "JSON unit",
    owner = CatalogAggregate,
    json
)]
struct JsonUnit;

#[domain_actions(aggregate)]
pub trait CatalogActions {
    #[action(id = "apply-status", label = "Apply status")]
    fn apply_status(root: &mut CatalogRoot, input: Status);
}

impl CatalogActions for CatalogAggregate {
    fn apply_status(root: &mut CatalogRoot, input: Status) {
        let _ = (root, input);
    }
}

#[domain_actions(domain_service)]
pub trait CatalogSyncActions {
    #[action(id = "start-sync", label = "Start sync")]
    fn start_sync(input: Status);

    #[action(id = "inspect-status", label = "Inspect status")]
    fn inspect_status(input: Status);
}

impl CatalogSyncActions for CatalogSync {
    fn start_sync(input: Status) {
        let _ = input;
    }

    fn inspect_status(input: Status) {
        let _ = input;
    }
}

#[test]
fn describes_structural_command_fields() {
    let descriptor = ChangeStatus::DESCRIPTOR;
    let fields = descriptor.fields;
    assert_eq!(descriptor.id.local, "change-status");
    assert_eq!(descriptor.label, "Change status");
    assert_eq!(ChangeStatus::SCHEMA_VERSION, 1);
    assert_eq!(SyncCatalog::SCHEMA_VERSION, 2);
    assert_eq!(
        descriptor.id.owner,
        CommandOwnerId::Aggregate(CatalogAggregate::DESCRIPTOR.id)
    );
    assert_eq!(
        fields.iter().map(|field| field.name).collect::<Vec<_>>(),
        ["target_id", "status", "alternatives"]
    );
    assert_eq!(
        fields[0].value.kind,
        FieldKind::DomainIdentity(ProductId::DESCRIPTOR.id)
    );
    assert!(
        matches!(fields[1].value.kind, FieldKind::ValueObject(id) if id == Status::DESCRIPTOR.id)
    );
    assert_eq!(
        fields[2].value.wrappers,
        &[FieldWrapper::Optional, FieldWrapper::List]
    );
}

#[test]
fn inventories_aggregate_and_domain_service_commands() {
    let model = domain_model! {
        contexts: [Catalog],
        aggregates: [CatalogAggregate],
        entities: [CatalogRoot],
        identities: [ProductId],
        value_objects: [Status],
        services: [CatalogSync],
        commands: [ChangeStatus, SyncCatalog],
        errors: [],
        query_groups: [],
    }
    .expect("command domain model should be valid");

    assert_eq!(model["commands"].as_array().unwrap().len(), 2);
    assert_eq!(model["commands"][0]["id"]["owner"]["kind"], "aggregate");
    assert_eq!(model["commands"][1]["id"]["owner"]["kind"], "domainService");
    assert_eq!(model["commands"][1]["id"]["local"], "sync-catalog");
    assert_eq!(
        model["actions"][0]["input"]["id"],
        model["valueObjects"][0]["id"]
    );
    assert!(
        model["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["input"]["kind"] == "valueObject")
    );
}

#[test]
fn generated_json_commands_enforce_their_exact_struct_shape() {
    let named = JsonChange {
        value: "ready".to_owned(),
        optional: Some(2),
    };
    assert_eq!(
        named.encode_json().unwrap(),
        json!({ "value": "ready", "optional": 2 })
    );
    assert_eq!(
        JsonChange::decode_json(&named.encode_json().unwrap()).unwrap(),
        named
    );
    assert_eq!(
        JsonChange::decode_json(&json!({ "value": "ready" })).unwrap(),
        JsonChange {
            value: "ready".to_owned(),
            optional: None,
        }
    );
    assert!(
        JsonChange::decode_json(&json!({ "value": "ready", "unknown": true }))
            .unwrap_err()
            .contains("unknown command field")
    );
    assert!(JsonChange::decode_json(&json!({})).is_err());

    assert_eq!(
        JsonTuple::decode_json(&json!(["ready", 2])).unwrap(),
        JsonTuple("ready".to_owned(), 2)
    );
    assert_eq!(
        JsonTuple("ready".to_owned(), 2).encode_json().unwrap(),
        json!(["ready", 2])
    );
    assert!(JsonTuple::decode_json(&json!(["ready", 2, 3])).is_err());
    assert_eq!(JsonUnit::decode_json(&Value::Null).unwrap(), JsonUnit);
    assert_eq!(JsonUnit.encode_json().unwrap(), Value::Null);
    assert_eq!(JsonUnit::decode_json(&json!({})).unwrap(), JsonUnit);
    assert!(JsonUnit::decode_json(&json!({ "unexpected": true })).is_err());
}

#[test]
fn generated_json_supports_commands_without_a_rejection() {
    fn assert_json_error_payload<T: JsonErrorPayload>() {}

    assert_json_error_payload::<Infallible>();
}

#[test]
fn rejects_duplicate_command_ids_deterministically() {
    let error = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        identities: [],
        value_objects: [],
        services: [],
        commands: [ChangeStatus, ChangeStatus],
        errors: [],
        query_groups: [],
    }
    .expect_err("duplicate command IDs should be rejected");
    let id = ChangeStatus::DESCRIPTOR.id;

    assert_eq!(
        error,
        DomainModelError::DuplicateCommandId { id: Box::new(id) }
    );
    assert_eq!(error.to_string(), format!("duplicate CommandId: {id:?}"));
}
