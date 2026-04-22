use std::sync::Arc;

use async_trait::async_trait;
use rostfrei::{
    CommandBus, CommandBusErrorKind, CommandBusObserver, CommandPublication,
    CommandResponseOutcome, DynamicCommandRequest,
};

use crate::{
    DispatchAdapter, DispatchError, DispatchErrorKind, DispatchInvocation, DispatchObserver,
    DispatchPublication, DispatchReceipt, DispatchRejection,
};

/// Adapts the control plane's raw JSON invocation to the transport-neutral command bus.
pub struct CommandBusDispatchAdapter {
    bus: CommandBus,
}

impl CommandBusDispatchAdapter {
    pub const fn new(bus: CommandBus) -> Self {
        Self { bus }
    }
}

struct DispatchObserverBridge {
    observer: Arc<dyn DispatchObserver>,
}

#[async_trait]
impl CommandBusObserver for DispatchObserverBridge {
    async fn published(&self, publication: CommandPublication) {
        self.observer
            .command_published(DispatchPublication::new(
                publication.message_id().as_str(),
                publication.duplicate(),
            ))
            .await;
    }
}

#[async_trait]
impl DispatchAdapter for CommandBusDispatchAdapter {
    fn maximum_payload_len(&self) -> usize {
        self.bus.maximum_payload_len()
    }

    async fn dispatch(
        &self,
        invocation: DispatchInvocation,
        observer: Arc<dyn DispatchObserver>,
    ) -> Result<DispatchReceipt, DispatchError> {
        let request = DynamicCommandRequest::new(
            invocation.operation_id().clone(),
            invocation.aggregate_type(),
            invocation.aggregate_id().clone(),
            invocation.command(),
            invocation.schema_version(),
            invocation.payload().clone(),
        )
        .map_err(|error| dispatch_error(&error))?;
        let receipt = self
            .bus
            .dispatch_dynamic_observed(request, Arc::new(DispatchObserverBridge { observer }))
            .await
            .map_err(|error| dispatch_error(&error))?;
        let publication_duplicate = receipt.publication_duplicate();
        let response = receipt.into_response();
        let command_message_id = response.command_message_id().as_str().to_owned();
        let response_message_id = response.message_id().as_str().to_owned();
        match response.into_outcome() {
            CommandResponseOutcome::Accepted => Ok(DispatchReceipt::accepted(
                command_message_id,
                response_message_id,
                publication_duplicate,
            )),
            CommandResponseOutcome::Rejected(rejection) => {
                let rejection = serde_json::from_value::<DispatchRejection>(
                    serde_json::to_value(rejection).map_err(|error| {
                        DispatchError::new(DispatchErrorKind::InvalidResponse, error.to_string())
                    })?,
                )
                .map_err(|error| {
                    DispatchError::new(DispatchErrorKind::InvalidResponse, error.to_string())
                })?;
                Ok(DispatchReceipt::rejected(
                    command_message_id,
                    response_message_id,
                    publication_duplicate,
                    rejection,
                ))
            }
        }
    }
}

fn dispatch_error(error: &rostfrei::CommandBusError) -> DispatchError {
    let kind = match error.kind() {
        CommandBusErrorKind::Encoding | CommandBusErrorKind::InvalidMessage => {
            DispatchErrorKind::InvalidRequest
        }
        CommandBusErrorKind::Timeout => DispatchErrorKind::Timeout,
        CommandBusErrorKind::InvalidConfiguration => DispatchErrorKind::InvalidConfiguration,
        CommandBusErrorKind::InvalidResponse => DispatchErrorKind::InvalidResponse,
        _ => DispatchErrorKind::Unavailable,
    };
    DispatchError::new(kind, error.to_string())
}
