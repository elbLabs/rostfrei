use std::sync::Arc;

use async_trait::async_trait;
use rostfrei::{
    CommandBus, CommandBusErrorKind, CommandBusObserver,
    CommandPublication as BusCommandPublication, CommandResponseOutcome, CorrelationId,
    DynamicCommandRequest,
};

use crate::{
    CommandInvocation, CommandPublication, CommandReceipt, CommandRejection, CommandTransport,
    CommandTransportError, CommandTransportErrorKind, CommandTransportObserver,
};

/// Adapts the tracer's command transport contract to a transport-neutral command bus.
pub struct CommandBusTransportAdapter {
    bus: CommandBus,
}

impl CommandBusTransportAdapter {
    pub const fn new(bus: CommandBus) -> Self {
        Self { bus }
    }
}

struct CommandTransportObserverBridge {
    observer: Arc<dyn CommandTransportObserver>,
}

#[async_trait]
impl CommandBusObserver for CommandTransportObserverBridge {
    async fn published(&self, publication: BusCommandPublication) {
        self.observer
            .command_published(CommandPublication::new(
                publication.message_id().as_str(),
                publication.duplicate(),
            ))
            .await;
    }
}

#[async_trait]
impl CommandTransport for CommandBusTransportAdapter {
    fn maximum_payload_len(&self) -> usize {
        self.bus.maximum_payload_len()
    }

    async fn invoke(
        &self,
        invocation: CommandInvocation,
        observer: Arc<dyn CommandTransportObserver>,
    ) -> Result<CommandReceipt, CommandTransportError> {
        if invocation
            .aggregate_type()
            .split_once('/')
            .is_none_or(|(context, _)| context != self.bus.context().name().as_str())
        {
            return Err(CommandTransportError::new(
                CommandTransportErrorKind::InvalidRequest,
                "command aggregate type does not belong to the transport bounded context",
            ));
        }
        let execution_fingerprint = rostfrei::command_execution_fingerprint(
            invocation.aggregate_type(),
            invocation.aggregate_id().as_str(),
            invocation.command(),
            invocation.schema_version(),
            invocation.payload(),
        )
        .map_err(|error| command_transport_error(&error))?;
        if invocation.execution_fingerprint() != execution_fingerprint {
            return Err(CommandTransportError::new(
                CommandTransportErrorKind::InvalidRequest,
                "command invocation execution fingerprint does not match its content",
            ));
        }
        let correlation_id = CorrelationId::new(invocation.correlation_id()).map_err(|error| {
            CommandTransportError::new(CommandTransportErrorKind::InvalidRequest, error.to_string())
        })?;
        let request = DynamicCommandRequest::new(
            invocation.operation_id().clone(),
            invocation.aggregate_type(),
            invocation.aggregate_id().clone(),
            invocation.command(),
            invocation.schema_version(),
            invocation.payload().clone(),
        )
        .map_err(|error| command_transport_error(&error))?
        .with_correlation_id(correlation_id);
        let receipt = self
            .bus
            .dispatch_dynamic_observed(
                request,
                Arc::new(CommandTransportObserverBridge { observer }),
            )
            .await
            .map_err(|error| command_transport_error(&error))?;
        let publication_duplicate = receipt.publication_duplicate();
        let response = receipt.into_response();
        let command_message_id = response.command_message_id().as_str().to_owned();
        let response_message_id = response.message_id().as_str().to_owned();
        match response.into_outcome() {
            CommandResponseOutcome::Accepted => Ok(CommandReceipt::accepted(
                command_message_id,
                response_message_id,
                publication_duplicate,
            )),
            CommandResponseOutcome::Rejected(rejection) => {
                let rejection = serde_json::from_value::<CommandRejection>(
                    serde_json::to_value(rejection).map_err(|error| {
                        CommandTransportError::new(
                            CommandTransportErrorKind::InvalidResponse,
                            error.to_string(),
                        )
                    })?,
                )
                .map_err(|error| {
                    CommandTransportError::new(
                        CommandTransportErrorKind::InvalidResponse,
                        error.to_string(),
                    )
                })?;
                Ok(CommandReceipt::rejected(
                    command_message_id,
                    response_message_id,
                    publication_duplicate,
                    rejection,
                ))
            }
        }
    }
}

fn command_transport_error(error: &rostfrei::CommandBusError) -> CommandTransportError {
    let kind = match error.kind() {
        CommandBusErrorKind::Encoding | CommandBusErrorKind::InvalidMessage => {
            CommandTransportErrorKind::InvalidRequest
        }
        CommandBusErrorKind::Timeout => CommandTransportErrorKind::Timeout,
        CommandBusErrorKind::InvalidConfiguration => {
            CommandTransportErrorKind::InvalidConfiguration
        }
        CommandBusErrorKind::InvalidResponse => CommandTransportErrorKind::InvalidResponse,
        _ => CommandTransportErrorKind::Unavailable,
    };
    CommandTransportError::new(kind, error.to_string())
}
