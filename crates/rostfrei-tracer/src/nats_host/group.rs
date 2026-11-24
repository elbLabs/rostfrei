use futures_util::future::join_all;
use std::collections::HashSet;

use super::{
    Arc, CorrelationObserver, Fixture, FixtureEventSet, NatsHostError, NatsRuntime,
    NatsRuntimeConfig, Ordering, RuntimeGroup, TestScenarioReset, TestScenarioResetError,
    TrafficScope, async_trait, provision_application_messaging,
};

pub(super) fn private_stream_names(config: &NatsRuntimeConfig) -> impl Iterator<Item = &str> {
    let state = config.event_store.outcome_state_stream_name();
    std::iter::once(config.event_store.stream_name()).chain((!state.is_empty()).then_some(state))
}

pub(super) fn validate_scope<'a>(
    expected: &NatsRuntimeConfig,
    configs: impl IntoIterator<Item = &'a NatsRuntimeConfig>,
) -> Result<(), NatsHostError> {
    let mut contexts = HashSet::new();
    let mut private_streams = HashSet::new();
    for config in configs {
        if config.context.application() != expected.context.application()
            || config.context.traffic_scope() != expected.context.traffic_scope()
            || config.messaging != expected.messaging
        {
            return Err(NatsHostError::Configuration(
                "contexts in a scope must share one application and messaging resource plan".into(),
            ));
        }
        let context = config.context.name().as_str();
        if !contexts.insert(context.to_owned()) {
            return Err(NatsHostError::Configuration(format!(
                "duplicate runtime for context `{context}`"
            )));
        }
        for stream in private_stream_names(config) {
            let topology = expected.messaging.topology();
            if !private_streams.insert(stream.to_owned())
                || [
                    topology.command_stream(),
                    topology.command_response_stream(),
                    topology.integration_event_stream(),
                    topology.quarantine_stream(),
                ]
                .iter()
                .any(|shared| shared.as_str() == stream)
            {
                return Err(NatsHostError::Configuration(format!(
                    "event-store stream `{stream}` must belong to exactly one context"
                )));
            }
        }
    }
    Ok(())
}

pub(super) struct TestRuntimeGroup(pub Arc<RuntimeGroup>);

impl TestRuntimeGroup {
    pub(super) async fn apply_fixture(&self, fixture: &Fixture) -> Result<(), NatsHostError> {
        let events = self.0.apply_fixture(fixture).await?;
        for runtime in self.0.members() {
            runtime.fixture_events.write().await.extend(events.clone());
        }
        Ok(())
    }

    pub(super) async fn reset_scope(&self, fixture: &Fixture) -> Result<(), NatsHostError> {
        let members = self.0.members();
        let Some(first) = members.first() else {
            return Err(NatsHostError::Configuration(
                "reset requires at least one runtime".into(),
            ));
        };
        if first.config.context.traffic_scope() != TrafficScope::Test {
            return Err(NatsHostError::Configuration(
                "reset requires Test scope".into(),
            ));
        }
        // Take the gates in membership order to serialize resets and let every
        // command already admitted to the retiring stores finish before deletion.
        let _gates = lock_members(members).await;
        if members
            .iter()
            .any(|runtime| runtime.shutting_down.load(Ordering::Acquire))
        {
            return Err(NatsHostError::Configuration(
                "runtime is shutting down".into(),
            ));
        }
        for runtime in members {
            runtime.available.store(false, Ordering::Release);
            runtime.reset_required.store(true, Ordering::Release);
        }
        join_all(members.iter().map(|runtime| runtime.stop_in_scope())).await;
        self.0.validate_fixture(fixture)?;
        for runtime in members {
            *runtime.fixture_events.write().await = FixtureEventSet::default();
        }
        first.delete_shared_streams().await?;
        for runtime in members {
            runtime.delete_private_streams().await?;
        }
        provision_application_messaging(first.connection.jetstream(), &first.config.messaging)
            .await?;
        for runtime in members {
            runtime
                .config
                .provision_context(&runtime.connection)
                .await?;
        }
        let mut backends = Vec::with_capacity(members.len());
        for runtime in members {
            backends.push(runtime.connect_backend().await?);
        }
        for (runtime, backend) in members.iter().zip(backends) {
            runtime.replace_backend(backend);
        }
        self.apply_fixture(fixture).await?;
        start_fresh_in_scope(members).await?;
        for runtime in members {
            runtime.reset_required.store(false, Ordering::Release);
        }
        Ok(())
    }
}

#[async_trait]
impl TestScenarioReset for TestRuntimeGroup {
    async fn reset(&self, fixture: &Fixture) -> Result<(), TestScenarioResetError> {
        self.reset_scope(fixture)
            .await
            .map_err(|error| TestScenarioResetError::Failed(error.to_string()))
    }
}

async fn lock_members(members: &[Arc<NatsRuntime>]) -> Vec<tokio::sync::RwLockWriteGuard<'_, ()>> {
    let mut gates = Vec::with_capacity(members.len());
    for runtime in members {
        gates.push(runtime.gate.write().await);
    }
    gates
}

pub(super) async fn attach_dispatch_observers(
    members: &[Arc<NatsRuntime>],
    observer: CorrelationObserver,
) -> Result<(), NatsHostError> {
    let _gates = lock_members(members).await;
    let mut previous = Vec::with_capacity(members.len());
    for member in members {
        previous.push(member.observer.lock().await.clone());
        if let Err(error) = member
            .attach_observer_in_scope(observer.clone(), members.len() > 1)
            .await
        {
            // A failed attachment can change the failing member's setting too.
            // The host stops workers on rollback; restore what a subsequent
            // caller-owned restart will use, not the failed host's observer.
            for (member, configured) in members.iter().zip(previous) {
                *member.observer.lock().await = configured;
            }
            return Err(error);
        }
    }
    Ok(())
}

pub(super) async fn start_fresh(members: &[Arc<NatsRuntime>]) -> Result<(), NatsHostError> {
    let _gates = lock_members(members).await;
    start_fresh_in_scope(members).await
}

async fn start_fresh_in_scope(members: &[Arc<NatsRuntime>]) -> Result<(), NatsHostError> {
    let mut prepared = Vec::with_capacity(members.len());
    for runtime in members {
        prepared.push(runtime.prepare_workers().await?);
    }
    let workers = join_all(members.iter().map(|runtime| runtime.workers.lock())).await;
    if members
        .iter()
        .any(|runtime| runtime.shutting_down.load(Ordering::Acquire))
    {
        drop(workers);
        return Err(NatsHostError::Configuration(
            "runtime is shutting down".into(),
        ));
    }
    // Launch only after preparing the complete group; the launch itself cannot
    // fail or be cancelled midway.
    for ((runtime, mut workers), prepared) in members.iter().zip(workers).zip(prepared) {
        prepared.launch(runtime, &mut workers);
    }
    Ok(())
}
