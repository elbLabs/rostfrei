use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::Duration,
};

use async_trait::async_trait;
use rostfrei::{
    AuthorizationError, CommandBusError, CommandBusErrorKind, CommandBusObserver,
    CommandBusReceipt, CommandMessageAdapter, CommandProcessingService, CommandProcessor,
    CommandProcessorError, CommandPublication, CompletionOrigin, EncodedCommand,
    EncodedIntegrationMessage, IntegrationEventBusError, IntegrationEventBusErrorKind,
    IntegrationMessageAdapter, OutcomeLookup, ProcessedCommand, TrustedCommandContext,
};
use rostfrei_messaging_core::{
    CommandAddress, CommandPublisher, CommandResponse, CommandResponseOutcome,
    CommandResponsePublisher, CommandResponseReadErrorKind, CommandResponseReader,
    DeliveryDisposition, IntegrationEventPublisher, MessageDelivery, MessageHandler,
    OutboundMessage, PublishError, PublishErrorKind, PublishReceipt, QuarantineReason, RetryDelay,
};
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::{NatsCommandResponseReader, NatsPublisher};

const MAXIMUM_PUBLISH_ATTEMPTS: usize = 3;
const PUBLISH_RETRY_DELAY: Duration = Duration::from_millis(100);
const RESPONSE_READ_SLICE: Duration = Duration::from_secs(1);
const RESPONSE_RECONCILIATION_TIMEOUT: Duration = Duration::from_millis(100);
const DELIVERY_RETRY_DELAY: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub struct NatsMessagingAdapter {
    outcome_service: Option<(Arc<dyn CommandProcessingService>, TrustedCommandContext)>,
    replay_publication: bool,
    command_publisher: Arc<dyn CommandPublisher>,
    response_publisher: Arc<dyn CommandResponsePublisher>,
    integration_publisher: Arc<dyn IntegrationEventPublisher>,
    response_reader: Arc<dyn CommandResponseReader>,
    response_timeout: Option<Duration>,
    native_response_publisher: Option<Arc<NatsPublisher>>,
}

impl NatsMessagingAdapter {
    pub fn new(publisher: NatsPublisher, response_reader: NatsCommandResponseReader) -> Self {
        let publisher = Arc::new(publisher);
        let native_response_publisher = Some(Arc::clone(&publisher));
        let command_publisher: Arc<dyn CommandPublisher> = publisher.clone();
        let response_publisher: Arc<dyn CommandResponsePublisher> = publisher.clone();
        let integration_publisher: Arc<dyn IntegrationEventPublisher> = publisher;
        let response_reader: Arc<dyn CommandResponseReader> = Arc::new(response_reader);
        Self {
            outcome_service: None,
            replay_publication: false,
            command_publisher,
            response_publisher,
            integration_publisher,
            response_reader,
            response_timeout: None,
            native_response_publisher,
        }
    }

    #[cfg(test)]
    fn with_components(
        command_publisher: Arc<dyn CommandPublisher>,
        response_publisher: Arc<dyn CommandResponsePublisher>,
        integration_publisher: Arc<dyn IntegrationEventPublisher>,
        response_reader: Arc<dyn CommandResponseReader>,
    ) -> Self {
        Self {
            outcome_service: None,
            replay_publication: false,
            command_publisher,
            response_publisher,
            integration_publisher,
            response_reader,
            response_timeout: None,
            native_response_publisher: None,
        }
    }

    #[must_use]
    pub const fn with_response_timeout(mut self, response_timeout: Duration) -> Self {
        self.response_timeout = Some(response_timeout);
        self
    }

    /// Requires actual command publication even when authorized admission finds a winner.
    ///
    /// With [`Self::with_outcome_service`], current authorization and outcome-detail
    /// lifecycle checks still precede publication. The original command is published
    /// through the normal bounded retry path; the observer and receipt use its actual
    /// broker acknowledgement's duplicate flag. After the awaited publication callback,
    /// current authority is consulted again, independently of the raw response's lifetime.
    /// If no winner is available, response reading and subsequent authoritative admission
    /// proceed normally. Native workers reconcile completed commands without reevaluating
    /// their handlers.
    ///
    /// By default, authorized completed replay returns without publication or an observer
    /// callback. This option does not itself configure an outcome service or caller.
    #[must_use]
    pub const fn with_replay_publication(mut self) -> Self {
        self.replay_publication = true;
        self
    }

    pub fn command_handler(&self, processor: Arc<CommandProcessor>) -> NatsCommandHandler {
        let mut handler = NatsCommandHandler::new(
            processor,
            Arc::clone(&self.response_publisher),
            Arc::clone(&self.response_reader),
        );
        handler
            .native_response_publisher
            .clone_from(&self.native_response_publisher);
        handler
    }

    /// Authorized caller admission. The trusted context is supplied by the application,
    /// never derived from transport metadata. Each invocation consults current authority.
    #[must_use]
    pub fn with_outcome_service(
        mut self,
        service: Arc<dyn CommandProcessingService>,
        caller: TrustedCommandContext,
    ) -> Self {
        self.outcome_service = Some((service, caller));
        self
    }

    pub fn command_handler_with_resolver(
        &self,
        service: Arc<dyn CommandProcessingService>,
        resolver: Arc<dyn TrustedCommandContextResolver>,
    ) -> NatsCommandHandler {
        NatsCommandHandler {
            processor: HandlerProcessor::Authorized { service, resolver },
            response_publisher: self.response_publisher.clone(),
            response_reader: self.response_reader.clone(),
            execution_gates: CommandExecutionGates::default(),
            native_response_publisher: self.native_response_publisher.clone(),
        }
    }

