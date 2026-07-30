use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

use async_trait::async_trait;
use rostfrei_core::{
    Aggregate, CommittedDomainEvent, DomainEventDispatchOutcome, DomainEventDispatcher,
    DomainEventHandler, DomainEventHandlerError, DomainEventHandlerErrorKind, Event, EventCodec,
    EventHistory, EventVariant, JsonEventCodec, ReadModel, ReadModelError, ReadModelErrorKind,
    ReadModelKey, ReadModelStore, RecordedEvent,
};
use rostfrei_messaging_core::BoundedContext;

use super::{
    Change, DomainKey, ReadModelDomainEvent, ReadModelIntegrationBinding, ReadModelProcessingError,
    ReadModelState,
};

pub(super) trait DomainReducer<M>: Send + Sync {
    fn key(&self, event: &RecordedEvent) -> Result<ReadModelKey, ReadModelProcessingError>;
    fn apply(&self, model: &mut M, event: &RecordedEvent) -> Result<(), ReadModelProcessingError>;
}

pub(super) struct TypedDomainReducer<M, A, E> {
    pub key: DomainKey<E>,
    pub change: Change<M, E>,
    pub marker: std::marker::PhantomData<fn() -> A>,
}

impl<M, A, E> TypedDomainReducer<M, A, E>
where
    A: Aggregate,
    A::Event: Event + EventVariant<E>,
{
    fn decode(event: &RecordedEvent) -> Result<E, ReadModelProcessingError> {
        let value = <JsonEventCodec as EventCodec<A>>::decode(&JsonEventCodec, event)
            .map_err(|error| ReadModelProcessingError::InvalidEvent(error.to_string()))?;
        value.into_event().ok_or_else(|| {
            ReadModelProcessingError::InvalidEvent("domain event variant mismatch".to_owned())
        })
    }
}

impl<M, A, E> DomainReducer<M> for TypedDomainReducer<M, A, E>
where
    A: Aggregate,
    A::Event: Event + EventVariant<E>,
{
    fn key(&self, event: &RecordedEvent) -> Result<ReadModelKey, ReadModelProcessingError> {
        ReadModelKey::new((self.key)(&ReadModelDomainEvent {
            event: &Self::decode(event)?,
            recorded: event,
        }))
        .map_err(|error| ReadModelProcessingError::InvalidEvent(error.to_string()))
    }
    fn apply(&self, model: &mut M, event: &RecordedEvent) -> Result<(), ReadModelProcessingError> {
        (self.change)(model, &Self::decode(event)?)
    }
}

pub(super) type DomainReducers<M> = BTreeMap<(String, String), Arc<dyn DomainReducer<M>>>;

pub(super) struct Resources<M: ReadModel> {
    pub store: Arc<dyn ReadModelStore<ReadModelState<M>>>,
    pub history: Option<Arc<dyn EventHistory>>,
    pub domain: DomainReducers<M>,
}

pub(super) struct Session<M: ReadModel> {
    pub context: BoundedContext,
    pub resources: OnceLock<Resources<M>>,
    pub retries: u32,
}

#[async_trait]
pub(super) trait Mutation<M: ReadModel>: Send + Sync {
    async fn apply(&self, model: &mut M, applied: u64) -> Result<(), ReadModelProcessingError>;
}

impl<M: ReadModel> Session<M> {
    pub fn resources(&self) -> Result<&Resources<M>, ReadModelProcessingError> {
        self.resources.get().ok_or_else(|| {
            ReadModelProcessingError::Configuration("read model has not been connected".to_owned())
        })
    }

