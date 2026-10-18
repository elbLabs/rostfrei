//! Technical snapshots have a stable logical ID and a separate CAS publication identity.
use std::time::Duration;

use async_nats::{
    HeaderMap,
    jetstream::{self, message::PublishMessage},
};
use rostfrei_messaging_core::{
    CommandResponse, CommandResponseAddress, CommandResponseOutcome, MessageId, OutboundMessage,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::time::timeout;

use crate::{
    error::NatsError,
    messaging_config::StreamName,
    publish::NatsPublishAck,
    publish::{
        CONTENT_TYPE_HEADER, CORRELATION_ID_HEADER, JSON_CONTENT_TYPE, classify_publish_error,
    },
};

pub const DIAGNOSTIC_TRANSITION_HEADER: &str = "rostfrei-Control-Response-Transition";

/// Broker generation/predecessor evidence, not operation identity or a business outcome.
///
/// Its publication ID binds the token, subject and exact payload. Only preparation against
/// a live stream creates a publishable request; deserializing a token is not authority to write.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticTransitionToken {
    version: u8,
    stream: String,
    created: i128,
    generation: u64,
    predecessor: u64,
}

impl DiagnosticTransitionToken {
    pub const fn generation(&self) -> u64 {
        self.generation
    }
    pub const fn predecessor(&self) -> u64 {
        self.predecessor
    }

    fn validate(&self, stream: &str, created: i128) -> Result<(), NatsError> {
        if self.version != 1
            || self.stream != stream
            || self.created <= 0
            || self.created != created
            || self.predecessor > self.generation
        {
            return Err(NatsError::InvalidMessage);
        }
        Ok(())
    }

    fn publication_id(&self, subject: &str, payload: &[u8]) -> Result<MessageId, NatsError> {
        let token = serde_json::to_vec(self).map_err(|_| NatsError::InvalidMessage)?;
        let mut hash = Sha256::new();
        for frame in [
            b"rostfrei:diagnostic-publication:v1".as_slice(),
            subject.as_bytes(),
            payload,
            &token,
        ] {
            hash.update(
                u64::try_from(frame.len())
                    .map_err(|_| NatsError::InvalidMessage)?
                    .to_be_bytes(),
            );
            hash.update(frame);
        }
        MessageId::new(format!("{:x}", hash.finalize())).map_err(|_| NatsError::InvalidMessage)
    }
}

/// Immutable prepared publication. Retry this SAME value after uncertain delivery; do not
/// prepare a fresh generation until reconciliation/re-evaluation chooses a new transition.
///
/// Payload, headers, broker ID and subject CAS expectation are retained together.
/// Coordinate/fence stream administration with all publishers and prepared attempts.
/// Epoch checks are observations, not an atomic administrative lock.
#[derive(Clone, Debug)]
pub struct PreparedCommandResponse {
    message: OutboundMessage<CommandResponseAddress>,
    response: CommandResponse,
    headers: HeaderMap,
    stream: StreamName,
    created: i128,
    predecessor: u64,
    publication_id: MessageId,
    transition: Option<DiagnosticTransitionToken>,
    observed_exact: bool,
}

impl PreparedCommandResponse {
    pub const fn publication_message_id(&self) -> &MessageId {
        &self.publication_id
    }
    pub const fn transition(&self) -> Option<&DiagnosticTransitionToken> {
        self.transition.as_ref()
    }
    pub const fn message(&self) -> &OutboundMessage<CommandResponseAddress> {
        &self.message
    }
}

#[derive(Clone, Copy)]
pub struct StoredPublication<'a> {
    pub stream: &'a str,
    pub created: i128,
    pub sequence: u64,
}

/// Validates the wire publication identity independently of the logical response identity.
pub fn validate_stored_publication(
    response: &CommandResponse,
    subject: &str,
    payload: &[u8],
    headers: &HeaderMap,
    stored: StoredPublication<'_>,
) -> Result<Option<DiagnosticTransitionToken>, NatsError> {
    let outer = one_header(headers, "Nats-Msg-Id")?.ok_or(NatsError::InvalidMessage)?;
    if !matches!(
        response.outcome(),
        CommandResponseOutcome::ExecutionFailed(_)
    ) {
        if outer != response.message_id().as_str() {
            return Err(NatsError::IdentityConflict);
        }
        return Ok(None);
    }
    // The new technical protocol requires one matching outer correlation value.
    // Legacy business readers keep their historical optional-header contract.
    if one_header(headers, CORRELATION_ID_HEADER)? != Some(response.correlation_id().as_str()) {
        return Err(NatsError::IdentityConflict);
    }
    validate_technical_logical_identity(response)?;
    let encoded =
        one_header(headers, DIAGNOSTIC_TRANSITION_HEADER)?.ok_or(NatsError::InvalidMessage)?;
    let token: DiagnosticTransitionToken =
        serde_json::from_str(encoded).map_err(|_| NatsError::InvalidMessage)?;
    token.validate(stored.stream, stored.created)?;
    if token.generation >= stored.sequence
        || serde_json::to_string(&token).map_err(|_| NatsError::InvalidMessage)? != encoded
        || token.publication_id(subject, payload)?.as_str() != outer
    {
        return Err(NatsError::IdentityConflict);
    }
    Ok(Some(token))
}