    async fn authorized_response(
        &self,
        command: &EncodedCommand,
    ) -> Result<Option<CommandResponse>, CommandBusError> {
        let Some((service, caller)) = &self.outcome_service else {
            return Ok(None);
        };
        authorized_lookup(service.as_ref(), caller, command)
            .await
            .map_err(|e| CommandBusError::from_processor(&e))
    }

    async fn publish_command(
        &self,
        command: &EncodedCommand,
    ) -> Result<PublishReceipt, CommandBusError> {
        let mut attempts_remaining = MAXIMUM_PUBLISH_ATTEMPTS;
        loop {
            match self
                .command_publisher
                .publish_command(command.message().clone())
                .await
            {
                Ok(receipt) => return Ok(receipt),
                Err(error)
                    if attempts_remaining > 1
                        && matches!(
                            error.kind(),
                            PublishErrorKind::Timeout | PublishErrorKind::Unavailable
                        ) =>
                {
                    attempts_remaining = attempts_remaining.saturating_sub(1);
                    tokio::time::sleep(PUBLISH_RETRY_DELAY).await;
                }
                Err(error) => return Err(command_publish_error(error)),
            }
        }
    }

    async fn read_response(
        &self,
        command: &EncodedCommand,
    ) -> Result<CommandResponse, CommandBusError> {
        let address = command.response_address().map_err(|error| {
            CommandBusError::new(CommandBusErrorKind::InvalidConfiguration, error.to_string())
        })?;
        loop {
            match self
                .response_reader
                .read_command_response(
                    &address,
                    command.operation_id(),
                    command.message_id(),
                    RESPONSE_READ_SLICE,
                )
                .await
            {
                Ok(response) => return Ok(response),
                Err(error) if error.kind() == CommandResponseReadErrorKind::Timeout => {}
                Err(error) if error.kind() == CommandResponseReadErrorKind::Unavailable => {
                    tokio::time::sleep(PUBLISH_RETRY_DELAY).await;
                }
                Err(error) => {
                    return Err(CommandBusError::new(
                        match error.kind() {
                            CommandResponseReadErrorKind::InvalidConfiguration => {
                                CommandBusErrorKind::InvalidConfiguration
                            }
                            _ => CommandBusErrorKind::InvalidResponse,
                        },
                        error.to_string(),
                    ));
                }
            }
        }
    }

    async fn read_response_with_timeout(
        &self,
        command: &EncodedCommand,
    ) -> Result<CommandResponse, CommandBusError> {
        let Some(timeout) = self.response_timeout else {
            return self.read_response(command).await;
        };
        tokio::time::timeout(timeout, self.read_response(command))
            .await
            .map_err(|_| {
                CommandBusError::new(
                    CommandBusErrorKind::Timeout,
                    "timed out waiting for the durable command response",
                )
            })?
    }
}

#[async_trait]
impl CommandMessageAdapter for NatsMessagingAdapter {
    async fn dispatch(
        &self,
        command: EncodedCommand,
        observer: Arc<dyn CommandBusObserver>,
    ) -> Result<CommandBusReceipt, CommandBusError> {
        command
            .validate()
            .map_err(|e| CommandBusError::from_processor(&e))?;
        if let Some(winner) = self.authorized_response(&command).await?
            && !self.replay_publication
        {
            return Ok(CommandBusReceipt::new(true, winner));
        }
        let publication = self.publish_command(&command).await?;
        observer
            .published(CommandPublication::new(
                command.message_id().clone(),
                publication.duplicate(),
            ))
            .await;
        // A duplicate PubAck may enqueue no delivery to recreate an expired response.
        // Admit current authority before depending on that transport projection.
        if self.replay_publication
            && let Some(winner) = self.authorized_response(&command).await?
        {
            return Ok(CommandBusReceipt::new(publication.duplicate(), winner));
        }
        let response = self.read_response_with_timeout(&command).await?;
        if self.outcome_service.is_some() {
            if let Some(winner) = self.authorized_response(&command).await? {
                return Ok(CommandBusReceipt::new(publication.duplicate(), winner));
            }
            if !matches!(
                response.outcome(),
                CommandResponseOutcome::ExecutionFailed(_)
            ) {
                return Err(CommandBusError::new(
                    CommandBusErrorKind::CompletedDetailsBlocked,
                    "transport result has no authorized authoritative winner",
                ));
            }
        }
        command.validate_response(&response).map_err(|error| {
            CommandBusError::new(CommandBusErrorKind::InvalidResponse, error.to_string())
        })?;
        if let CommandResponseOutcome::ExecutionFailed(failure) = response.outcome() {
            return Err(CommandBusError::execution_failed(*failure));
        }
        Ok(CommandBusReceipt::new(publication.duplicate(), response))
    }
}

#[async_trait]
impl IntegrationMessageAdapter for NatsMessagingAdapter {
    async fn publish(
        &self,
        message: EncodedIntegrationMessage,
    ) -> Result<PublishReceipt, IntegrationEventBusError> {
        let mut attempts_remaining = MAXIMUM_PUBLISH_ATTEMPTS;
        loop {
            match self
                .integration_publisher
                .publish_integration_event(message.message().clone())
                .await
            {
                Ok(receipt) => return Ok(receipt),
                Err(error)
                    if attempts_remaining > 1
                        && matches!(
                            error.kind(),
                            PublishErrorKind::Timeout | PublishErrorKind::Unavailable
                        ) =>
                {
                    attempts_remaining = attempts_remaining.saturating_sub(1);
                    tokio::time::sleep(PUBLISH_RETRY_DELAY).await;
                }
                Err(error) => return Err(integration_publish_error(error)),
            }
        }
    }
}