    pub async fn mutate(
        &self,
        key: &ReadModelKey,
        source: &str,
        version: u64,
        mutation: &impl Mutation<M>,
    ) -> Result<(), ReadModelProcessingError> {
        if version == 0 {
            return Err(ReadModelProcessingError::InvalidEvent(
                "source version must be positive".to_owned(),
            ));
        }
        let store = &self.resources()?.store;
        for _ in 0..self.retries {
            let entry = store.read(key).await?;
            let (mut state, revision) = entry.map_or_else(
                || (ReadModelState::<M>::default(), None),
                |entry| (entry.value, Some(entry.revision)),
            );
            state.validate()?;
            let applied = state.positions.get(source).copied().unwrap_or_default();
            if version <= applied {
                return Ok(());
            }
            mutation.apply(&mut state.value, applied).await?;
            state.positions.insert(source.to_owned(), version);
            let result = match revision {
                Some(revision) => store.update(key, &revision, &state).await,
                None => store.create(key, &state).await,
            };
            match result {
                Ok(_) => return Ok(()),
                Err(error) if error.kind() == ReadModelErrorKind::Conflict => {
                    tokio::task::yield_now().await;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(ReadModelError::new(
            ReadModelErrorKind::Conflict,
            "read-model contention; retry delivery later",
        )
        .into())
    }

    pub async fn domain(&self, event: &RecordedEvent) -> Result<(), ReadModelProcessingError> {
        let resources = self.resources()?;
        let reducer = resources
            .domain
            .get(&(
                event.stream_id().aggregate_type().as_str().to_owned(),
                event.event_type().to_owned(),
            ))
            .ok_or_else(|| {
                ReadModelProcessingError::InvalidEvent("unregistered domain event".to_owned())
            })?;
        let key = reducer.key(event)?;
        let source = serde_json::to_string(&(
            "domain",
            self.context.name().as_str(),
            event.stream_id().aggregate_type().as_str(),
            event.stream_id().aggregate_id().as_str(),
        ))
        .map_err(|error| ReadModelProcessingError::InvalidEvent(error.to_string()))?;
        let version = event.stream_version().value();
        self.mutate(
            &key,
            &source,
            version,
            &DomainMutation {
                resources,
                reducer: reducer.as_ref(),
                event,
                key: &key,
            },
        )
        .await
    }
}

struct DomainMutation<'a, M: ReadModel> {
    resources: &'a Resources<M>,
    reducer: &'a dyn DomainReducer<M>,
    event: &'a RecordedEvent,
    key: &'a ReadModelKey,
}

#[async_trait]
impl<M: ReadModel> Mutation<M> for DomainMutation<'_, M> {
    async fn apply(&self, model: &mut M, applied: u64) -> Result<(), ReadModelProcessingError> {
        let Self {
            resources,
            reducer,
            event,
            key,
        } = self;
        let version = event.stream_version().value();
        if applied.checked_add(1) == Some(version) {
            return reducer.apply(model, event);
        }
        let history = resources.history.as_ref().ok_or_else(|| {
            ReadModelProcessingError::Configuration("domain history is required".to_owned())
        })?;
        let records = history.load(event.stream_id()).await.map_err(|error| {
            let kind = if error.kind() == rostfrei_core::EventStoreErrorKind::Unavailable {
                ReadModelErrorKind::Unavailable
            } else {
                ReadModelErrorKind::InvalidData
            };
            ReadModelProcessingError::Storage(ReadModelError::new(kind, error.to_string()))
        })?;
        let mut position = applied;
        for record in records.iter().filter(|record| {
            record.stream_version().value() > applied && record.stream_version().value() <= version
        }) {
            let expected = position.checked_add(1).ok_or_else(|| {
                ReadModelProcessingError::InvalidEvent("source version overflow".to_owned())
            })?;
            if record.stream_version().value() != expected {
                return Err(ReadModelProcessingError::SourceGap {
                    expected,
                    actual: record.stream_version().value(),
                });
            }
            if record.stream_id() != event.stream_id()
                || (record.stream_version().value() == version && record != *event)
            {
                return Err(ReadModelProcessingError::InvalidEvent(
                    "delivery disagrees with authoritative history".to_owned(),
                ));
            }
            if let Some(reducer) = resources.domain.get(&(
                record.stream_id().aggregate_type().as_str().to_owned(),
                record.event_type().to_owned(),
            )) && &reducer.key(record)? == *key
            {
                reducer.apply(model, record)?;
            }
            position = expected;
        }
        if position != version {
            return Err(ReadModelProcessingError::SourceGap {
                expected: position.saturating_add(1),
                actual: version,
            });
        }
        Ok(())
    }
}

pub(super) struct DomainHandler<M: ReadModel>(pub Arc<Session<M>>);

#[async_trait]
impl<M: ReadModel, E: Send + Sync> DomainEventHandler<E> for DomainHandler<M> {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, E>,
    ) -> Result<(), DomainEventHandlerError> {
        self.0.domain(event.recorded()).await.map_err(|error| {
            let kind = if error.is_retryable() {
                DomainEventHandlerErrorKind::Retryable
            } else if matches!(error, ReadModelProcessingError::InvalidEvent(_)) {
                DomainEventHandlerErrorKind::InvalidCommittedEvent
            } else {
                DomainEventHandlerErrorKind::OperatorBlocking
            };
            DomainEventHandlerError::new(kind, error.to_string())
        })
    }
}

/// Connected read model, with typed event adapters and a read-only query handle.
pub struct ReadModelRuntime<M: ReadModel> {
    pub(super) session: Arc<Session<M>>,
    pub(super) dispatcher: Arc<DomainEventDispatcher>,
    pub(super) integrations: Vec<ReadModelIntegrationBinding>,
    pub(super) has_domain: bool,
}

impl<M: ReadModel> ReadModelRuntime<M> {
    pub fn reader(&self) -> ReadModelReader<M> {
        ReadModelReader {
            session: self.session.clone(),
        }
    }
    pub fn context(&self) -> &BoundedContext {
        &self.session.context
    }
    pub const fn has_domain_events(&self) -> bool {
        self.has_domain
    }
    pub fn domain_dispatcher(&self) -> Arc<DomainEventDispatcher> {
        self.dispatcher.clone()
    }
    pub fn integration_bindings(&self) -> &[ReadModelIntegrationBinding] {
        &self.integrations
    }
    pub async fn dispatch_domain(
        &self,
        event: &RecordedEvent,
    ) -> Result<DomainEventDispatchOutcome, DomainEventHandlerError> {
        self.dispatcher.dispatch(event).await
    }
}

/// Query capability: returns business data and exposes no mutation methods.
pub struct ReadModelReader<M: ReadModel> {
    session: Arc<Session<M>>,
}

impl<M: ReadModel> Clone for ReadModelReader<M> {
    fn clone(&self) -> Self {
        Self {
            session: self.session.clone(),
        }
    }
}

impl<M: ReadModel> ReadModelReader<M> {
    pub async fn read(&self, key: &ReadModelKey) -> Result<Option<M>, ReadModelError> {
        let resources = self.session.resources.get().ok_or_else(|| {
            ReadModelError::new(
                ReadModelErrorKind::ConfigurationMismatch,
                "read model not connected",
            )
        })?;
        resources
            .store
            .read(key)
            .await?
            .map(|entry| {
                entry.value.validate()?;
                Ok(entry.value.value)
            })
            .transpose()
    }
}
