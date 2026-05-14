use std::{
    collections::HashMap,
    convert::Infallible,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use domain::{BoundedContextType, DomainError, JsonCommandPayload, JsonErrorPayload};
use rostfrei_core::{
    Aggregate, AggregateCodecs, CommandExecutionError, CommandExecutionMetadata, CommandExecutor,
    CommandHandler, CommandOutcome, ContentFingerprint, EventCodec, EventStore,
    EventStoreErrorKind, OperationId as CoreOperationId,
};
use rostfrei_messaging_core::{
    ApplicationErrorCode, BoundedContext, BoundedContextName, COMMAND_RESPONSE_SCHEMA_VERSION,
    CausationId, CommandAddress, CommandEnvelope, CommandRejection, CommandRejectionClassification,
    CommandResponse, CommandResponseOutcome, ContractError, CorrelationId, EnvelopeContext,
    MessageBuildError, MessageBuildErrorKind, MessageId, MessageTimestamp, OperationId,
    OutboundMessage, SchemaVersion, derive_command_response_address,
};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;
use thiserror::Error;

const INVALID_COMMAND_CODE: &str = "rostfrei.command.invalid";
const INVALID_PAYLOAD_CODE: &str = "rostfrei.command.invalid-payload";
const UNKNOWN_COMMAND_CODE: &str = "rostfrei.command.unknown";
const OPERATION_CONFLICT_CODE: &str = "rostfrei.operation.identity-conflict";

#[derive(Clone, Debug)]
pub struct CommandRequest<C> {
    operation_id: CoreOperationId,
    command: C,
    correlation_id: Option<CorrelationId>,
    causation_id: Option<CausationId>,
    created_at: Option<MessageTimestamp>,
    events_caused_by_command: bool,
}

impl<C> CommandRequest<C> {
    pub const fn new(operation_id: CoreOperationId, command: C) -> Self {
        Self {
            operation_id,
            command,
            correlation_id: None,
            causation_id: None,
            created_at: None,
            events_caused_by_command: false,
        }
    }

    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: CorrelationId) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    #[must_use]
    pub fn with_causation_id(mut self, causation_id: CausationId) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    #[must_use]
    pub const fn with_created_at(mut self, created_at: MessageTimestamp) -> Self {
        self.created_at = Some(created_at);
        self
    }

    pub(crate) const fn with_events_caused_by_command(mut self) -> Self {
        self.events_caused_by_command = true;
        self
    }
}

#[derive(Clone, Debug)]
pub struct DynamicCommandRequest {
    operation_id: CoreOperationId,
    command: String,
    schema_version: u32,
    payload: Value,
    correlation_id: Option<CorrelationId>,
    causation_id: Option<CausationId>,
    created_at: Option<MessageTimestamp>,
    events_caused_by_command: bool,
}

