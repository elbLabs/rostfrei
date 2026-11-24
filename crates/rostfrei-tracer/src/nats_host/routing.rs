use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use rostfrei_core::{
    AggregateCodecs, AggregateType, AppendOutcome, EventBatch, EventHistory, EventStoreError,
    EventStoreErrorKind, ExpectedVersion, FixtureEventStore, RecordedEvent, StreamDirectory,
    StreamId, StreamSummary,
};
use rostfrei_fixtures::{Fixture, FixtureEventSet, MessageSeriesEngine};
use rostfrei_messaging_core::TrafficScope;
use rostfrei_registry::DomainRegistry;
use serde_json::Value;

use super::{NatsHostError, NatsRuntime, backend::CurrentBackend, group::validate_scope};
use crate::{
    ApplicationBindingError, CommandInvocation, CommandReceipt, CommandTransport,
    CommandTransportError, CommandTransportErrorKind, CommandTransportObserver, TracerBuilder,
    catalog::merge_catalog_models, runtime::RuntimeBindings,
};

/// Routes capabilities across context runtimes in one application and traffic
/// scope. Each member keeps its own command handlers and event authority.
pub struct RuntimeGroup {
    members: Vec<Arc<NatsRuntime>>,
    contexts: HashMap<String, Arc<NatsRuntime>>,
    aggregates: HashMap<AggregateType, CurrentBackend>,
    scope: TrafficScope,
    bindings: RuntimeBindings,
    codecs: Arc<AggregateCodecs>,
    domain_model: Value,
}

impl RuntimeGroup {
    /// All members must use the same application messaging resources and own
    /// distinct contexts. Empty groups and inconsistent bindings are rejected.
    pub fn new(members: Vec<Arc<NatsRuntime>>) -> Result<Self, NatsHostError> {
        let first = members.first().ok_or_else(|| {
            NatsHostError::Configuration("a runtime group must not be empty".into())
        })?;
        validate_scope(&first.config, members.iter().map(|member| &member.config))?;
        let scope = first.config.context.traffic_scope();
        let mut contexts = HashMap::new();
        let mut aggregates = HashMap::new();
        let mut bindings = RuntimeBindings::new(DomainRegistry::new());
        let mut codecs = AggregateCodecs::default();
        for member in &members {
            let context = member.config.context.name().as_str();
            contexts.insert(context.to_owned(), member.clone());
            validate_binding_ownership(member, context)?;
            bindings.merge(&member.bindings.metadata)?;
            // An installed codec may be shared application-wide. It does not
            // expose an aggregate or select its event authority.
            for aggregate in member.bindings.registry().aggregates() {
                let aggregate_type = AggregateType::new(aggregate)
                    .map_err(rostfrei_core::CodecRegistrationError::from)
                    .map_err(ApplicationBindingError::from)?;
                if !codecs
                    .import(&member.bindings.codecs, &aggregate_type)
                    .map_err(ApplicationBindingError::from)?
                {
                    return Err(
                        ApplicationBindingError::UnknownAggregateCodec { aggregate_type }.into(),
                    );
                }
                aggregates.insert(aggregate_type, member.backend.clone());
            }
        }
        let domain_model = merge_catalog_models(
            members
                .iter()
                .filter_map(|member| member.bindings.domain_model()),
        )
        .map_err(NatsHostError::Configuration)?;
        Ok(Self {
            members,
            contexts,
            aggregates,
            scope,
            bindings,
            codecs: Arc::new(codecs),
            domain_model,
        })
    }

    pub fn members(&self) -> &[Arc<NatsRuntime>] {
        &self.members
    }

    /// Builds catalog and diagnostics from the registered members, following
    /// their current backends across resets. Transport and reset are opt-in.
    pub fn tracer_builder(self: &Arc<Self>) -> TracerBuilder {
        let builder = TracerBuilder::new(self.clone(), self.bindings.registry.clone())
            .with_bindings(self.bindings.clone())
            .with_stream_directory(self.clone())
            .with_event_codecs(self.codecs.clone())
            .with_domain_model(self.domain_model.clone());
        if self.scope == TrafficScope::Test {
            builder.with_test_event_store(self.clone())
        } else {
            builder
        }
    }