pub fn validate_technical_logical_identity(response: &CommandResponse) -> Result<(), NatsError> {
    if let CommandResponseOutcome::ExecutionFailed(failure) = response.outcome()
        && response.message_id()
            != &rostfrei::command_technical_response_message_id(
                response.command_message_id(),
                *failure,
            )
            .map_err(|_| NatsError::InvalidMessage)?
    {
        return Err(NatsError::IdentityConflict);
    }
    Ok(())
}

pub async fn prepare(
    context: &jetstream::Context,
    message: OutboundMessage<CommandResponseAddress>,
    response: CommandResponse,
    stream_name: &StreamName,
    mut headers: HeaderMap,
    operation_timeout: Duration,
) -> Result<PreparedCommandResponse, NatsError> {
    if operation_timeout.is_zero() {
        return Err(NatsError::Configuration);
    }
    validate_technical_logical_identity(&response)?;
    timeout(operation_timeout, async {
        let stream = context
            .get_stream(stream_name.as_str())
            .await
            .map_err(|_| NatsError::StreamNotFound)?;
        let info = stream.cached_info();
        let created = info.created.unix_timestamp_nanos();
        let stored = crate::response_storage::latest(
            context,
            stream_name.as_str(),
            message.address().as_str(),
        )
        .await?;
        let predecessor = stored.as_ref().map_or(0, |value| value.sequence);
        let generation = info.state.last_sequence.max(predecessor);
        let mut publication_id = message.message_id().clone();
        let mut transition = None;
        let mut observed_exact = false;
        if let Some(stored) = &stored {
            let previous = decode_existing(stored, stream_name.as_str(), created, &response)?;
            if stored.payload.as_ref() == message.payload() && previous == response {
                publication_id = MessageId::new(
                    one_header(&stored.headers, "Nats-Msg-Id")?.ok_or(NatsError::InvalidMessage)?,
                )
                .map_err(|_| NatsError::InvalidMessage)?;
                transition = validate_stored_publication(
                    &previous,
                    stored.subject.as_str(),
                    &stored.payload,
                    &stored.headers,
                    StoredPublication {
                        stream: stream_name.as_str(),
                        created,
                        sequence: stored.sequence,
                    },
                )?;
                observed_exact = true;
            } else if !matches!(
                previous.outcome(),
                CommandResponseOutcome::ExecutionFailed(_)
            ) {
                return Err(NatsError::IdentityConflict);
            }
        }
        if !observed_exact
            && matches!(
                response.outcome(),
                CommandResponseOutcome::ExecutionFailed(_)
            )
        {
            let token = DiagnosticTransitionToken {
                version: 1,
                stream: stream_name.as_str().to_owned(),
                created,
                generation,
                predecessor,
            };
            token.validate(stream_name.as_str(), created)?;
            publication_id = token.publication_id(message.address().as_str(), message.payload())?;
            transition = Some(token);
        }
        headers.insert(CONTENT_TYPE_HEADER, JSON_CONTENT_TYPE);
        if let Some(token) = &transition {
            headers.insert(
                DIAGNOSTIC_TRANSITION_HEADER,
                serde_json::to_string(token).map_err(|_| NatsError::InvalidMessage)?,
            );
        }
        Ok(PreparedCommandResponse {
            message,
            response,
            headers,
            stream: stream_name.clone(),
            created,
            predecessor,
            publication_id,
            transition,
            observed_exact,
        })
    })
    .await
    .map_err(|_| NatsError::PublishTimeout)?
}

pub async fn publish(
    context: &jetstream::Context,
    prepared: &PreparedCommandResponse,
    expected_stream: &StreamName,
    operation_timeout: Duration,
) -> Result<NatsPublishAck, NatsError> {
    if operation_timeout.is_zero() {
        return Err(NatsError::Configuration);
    }
    if &prepared.stream != expected_stream {
        return Err(NatsError::InvalidMessage);
    }
    timeout(operation_timeout, async {
        let stream = context
            .get_stream(expected_stream.as_str())
            .await
            .map_err(|_| NatsError::StreamNotFound)?;
        if stream.cached_info().created.unix_timestamp_nanos() != prepared.created {
            return Err(NatsError::IdentityConflict);
        }
        if prepared.observed_exact {
            return reconcile(context, prepared).await;
        }
        let request = PublishMessage::build()
            .payload(prepared.message.payload().to_vec().into())
            .headers(prepared.headers.clone())
            .message_id(prepared.publication_id.as_str())
            .expected_stream(expected_stream.as_str())
            .expected_last_subject_sequence(prepared.predecessor);
        let result = context
            .send_publish(prepared.message.address().as_str().to_owned(), request)
            .await
            .map_err(|error| classify_publish_error(&error))?
            .await
            .map_err(|error| classify_publish_error(&error));
        match result {
            Ok(ack) if !ack.duplicate => {
                if ack.stream != expected_stream.as_str()
                    || ack.sequence == 0
                    || prepared
                        .transition
                        .as_ref()
                        .is_some_and(|token| ack.sequence <= token.generation)
                {
                    return Err(NatsError::PublishExpectation);
                }
                // A fresh broker Ack alone is insufficient: administration may have recreated
                // this name after the precheck, or removed the acknowledged record. Validate
                // the exact sequence (not the latest snapshot, which can legitimately advance)
                // and then re-observe the current incarnation before reporting usable evidence.
                let stored = crate::response_storage::sequence(
                    context,
                    expected_stream.as_str(),
                    ack.sequence,
                )
                .await?
                .ok_or(NatsError::Publish)?;
                if stored.sequence != ack.sequence {
                    return Err(NatsError::IdentityConflict);
                }
                verify_exact_record(&stored, prepared).map_err(|_| NatsError::IdentityConflict)?;
                verify_current_incarnation(context, prepared).await?;
                Ok(NatsPublishAck {
                    stream: expected_stream.clone(),
                    sequence: ack.sequence,
                    duplicate: false,
                })
            }
            Ok(_) | Err(NatsError::PublishExpectation) => reconcile(context, prepared).await,
            Err(error) => Err(error),
        }
    })
    .await
    .map_err(|_| NatsError::PublishTimeout)?
}

