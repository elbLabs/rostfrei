use std::sync::Arc;

use async_trait::async_trait;
use rostfrei::{
    CommandBus, CommandBusErrorKind, CommandBusObserver, CommandPublication as BusPublication,
    CommandResponseOutcome, CorrelationId, DynamicCommandRequest,
};

use crate::{
    CommandInvocation, CommandPublication, CommandReceipt, CommandRejection, CommandTransport,
    CommandTransportError, CommandTransportErrorKind, CommandTransportObserver,
};

const COMMAND_ENVELOPE_OVERHEAD: usize = 64 * 1024;

/// Adapts Tracer command invocations to the transport-neutral command bus.
pub struct CommandBusTransport {
    bus: CommandBus,
}

impl CommandBusTransport {
    pub const fn new(bus: CommandBus) -> Self {
        Self { bus }
    }
}

struct TransportObserverBridge {
    observer: Arc<dyn CommandTransportObserver>,
}

#[async_trait]
impl CommandBusObserver for TransportObserverBridge {
    async fn published(&self, publication: BusPublication) {
        self.observer
            .command_published(CommandPublication::new(
                publication.message_id().as_str(),
                publication.duplicate(),
            ))
            .await;
    }
}

#[async_trait]
impl CommandTransport for CommandBusTransport {
    fn maximum_payload_len(&self) -> usize {
        self.bus
            .maximum_payload_len()
            .saturating_sub(COMMAND_ENVELOPE_OVERHEAD)
    }

    async fn invoke(
        &self,
        invocation: CommandInvocation,
        observer: Arc<dyn CommandTransportObserver>,
    ) -> Result<CommandReceipt, CommandTransportError> {
        let expected_fingerprint = rostfrei::command_execution_fingerprint(
            invocation.aggregate_type(),
            invocation.aggregate_id().as_str(),
            invocation.command(),
            invocation.schema_version(),
            invocation.payload(),
        )
        .map_err(|error| command_bus_error(&error))?;
        if expected_fingerprint != invocation.execution_fingerprint() {
            return Err(CommandTransportError::new(
                CommandTransportErrorKind::InvalidRequest,
                "command invocation execution fingerprint is invalid",
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
        .map_err(|error| command_bus_error(&error))?
        .with_correlation_id(correlation_id);
        let receipt = self
            .bus
            .dispatch_dynamic_observed(request, Arc::new(TransportObserverBridge { observer }))
            .await
            .map_err(|error| command_bus_error(&error))?;
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

fn command_bus_error(error: &rostfrei::CommandBusError) -> CommandTransportError {
    let kind = match error.kind() {
        CommandBusErrorKind::Encoding | CommandBusErrorKind::InvalidMessage => {
            CommandTransportErrorKind::InvalidRequest
        }
        CommandBusErrorKind::Timeout => CommandTransportErrorKind::Timeout,
        CommandBusErrorKind::Rejected => CommandTransportErrorKind::Rejected,
        CommandBusErrorKind::InvalidConfiguration => {
            CommandTransportErrorKind::InvalidConfiguration
        }
        CommandBusErrorKind::InvalidResponse => CommandTransportErrorKind::InvalidResponse,
        _ => CommandTransportErrorKind::Unavailable,
    };
    CommandTransportError::new(kind, error.to_string())
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use std::error::Error;

    use rostfrei::{CommandBusError, CommandBusReceipt, CommandMessageAdapter, EncodedCommand};
    use rostfrei_messaging_core::ApplicationName;

    use super::*;

    struct MaximumMessageAdapter;

    #[async_trait]
    impl CommandMessageAdapter for MaximumMessageAdapter {
        async fn dispatch(
            &self,
            _command: EncodedCommand,
            _observer: Arc<dyn CommandBusObserver>,
        ) -> Result<CommandBusReceipt, CommandBusError> {
            Err(CommandBusError::new(
                CommandBusErrorKind::Unavailable,
                "unused test adapter",
            ))
        }
    }

    #[test]
    fn raw_payload_limit_reserves_command_envelope_capacity() -> Result<(), Box<dyn Error>> {
        let context = ApplicationName::new("tracer-test")?.bounded_context("orders")?;
        let adapter: Arc<dyn CommandMessageAdapter> = Arc::new(MaximumMessageAdapter);
        let transport = CommandBusTransport::new(CommandBus::new(context, adapter));

        assert_eq!(
            transport.maximum_payload_len(),
            (1024 * 1024) - COMMAND_ENVELOPE_OVERHEAD
        );
        Ok(())
    }

    #[test]
    fn broker_rejections_remain_transport_rejections() {
        let error = command_bus_error(&CommandBusError::new(
            CommandBusErrorKind::Rejected,
            "publication rejected",
        ));

        assert_eq!(error.kind(), CommandTransportErrorKind::Rejected);
    }
}
