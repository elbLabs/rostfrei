//! Independent lossless typed technical-error bridge control, not production changes.
#![allow(clippy::panic_in_result_fn)]
use async_trait::async_trait;
use rostfrei::{
    CodecFailureKind, CommandBus, CommandBusError, CommandBusObserver, CommandBusReceipt,
    CommandMessageAdapter, EncodedCommand, ExecutionFailure, ProcessorFailureKind,
    StoreFailureKind,
};
use rostfrei_tracer::{
    CommandBusTransport, CommandInvocation, CommandPublication, CommandTransport,
    CommandTransportErrorKind, CommandTransportObserver,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct TechnicalAdapter(ExecutionFailure);
#[async_trait]
impl CommandMessageAdapter for TechnicalAdapter {
    async fn dispatch(
        &self,
        command: EncodedCommand,
        observer: Arc<dyn CommandBusObserver>,
    ) -> Result<CommandBusReceipt, CommandBusError> {
        observer
            .published(rostfrei::CommandPublication::new(
                command.message_id().clone(),
                false,
            ))
            .await;
        Err(CommandBusError::execution_failed(self.0))
    }
}
struct Observer(Arc<AtomicUsize>);
#[async_trait]
impl CommandTransportObserver for Observer {
    async fn command_published(&self, _: CommandPublication) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn bridge_preserves_all_technical_sources_and_does_not_turn_blocked_failures_into_unavailable()
-> Result<(), Box<dyn std::error::Error>> {
    for failure in [
        ExecutionFailure::Store(StoreFailureKind::CorruptHistory),
        ExecutionFailure::Store(StoreFailureKind::CapacityExhausted),
        ExecutionFailure::Store(StoreFailureKind::ConfigurationMismatch),
        ExecutionFailure::Store(StoreFailureKind::Conflict),
        ExecutionFailure::Store(StoreFailureKind::Unavailable),
        ExecutionFailure::Codec(CodecFailureKind::UnknownEventType),
        ExecutionFailure::Codec(CodecFailureKind::UnsupportedSchemaVersion),
        ExecutionFailure::Codec(CodecFailureKind::MalformedPayload),
        ExecutionFailure::Codec(CodecFailureKind::InvalidEnvelope),
        ExecutionFailure::Codec(CodecFailureKind::EncodingFailed),
        ExecutionFailure::Processor(ProcessorFailureKind::InvalidConfiguration),
    ] {
        let context = rostfrei_messaging_core::ApplicationName::new("review-tracer")?
            .bounded_context("orders")?;
        let transport = CommandBusTransport::new(CommandBus::new(
            context,
            Arc::new(TechnicalAdapter(failure)),
        ));
        let payload = serde_json::json!({"order":"one"});
        let invocation = CommandInvocation::new(
            rostfrei_core::OperationId::new("operation")?,
            "correlation",
            rostfrei_tracer::command_execution_fingerprint("orders", "place-order", 1, &payload),
            "orders",
            "place-order",
            1,
            payload,
        );
        let observed = Arc::new(AtomicUsize::new(0));
        let result = transport
            .invoke(invocation, Arc::new(Observer(observed.clone())))
            .await;
        assert!(result.is_err());
        let error = result
            .err()
            .ok_or("technical failure became successful business processing")?;
        assert_eq!(error.execution_failure(), Some(failure));
        assert_eq!(
            error.kind(),
            if failure.is_retryable() {
                CommandTransportErrorKind::Unavailable
            } else {
                CommandTransportErrorKind::ExecutionFailed
            }
        );
        assert_eq!(observed.load(Ordering::SeqCst), 1);
    }
    Ok(())
}
