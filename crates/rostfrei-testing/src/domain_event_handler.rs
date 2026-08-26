use std::sync::Arc;

use rostfrei_core::{
    Aggregate, DomainEventDispatchOutcome, DomainEventDispatcher, DomainEventHandler,
    DomainEventHandlerError, DomainEventRegistrationError, EventCodec, RecordedEvent,
};

#[derive(Default)]
pub struct DomainEventHandlerHarness {
    dispatcher: DomainEventDispatcher,
}

impl DomainEventHandlerHarness {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<A, C, H>(
        &mut self,
        event_type: impl Into<String>,
        codec: Arc<C>,
        handler: Arc<H>,
    ) -> Result<(), DomainEventRegistrationError>
    where
        A: Aggregate + Send + Sync + 'static,
        A::Event: Send + Sync + 'static,
        C: EventCodec<A> + 'static,
        H: DomainEventHandler<A::Event> + 'static,
    {
        self.dispatcher
            .register::<A, C, H>(event_type, codec, handler)
    }

    pub async fn handle(
        &self,
        event: &RecordedEvent,
    ) -> Result<DomainEventDispatchOutcome, DomainEventHandlerError> {
        self.dispatcher.dispatch(event).await
    }
}