impl DynamicCommandRequest {
    pub fn new(
        operation_id: CoreOperationId,
        command: impl Into<String>,
        schema_version: u32,
        payload: Value,
    ) -> Result<Self, CommandBusError> {
        let command = command.into();
        CommandAddress::new("rostfrei", "dynamic-command", &command)
            .map_err(|error| CommandBusError::encoding(error.to_string()))?;
        SchemaVersion::new(schema_version)
            .map_err(|error| CommandBusError::encoding(error.to_string()))?;
        Ok(Self {
            operation_id,
            command,
            schema_version,
            payload,
            correlation_id: None,
            causation_id: None,
            created_at: None,
            events_caused_by_command: false,
        })
    }

    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: CorrelationId) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    #[must_use]
    pub fn with_causation_id(mut self, causation_id: CausationId) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    #[must_use]
    pub const fn with_created_at(mut self, created_at: MessageTimestamp) -> Self {
        self.created_at = Some(created_at);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RoutedCommand {
    command: String,
    schema_version: u32,
    payload: Value,
    #[serde(skip_serializing_if = "is_false")]
    events_caused_by_command: bool,
}

impl RoutedCommand {
    pub fn new(
        command: impl Into<String>,
        schema_version: u32,
        payload: Value,
    ) -> Result<Self, RoutedCommandError> {
        let command = command.into();
        CommandAddress::new("rostfrei", "routed-command", &command)
            .map_err(RoutedCommandError::CommandName)?;
        SchemaVersion::new(schema_version).map_err(RoutedCommandError::SchemaVersion)?;
        Ok(Self {
            command,
            schema_version,
            payload,
            events_caused_by_command: false,
        })
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub const fn payload(&self) -> &Value {
        &self.payload
    }

    const fn with_events_caused_by_command(mut self, enabled: bool) -> Self {
        self.events_caused_by_command = enabled;
        self
    }

    const fn events_caused_by_command(&self) -> bool {
        self.events_caused_by_command
    }
}

#[derive(Deserialize)]
struct RoutedCommandWire {
    command: String,
    schema_version: u32,
    payload: Value,
    #[serde(default)]
    events_caused_by_command: bool,
}

impl<'de> Deserialize<'de> for RoutedCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RoutedCommandWire::deserialize(deserializer)?;
        Self::new(wire.command, wire.schema_version, wire.payload)
            .map(|command| command.with_events_caused_by_command(wire.events_caused_by_command))
            .map_err(D::Error::custom)
    }
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires a shared reference"
)]
const fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RoutedCommandError {
    #[error("invalid routed command name: {0}")]
    CommandName(ContractError),
    #[error("invalid routed command schema version: {0}")]
    SchemaVersion(ContractError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedCommand {
    message: OutboundMessage<CommandAddress>,
    operation_id: OperationId,
    correlation_id: CorrelationId,
    fingerprint: ContentFingerprint,
}

impl EncodedCommand {
    pub(crate) const fn new(
        message: OutboundMessage<CommandAddress>,
        operation_id: OperationId,
        correlation_id: CorrelationId,
        fingerprint: ContentFingerprint,
    ) -> Self {
        Self {
            message,
            operation_id,
            correlation_id,
            fingerprint,
        }
    }

    pub fn from_delivery(
        address: CommandAddress,
        message_id: MessageId,
        payload: Vec<u8>,
    ) -> Result<Self, CommandProcessorError> {
        let envelope: CommandEnvelope<RoutedCommand> = serde_json::from_slice(&payload)
            .map_err(|error| CommandProcessorError::invalid_message(error.to_string()))?;
        let fingerprint =
            routed_command_execution_fingerprint(address.context(), envelope.payload())
                .map_err(|error| CommandProcessorError::invalid_message(error.to_string()))?;
        let message = OutboundMessage::new(address, message_id, payload)
            .map_err(|error| CommandProcessorError::invalid_message(error.to_string()))?;
        Ok(Self::new(
            message,
            envelope.operation_id().clone(),
            envelope.correlation_id().clone(),
            fingerprint,
        ))
    }

    pub const fn message(&self) -> &OutboundMessage<CommandAddress> {
        &self.message
    }

    pub const fn address(&self) -> &CommandAddress {
        self.message.address()
    }

    pub const fn message_id(&self) -> &MessageId {
        self.message.message_id()
    }

    pub fn payload(&self) -> &[u8] {
        self.message.payload()
    }

    pub const fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    pub const fn fingerprint(&self) -> ContentFingerprint {
        self.fingerprint
    }

    pub fn response_address(
        &self,
    ) -> Result<rostfrei_messaging_core::CommandResponseAddress, ContractError> {
        derive_command_response_address(self.address(), self.operation_id(), self.message_id())
    }

    pub fn validate_response(
        &self,
        response: &CommandResponse,
    ) -> Result<(), InvalidCommandResponse> {
        if response.message_id()
            != &command_response_message_id(self.message_id())
                .map_err(|_| InvalidCommandResponse::ResponseMessageId)?
        {
            return Err(InvalidCommandResponse::ResponseMessageId);
        }
        if response.command_address() != self.address() {
            return Err(InvalidCommandResponse::CommandAddress);
        }
        if response.command_message_id() != self.message_id() {
            return Err(InvalidCommandResponse::CommandMessageId);
        }
        if response.operation_id() != self.operation_id() {
            return Err(InvalidCommandResponse::OperationId);
        }
        if response.schema_version().get() != COMMAND_RESPONSE_SCHEMA_VERSION {
            return Err(InvalidCommandResponse::SchemaVersion);
        }
        if response.correlation_id() != self.correlation_id() {
            return Err(InvalidCommandResponse::CorrelationId);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum InvalidCommandResponse {
    #[error("response message ID does not match the command")]
    ResponseMessageId,
    #[error("response command address does not match the command")]
    CommandAddress,
    #[error("response command message ID does not match the command")]
    CommandMessageId,
    #[error("response operation ID does not match the command")]
    OperationId,
    #[error("response schema version is invalid")]
    SchemaVersion,
    #[error("response correlation ID does not match the command")]
    CorrelationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandBusReceipt {
    publication_duplicate: bool,
    response: CommandResponse,
}

impl CommandBusReceipt {
    pub const fn new(publication_duplicate: bool, response: CommandResponse) -> Self {
        Self {
            publication_duplicate,
            response,
        }
    }

    pub const fn publication_duplicate(&self) -> bool {
        self.publication_duplicate
    }

    pub const fn response(&self) -> &CommandResponse {
        &self.response
    }

    pub fn into_response(self) -> CommandResponse {
        self.response
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandPublication {
    message_id: MessageId,
    duplicate: bool,
}

impl CommandPublication {
    pub const fn new(message_id: MessageId, duplicate: bool) -> Self {
        Self {
            message_id,
            duplicate,
        }
    }

    pub const fn message_id(&self) -> &MessageId {
        &self.message_id
    }

    pub const fn duplicate(&self) -> bool {
        self.duplicate
    }
}

#[async_trait]
pub trait CommandBusObserver: Send + Sync {
    async fn published(&self, publication: CommandPublication);
}

struct IgnoreCommandPublications;

#[async_trait]
impl CommandBusObserver for IgnoreCommandPublications {
    async fn published(&self, _publication: CommandPublication) {}
}

#[async_trait]
pub trait CommandMessageAdapter: Send + Sync {
    fn maximum_payload_len(&self) -> usize {
        rostfrei_messaging_core::MAX_MESSAGE_PAYLOAD_BYTES
    }

    async fn dispatch(
        &self,
        command: EncodedCommand,
        observer: Arc<dyn CommandBusObserver>,
    ) -> Result<CommandBusReceipt, CommandBusError>;
}

#[derive(Clone)]
pub struct CommandBus {
    context: BoundedContext,
    adapter: Arc<dyn CommandMessageAdapter>,
}

impl CommandBus {
    pub const fn new(context: BoundedContext, adapter: Arc<dyn CommandMessageAdapter>) -> Self {
        Self { context, adapter }
    }

    pub const fn context(&self) -> &BoundedContext {
        &self.context
    }

    pub fn maximum_payload_len(&self) -> usize {
        self.adapter.maximum_payload_len()
    }

    pub async fn dispatch<C>(
        &self,
        request: CommandRequest<C>,
    ) -> Result<CommandBusReceipt, CommandBusError>
    where
        C: JsonCommandPayload,
    {
        self.dispatch_observed::<C>(request, Arc::new(IgnoreCommandPublications))
            .await
    }

    pub async fn dispatch_observed<C>(
        &self,
        request: CommandRequest<C>,
        observer: Arc<dyn CommandBusObserver>,
    ) -> Result<CommandBusReceipt, CommandBusError>
    where
        C: JsonCommandPayload,
    {
        let encoded = self.encode::<C>(request)?;
        self.adapter.dispatch(encoded, observer).await
    }

    pub fn encode<C>(&self, request: CommandRequest<C>) -> Result<EncodedCommand, CommandBusError>
    where
        C: JsonCommandPayload,
    {
        let command_context = <C::Context as BoundedContextType>::DESCRIPTOR.id.0;
        if command_context != self.context.name().as_str() {
            return Err(CommandBusError::encoding(format!(
                "command context `{command_context}` does not match command bus context `{}`",
                self.context.name().as_str()
            )));
        }
        let payload = request
            .command
            .encode_json()
            .map_err(CommandBusError::encoding)?;
        self.encode_dynamic(DynamicCommandRequest {
            operation_id: request.operation_id,
            command: C::LOCAL_ID.to_owned(),
            schema_version: C::SCHEMA_VERSION,
            payload,
            correlation_id: request.correlation_id,
            causation_id: request.causation_id,
            created_at: request.created_at,
            events_caused_by_command: request.events_caused_by_command,
        })
    }

    pub async fn dispatch_dynamic(
        &self,
        request: DynamicCommandRequest,
    ) -> Result<CommandBusReceipt, CommandBusError> {
        self.dispatch_dynamic_observed(request, Arc::new(IgnoreCommandPublications))
            .await
    }

    pub async fn dispatch_dynamic_observed(
        &self,
        request: DynamicCommandRequest,
        observer: Arc<dyn CommandBusObserver>,
    ) -> Result<CommandBusReceipt, CommandBusError> {
        let encoded = self.encode_dynamic(request)?;
        self.adapter.dispatch(encoded, observer).await
    }

    pub fn encode_dynamic(
        &self,
        request: DynamicCommandRequest,
    ) -> Result<EncodedCommand, CommandBusError> {
        let address = self
            .context
            .command_address(&request.command)
            .map_err(|error| CommandBusError::encoding(error.to_string()))?;
        let operation_id = OperationId::new(request.operation_id.as_str())
            .map_err(|error| CommandBusError::encoding(error.to_string()))?;
        let correlation_id = match request.correlation_id {
            Some(correlation_id) => correlation_id,
            None => CorrelationId::new(operation_id.as_str())
                .map_err(|error| CommandBusError::encoding(error.to_string()))?,
        };
        let created_at = match request.created_at {
            Some(created_at) => created_at,
            None => current_timestamp()?,
        };
        let routed = RoutedCommand::new(request.command, request.schema_version, request.payload)
            .map_err(|error| CommandBusError::encoding(error.to_string()))?
            .with_events_caused_by_command(request.events_caused_by_command);
        let fingerprint =
            routed_command_execution_fingerprint(self.context.name().as_str(), &routed)?;
        let message_id = command_message_id(
            &address,
            &operation_id,
            fingerprint,
            &correlation_id,
            request.causation_id.as_ref(),
        )?;
        let envelope = CommandEnvelope::new(
            EnvelopeContext::new(
                message_id.clone(),
                SchemaVersion::new(routed.schema_version())
                    .map_err(|error| CommandBusError::encoding(error.to_string()))?,
                correlation_id.clone(),
                request.causation_id,
            ),
            operation_id.clone(),
            created_at,
            routed,
        )
        .map_err(|error| command_message_build_error(&error))?;
        let payload = canonical_serialize(&envelope)?;
        if payload.len() > self.maximum_payload_len() {
            return Err(CommandBusError::new(
                CommandBusErrorKind::PayloadTooLarge,
                format!(
                    "command payload exceeds its {}-byte adapter limit",
                    self.maximum_payload_len()
                ),
            ));
        }
        let message = OutboundMessage::new(address, message_id, payload)
            .map_err(|error| CommandBusError::encoding(error.to_string()))?
            .with_correlation_id(correlation_id.clone());
        Ok(EncodedCommand::new(
            message,
            operation_id,
            correlation_id,
            fingerprint,
        ))
    }
}

fn command_message_build_error(error: &MessageBuildError) -> CommandBusError {
    let kind = if error.kind() == MessageBuildErrorKind::PayloadTooLarge {
        CommandBusErrorKind::PayloadTooLarge
    } else {
        CommandBusErrorKind::Encoding
    };
    CommandBusError::new(kind, error.to_string())
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum CommandBusErrorKind {
    #[error("command encoding failed")]
    Encoding,
    #[error("command payload is too large")]
    PayloadTooLarge,
    #[error("command message is invalid")]
    InvalidMessage,
    #[error("command dispatch timed out")]
    Timeout,
    #[error("command publication was rejected")]
    Rejected,
    #[error("command messaging is unavailable")]
    Unavailable,
    #[error("command messaging configuration is invalid")]
    InvalidConfiguration,
    #[error("command response is invalid")]
    InvalidResponse,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{message}")]
pub struct CommandBusError {
    kind: CommandBusErrorKind,
    message: String,
}

impl CommandBusError {
    pub fn new(kind: CommandBusErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn encoding(message: impl Into<String>) -> Self {
        Self::new(CommandBusErrorKind::Encoding, message)
    }

    pub const fn kind(&self) -> CommandBusErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CommandBindingKey {
    context: String,
    command: String,
    schema_version: u32,
}

impl CommandBindingKey {
    fn new(context: &str, command: &str, schema_version: u32) -> Self {
        Self {
            context: context.to_owned(),
            command: command.to_owned(),
            schema_version,
        }
    }
}

pub trait CommandRejectionMapper<R>: Send + Sync {
    fn map(&self, rejection: &R) -> Result<CommandRejection, String>;
}

impl<R, F> CommandRejectionMapper<R> for F
where
    F: Fn(&R) -> Result<CommandRejection, String> + Send + Sync,
{
    fn map(&self, rejection: &R) -> Result<CommandRejection, String> {
        self(rejection)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct JsonDomainRejectionMapper {
    classification: CommandRejectionClassification,
}

impl JsonDomainRejectionMapper {
    pub const fn new(classification: CommandRejectionClassification) -> Self {
        Self { classification }
    }
}

impl<R> CommandRejectionMapper<R> for JsonDomainRejectionMapper
where
    R: DomainError + JsonErrorPayload,
{
    fn map(&self, rejection: &R) -> Result<CommandRejection, String> {
        let descriptor = R::DESCRIPTOR;
        let details = rejection.encode_json()?;
        CommandRejection::new(
            self.classification,
            ApplicationErrorCode::new(descriptor.code).map_err(|error| error.to_string())?,
            descriptor.message,
            Some(details),
        )
        .map_err(|error| error.to_string())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InfallibleCommandRejectionMapper;

impl CommandRejectionMapper<Infallible> for InfallibleCommandRejectionMapper {
    fn map(&self, _rejection: &Infallible) -> Result<CommandRejection, String> {
        Err("an infallible command produced a rejection".to_owned())
    }
}

#[async_trait]
trait ErasedCommandBinding: Send + Sync {
    async fn execute(
        &self,
        store: Arc<dyn EventStore>,
        codecs: Arc<AggregateCodecs>,
        metadata: CommandExecutionMetadata,
        payload: &Value,
    ) -> Result<CommandResponseOutcome, BindingError>;
}

struct TypedCommandBinding<C, H, M> {
    handler: H,
    rejection_mapper: M,
    marker: std::marker::PhantomData<fn() -> C>,
}

#[async_trait]
impl<C, H, M> ErasedCommandBinding for TypedCommandBinding<C, H, M>
where
    C: JsonCommandPayload + Send + Sync,
    H: CommandHandler<C> + 'static,
    M: CommandRejectionMapper<H::Rejection> + 'static,
{
    async fn execute(
        &self,
        store: Arc<dyn EventStore>,
        codecs: Arc<AggregateCodecs>,
        metadata: CommandExecutionMetadata,
        payload: &Value,
    ) -> Result<CommandResponseOutcome, BindingError> {
        let command = C::decode_json(payload).map_err(BindingError::InvalidPayload)?;
        match CommandExecutor::with_codecs(store, codecs)
            .execute(&self.handler, metadata, &command)
            .await
            .map_err(BindingError::Execution)?
        {
            CommandOutcome::Accepted(_) => Ok(CommandResponseOutcome::Accepted),
            CommandOutcome::Rejected(rejection) => self
                .rejection_mapper
                .map(&rejection)
                .map(CommandResponseOutcome::Rejected)
                .map_err(BindingError::RejectionMapping),
        }
    }
}

enum BindingError {
    InvalidPayload(String),
    Execution(CommandExecutionError),
    RejectionMapping(String),
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum CommandProcessorErrorKind {
    #[error("invalid command message")]
    InvalidMessage,
    #[error("command processing is unavailable")]
    Unavailable,
    #[error("command processor configuration is invalid")]
    InvalidConfiguration,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{message}")]
pub struct CommandProcessorError {
    kind: CommandProcessorErrorKind,
    message: String,
}

impl CommandProcessorError {
    pub fn new(kind: CommandProcessorErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn invalid_message(message: impl Into<String>) -> Self {
        Self::new(CommandProcessorErrorKind::InvalidMessage, message)
    }

    pub const fn kind(&self) -> CommandProcessorErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub const fn is_retryable(&self) -> bool {
        matches!(self.kind, CommandProcessorErrorKind::Unavailable)
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CommandBindingRegistrationError {
    #[error(
        "command context `{command_context}` does not match processor context `{processor_context}`"
    )]
    ContextMismatch {
        command_context: &'static str,
        processor_context: String,
    },
    #[error(
        "command `{command}` version {schema_version} for context `{context}` is already bound"
    )]
    Duplicate {
        context: &'static str,
        command: &'static str,
        schema_version: u32,
    },
}

pub struct CommandProcessor {
    context: BoundedContextName,
    store: Arc<dyn EventStore>,
    codecs: Arc<AggregateCodecs>,
    bindings: HashMap<CommandBindingKey, Arc<dyn ErasedCommandBinding>>,
}

impl CommandProcessor {
    pub fn new(context: BoundedContextName, store: Arc<dyn EventStore>) -> Self {
        Self {
            context,
            store,
            codecs: Arc::new(AggregateCodecs::default()),
            bindings: HashMap::new(),
        }
    }

    pub fn register_codec<A, C>(&mut self, codec: C) -> &mut Self
    where
        A: Aggregate + 'static,
        C: EventCodec<A> + 'static,
    {
        Arc::make_mut(&mut self.codecs).register::<A, C>(codec);
        self
    }

    pub fn register<C, H>(
        &mut self,
        handler: H,
        rejection_mapper: impl CommandRejectionMapper<H::Rejection> + 'static,
    ) -> Result<&mut Self, CommandBindingRegistrationError>
    where
        C: JsonCommandPayload + Send + Sync,
        H: CommandHandler<C> + 'static,
    {
        let context = <C::Context as BoundedContextType>::DESCRIPTOR.id.0;
        if context != self.context.as_str() {
            return Err(CommandBindingRegistrationError::ContextMismatch {
                command_context: context,
                processor_context: self.context.as_str().to_owned(),
            });
        }
        let key = CommandBindingKey::new(context, C::LOCAL_ID, C::SCHEMA_VERSION);
        if self.bindings.contains_key(&key) {
            return Err(CommandBindingRegistrationError::Duplicate {
                context,
                command: C::LOCAL_ID,
                schema_version: C::SCHEMA_VERSION,
            });
        }
        self.bindings.insert(
            key,
            Arc::new(TypedCommandBinding::<C, H, _> {
                handler,
                rejection_mapper,
                marker: std::marker::PhantomData,
            }),
        );
        Ok(self)
    }

    pub async fn process(
        &self,
        encoded: &EncodedCommand,
    ) -> Result<CommandResponse, CommandProcessorError> {
        let (envelope, metadata, key) = validate_command(encoded, &self.context)?;
        let routed = envelope.payload();
        let outcome = match self.bindings.get(&key) {
            None => CommandResponseOutcome::Rejected(framework_rejection(
                CommandRejectionClassification::InvalidRequest,
                UNKNOWN_COMMAND_CODE,
                "The command name or schema version is not registered.",
                Some(serde_json::json!({
                    "context": encoded.address().context(),
                    "command": routed.command(),
                    "schema_version": routed.schema_version(),
                })),
            )?),
            Some(binding) => match binding
                .execute(
                    Arc::clone(&self.store),
                    Arc::clone(&self.codecs),
                    metadata,
                    routed.payload(),
                )
                .await
            {
                Ok(outcome) => outcome,
                Err(BindingError::InvalidPayload(message)) => {
                    CommandResponseOutcome::Rejected(framework_rejection(
                        CommandRejectionClassification::InvalidRequest,
                        INVALID_PAYLOAD_CODE,
                        "The command payload is invalid.",
                        Some(serde_json::json!({ "reason": message })),
                    )?)
                }
                Err(BindingError::Execution(CommandExecutionError::Store(error)))
                    if error.kind() == EventStoreErrorKind::IdentityConflict =>
                {
                    CommandResponseOutcome::Rejected(framework_rejection(
                        CommandRejectionClassification::Conflict,
                        OPERATION_CONFLICT_CODE,
                        "The operation ID was already used for different command content or context.",
                        None,
                    )?)
                }
                Err(BindingError::Execution(CommandExecutionError::Store(error)))
                    if error.kind() == EventStoreErrorKind::InvalidRequest =>
                {
                    CommandResponseOutcome::Rejected(framework_rejection(
                        CommandRejectionClassification::InvalidRequest,
                        INVALID_COMMAND_CODE,
                        "The command cannot be executed for the requested unit of work.",
                        Some(serde_json::json!({ "reason": error.message() })),
                    )?)
                }
                Err(BindingError::Execution(error)) => {
                    return Err(CommandProcessorError::new(
                        CommandProcessorErrorKind::Unavailable,
                        error.to_string(),
                    ));
                }
                Err(BindingError::RejectionMapping(message)) => {
                    return Err(CommandProcessorError::new(
                        CommandProcessorErrorKind::InvalidConfiguration,
                        message,
                    ));
                }
            },
        };
        build_response(encoded, outcome)
    }
}

fn validate_command(
    encoded: &EncodedCommand,
    processor_context: &BoundedContextName,
) -> Result<
    (
        CommandEnvelope<RoutedCommand>,
        CommandExecutionMetadata,
        CommandBindingKey,
    ),
    CommandProcessorError,
> {
    if encoded.address().context() != processor_context.as_str() {
        return Err(CommandProcessorError::invalid_message(format!(
            "command context `{}` does not match processor context `{}`",
            encoded.address().context(),
            processor_context.as_str()
        )));
    }
    let envelope: CommandEnvelope<RoutedCommand> = serde_json::from_slice(encoded.payload())
        .map_err(|error| CommandProcessorError::invalid_message(error.to_string()))?;
    let routed = envelope.payload();
    let fingerprint = routed_command_execution_fingerprint(encoded.address().context(), routed)
        .map_err(|error| CommandProcessorError::invalid_message(error.to_string()))?;
    let expected_message_id = command_message_id(
        encoded.address(),
        envelope.operation_id(),
        fingerprint,
        envelope.correlation_id(),
        envelope.causation_id(),
    )
    .map_err(|error| CommandProcessorError::invalid_message(error.to_string()))?;
    if envelope.message_id() != encoded.message_id()
        || encoded.message_id() != &expected_message_id
        || envelope.operation_id() != encoded.operation_id()
        || envelope.correlation_id() != encoded.correlation_id()
        || envelope.schema_version().get() != routed.schema_version()
        || encoded.address().name() != routed.command()
        || encoded.fingerprint() != fingerprint
    {
        return Err(CommandProcessorError::invalid_message(
            "command envelope identity, route, or fingerprint is inconsistent",
        ));
    }

    let operation_id = CoreOperationId::new(envelope.operation_id().as_str())
        .map_err(|error| CommandProcessorError::invalid_message(error.to_string()))?;
    let mut metadata = CommandExecutionMetadata::new(operation_id, fingerprint)
        .with_bounded_context(processor_context.clone())
        .with_correlation_id(envelope.correlation_id().clone());
    let causation_id = if routed.events_caused_by_command() {
        CausationId::new(encoded.message_id().as_str())
            .map_err(|error| CommandProcessorError::invalid_message(error.to_string()))?
    } else {
        match envelope.causation_id() {
            Some(causation_id) => causation_id.clone(),
            None => CausationId::new(encoded.message_id().as_str())
                .map_err(|error| CommandProcessorError::invalid_message(error.to_string()))?,
        }
    };
    metadata = metadata.with_causation_id(causation_id);

    let key = CommandBindingKey::new(
        encoded.address().context(),
        routed.command(),
        routed.schema_version(),
    );
    Ok((envelope, metadata, key))
}

fn build_response(
    encoded: &EncodedCommand,
    outcome: CommandResponseOutcome,
) -> Result<CommandResponse, CommandProcessorError> {
    let message_id = command_response_message_id(encoded.message_id())
        .map_err(|error| CommandProcessorError::invalid_message(error.to_string()))?;
    match outcome {
        CommandResponseOutcome::Accepted => CommandResponse::accepted(
            message_id,
            encoded.message_id().clone(),
            encoded.address().clone(),
            encoded.operation_id().clone(),
            encoded.correlation_id().clone(),
        ),
        CommandResponseOutcome::Rejected(rejection) => CommandResponse::rejected(
            message_id,
            encoded.message_id().clone(),
            encoded.address().clone(),
            encoded.operation_id().clone(),
            encoded.correlation_id().clone(),
            rejection,
        ),
    }
    .map_err(|error| {
        CommandProcessorError::new(
            CommandProcessorErrorKind::InvalidConfiguration,
            error.to_string(),
        )
    })
}

fn framework_rejection(
    classification: CommandRejectionClassification,
    code: &'static str,
    message: &'static str,
    details: Option<Value>,
) -> Result<CommandRejection, CommandProcessorError> {
    let code = ApplicationErrorCode::new(code).map_err(|error| {
        CommandProcessorError::new(
            CommandProcessorErrorKind::InvalidConfiguration,
            error.to_string(),
        )
    })?;
    CommandRejection::new(classification, code, message, details).map_err(|error| {
        CommandProcessorError::new(
            CommandProcessorErrorKind::InvalidConfiguration,
            error.to_string(),
        )
    })
}

pub fn command_execution_fingerprint(
    bounded_context: &str,
    command: &str,
    schema_version: u32,
    payload: &Value,
) -> Result<ContentFingerprint, CommandBusError> {
    command_execution_fingerprint_with_provenance(
        bounded_context,
        command,
        schema_version,
        payload,
        false,
    )
}

fn command_execution_fingerprint_with_provenance(
    bounded_context: &str,
    command: &str,
    schema_version: u32,
    payload: &Value,
    events_caused_by_command: bool,
) -> Result<ContentFingerprint, CommandBusError> {
    let schema_version = schema_version.to_be_bytes();
    let payload = canonical_json_bytes(payload)?;
    let provenance = [u8::from(events_caused_by_command)];
    Ok(framed_fingerprint(&[
        b"rostfrei:command-execution:v2",
        bounded_context.as_bytes(),
        command.as_bytes(),
        &schema_version,
        &payload,
        &provenance,
    ]))
}

fn routed_command_execution_fingerprint(
    bounded_context: &str,
    command: &RoutedCommand,
) -> Result<ContentFingerprint, CommandBusError> {
    command_execution_fingerprint_with_provenance(
        bounded_context,
        command.command(),
        command.schema_version(),
        command.payload(),
        command.events_caused_by_command(),
    )
}

pub fn command_message_id(
    address: &CommandAddress,
    operation_id: &OperationId,
    fingerprint: ContentFingerprint,
    correlation_id: &CorrelationId,
    causation_id: Option<&CausationId>,
) -> Result<MessageId, CommandBusError> {
    let empty = [];
    let causation = causation_id.map_or(empty.as_slice(), |value| value.as_str().as_bytes());
    let digest = framed_fingerprint(&[
        b"rostfrei:command-message:v1",
        address.as_str().as_bytes(),
        operation_id.as_str().as_bytes(),
        fingerprint.as_bytes(),
        correlation_id.as_str().as_bytes(),
        causation,
    ]);
    MessageId::new(digest.to_hex()).map_err(|error| CommandBusError::encoding(error.to_string()))
}

pub fn command_response_message_id(
    command_message_id: &MessageId,
) -> Result<MessageId, ContractError> {
    let digest = framed_fingerprint(&[
        b"rostfrei:command-response-message:v1",
        command_message_id.as_str().as_bytes(),
    ]);
    MessageId::new(digest.to_hex())
}

pub fn canonical_serialize<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, CommandBusError> {
    let value = serde_json::to_value(value)
        .map_err(|error| CommandBusError::encoding(error.to_string()))?;
    canonical_json_bytes(&value)
}

pub fn canonical_json_bytes(value: &Value) -> Result<Vec<u8>, CommandBusError> {
    fn write_value(value: &Value, output: &mut Vec<u8>) -> Result<(), serde_json::Error> {
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                serde_json::to_writer(output, value)
            }
            Value::Array(values) => {
                output.push(b'[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        output.push(b',');
                    }
                    write_value(value, output)?;
                }
                output.push(b']');
                Ok(())
            }
            Value::Object(values) => {
                output.push(b'{');
                let mut entries = values.iter().collect::<Vec<_>>();
                entries.sort_unstable_by_key(|(key, _)| *key);
                for (index, (key, value)) in entries.into_iter().enumerate() {
                    if index > 0 {
                        output.push(b',');
                    }
                    serde_json::to_writer(&mut *output, key)?;
                    output.push(b':');
                    write_value(value, output)?;
                }
                output.push(b'}');
                Ok(())
            }
        }
    }

    let mut encoded = Vec::new();
    write_value(value, &mut encoded)
        .map_err(|error| CommandBusError::encoding(error.to_string()))?;
    Ok(encoded)
}

pub fn framed_fingerprint(parts: &[&[u8]]) -> ContentFingerprint {
    let capacity = parts.iter().fold(0_usize, |total, part| {
        total.saturating_add(8).saturating_add(part.len())
    });
    let mut framed = Vec::with_capacity(capacity);
    for part in parts {
        let length = u64::try_from(part.len()).unwrap_or(u64::MAX);
        framed.extend_from_slice(&length.to_be_bytes());
        framed.extend_from_slice(part);
    }
    ContentFingerprint::digest(framed)
}

fn current_timestamp() -> Result<MessageTimestamp, CommandBusError> {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CommandBusError::encoding("system clock is before the Unix epoch"))?
        .as_millis();
    let milliseconds = u64::try_from(milliseconds).map_err(|_| {
        CommandBusError::encoding("system clock is outside the message timestamp range")
    })?;
    MessageTimestamp::from_unix_milliseconds(milliseconds)
        .map_err(|error| CommandBusError::encoding(error.to_string()))
}