async fn reconcile(
    context: &jetstream::Context,
    prepared: &PreparedCommandResponse,
) -> Result<NatsPublishAck, NatsError> {
    let stored = crate::response_storage::latest(
        context,
        prepared.stream.as_str(),
        prepared.message.address().as_str(),
    )
    .await?
    .ok_or(NatsError::PublishExpectation)?;
    verify_exact_record(&stored, prepared)?;
    verify_current_incarnation(context, prepared).await?;
    // A duplicate acknowledgement is never evidence of fresh durability. Verify the retained
    // bytes AND validated publication identity; an absent/deleted/superseded record is not success.
    Ok(NatsPublishAck {
        stream: prepared.stream.clone(),
        sequence: stored.sequence,
        duplicate: true,
    })
}

fn verify_exact_record(
    stored: &jetstream::message::StreamMessage,
    prepared: &PreparedCommandResponse,
) -> Result<(), NatsError> {
    let previous = decode_existing(
        stored,
        prepared.stream.as_str(),
        prepared.created,
        &prepared.response,
    )?;
    if stored.payload.as_ref() != prepared.message.payload()
        || previous != prepared.response
        || one_header(&stored.headers, "Nats-Msg-Id")? != Some(prepared.publication_id.as_str())
    {
        return Err(
            if matches!(
                previous.outcome(),
                CommandResponseOutcome::ExecutionFailed(_)
            ) {
                NatsError::PublishExpectation
            } else {
                NatsError::IdentityConflict
            },
        );
    }
    Ok(())
}

async fn verify_current_incarnation(
    context: &jetstream::Context,
    prepared: &PreparedCommandResponse,
) -> Result<(), NatsError> {
    let current = context
        .get_stream(prepared.stream.as_str())
        .await
        .map_err(|_| NatsError::StreamNotFound)?;
    if current.cached_info().created.unix_timestamp_nanos() != prepared.created {
        return Err(NatsError::IdentityConflict);
    }
    // This does not fence an administrator acting AFTER this observation. Administrative
    // recreation/deletion still requires coordinated publisher/prepared-attempt quiescence.
    Ok(())
}

fn decode_existing(
    stored: &jetstream::message::StreamMessage,
    stream: &str,
    created: i128,
    response: &CommandResponse,
) -> Result<CommandResponse, NatsError> {
    if stored.sequence == 0
        || one_header(&stored.headers, CONTENT_TYPE_HEADER)? != Some(JSON_CONTENT_TYPE)
        || one_header(&stored.headers, CORRELATION_ID_HEADER)?
            != Some(response.correlation_id().as_str())
    {
        return Err(NatsError::IdentityConflict);
    }
    let previous: CommandResponse =
        serde_json::from_slice(&stored.payload).map_err(|_| NatsError::InvalidMessage)?;
    if previous.command_message_id() != response.command_message_id()
        || previous.command_address() != response.command_address()
        || previous.operation_id() != response.operation_id()
        || previous.correlation_id() != response.correlation_id()
        || stored.subject.as_str()
            != rostfrei_messaging_core::derive_command_response_address(
                previous.command_address(),
                previous.operation_id(),
                previous.command_message_id(),
            )
            .map_err(|_| NatsError::InvalidMessage)?
            .as_str()
    {
        return Err(NatsError::IdentityConflict);
    }
    validate_stored_publication(
        &previous,
        stored.subject.as_str(),
        &stored.payload,
        &stored.headers,
        StoredPublication {
            stream,
            created,
            sequence: stored.sequence,
        },
    )?;
    Ok(previous)
}

fn one_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, NatsError> {
    let mut values = headers.get_all(name.to_owned());
    let first = values.next().map(async_nats::HeaderValue::as_str);
    if values.next().is_some() {
        return Err(NatsError::InvalidMessage);
    }
    Ok(first)
}
