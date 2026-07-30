use std::{
    any::{Any, TypeId},
    collections::{BTreeMap, HashMap, HashSet},
    marker::PhantomData,
    sync::{Arc, Mutex, OnceLock},
};

use domain::DomainEvent;
use rostfrei_core::{Aggregate, DomainEventDispatcher, Event, EventVariant, ReadModel};
use rostfrei_messaging_core::{BoundedContext, BoundedContextName};

use super::{
    IntegrationEventOrder, ReadModelBackend, ReadModelDomainEvent, ReadModelIntegrationBinding,
    ReadModelProcessingError, ReadModelRuntime,
    integration::{IntegrationHandler, IntegrationSource},
    runtime::{DomainHandler, DomainReducers, Resources, Session, TypedDomainReducer},
};
use crate::IntegrationEvent;

/// Application-scoped materialization registry backed by an explicitly supplied adapter.
pub struct ReadModels<B: ReadModelBackend> {
    context: BoundedContext,
    backend: Arc<B>,
    registered: Arc<Mutex<HashSet<String>>>,
}

impl<B: ReadModelBackend> ReadModels<B> {
    pub fn new(context: BoundedContext, backend: B) -> Self {
        Self {
            context,
            backend: Arc::new(backend),
            registered: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn register<M: ReadModel>(&self) -> ReadModelBuilder<M, B> {
        let error = if M::SCHEMA_VERSION == 0 || BoundedContextName::new(M::NAME).is_err() {
            Some(ReadModelProcessingError::Configuration(
                "invalid read-model name or schema version".to_owned(),
            ))
        } else {
            None
        };
        ReadModelBuilder {
            backend: self.backend.clone(),
            registered: self.registered.clone(),
            context: self.context.clone(),
            session: Arc::new(Session {
                context: self.context.clone(),
                resources: OnceLock::new(),
                retries: 8,
            }),
            dispatcher: DomainEventDispatcher::new(),
            domain: BTreeMap::new(),
            integrations: Vec::new(),
            sources: HashMap::new(),
            error,
            marker: PhantomData,
        }
    }
}

pub struct NoDomainSource;

type Source = Box<dyn Any + Send + Sync>;
type BindIntegration = Box<
    dyn FnOnce(Source) -> Result<ReadModelIntegrationBinding, ReadModelProcessingError>
        + Send
        + Sync,
>;

struct PendingIntegration {
    event_type: TypeId,
    bind: BindIntegration,
}

/// Registers only typed domain/integration events. Closures are synchronous and
/// may be reevaluated after CAS conflicts; keep external effects outside them.
pub struct ReadModelBuilder<M: ReadModel, B: ReadModelBackend, A = NoDomainSource> {
    backend: Arc<B>,
    registered: Arc<Mutex<HashSet<String>>>,
    context: BoundedContext,
    session: Arc<Session<M>>,
    dispatcher: DomainEventDispatcher,
    domain: DomainReducers<M>,
    integrations: Vec<PendingIntegration>,
    sources: HashMap<TypeId, Source>,
    error: Option<ReadModelProcessingError>,
    marker: PhantomData<fn() -> A>,
}

impl<M: ReadModel, B: ReadModelBackend, A> ReadModelBuilder<M, B, A> {
    /// Select the aggregate owning subsequent domain registrations. This makes
    /// event-set membership and context ownership explicit and checkable.
    pub fn from_aggregate<Next: Aggregate>(self) -> ReadModelBuilder<M, B, Next> {
        ReadModelBuilder {
            backend: self.backend,
            registered: self.registered,
            context: self.context,
            session: self.session,
            dispatcher: self.dispatcher,
            domain: self.domain,
            integrations: self.integrations,
            sources: self.sources,
            error: self.error,
            marker: PhantomData,
        }
    }

    #[must_use]
    pub fn on_integration_event<E: IntegrationEvent>(
        self,
        key: impl Fn(&E) -> String + Send + Sync + 'static,
        change: impl Fn(&mut M, &E) + Send + Sync + 'static,
    ) -> Self {
        self.try_on_integration_event::<E>(key, move |model, event| {
            change(model, event);
            Ok(())
        })
    }

    #[must_use]
    pub fn try_on_integration_event<E: IntegrationEvent>(
        mut self,
        key: impl Fn(&E) -> String + Send + Sync + 'static,
        change: impl Fn(&mut M, &E) -> Result<(), ReadModelProcessingError> + Send + Sync + 'static,
    ) -> Self {
        if self
            .integrations
            .iter()
            .any(|input| input.event_type == TypeId::of::<E>())
        {
            self.fail("integration event handler registered twice");
            return self;
        }
        if E::SCHEMA_VERSION == 0 {
            self.fail("integration schema version must be positive");
        }
        let session = self.session.clone();
        let key = Arc::new(key);
        let change = Arc::new(change);
        self.integrations.push(PendingIntegration {
            event_type: TypeId::of::<E>(),
            bind: Box::new(move |source| {
                let source = source.downcast::<IntegrationSource<E>>().map_err(|_| {
                    ReadModelProcessingError::Configuration(
                        "integration policy type mismatch".to_owned(),
                    )
                })?;
                IntegrationHandler::bind(session, *source, key, change)
            }),
        });
        self
    }

    /// Required producer identity and per-key source-order contract. A transport
    /// message ID or delivery sequence is not inferred to be a business version.
    #[must_use]
    pub fn integration_source<E: IntegrationEvent>(
        mut self,
        context: BoundedContext,
        order: IntegrationEventOrder<E>,
    ) -> Self {
        if context.application() != self.context.application()
            || context.traffic_scope() != self.context.traffic_scope()
            || BoundedContextName::new(&order.source).is_err()
        {
            self.fail(
                "integration source must have a valid name and the same application/traffic scope",
            );
        }
        if self
            .sources
            .insert(
                TypeId::of::<E>(),
                Box::new(IntegrationSource { context, order }),
            )
            .is_some()
        {
            self.fail("integration source policy registered twice");
        }
        self
    }

    fn fail(&mut self, message: &str) {
        if self.error.is_none() {
            self.error = Some(ReadModelProcessingError::Configuration(message.to_owned()));
        }
    }

    /// Opens/verifies storage and creates adapter-ready handlers; never provisions.
    pub async fn build(mut self) -> Result<ReadModelRuntime<M>, ReadModelProcessingError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.domain.is_empty() && self.integrations.is_empty() {
            return Err(ReadModelProcessingError::Configuration(
                "read model needs at least one event handler".to_owned(),
            ));
        }
        let mut bindings = Vec::new();
        for input in self.integrations {
            let source = self.sources.remove(&input.event_type).ok_or_else(|| {
                ReadModelProcessingError::Configuration(
                    "integration handler requires an explicit integration_source ordering policy"
                        .to_owned(),
                )
            })?;
            let binding = (input.bind)(source)?;
            if bindings
                .iter()
                .any(|existing: &ReadModelIntegrationBinding| {
                    existing.address() == binding.address()
                })
            {
                return Err(ReadModelProcessingError::Configuration(
                    "multiple integration handlers use the same address".to_owned(),
                ));
            }
            bindings.push(binding);
        }
        if !self.sources.is_empty() {
            return Err(ReadModelProcessingError::Configuration(
                "integration policy has no registered handler".to_owned(),
            ));
        }
        let has_domain = !self.domain.is_empty();
        let store = self.backend.open::<M>(&self.context).await?;
        let history = if has_domain {
            Some(self.backend.history(&self.context).await?)
        } else {
            None
        };
        self.session
            .resources
            .set(Resources {
                store,
                history,
                domain: self.domain,
            })
            .map_err(|_| {
                ReadModelProcessingError::Configuration("read model connected twice".to_owned())
            })?;
        let claimed = self
            .registered
            .lock()
            .map_err(|_| {
                ReadModelProcessingError::Configuration(
                    "read-model registry unavailable".to_owned(),
                )
            })?
            .insert(M::NAME.to_owned());
        if !claimed {
            return Err(ReadModelProcessingError::Configuration(
                "read-model name already registered".to_owned(),
            ));
        }
        Ok(ReadModelRuntime {
            session: self.session,
            dispatcher: Arc::new(self.dispatcher),
            integrations: bindings,
            has_domain,
        })
    }
}

impl<M, B, A> ReadModelBuilder<M, B, A>
where
    M: ReadModel,
    B: ReadModelBackend,
    A: Aggregate + Send + Sync + 'static,
    A::Event: Event + Send + Sync + 'static,
{
    #[must_use]
    pub fn on_domain_event<E>(
        self,
        key: impl Fn(&ReadModelDomainEvent<'_, E>) -> String + Send + Sync + 'static,
        change: impl Fn(&mut M, &E) + Send + Sync + 'static,
    ) -> Self
    where
        E: DomainEvent + Send + Sync,
        A::Event: EventVariant<E>,
    {
        self.try_on_domain_event::<E>(key, move |model, event| {
            change(model, event);
            Ok(())
        })
    }

    #[must_use]
    pub fn try_on_domain_event<E>(
        mut self,
        key: impl Fn(&ReadModelDomainEvent<'_, E>) -> String + Send + Sync + 'static,
        change: impl Fn(&mut M, &E) -> Result<(), ReadModelProcessingError> + Send + Sync + 'static,
    ) -> Self
    where
        E: DomainEvent + Send + Sync,
        A::Event: EventVariant<E>,
    {
        if A::BOUNDED_CONTEXT != self.context.name().as_str() {
            self.fail("domain aggregate belongs to another bounded context");
        }
        let reducer = TypedDomainReducer::<M, A, E> {
            key: Arc::new(key),
            change: Arc::new(change),
            marker: PhantomData,
        };
        let identity = (A::aggregate_type().into_owned(), E::LOCAL_ID.to_owned());
        if self.domain.insert(identity, Arc::new(reducer)).is_some() {
            self.fail("domain event handler registered twice");
        }
        if let Err(error) = self
            .dispatcher
            .register::<A, E, _>(E::LOCAL_ID, Arc::new(DomainHandler(self.session.clone())))
        {
            self.fail(&error.to_string());
        }
        self
    }
}
