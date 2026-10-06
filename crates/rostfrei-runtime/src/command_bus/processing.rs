//! Policy-enabled processing and explicitly isolated trusted-infrastructure compatibility.
use std::time::Duration;

use rostfrei_core::{
    BoundedConflictRetries, CompletedCommit, CompletedOutcome, ConflictRetryPolicy,
    EncodedOutcomeDetails, JsonRejectionCodec, OutcomeKey, OutcomeLookup, RejectionCodec,
};

use super::{
    AggregateCodecs, Arc, BindingError, BoundedContext, CommandExecutionMetadata, CommandExecutor,
    CommandProcessor, CommandProcessorError, CommandProcessorErrorKind, CommandRejection,
    CommandRejectionClassification, CommandResponse, CommandResponseOutcome, EncodedCommand,
    ExecutionStore, FixedRejection, INVALID_PAYLOAD_CODE, UNKNOWN_COMMAND_CODE, async_trait,
    build_response, framework_rejection, validate_command,
};
use crate::command_service::{
    AttemptId, AuditDelivery, AuditError, AuditPhase, AuditTerminal, CommandAccess,
    CommandAuditRecord, CommandAuditSink, CommandAuthorizer, CommandProcessingService,
    CompletionOrigin, ProcessedCommand, ServiceReport, TrustedCommandContext, ValidatedCommand,
};

pub(super) struct Configuration {
    pub(super) shared_codecs: bool,
    route: Option<BoundedContext>,
    authorizer: Option<Arc<dyn CommandAuthorizer>>,
    pub(super) rejection_codec: Arc<dyn RejectionCodec<CommandRejection>>,
    retry: Arc<dyn ConflictRetryPolicy>,
    audit: Option<Arc<dyn CommandAuditSink>>,
    timeout: Duration,
}
impl Configuration {
    pub(super) fn trusted() -> Self {
        Self {
            shared_codecs: false,
            route: None,
            authorizer: None,
            rejection_codec: Arc::new(JsonRejectionCodec::new("rostfrei.command-rejection", 1)),
            retry: Arc::new(BoundedConflictRetries::new(3)),
            audit: None,
            timeout: Duration::from_secs(1),
        }
    }
}

#[must_use]
pub struct CommandProcessorBuilder {
    processor: CommandProcessor,
}
impl CommandProcessorBuilder {
    pub fn authorizer(mut self, policy: Arc<dyn CommandAuthorizer>) -> Self {
        self.processor.configuration.authorizer = Some(policy);
        self
    }
    pub fn event_codecs(mut self, codecs: Arc<AggregateCodecs>) -> Self {
        self.processor.codecs = codecs;
        self.processor.configuration.shared_codecs = true;
        self
    }
    pub fn rejection_codec(mut self, codec: Arc<dyn RejectionCodec<CommandRejection>>) -> Self {
        self.processor.configuration.rejection_codec = codec;
        self
    }
    pub fn conflict_retry_policy(mut self, policy: Arc<dyn ConflictRetryPolicy>) -> Self {
        self.processor.configuration.retry = policy;
        self
    }
    pub fn audit_sink(mut self, sink: Arc<dyn CommandAuditSink>) -> Self {
        self.processor.configuration.audit = Some(sink);
        self
    }
    pub const fn audit_timeout(mut self, timeout: Duration) -> Self {
        self.processor.configuration.timeout = timeout;
        self
    }
    pub fn build(self) -> Result<CommandProcessor, CommandProcessorError> {
        if self.processor.configuration.authorizer.is_none()
            || self.processor.configuration.timeout.is_zero()
        {
            return Err(configuration(
                "an explicit authorizer and a positive audit timeout are required",
            ));
        }
        Ok(self.processor)
    }
}

fn configuration(message: impl Into<String>) -> CommandProcessorError {
    CommandProcessorError::new(CommandProcessorErrorKind::InvalidConfiguration, message)
}

impl CommandProcessor {
    pub fn builder(
        context: BoundedContext,
        backend: Arc<dyn ExecutionStore>,
    ) -> CommandProcessorBuilder {
        let mut processor = Self::new(context.name().clone(), backend);
        processor.configuration.route = Some(context);
        CommandProcessorBuilder { processor }
    }

    pub(super) fn require_trusted_profile(&self) -> Result<(), CommandProcessorError> {
        if self.configuration.authorizer.is_some() || self.configuration.route.is_some() {
            return Err(configuration(
                "policy-enabled processing requires trusted caller context",
            ));
        }
        Ok(())
    }

    fn validate_service(&self, encoded: &EncodedCommand) -> Result<(), CommandProcessorError> {
        validate_command(encoded, &self.context)?;
        let route = self.configuration.route.as_ref().ok_or_else(|| {
            configuration("use the processor builder for context-aware processing")
        })?;
        if encoded.address().application() != route.application().as_str()
            || encoded.address().traffic_scope() != route.traffic_scope()
        {
            return Err(CommandProcessorError::invalid_message(
                "command application or traffic scope differs from processor route",
            ));
        }
        Ok(())
    }