    fn ensure_test(&self) -> Result<(), EventStoreError> {
        if self.scope != TrafficScope::Test {
            return Err(EventStoreError::new(
                EventStoreErrorKind::InvalidRequest,
                "fixture injection requires Test scope",
            ));
        }
        Ok(())
    }

    fn validate_fixture_routes(&self, fixture: &Fixture) -> Result<(), NatsHostError> {
        self.ensure_test()?;
        for message in fixture.messages().iter() {
            self.backend_for(message.aggregate().aggregate_type())?;
        }
        Ok(())
    }

    /// Preflight a fixture before beginning a destructive lifecycle operation.
    pub(super) fn validate_fixture(&self, fixture: &Fixture) -> Result<(), NatsHostError> {
        self.validate_fixture_routes(fixture)?;
        MessageSeriesEngine::with_codecs(self.codecs.clone()).validate_fixture(fixture)?;
        Ok(())
    }

    /// Load the whole scenario through the shared fixture engine. The caller
    /// must quiesce members and install the returned suppression set before
    /// starting workers. Cross-authority fixture writes are not atomic.
    pub async fn apply_fixture(&self, fixture: &Fixture) -> Result<FixtureEventSet, NatsHostError> {
        self.validate_fixture_routes(fixture)?;
        let events = FixtureEventSet::new(std::slice::from_ref(fixture))?;
        MessageSeriesEngine::with_codecs(self.codecs.clone())
            .apply(self, fixture)
            .await?;
        Ok(events)
    }

    fn backend_for(&self, aggregate: &AggregateType) -> Result<&CurrentBackend, EventStoreError> {
        self.aggregates.get(aggregate).ok_or_else(|| {
            EventStoreError::new(
                EventStoreErrorKind::InvalidRequest,
                format!("no runtime owns aggregate `{aggregate}`"),
            )
        })
    }
}

fn aggregate_context(aggregate: &str) -> Option<&str> {
    let (context, local) = aggregate.split_once('/')?;
    (!local.is_empty() && !local.contains('/')).then_some(context)
}

fn validate_binding_ownership(runtime: &NatsRuntime, context: &str) -> Result<(), NatsHostError> {
    let registry = runtime.bindings.registry();
    for aggregate in registry.aggregates() {
        if aggregate_context(aggregate) != Some(context) {
            return Err(NatsHostError::Configuration(format!(
                "runtime `{context}` registers foreign aggregate `{aggregate}`"
            )));
        }
    }
    for command in registry.commands() {
        if command.bounded_context != context {
            return Err(NatsHostError::Configuration(format!(
                "runtime `{context}` registers foreign command `{}/{}`",
                command.bounded_context, command.command_name
            )));
        }
    }
    Ok(())
}

#[async_trait]
impl CommandTransport for RuntimeGroup {
    fn maximum_payload_len(&self) -> usize {
        self.members
            .iter()
            .map(|member| member.transport.maximum_payload_len())
            .min()
            .unwrap_or_default()
    }

    fn supports_command(&self, context: &str, command: &str, schema_version: u32) -> bool {
        self.bindings
            .registry
            .command(context, command, schema_version)
            .is_some()
    }

    async fn invoke(
        &self,
        invocation: CommandInvocation,
        observer: Arc<dyn CommandTransportObserver>,
    ) -> Result<CommandReceipt, CommandTransportError> {
        let member = self.contexts.get(invocation.context()).ok_or_else(|| {
            CommandTransportError::new(
                CommandTransportErrorKind::InvalidConfiguration,
                format!("no runtime owns context `{}`", invocation.context()),
            )
        })?;
        member.transport.invoke(invocation, observer).await
    }
}

#[async_trait]
impl EventHistory for RuntimeGroup {
    async fn load(&self, stream: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.backend_for(stream.aggregate_type())?
            .load(stream)
            .await
    }
}

#[async_trait]
impl StreamDirectory for RuntimeGroup {
    async fn list_streams(
        &self,
        aggregate: &AggregateType,
    ) -> Result<Vec<StreamSummary>, EventStoreError> {
        self.backend_for(aggregate)?.list_streams(aggregate).await
    }
}

#[async_trait]
impl FixtureEventStore for RuntimeGroup {
    async fn append_fixture(
        &self,
        stream: &StreamId,
        expected: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.ensure_test()?;
        self.backend_for(stream.aggregate_type())?
            .append_fixture(stream, expected, batch)
            .await
    }
}
