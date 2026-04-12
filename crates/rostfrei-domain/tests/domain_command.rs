#![allow(dead_code)]

use rostfrei_domain::{
    Aggregate, AggregateType, BoundedContext, DomainCommand, DomainCommandOwnerId,
    DomainCommandType, DomainIdentity, DomainIdentityType, DomainService, Entity, FieldKind,
    FieldWrapper, ValueObject, ValueObjectType, domain_actions, domain_model,
};

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
#[domain(
    id = "catalog",
    label = "Catalog",
    context = Catalog,
    root = CatalogRoot,
    actions = [CatalogCommandActions]
)]
pub struct CatalogAggregate;

#[derive(ValueObject)]
#[domain(id = "status", label = "Status", owner = Catalog)]
pub struct Status(String);

#[derive(DomainCommand)]
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

#[derive(DomainCommand)]
#[domain(id = "sync-catalog", label = "Sync catalog", owner = CatalogSync)]
pub struct SyncCatalog;

#[domain_actions(aggregate)]
pub trait CatalogCommandActions {
    #[action(id = "apply-status", label = "Apply status")]
    fn apply_status(root: &mut CatalogRoot, input: ChangeStatus);
}

impl CatalogCommandActions for CatalogAggregate {
    fn apply_status(root: &mut CatalogRoot, input: ChangeStatus) {
        let _ = (root, input);
    }
}

#[domain_actions(domain_service)]
pub trait CatalogSyncActions {
    #[action(id = "start-sync", label = "Start sync")]
    fn start_sync(input: SyncCatalog);

    #[action(id = "inspect-status", label = "Inspect status")]
    fn inspect_status(input: Status);
}

impl CatalogSyncActions for CatalogSync {
    fn start_sync(input: SyncCatalog) {
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
    assert_eq!(
        descriptor.id.owner,
        DomainCommandOwnerId::Aggregate(CatalogAggregate::DESCRIPTOR.id)
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
        events: [],
        errors: [],
        query_groups: [],
    };

    assert_eq!(model["domainCommands"].as_array().unwrap().len(), 2);
    assert_eq!(
        model["domainCommands"][0]["id"]["owner"]["kind"],
        "aggregate"
    );
    assert_eq!(
        model["domainCommands"][1]["id"]["owner"]["kind"],
        "domainService"
    );
    assert_eq!(model["domainCommands"][1]["id"]["local"], "sync-catalog");
    assert_eq!(
        model["actions"][0]["input"]["id"],
        model["domainCommands"][0]["id"]
    );
    assert_eq!(model["actions"][2]["input"]["kind"], "valueObject");
}

#[test]
#[should_panic(expected = "duplicate DomainCommandId")]
fn rejects_duplicate_command_ids_deterministically() {
    let _ = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        identities: [],
        value_objects: [],
        services: [],
        commands: [ChangeStatus, ChangeStatus],
        events: [],
        errors: [],
        query_groups: [],
    };
}
