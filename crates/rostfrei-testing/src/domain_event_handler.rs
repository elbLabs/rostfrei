use std::sync::Arc;

use rostfrei_core::{
    Aggregate, DomainEventDispatchOutcome, DomainEventDispatcher, DomainEventHandler,
    DomainEventHandlerError, DomainEventRegistrationError, Event, EventCodec, EventVariant,
    RecordedEvent,
};

#[derive(Default)]
pub struct DomainEventHandlerHarness {
    dispatcher: DomainEventDispatcher,
}

impl DomainEventHandlerHarness {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<A, E, H>(
        &mut self,
        event_type: impl Into<String>,
        handler: Arc<H>,
    ) -> Result<(), DomainEventRegistrationError>
    where
        A: Aggregate + Send + Sync + 'static,
        A::Event: Event + EventVariant<E> + Send + Sync + 'static,
        E: Send + Sync + 'static,
        H: DomainEventHandler<E> + 'static,
    {
        self.dispatcher.register::<A, E, H>(event_type, handler)
    }

    pub fn register_with_codec<A, E, C, H>(
        &mut self,
        event_type: impl Into<String>,
        codec: Arc<C>,
        handler: Arc<H>,
    ) -> Result<(), DomainEventRegistrationError>
    where
        A: Aggregate + Send + Sync + 'static,
        A::Event: EventVariant<E> + Send + Sync + 'static,
        E: Send + Sync + 'static,
        C: EventCodec<A> + 'static,
        H: DomainEventHandler<E> + 'static,
    {
        self.dispatcher
            .register_with_codec::<A, E, C, H>(event_type, codec, handler)
    }

    pub async fn handle(
        &self,
        event: &RecordedEvent,
    ) -> Result<DomainEventDispatchOutcome, DomainEventHandlerError> {
        self.dispatcher.dispatch(event).await
    }
}