pub struct NatsCommandHandler {
    processor: HandlerProcessor,
    response_publisher: Arc<dyn CommandResponsePublisher>,
    response_reader: Arc<dyn CommandResponseReader>,
    execution_gates: CommandExecutionGates,
    native_response_publisher: Option<Arc<NatsPublisher>>,
}

#[async_trait]
pub trait TrustedCommandContextResolver: Send + Sync {
    async fn resolve(
        &self,
        delivery: &MessageDelivery<CommandAddress>,
    ) -> Result<TrustedCommandContext, AuthorizationError>;
}

/// Explicit service-principal profile for already trusted infrastructure deliveries.
///
/// Producers on this boundary are trusted to assert original actor attribution.
/// This resolver does not authenticate the actor or prove that a named person acted;
/// a resolver/authorizer with application evidence must enforce any such restriction.
pub struct ServicePrincipalContext(pub TrustedCommandContext);
#[async_trait]
impl TrustedCommandContextResolver for ServicePrincipalContext {
    async fn resolve(
        &self,
        _: &MessageDelivery<CommandAddress>,
    ) -> Result<TrustedCommandContext, AuthorizationError> {
        Ok(self.0.clone())
    }
}
enum HandlerProcessor {
    Trusted(Arc<CommandProcessor>),
    Authorized {
        service: Arc<dyn CommandProcessingService>,
        resolver: Arc<dyn TrustedCommandContextResolver>,
    },
}

async fn authorized_lookup(
    service: &dyn CommandProcessingService,
    caller: &TrustedCommandContext,
    command: &EncodedCommand,
) -> Result<Option<CommandResponse>, CommandProcessorError> {
    match service.lookup(caller, command).await.into_result()? {
        OutcomeLookup::Missing => Ok(None),
        OutcomeLookup::Completed(outcome) => Ok(Some(
            ProcessedCommand::new(CompletionOrigin::Replayed, outcome)
                .admit(caller, command)?
                .clone(),
        )),
    }
}

impl NatsCommandHandler {
    fn new(
        processor: Arc<CommandProcessor>,
        response_publisher: Arc<dyn CommandResponsePublisher>,
        response_reader: Arc<dyn CommandResponseReader>,
    ) -> Self {
        Self {
            processor: HandlerProcessor::Trusted(processor),
            response_publisher,
            response_reader,
            execution_gates: CommandExecutionGates::default(),
            native_response_publisher: None,
        }
    }

    async fn reconcile_completed(
        &self,
        caller: Option<&TrustedCommandContext>,
        command: &EncodedCommand,
    ) -> Result<Option<CommandResponse>, CommandProcessorError> {
        match (&self.processor, caller) {
            (HandlerProcessor::Trusted(processor), _) => {
                processor.reconcile_completed(command).await
            }
            (HandlerProcessor::Authorized { service, .. }, Some(caller)) => {
                authorized_lookup(service.as_ref(), caller, command).await
            }
            _ => Err(CommandProcessorError::authorization(
                AuthorizationError::InvalidConfiguration,
            )),
        }
    }
    async fn process(
        &self,
        caller: Option<&TrustedCommandContext>,
        command: &EncodedCommand,
    ) -> Result<CommandResponse, CommandProcessorError> {
        match (&self.processor, caller) {
            (HandlerProcessor::Trusted(processor), _) => processor.process(command).await,
            (HandlerProcessor::Authorized { service, .. }, Some(caller)) => Ok(service
                .process(caller, command)
                .await
                .into_result()?
                .admit(caller, command)?
                .clone()),
            _ => Err(CommandProcessorError::authorization(
                AuthorizationError::InvalidConfiguration,
            )),
        }
    }

    async fn reconcile_response(
        &self,
        command: &EncodedCommand,
    ) -> Result<Option<CommandResponse>, ReconciliationError> {
        let address = command
            .response_address()
            .map_err(|_| ReconciliationError::Invalid)?;
        match self
            .response_reader
            .find_command_response(
                &address,
                command.operation_id(),
                command.message_id(),
                RESPONSE_RECONCILIATION_TIMEOUT,
            )
            .await
        {
            Ok(Some(response)) => {
                command
                    .validate_response(&response)
                    .map_err(|_| ReconciliationError::Invalid)?;
                Ok(Some(response))
            }
            Ok(None) => Ok(None),
            Err(error)
                if matches!(
                    error.kind(),
                    CommandResponseReadErrorKind::Timeout
                        | CommandResponseReadErrorKind::Unavailable
                ) =>
            {
                Err(ReconciliationError::Unavailable)
            }
            Err(_) => Err(ReconciliationError::Invalid),
        }
    }

