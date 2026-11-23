//! Application declarations and registrations, independent of live broker inspection.

use std::{collections::BTreeMap, num::NonZeroU32};

use rostfrei_core::ReadModel;
use rostfrei_messaging_core::BoundedContextName;
use rostfrei_registry::DomainRegistry;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Logical domain inventory. A listed type does not imply persisted instances.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainInventory {
    /// Whether the application supplied its compiled domain model.
    pub compiled_model_available: bool,
    pub bounded_contexts: Vec<DomainInventoryContext>,
    pub aggregates: Vec<DomainInventoryAggregate>,
    pub entities: Vec<Value>,
    pub domain_identities: Vec<Value>,
    pub value_objects: Vec<Value>,
    pub domain_services: Vec<Value>,
    pub domain_events: Vec<Value>,
    pub domain_errors: Vec<Value>,
    pub actions: Vec<Value>,
    pub policies: Vec<Value>,
    pub queries: Vec<Value>,
    pub invariants: Vec<Value>,
    pub registered_commands: Vec<DomainInventoryOperation>,
    pub registered_queries: Vec<DomainInventoryOperation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainInventoryContext {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DomainInventoryAggregateId {
    pub context: String,
    pub local: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DomainInventoryEntityId {
    pub aggregate: DomainInventoryAggregateId,
    pub local: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainInventoryAggregate {
    pub id: DomainInventoryAggregateId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<DomainInventoryEntityId>,
    #[serde(default)]
    pub anchor: bool,
    /// Runtime routing identity, present only when registered.
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub aggregate_type: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainInventoryOperation {
    pub context: &'static str,
    pub name: &'static str,
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<&'static str>,
    /// Modeled command payload. Absent for registrations that do not describe fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<Value>>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct CompiledDomainModel {
    bounded_contexts: Vec<DomainInventoryContext>,
    aggregates: Vec<DomainInventoryAggregate>,
    entities: Vec<Value>,
    domain_identities: Vec<Value>,
    value_objects: Vec<Value>,
    domain_services: Vec<Value>,
    domain_events: Vec<Value>,
    domain_errors: Vec<Value>,
    actions: Vec<Value>,
    policies: Vec<Value>,
    queries: Vec<Value>,
    invariants: Vec<Value>,
}

impl DomainInventory {
    /// Use the projection emitted by `domain_model!` and the application's registry.
    /// Runtime registrations augment the model; they never imply storage links.
    pub fn from_model(
        model: Option<&Value>,
        registry: &DomainRegistry,
    ) -> Result<Self, serde_json::Error> {
        let compiled = model
            .map(CompiledDomainModel::deserialize)
            .transpose()?
            .unwrap_or_default();
        let mut contexts: BTreeMap<_, _> = compiled
            .bounded_contexts
            .into_iter()
            .map(|context| (context.id.clone(), context))
            .collect();
        let mut aggregates: BTreeMap<_, _> = compiled
            .aggregates
            .into_iter()
            .map(|aggregate| (aggregate.id.clone(), aggregate))
            .collect();
        for aggregate_type in registry.aggregates() {
            // The registry validates context-qualified identities on registration.
            let Some((context, local)) = aggregate_type.split_once('/') else {
                continue;
            };
            let id = DomainInventoryAggregateId {
                context: context.to_owned(),
                local: local.to_owned(),
            };
            aggregates
                .entry(id.clone())
                .or_insert(DomainInventoryAggregate {
                    id,
                    label: None,
                    root: None,
                    anchor: false,
                    aggregate_type: None,
                })
                .aggregate_type = Some(aggregate_type.to_owned());
        }
        let registered_commands: Vec<_> = registry
            .commands()
            .map(|command| DomainInventoryOperation {
                context: command.bounded_context,
                name: command.command_name,
                schema_version: command.schema_version,
                label: Some(command.modeled_command().label),
                fields: Some(command.project_modeled_fields()),
            })
            .collect();
        let registered_queries: Vec<_> = registry
            .queries()
            .map(|query| DomainInventoryOperation {
                context: query.bounded_context,
                name: query.query_name,
                schema_version: query.schema_version,
                label: query.modeled_query().map(|query| query.label),
                fields: None,
            })
            .collect();
        for context_id in aggregates
            .keys()
            .map(|id| id.context.as_str())
            .chain(
                registered_commands
                    .iter()
                    .map(|operation| operation.context),
            )
            .chain(registered_queries.iter().map(|operation| operation.context))
        {
            contexts
                .entry(context_id.to_owned())
                .or_insert_with(|| DomainInventoryContext {
                    id: context_id.to_owned(),
                    label: None,
                });
        }
        Ok(Self {
            compiled_model_available: model.is_some(),
            bounded_contexts: contexts.into_values().collect(),
            aggregates: aggregates.into_values().collect(),
            entities: compiled.entities,
            domain_identities: compiled.domain_identities,
            value_objects: compiled.value_objects,
            domain_services: compiled.domain_services,
            domain_events: compiled.domain_events,
            domain_errors: compiled.domain_errors,
            actions: compiled.actions,
            policies: compiled.policies,
            queries: compiled.queries,
            invariants: compiled.invariants,
            registered_commands,
            registered_queries,
        })
    }
}

/// Explicit application declaration; `ReadModel` types are not in `DomainRegistry`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadModelDeclaration {
    pub context: String,
    pub name: String,
    pub schema_version: NonZeroU32,
    /// Only the policy explicitly supplied for this catalog's application and scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<ReadModelStorage>,
}

impl ReadModelDeclaration {
    /// A logical declaration says nothing about which storage, if any, is used.
    pub fn for_model<M: ReadModel>(context: &BoundedContextName) -> Option<Self> {
        let name = BoundedContextName::new(M::NAME).ok()?;
        Some(Self {
            context: context.as_str().to_owned(),
            name: name.as_str().to_owned(),
            schema_version: NonZeroU32::new(M::SCHEMA_VERSION)?,
            storage: None,
        })
    }

    /// Describe a policy the application actually supplied for this model/scope.
    /// Does not connect, provision, or assert that a stream currently exists.
    /// Returns `None` for a mismatched model name or invalid declaration.
    #[cfg(feature = "nats-host")]
    pub fn from_nats_config<M: ReadModel>(
        config: &rostfrei_nats::NatsReadModelConfig,
    ) -> Option<Self> {
        use rostfrei_messaging_core::TrafficScope;
        use rostfrei_nats::StreamStorage;

        if config.name() != M::NAME {
            return None;
        }
        let context = config.context();
        let mut declaration = Self::for_model::<M>(context.name())?;
        declaration.storage = Some(ReadModelStorage::NatsKv {
            application: context.application().as_str().to_owned(),
            scope: match context.traffic_scope() {
                TrafficScope::Normal => crate::ObservationScope::Production,
                TrafficScope::Test => crate::ObservationScope::Test,
            },
            bucket_name: config.bucket_name().to_owned(),
            stream_name: config.stream_name(),
            max_bytes: config.max_bytes().to_string(),
            max_value_bytes: config.max_value_bytes(),
            storage: match config.storage() {
                StreamStorage::File => ReadModelStorageMedium::File,
                StreamStorage::Memory => ReadModelStorageMedium::Memory,
            },
            replicas: config.replicas(),
            history: config.history(),
            ttl_nanos: config.ttl().as_nanos().to_string(),
        });
        Some(declaration)
    }
}

/// Configured storage policy (not usage, topology discovery, or a health check).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ReadModelStorage {
    NatsKv {
        application: String,
        scope: crate::ObservationScope,
        bucket_name: String,
        stream_name: String,
        max_bytes: String,
        max_value_bytes: i32,
        storage: ReadModelStorageMedium,
        replicas: usize,
        history: i64,
        ttl_nanos: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadModelStorageMedium {
    File,
    Memory,
}