    async fn lookup_metadata(
        &self,
        encoded: &EncodedCommand,
        metadata: &CommandExecutionMetadata,
    ) -> Result<OutcomeLookup<CommandResponse>, CommandProcessorError> {
        let key = OutcomeKey::new(
            self.store.authority().clone(),
            self.context.clone(),
            metadata.operation_id().clone(),
        );
        match self
            .store
            .lookup_outcome(&key)
            .await
            .map_err(|e| CommandProcessorError::execution(&e.in_outcome_recovery().into()))?
        {
            OutcomeLookup::Missing => Ok(OutcomeLookup::Missing),
            OutcomeLookup::Completed(outcome) => {
                outcome.validate_for(&key, metadata).map_err(|e| {
                    CommandProcessorError::execution(&e.in_outcome_recovery().into())
                })?;
                Ok(OutcomeLookup::Completed(self.project(encoded, outcome)?))
            }
        }
    }

    fn project(
        &self,
        encoded: &EncodedCommand,
        outcome: CompletedOutcome<EncodedOutcomeDetails>,
    ) -> Result<CompletedOutcome<CommandResponse>, CommandProcessorError> {
        outcome.try_map(|details| {
            let response = match details.rejection() {
                Some(rejection) => CommandResponseOutcome::Rejected(
                    self.configuration
                        .rejection_codec
                        .decode(rejection)
                        .map_err(|e| {
                            CommandProcessorError::new(
                                CommandProcessorErrorKind::CompletedDetailsBlocked,
                                e.to_string(),
                            )
                        })?,
                ),
                None => CommandResponseOutcome::Accepted,
            };
            build_response(encoded, response)
        })
    }

    pub(super) async fn execute_missing(
        &self,
        encoded: &EncodedCommand,
        metadata: CommandExecutionMetadata,
    ) -> Result<ProcessedCommand, CommandProcessorError> {
        let (envelope, _, key) = validate_command(encoded, &self.context)?;
        let routed = envelope.payload();
        let rejection = match self.bindings.get(&key) {
            Some(binding) => match binding
                .execute(
                    self.store.clone(),
                    self.codecs.clone(),
                    self.configuration.retry.clone(),
                    self.configuration.rejection_codec.clone(),
                    metadata.clone(),
                    routed.payload(),
                )
                .await
            {
                Ok(commit) => return self.project_commit(encoded, commit),
                Err(BindingError::Execution(error)) => {
                    return Err(CommandProcessorError::execution(&error));
                }
                Err(BindingError::InvalidPayload(reason)) => framework_rejection(
                    CommandRejectionClassification::InvalidRequest,
                    INVALID_PAYLOAD_CODE,
                    "The command payload is invalid.",
                    Some(serde_json::json!({"reason": reason})),
                )?,
            },
            None => framework_rejection(
                CommandRejectionClassification::InvalidRequest,
                UNKNOWN_COMMAND_CODE,
                "The command name or schema version is not registered.",
                Some(serde_json::json!({
                    "context": encoded.address().context(), "command": routed.command(), "schema_version": routed.schema_version()
                })),
            )?,
        };
        let commit = CommandExecutor::with_codecs(self.store.clone(), self.codecs.clone())
            .with_conflict_retry_policy(self.configuration.retry.clone())
            .execute_completed_with_rejection_codec(
                &FixedRejection(rejection),
                metadata,
                &(),
                self.configuration.rejection_codec.as_ref(),
            )
            .await
            .map_err(|e| CommandProcessorError::execution(&e))?;
        self.project_commit(encoded, commit)
    }

    fn project_commit(
        &self,
        encoded: &EncodedCommand,
        commit: CompletedCommit,
    ) -> Result<ProcessedCommand, CommandProcessorError> {
        let (origin, outcome) = match commit {
            CompletedCommit::Appended(outcome) => (CompletionOrigin::Committed, outcome),
            CompletedCommit::ExactReplay(outcome) => (CompletionOrigin::Replayed, outcome),
        };
        Ok(ProcessedCommand {
            origin,
            outcome: self.project(encoded, outcome)?,
        })
    }

    async fn authorized_lookup(
        &self,
        caller: &TrustedCommandContext,
        encoded: &EncodedCommand,
    ) -> Result<OutcomeLookup<CommandResponse>, CommandProcessorError> {
        let policy = self
            .configuration
            .authorizer
            .as_ref()
            .ok_or_else(|| configuration("no authorization policy"))?;
        policy
            .authorize(
                caller,
                &ValidatedCommand {
                    original: encoded.clone(),
                },
                CommandAccess::ReadOutcome,
            )
            .await
            .map_err(CommandProcessorError::authorization)?;
        let (_, metadata, _) = validate_command(encoded, &self.context)?;
        self.lookup_metadata(
            encoded,
            &metadata.with_security_scope(caller.security_scope().clone()),
        )
        .await
    }

