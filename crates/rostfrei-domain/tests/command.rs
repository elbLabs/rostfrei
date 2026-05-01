#![allow(dead_code)]

use std::convert::Infallible;

use domain::{
    Aggregate, BoundedContext, Command, DomainIdentity, Entity, FieldKind, FieldWrapper,
    JsonCommandPayload, JsonErrorPayload, ValueObject,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(Deserialize, DomainIdentity, Serialize)]
struct ProductId(u64);

#[derive(Entity)]
#[domain(id = "catalog-root", label = "Catalog")]
struct CatalogRoot {
    id: ProductId,
}

impl domain::EntityDefinition for CatalogRoot {
    type Owner = CatalogAggregate;
    type Identity = ProductId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Aggregate)]
#[domain(id = "catalog", label = "Catalog")]
struct CatalogAggregate;

impl domain::AggregateDefinition for CatalogAggregate {
    type Context = Catalog;
    type Root = CatalogRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(Deserialize, Serialize, ValueObject)]
#[domain(id = "status", label = "Status")]
struct Status(String);

#[derive(Command)]
#[domain(id = "change-status", label = "Change status")]
struct ChangeStatus {
    target_id: ProductId,
    status: Status,
    alternatives: Option<Vec<Status>>,
}

#[derive(Command)]
#[domain(id = "sync-catalog", label = "Sync catalog", schema_version = 2)]
struct SyncCatalog;

#[derive(Command, Debug, Eq, PartialEq)]
#[domain(id = "json-change", label = "JSON change")]
struct JsonChange {
    value: String,
    optional: Option<u32>,
}

#[derive(Command, Debug, Eq, PartialEq)]
#[domain(id = "json-tuple", label = "JSON tuple")]
struct JsonTuple(String, u32);

#[derive(Command, Debug, Eq, PartialEq)]
#[domain(id = "json-unit", label = "JSON unit")]
struct JsonUnit;

#[test]
fn describes_owner_independent_command_fields_and_schema() {
    let descriptor = ChangeStatus::DESCRIPTOR;
    let fields = descriptor.fields;

    assert_eq!(descriptor.local_id, "change-status");
    assert_eq!(descriptor.label, "Change status");
    assert_eq!(descriptor.schema_version, 1);
    assert_eq!(SyncCatalog::SCHEMA_VERSION, 2);
    assert_eq!(SyncCatalog::DESCRIPTOR.schema_version, 2);
    assert_eq!(
        fields.iter().map(|field| field.name).collect::<Vec<_>>(),
        ["target_id", "status", "alternatives"]
    );
    assert_eq!(fields[0].value.kind, FieldKind::Opaque);
    assert_eq!(fields[1].value.kind, FieldKind::Opaque);
    assert_eq!(fields[2].value.kind, FieldKind::Opaque);
    assert_eq!(
        fields[2].value.wrappers,
        &[FieldWrapper::Optional, FieldWrapper::List]
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
fn generated_json_supports_commands_without_modeled_rejection_metadata() {
    fn assert_json_error_payload<T: JsonErrorPayload>() {}

    assert_json_error_payload::<Infallible>();
}
rostfrei_domain_macros::__install_test_macro_support!();