    async fn publish_response(
        &self,
        command: &EncodedCommand,
        response: &CommandResponse,
    ) -> Result<PublishReceipt, PublishError> {
        let address = command
            .response_address()
            .map_err(|_| PublishError::new(PublishErrorKind::InvalidConfiguration))?;
        let message = OutboundMessage::json(address, response.message_id().clone(), response)
            .map_err(|_| PublishError::new(PublishErrorKind::InvalidConfiguration))?;
        // A single processing attempt retains one CAS token/payload across uncertain retries.
        // Only a later source re-delivery/re-evaluation may prepare a fresh transition.
        let prepared = match &self.native_response_publisher {
            Some(publisher) => Some(
                publisher
                    .prepare_command_response(message.clone(), publisher.publish_timeout())
                    .await
                    .map_err(|error| crate::publish::core_publish_error(&error))?,
            ),
            None => None,
        };
        let mut attempts_remaining = MAXIMUM_PUBLISH_ATTEMPTS;
        loop {
            let result = match (&self.native_response_publisher, &prepared) {
                (Some(publisher), Some(prepared)) => publisher
                    .publish_prepared_command_response(prepared, publisher.publish_timeout())
                    .await
                    .map(|ack| PublishReceipt::new(ack.duplicate()))
                    .map_err(|error| crate::publish::core_publish_error(&error)),
                _ => {
                    self.response_publisher
                        .publish_command_response(message.clone())
                        .await
                }
            };
            match result {
                Ok(receipt) => return Ok(receipt),
                Err(error)
                    if attempts_remaining > 1
                        && matches!(
                            error.kind(),
                            PublishErrorKind::Timeout | PublishErrorKind::Unavailable
                        ) =>
                {
                    attempts_remaining = attempts_remaining.saturating_sub(1);
                    tokio::time::sleep(PUBLISH_RETRY_DELAY).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

#[async_trait]
impl MessageHandler<CommandAddress> for NatsCommandHandler {
    async fn handle(&self, delivery: MessageDelivery<CommandAddress>) -> DeliveryDisposition {
        let transport_correlation_id = delivery.correlation_id().cloned();
        let Ok(command) = EncodedCommand::from_delivery(
            delivery.address().clone(),
            delivery.message_id().clone(),
            delivery.payload().to_vec(),
        ) else {
            return quarantine("invalid command envelope");
        };
        if transport_correlation_id
            .as_ref()
            .is_some_and(|correlation_id| correlation_id != command.correlation_id())
        {
            return quarantine("invalid command correlation identity");
        }
        if command.validate().is_err() {
            return quarantine("invalid original command evidence");
        }
        let caller = match &self.processor {
            HandlerProcessor::Trusted(_) => None,
            HandlerProcessor::Authorized { resolver, .. } => {
                match resolver.resolve(&delivery).await {
                    Ok(caller) => Some(caller),
                    Err(AuthorizationError::Unavailable) => return retry(),
                    Err(_) => {
                        return quarantine("trusted command context resolution refused delivery");
                    }
                }
            }
        };
        let _execution = self.execution_gates.acquire(command.message_id()).await;
        match self.reconcile_response(&command).await {
            Ok(Some(response)) => match response.outcome() {
                CommandResponseOutcome::ExecutionFailed(failure) if failure.is_retryable() => {}
                CommandResponseOutcome::ExecutionFailed(_) => {
                    // A diagnostic never overrides an already durable completion. This
                    // lookup cannot invoke a handler; without a winner the existing blocked
                    // repair/redrive policy is unchanged. Keep the old record for audit.
                    let authoritative = match self
                        .reconcile_completed(caller.as_ref(), &command)
                        .await
                    {
                        Ok(Some(response)) => response,
                        Ok(None) => {
                            return quarantine(
                                "technical command execution is blocked; repair and redrive required",
                            );
                        }
                        Err(error) if error.is_retryable() => return retry(),
                        Err(_) => return quarantine("shared outcome reconciliation is blocked"),
                    };
                    return match self.publish_response(&command, &authoritative).await {
                        Ok(_) => DeliveryDisposition::Acknowledge,
                        Err(_) => retry(),
                    };
                }
                _ => {
                    // A retained business response is a projection, never an independent
                    // authority. Legacy-only responses require explicit evidence-bound import.
                    return match self.reconcile_completed(caller.as_ref(), &command).await {
                        Ok(Some(authoritative)) if authoritative == response => {
                            DeliveryDisposition::Acknowledge
                        }
                        Ok(Some(_) | None) => quarantine(
                            "retained business response lacks matching shared authority; migration required",
                        ),
                        Err(error) if error.is_retryable() => retry(),
                        Err(_) => quarantine("shared outcome reconciliation is blocked"),
                    };
                }
            },
            Ok(None) => {}
            Err(ReconciliationError::Unavailable) => return retry(),
            Err(ReconciliationError::Invalid) => {
                return quarantine("invalid retained command response");
            }
        }

        let response = match self.process(caller.as_ref(), &command).await {
            Ok(response) => response,
            Err(error) => {
                // Notify the remote caller before asking the existing native consumer to
                // durably quarantine the source. A missing response sink selects retry here;
                // the consumer's finite delivery budget can separately quarantine exhaustion.
                match error.technical_response(&command) {
                    Ok(Some(response)) => {
                        if self.publish_response(&command, &response).await.is_err() {
                            return retry();
                        }
                    }
                    Ok(None) => {}
                    Err(_) => return retry(),
                }
                if error.is_retryable() {
                    return retry();
                }
                return quarantine("command processing failed permanently");
            }
        };
        match self.publish_response(&command, &response).await {
            Ok(_) => DeliveryDisposition::Acknowledge,
            Err(_) => retry(),
        }
    }
}

#[derive(Default)]
struct CommandExecutionGates {
    gates: Mutex<HashMap<String, Weak<Mutex<()>>>>,
}

impl CommandExecutionGates {
    async fn acquire(
        &self,
        message_id: &rostfrei_messaging_core::MessageId,
    ) -> OwnedMutexGuard<()> {
        let gate = {
            let mut gates = self.gates.lock().await;
            gates.retain(|_, gate| gate.strong_count() > 0);
            let existing = gates.get(message_id.as_str()).and_then(Weak::upgrade);
            existing.unwrap_or_else(|| {
                let gate = Arc::new(Mutex::new(()));
                gates.insert(message_id.as_str().to_owned(), Arc::downgrade(&gate));
                gate
            })
        };
        gate.lock_owned().await
    }
}

enum ReconciliationError {
    Invalid,
    Unavailable,
}

fn command_publish_error(error: PublishError) -> CommandBusError {
    let kind = match error.kind() {
        PublishErrorKind::Timeout => CommandBusErrorKind::Timeout,
        PublishErrorKind::Rejected => CommandBusErrorKind::Rejected,
        PublishErrorKind::Unavailable => CommandBusErrorKind::Unavailable,
        PublishErrorKind::InvalidConfiguration => CommandBusErrorKind::InvalidConfiguration,
        _ => CommandBusErrorKind::InvalidMessage,
    };
    CommandBusError::new(kind, error.to_string())
}

fn integration_publish_error(error: PublishError) -> IntegrationEventBusError {
    let kind = match error.kind() {
        PublishErrorKind::Timeout => IntegrationEventBusErrorKind::Timeout,
        PublishErrorKind::Unavailable => IntegrationEventBusErrorKind::Unavailable,
        PublishErrorKind::InvalidConfiguration => {
            IntegrationEventBusErrorKind::InvalidConfiguration
        }
        PublishErrorKind::Rejected => IntegrationEventBusErrorKind::Rejected,
        _ => IntegrationEventBusErrorKind::InvalidMessage,
    };
    IntegrationEventBusError::new(kind, error.to_string())
}

fn retry() -> DeliveryDisposition {
    RetryDelay::new(DELIVERY_RETRY_DELAY).map_or(
        DeliveryDisposition::Terminate,
        DeliveryDisposition::RetryAfter,
    )
}

fn quarantine(reason: &'static str) -> DeliveryDisposition {
    QuarantineReason::new(reason).map_or(
        DeliveryDisposition::Terminate,
        DeliveryDisposition::Quarantine,
    )
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use super::*;

    use std::{
        error::Error,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use rostfrei::{
        CommandBus, DynamicCommandRequest, ExecutionStore, InMemoryEventStore,
        InMemoryMessagingAdapter, OperationId,
    };
    use rostfrei_messaging_core::{
        ApplicationName, CallerMetadata, CommandResponseAddress, CommandResponseReadError,
        CorrelationId, DeliveryInfo, IntegrationEventAddress, MessageId,
    };
    use tokio::sync::{Notify, OnceCell, Semaphore};

    type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

    #[derive(Default)]
    struct FlakyCommandPublisher {
        attempts: AtomicUsize,
        message_ids: Mutex<Vec<MessageId>>,
    }

    #[async_trait]
    impl CommandPublisher for FlakyCommandPublisher {
        async fn publish_command(
            &self,
            message: OutboundMessage<CommandAddress>,
        ) -> Result<PublishReceipt, PublishError> {
            self.message_ids
                .lock()
                .await
                .push(message.message_id().clone());
            if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(PublishError::new(PublishErrorKind::Timeout))
            } else {
                Ok(PublishReceipt::new(true))
            }
        }
    }

    struct AcceptedAfterTimeoutReader {
        command_address: CommandAddress,
        attempts: AtomicUsize,
    }

    #[async_trait]
    impl CommandResponseReader for AcceptedAfterTimeoutReader {
        async fn read_command_response(
            &self,
            _address: &CommandResponseAddress,
            operation_id: &rostfrei_messaging_core::OperationId,
            command_message_id: &MessageId,
            _read_timeout: Duration,
        ) -> Result<CommandResponse, CommandResponseReadError> {
            if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(CommandResponseReadError::new(
                    CommandResponseReadErrorKind::Timeout,
                ));
            }
            CommandResponse::accepted(
                rostfrei::command_response_message_id(command_message_id)
                    .map_err(|_| invalid_response())?,
                command_message_id.clone(),
                self.command_address.clone(),
                operation_id.clone(),
                CorrelationId::new(operation_id.as_str()).map_err(|_| invalid_response())?,
            )
            .map_err(|_| invalid_response())
        }
    }

    struct InvalidResponseReader;

    #[async_trait]
    impl CommandResponseReader for InvalidResponseReader {
        async fn read_command_response(
            &self,
            _address: &CommandResponseAddress,
            _operation_id: &rostfrei_messaging_core::OperationId,
            _command_message_id: &MessageId,
            _read_timeout: Duration,
        ) -> Result<CommandResponse, CommandResponseReadError> {
            Err(invalid_response())
        }
    }

    struct TimeoutReader;

    #[async_trait]
    impl CommandResponseReader for TimeoutReader {
        async fn read_command_response(
            &self,
            _address: &CommandResponseAddress,
            _operation_id: &rostfrei_messaging_core::OperationId,
            _command_message_id: &MessageId,
            _read_timeout: Duration,
        ) -> Result<CommandResponse, CommandResponseReadError> {
            Err(CommandResponseReadError::new(
                CommandResponseReadErrorKind::Timeout,
            ))
        }
    }

    struct LookupTimeoutReader;

    #[async_trait]
    impl CommandResponseReader for LookupTimeoutReader {
        async fn find_command_response(
            &self,
            _address: &CommandResponseAddress,
            _operation_id: &rostfrei_messaging_core::OperationId,
            _command_message_id: &MessageId,
            _read_timeout: Duration,
        ) -> Result<Option<CommandResponse>, CommandResponseReadError> {
            Err(CommandResponseReadError::new(
                CommandResponseReadErrorKind::Timeout,
            ))
        }

        async fn read_command_response(
            &self,
            _address: &CommandResponseAddress,
            _operation_id: &rostfrei_messaging_core::OperationId,
            _command_message_id: &MessageId,
            _read_timeout: Duration,
        ) -> Result<CommandResponse, CommandResponseReadError> {
            Err(CommandResponseReadError::new(
                CommandResponseReadErrorKind::Timeout,
            ))
        }
    }

    struct NoopCommandPublisher;

    struct DelayedCommandPublisher {
        delay: Duration,
    }

    #[async_trait]
    impl CommandPublisher for DelayedCommandPublisher {
        async fn publish_command(
            &self,
            _message: OutboundMessage<CommandAddress>,
        ) -> Result<PublishReceipt, PublishError> {
            tokio::time::sleep(self.delay).await;
            Ok(PublishReceipt::new(false))
        }
    }

    struct RejectedCommandPublisher;

    #[async_trait]
    impl CommandPublisher for RejectedCommandPublisher {
        async fn publish_command(
            &self,
            _message: OutboundMessage<CommandAddress>,
        ) -> Result<PublishReceipt, PublishError> {
            Err(PublishError::new(PublishErrorKind::Rejected))
        }
    }

    struct NoopCommandObserver;

    #[async_trait]
    impl CommandBusObserver for NoopCommandObserver {
        async fn published(&self, _publication: CommandPublication) {}
    }

    #[async_trait]
    impl CommandPublisher for NoopCommandPublisher {
        async fn publish_command(
            &self,
            _message: OutboundMessage<CommandAddress>,
        ) -> Result<PublishReceipt, PublishError> {
            Ok(PublishReceipt::new(false))
        }
    }

    struct NoopResponsePublisher;

    #[async_trait]
    impl CommandResponsePublisher for NoopResponsePublisher {
        async fn publish_command_response(
            &self,
            _message: OutboundMessage<CommandResponseAddress>,
        ) -> Result<PublishReceipt, PublishError> {
            Ok(PublishReceipt::new(false))
        }
    }

    struct NoopIntegrationPublisher;

    #[async_trait]
    impl IntegrationEventPublisher for NoopIntegrationPublisher {
        async fn publish_integration_event(
            &self,
            _message: OutboundMessage<IntegrationEventAddress>,
        ) -> Result<PublishReceipt, PublishError> {
            Ok(PublishReceipt::new(false))
        }
    }

    #[derive(Default)]
    struct FlakyIntegrationPublisher {
        attempts: AtomicUsize,
        message_ids: Mutex<Vec<MessageId>>,
    }

    #[async_trait]
    impl IntegrationEventPublisher for FlakyIntegrationPublisher {
        async fn publish_integration_event(
            &self,
            message: OutboundMessage<IntegrationEventAddress>,
        ) -> Result<PublishReceipt, PublishError> {
            self.message_ids
                .lock()
                .await
                .push(message.message_id().clone());
            if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(PublishError::new(PublishErrorKind::Unavailable))
            } else {
                Ok(PublishReceipt::new(true))
            }
        }
    }

    struct BlockingResponsePublisher {
        entered: Notify,
        release: Semaphore,
    }

    #[derive(Default)]
    struct LoopbackBroker {
        handler: OnceCell<Arc<NatsCommandHandler>>,
        response: Mutex<Option<CommandResponse>>,
    }

    #[async_trait]
    impl CommandPublisher for LoopbackBroker {
        async fn publish_command(
            &self,
            message: OutboundMessage<CommandAddress>,
        ) -> Result<PublishReceipt, PublishError> {
            let handler = self
                .handler
                .get()
                .ok_or_else(|| PublishError::new(PublishErrorKind::Unavailable))?;
            let delivery = MessageDelivery::new(
                message.address().clone(),
                message.message_id().clone(),
                message.payload().to_vec(),
                CallerMetadata::new(),
                DeliveryInfo::new(1, 0, 1, 1)
                    .map_err(|_| PublishError::new(PublishErrorKind::Rejected))?,
            )
            .map_err(|_| PublishError::new(PublishErrorKind::Rejected))?;
            if handler.handle(delivery).await == DeliveryDisposition::Acknowledge {
                Ok(PublishReceipt::new(false))
            } else {
                Err(PublishError::new(PublishErrorKind::Rejected))
            }
        }
    }

    #[async_trait]
    impl CommandResponsePublisher for LoopbackBroker {
        async fn publish_command_response(
            &self,
            message: OutboundMessage<CommandResponseAddress>,
        ) -> Result<PublishReceipt, PublishError> {
            let response = serde_json::from_slice(message.payload())
                .map_err(|_| PublishError::new(PublishErrorKind::Rejected))?;
            self.response.lock().await.replace(response);
            Ok(PublishReceipt::new(false))
        }
    }

    #[async_trait]
    impl CommandResponseReader for LoopbackBroker {
        async fn read_command_response(
            &self,
            _address: &CommandResponseAddress,
            _operation_id: &rostfrei_messaging_core::OperationId,
            _command_message_id: &MessageId,
            _read_timeout: Duration,
        ) -> Result<CommandResponse, CommandResponseReadError> {
            self.response
                .lock()
                .await
                .clone()
                .ok_or_else(|| CommandResponseReadError::new(CommandResponseReadErrorKind::Timeout))
        }
    }

    #[async_trait]
    impl IntegrationEventPublisher for LoopbackBroker {
        async fn publish_integration_event(
            &self,
            _message: OutboundMessage<IntegrationEventAddress>,
        ) -> Result<PublishReceipt, PublishError> {
            Ok(PublishReceipt::new(false))
        }
    }

    impl Default for BlockingResponsePublisher {
        fn default() -> Self {
            Self {
                entered: Notify::new(),
                release: Semaphore::new(0),
            }
        }
    }

    #[async_trait]
    impl CommandResponsePublisher for BlockingResponsePublisher {
        async fn publish_command_response(
            &self,
            _message: OutboundMessage<CommandResponseAddress>,
        ) -> Result<PublishReceipt, PublishError> {
            self.entered.notify_one();
            self.release
                .acquire()
                .await
                .map_err(|_| PublishError::new(PublishErrorKind::Unavailable))?
                .forget();
            Ok(PublishReceipt::new(false))
        }
    }

    fn invalid_response() -> CommandResponseReadError {
        CommandResponseReadError::new(CommandResponseReadErrorKind::InvalidResponse)
    }

    fn context() -> TestResult<rostfrei_messaging_core::BoundedContext> {
        Ok(ApplicationName::new("nats-adapter-test")?.bounded_context("ledger")?)
    }

    fn empty_processor() -> Arc<CommandProcessor> {
        let store: Arc<dyn ExecutionStore> = Arc::new(InMemoryEventStore::new());
        let context = rostfrei_messaging_core::BoundedContextName::new("ledger")
            .expect("test command processor context must be valid");
        Arc::new(CommandProcessor::new(context, store))
    }

    fn encoded_command(adapter: Arc<NatsMessagingAdapter>) -> TestResult<EncodedCommand> {
        let erased: Arc<dyn CommandMessageAdapter> = adapter;
        let bus = CommandBus::new(context()?, erased);
        Ok(bus.encode_dynamic(DynamicCommandRequest::new(
            OperationId::new("nats-command")?,
            "credit-account",
            1,
            serde_json::json!({ "amount": 7 }),
        )?)?)
    }

    fn dynamic_request() -> TestResult<DynamicCommandRequest> {
        Ok(DynamicCommandRequest::new(
            OperationId::new("adapter-parity")?,
            "unknown-command",
            1,
            serde_json::json!({ "amount": 7 }),
        )?)
    }

    #[tokio::test]
    async fn command_publish_retries_exact_message_and_timeout_does_not_republish() -> TestResult {
        let publisher = Arc::new(FlakyCommandPublisher::default());
        let command_address = context()?.command_address("credit-account")?;
        let reader = Arc::new(AcceptedAfterTimeoutReader {
            command_address,
            attempts: AtomicUsize::new(0),
        });
        let adapter = Arc::new(NatsMessagingAdapter::with_components(
            publisher.clone(),
            Arc::new(NoopResponsePublisher),
            Arc::new(NoopIntegrationPublisher),
            reader.clone(),
        ));
        let command = encoded_command(Arc::clone(&adapter))?;

        let receipt = adapter
            .dispatch(command, Arc::new(NoopCommandObserver))
            .await?;

        assert!(receipt.publication_duplicate());
        assert_eq!(publisher.attempts.load(Ordering::SeqCst), 2);
        assert_eq!(reader.attempts.load(Ordering::SeqCst), 2);
        let message_ids = publisher.message_ids.lock().await;
        assert_eq!(message_ids.len(), 2);
        assert_eq!(message_ids.first(), message_ids.get(1));
        Ok(())
    }

    #[tokio::test]
    async fn response_timeout_starts_after_command_publication() -> TestResult {
        let reader = Arc::new(AcceptedAfterTimeoutReader {
            command_address: context()?.command_address("credit-account")?,
            attempts: AtomicUsize::new(0),
        });
        let adapter = Arc::new(
            NatsMessagingAdapter::with_components(
                Arc::new(DelayedCommandPublisher {
                    delay: Duration::from_millis(30),
                }),
                Arc::new(NoopResponsePublisher),
                Arc::new(NoopIntegrationPublisher),
                reader,
            )
            .with_response_timeout(Duration::from_millis(10)),
        );
        let command = encoded_command(Arc::clone(&adapter))?;

        let receipt = adapter
            .dispatch(command, Arc::new(NoopCommandObserver))
            .await?;

        assert!(matches!(
            receipt.response().outcome(),
            rostfrei::CommandResponseOutcome::Accepted
        ));
        Ok(())
    }

    #[tokio::test]
    async fn broker_publication_rejection_remains_distinct() -> TestResult {
        let adapter = Arc::new(NatsMessagingAdapter::with_components(
            Arc::new(RejectedCommandPublisher),
            Arc::new(NoopResponsePublisher),
            Arc::new(NoopIntegrationPublisher),
            Arc::new(InvalidResponseReader),
        ));
        let command = encoded_command(Arc::clone(&adapter))?;

        let error = adapter
            .dispatch(command, Arc::new(NoopCommandObserver))
            .await
            .expect_err("rejected publication should fail dispatch");

        assert_eq!(error.kind(), CommandBusErrorKind::Rejected);
        Ok(())
    }

    #[tokio::test]
    async fn invalid_durable_response_is_not_returned_to_the_caller() -> TestResult {
        let adapter = Arc::new(NatsMessagingAdapter::with_components(
            Arc::new(NoopCommandPublisher),
            Arc::new(NoopResponsePublisher),
            Arc::new(NoopIntegrationPublisher),
            Arc::new(InvalidResponseReader),
        ));
        let command = encoded_command(Arc::clone(&adapter))?;

        let error = adapter
            .dispatch(command, Arc::new(NoopCommandObserver))
            .await
            .expect_err("invalid response should fail dispatch");

        assert_eq!(error.kind(), CommandBusErrorKind::InvalidResponse);
        Ok(())
    }

    #[tokio::test]
    async fn integration_publish_retries_the_exact_message() -> TestResult {
        let publisher = Arc::new(FlakyIntegrationPublisher::default());
        let adapter = NatsMessagingAdapter::with_components(
            Arc::new(NoopCommandPublisher),
            Arc::new(NoopResponsePublisher),
            publisher.clone(),
            Arc::new(InvalidResponseReader),
        );
        let message = EncodedIntegrationMessage::from_delivery(
            context()?.integration_event_address("account-credited")?,
            MessageId::new("account-credited-message")?,
            br#"{"payload":{"amount":7}}"#.to_vec(),
            None,
        )?;

        let receipt = IntegrationMessageAdapter::publish(&adapter, message).await?;

        assert!(receipt.duplicate());
        assert_eq!(publisher.attempts.load(Ordering::SeqCst), 2);
        let message_ids = publisher.message_ids.lock().await;
        assert_eq!(message_ids.len(), 2);
        assert_eq!(message_ids.first(), message_ids.get(1));
        Ok(())
    }

    #[tokio::test]
    async fn command_delivery_waits_for_response_publication_before_acknowledging() -> TestResult {
        let response_publisher = Arc::new(BlockingResponsePublisher::default());
        let processor = empty_processor();
        let handler = Arc::new(NatsCommandHandler::new(
            processor,
            response_publisher.clone(),
            Arc::new(TimeoutReader),
        ));
        let adapter = Arc::new(NatsMessagingAdapter::with_components(
            Arc::new(NoopCommandPublisher),
            Arc::new(NoopResponsePublisher),
            Arc::new(NoopIntegrationPublisher),
            Arc::new(InvalidResponseReader),
        ));
        let command = encoded_command(adapter)?;
        let delivery = MessageDelivery::new(
            command.address().clone(),
            command.message_id().clone(),
            command.payload().to_vec(),
            CallerMetadata::new(),
            DeliveryInfo::new(1, 0, 1, 1)?,
        )?;
        let handling = tokio::spawn(async move { handler.handle(delivery).await });

        response_publisher.entered.notified().await;
        assert!(!handling.is_finished());
        response_publisher.release.add_permits(1);
        assert_eq!(handling.await?, DeliveryDisposition::Acknowledge);
        Ok(())
    }

    #[tokio::test]
    async fn command_handler_rejects_mismatched_correlation_header() -> TestResult {
        let handler = NatsCommandHandler::new(
            empty_processor(),
            Arc::new(NoopResponsePublisher),
            Arc::new(TimeoutReader),
        );
        let adapter = Arc::new(NatsMessagingAdapter::with_components(
            Arc::new(NoopCommandPublisher),
            Arc::new(NoopResponsePublisher),
            Arc::new(NoopIntegrationPublisher),
            Arc::new(InvalidResponseReader),
        ));
        let command = encoded_command(adapter)?;
        let delivery = MessageDelivery::new_with_transport_context(
            command.address().clone(),
            command.message_id().clone(),
            command.payload().to_vec(),
            CallerMetadata::new(),
            Some(CorrelationId::new("different-correlation")?),
            None,
            DeliveryInfo::new(1, 0, 1, 1)?,
        )?;

        assert!(matches!(
            handler.handle(delivery).await,
            DeliveryDisposition::Quarantine(_)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn uncertain_response_lookup_retries_without_processing_the_command() -> TestResult {
        let handler = NatsCommandHandler::new(
            empty_processor(),
            Arc::new(NoopResponsePublisher),
            Arc::new(LookupTimeoutReader),
        );
        let adapter = Arc::new(NatsMessagingAdapter::with_components(
            Arc::new(NoopCommandPublisher),
            Arc::new(NoopResponsePublisher),
            Arc::new(NoopIntegrationPublisher),
            Arc::new(InvalidResponseReader),
        ));
        let command = encoded_command(adapter)?;
        let delivery = MessageDelivery::new_with_transport_context(
            command.address().clone(),
            command.message_id().clone(),
            command.payload().to_vec(),
            CallerMetadata::new(),
            Some(command.correlation_id().clone()),
            None,
            DeliveryInfo::new(1, 0, 1, 1)?,
        )?;

        assert!(matches!(
            handler.handle(delivery).await,
            DeliveryDisposition::RetryAfter(_)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn in_memory_and_nats_adapters_return_the_same_command_contract() -> TestResult {
        let in_memory_processor = empty_processor();
        let in_memory_adapter = Arc::new(InMemoryMessagingAdapter::new(in_memory_processor));
        let in_memory_erased: Arc<dyn CommandMessageAdapter> = in_memory_adapter;
        let in_memory_bus = CommandBus::new(context()?, in_memory_erased);

        let broker = Arc::new(LoopbackBroker::default());
        let nats_adapter = Arc::new(NatsMessagingAdapter::with_components(
            broker.clone(),
            broker.clone(),
            broker.clone(),
            broker.clone(),
        ));
        let nats_processor = empty_processor();
        if broker
            .handler
            .set(Arc::new(nats_adapter.command_handler(nats_processor)))
            .is_err()
        {
            return Err("loopback command handler was already configured".into());
        }
        let nats_erased: Arc<dyn CommandMessageAdapter> = nats_adapter;
        let nats_bus = CommandBus::new(context()?, nats_erased);

        let in_memory = in_memory_bus.dispatch_dynamic(dynamic_request()?).await?;
        let nats = nats_bus.dispatch_dynamic(dynamic_request()?).await?;

        assert_eq!(in_memory.response(), nats.response());
        assert_eq!(
            in_memory.publication_duplicate(),
            nats.publication_duplicate()
        );
        Ok(())
    }
}