    async fn process_authorized(
        &self,
        caller: &TrustedCommandContext,
        encoded: &EncodedCommand,
    ) -> Result<ProcessedCommand, CommandProcessorError> {
        match self.authorized_lookup(caller, encoded).await? {
            OutcomeLookup::Completed(outcome) => Ok(ProcessedCommand {
                origin: CompletionOrigin::Replayed,
                outcome,
            }),
            OutcomeLookup::Missing => {
                let policy = self
                    .configuration
                    .authorizer
                    .as_ref()
                    .ok_or_else(|| configuration("no authorization policy"))?;
                policy
                    .authorize(
                        caller,
                        &ValidatedCommand {
                            original: encoded.clone(),
                        },
                        CommandAccess::Execute,
                    )
                    .await
                    .map_err(CommandProcessorError::authorization)?;
                let (_, metadata, _) = validate_command(encoded, &self.context)?;
                self.execute_missing(
                    encoded,
                    metadata.with_security_scope(caller.security_scope().clone()),
                )
                .await
            }
        }
    }

    async fn audit(&self, record: &CommandAuditRecord, errors: &mut Vec<(AuditPhase, AuditError)>) {
        if let Some(sink) = &self.configuration.audit {
            let result = tokio::time::timeout(self.configuration.timeout, sink.record(record))
                .await
                .unwrap_or_else(|_| Err(AuditError::new("audit delivery timed out")));
            if let Err(error) = result {
                errors.push((record.phase, error));
            }
        }
    }

    async fn report<T>(
        &self,
        caller: &TrustedCommandContext,
        encoded: &EncodedCommand,
        operation: impl std::future::Future<Output = Result<T, CommandProcessorError>> + Send,
        terminal: impl FnOnce(&T) -> AuditTerminal + Send,
    ) -> ServiceReport<T> {
        let attempt = AttemptId::generate();
        if let Err(error) = self.validate_service(encoded) {
            return ServiceReport {
                result: Err(error),
                attempt,
                audit: AuditDelivery::Disabled,
            };
        }
        let mut record = CommandAuditRecord {
            attempt: attempt.clone(),
            phase: AuditPhase::Started,
            terminal: None,
            command: encoded.message_id().clone(),
            original: encoded.payload().to_vec(),
            caller: caller.clone(),
        };
        let mut errors = Vec::new();
        self.audit(&record, &mut errors).await;
        let result = operation.await;
        record.phase = AuditPhase::Finished;
        record.terminal = Some(match &result {
            Ok(value) => terminal(value),
            Err(error)
                if matches!(
                    error.authorization_error(),
                    Some(
                        crate::AuthorizationError::Denied
                            | crate::AuthorizationError::Unauthenticated
                    )
                ) =>
            {
                AuditTerminal::Denied
            }
            Err(_) => AuditTerminal::Failed,
        });
        self.audit(&record, &mut errors).await;
        let audit = if self.configuration.audit.is_none() {
            AuditDelivery::Disabled
        } else if errors.is_empty() {
            AuditDelivery::Delivered
        } else {
            AuditDelivery::Failed { records: errors }
        };
        ServiceReport {
            result,
            attempt,
            audit,
        }
    }
}

const fn availability_terminal(
    outcome: &CompletedOutcome<CommandResponse>,
    available: AuditTerminal,
) -> AuditTerminal {
    match outcome.details() {
        rostfrei_core::OutcomeDetails::Available(_) => available,
        rostfrei_core::OutcomeDetails::Expired => AuditTerminal::Expired,
        _ => AuditTerminal::Unavailable,
    }
}

#[async_trait]
impl CommandProcessingService for CommandProcessor {
    async fn process(
        &self,
        caller: &TrustedCommandContext,
        encoded: &EncodedCommand,
    ) -> ServiceReport<ProcessedCommand> {
        self.report(
            caller,
            encoded,
            self.process_authorized(caller, encoded),
            |processed| {
                availability_terminal(
                    processed.outcome(),
                    match processed.origin() {
                        CompletionOrigin::Committed => AuditTerminal::Committed,
                        CompletionOrigin::Replayed => AuditTerminal::Replayed,
                    },
                )
            },
        )
        .await
    }
    async fn lookup(
        &self,
        caller: &TrustedCommandContext,
        encoded: &EncodedCommand,
    ) -> ServiceReport<OutcomeLookup<CommandResponse>> {
        self.report(
            caller,
            encoded,
            self.authorized_lookup(caller, encoded),
            |lookup| match lookup {
                OutcomeLookup::Missing => AuditTerminal::Missing,
                OutcomeLookup::Completed(outcome) => {
                    availability_terminal(outcome, AuditTerminal::Replayed)
                }
            },
        )
        .await
    }
}
