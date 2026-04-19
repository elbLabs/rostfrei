use std::time::Duration;

use async_nats::{HeaderMap, jetstream};
use async_trait::async_trait;
use rostfrei_messaging_core::{
    CommandResponse, CommandResponseAddress, CommandResponseReadError,
    CommandResponseReadErrorKind, CommandResponseReader, MAX_COMMAND_RESPONSE_TIMEOUT,
    MAX_ENVELOPE_BYTES, MessageId, OperationId, derive_command_response_address,
};
use tokio::time::{sleep, timeout};

use crate::{
    error::NatsError,
    messaging_config::MessagingTopology,
    publish::{CONTENT_TYPE_HEADER, JSON_CONTENT_TYPE},
};

pub const DEFAULT_COMMAND_RESPONSE_POLL_INTERVAL: Duration = Duration::from_millis(25);
pub const MAX_COMMAND_RESPONSE_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub struct NatsCommandResponseReader {
    context: jetstream::Context,
    topology: MessagingTopology,
    poll_interval: Duration,
}

impl NatsCommandResponseReader {
    pub const fn new(context: jetstream::Context, topology: MessagingTopology) -> Self {
        Self {
            context,
            topology,
            poll_interval: DEFAULT_COMMAND_RESPONSE_POLL_INTERVAL,
        }
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Result<Self, NatsError> {
        if poll_interval.is_zero() || poll_interval > MAX_COMMAND_RESPONSE_POLL_INTERVAL {
            return Err(NatsError::Configuration);
        }
        self.poll_interval = poll_interval;
        Ok(self)
    }

    pub const fn poll_interval(&self) -> Duration {
        self.poll_interval
    }
}

#[async_trait]
impl CommandResponseReader for NatsCommandResponseReader {
    async fn read_command_response(
        &self,
        address: &CommandResponseAddress,
        expected_operation_id: &OperationId,
        expected_command_message_id: &MessageId,
        read_timeout: Duration,
    ) -> Result<CommandResponse, CommandResponseReadError> {
        validate_read_configuration(address, self.topology.application().as_str(), read_timeout)?;

        timeout(read_timeout, async {
            let stream = self
                .context
                .get_stream(self.topology.command_response_stream().as_str())
                .await
                .map_err(|_| read_error(CommandResponseReadErrorKind::Unavailable))?;
            let mut poll_interval = self.poll_interval;
            loop {
                match stream
                    .get_last_raw_message_by_subject(address.as_str())
                    .await
                {
                    Ok(stored) => {
                        return decode_stored_command_response(
                            stored.subject.as_str(),
                            &stored.headers,
                            &stored.payload,
                            address,
                            expected_operation_id,
                            expected_command_message_id,
                        );
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            jetstream::stream::LastRawMessageErrorKind::NoMessageFound
                        ) =>
                    {
                        sleep(poll_interval).await;
                        poll_interval = next_poll_interval(poll_interval);
                    }
                    Err(_) => {
                        return Err(read_error(CommandResponseReadErrorKind::Unavailable));
                    }
                }
            }
        })
        .await
        .map_err(|_| read_error(CommandResponseReadErrorKind::Timeout))?
    }
}

fn validate_read_configuration(
    address: &CommandResponseAddress,
    application: &str,
    read_timeout: Duration,
) -> Result<(), CommandResponseReadError> {
    if address.application() != application
        || read_timeout.is_zero()
        || read_timeout > MAX_COMMAND_RESPONSE_TIMEOUT
    {
        return Err(read_error(
            CommandResponseReadErrorKind::InvalidConfiguration,
        ));
    }
    Ok(())
}

fn decode_stored_command_response(
    subject: &str,
    headers: &HeaderMap,
    payload: &[u8],
    expected_address: &CommandResponseAddress,
    expected_operation_id: &OperationId,
    expected_command_message_id: &MessageId,
) -> Result<CommandResponse, CommandResponseReadError> {
    if subject != expected_address.as_str() {
        return Err(read_error(CommandResponseReadErrorKind::IdentityConflict));
    }
    if payload.len() > MAX_ENVELOPE_BYTES
        || one_header(headers, CONTENT_TYPE_HEADER)? != Some(JSON_CONTENT_TYPE)
    {
        return Err(read_error(CommandResponseReadErrorKind::InvalidResponse));
    }
    let response: CommandResponse = serde_json::from_slice(payload)
        .map_err(|_| read_error(CommandResponseReadErrorKind::InvalidResponse))?;
    let outer_message_id = one_header(headers, "Nats-Msg-Id")?
        .ok_or_else(|| read_error(CommandResponseReadErrorKind::InvalidResponse))?;
    let outer_message_id = MessageId::new(outer_message_id)
        .map_err(|_| read_error(CommandResponseReadErrorKind::InvalidResponse))?;
    if response.message_id() != &outer_message_id {
        return Err(read_error(CommandResponseReadErrorKind::IdentityConflict));
    }
    if response.operation_id() != expected_operation_id
        || response.command_message_id() != expected_command_message_id
    {
        return Err(read_error(CommandResponseReadErrorKind::IdentityConflict));
    }
    let derived_address = derive_command_response_address(
        response.command_address(),
        response.operation_id(),
        response.command_message_id(),
    )
    .map_err(|_| read_error(CommandResponseReadErrorKind::InvalidResponse))?;
    if &derived_address != expected_address {
        return Err(read_error(CommandResponseReadErrorKind::IdentityConflict));
    }
    Ok(response)
}

fn next_poll_interval(current: Duration) -> Duration {
    current
        .saturating_mul(2)
        .min(MAX_COMMAND_RESPONSE_POLL_INTERVAL)
}

fn one_header<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> Result<Option<&'a str>, CommandResponseReadError> {
    let mut values = headers.get_all(name.to_owned());
    let first = values.next().map(async_nats::HeaderValue::as_str);
    if values.next().is_some() {
        return Err(read_error(CommandResponseReadErrorKind::InvalidResponse));
    }
    Ok(first)
}

const fn read_error(kind: CommandResponseReadErrorKind) -> CommandResponseReadError {
    CommandResponseReadError::new(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rostfrei_messaging_core::{
        COMMAND_RESPONSE_SCHEMA_VERSION, CommandAddress, CorrelationId,
        derive_command_response_address,
    };

    fn response_fixture() -> (
        CommandResponseAddress,
        OperationId,
        MessageId,
        CommandResponse,
    ) {
        let command = CommandAddress::new("acme", "orders", "place-order").unwrap();
        let operation_id = OperationId::new("operation-1").unwrap();
        let command_message_id = MessageId::new("command-1").unwrap();
        let address =
            derive_command_response_address(&command, &operation_id, &command_message_id).unwrap();
        let response = CommandResponse::accepted(
            MessageId::new("response-1").unwrap(),
            command_message_id.clone(),
            command,
            operation_id.clone(),
            CorrelationId::new("correlation-1").unwrap(),
        )
        .unwrap();
        (address, operation_id, command_message_id, response)
    }

    #[test]
    fn stored_response_requires_json_and_matching_identities() {
        let (address, operation_id, command_message_id, response) = response_fixture();
        let payload = serde_json::to_vec(&response).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE_HEADER, JSON_CONTENT_TYPE);
        headers.insert("Nats-Msg-Id", response.message_id().as_str());

        assert_eq!(
            decode_stored_command_response(
                address.as_str(),
                &headers,
                &payload,
                &address,
                &operation_id,
                &command_message_id,
            )
            .unwrap(),
            response
        );
        assert_eq!(
            decode_stored_command_response(
                address.as_str(),
                &headers,
                &payload,
                &address,
                &OperationId::new("different-operation").unwrap(),
                &command_message_id,
            )
            .unwrap_err()
            .kind(),
            CommandResponseReadErrorKind::IdentityConflict
        );
        let different_address = derive_command_response_address(
            &CommandAddress::new("acme", "orders", "cancel-order").unwrap(),
            &operation_id,
            &command_message_id,
        )
        .unwrap();
        assert_eq!(
            decode_stored_command_response(
                different_address.as_str(),
                &headers,
                &payload,
                &different_address,
                &operation_id,
                &command_message_id,
            )
            .unwrap_err()
            .kind(),
            CommandResponseReadErrorKind::IdentityConflict
        );
        headers.insert(CONTENT_TYPE_HEADER, "text/plain");
        assert_eq!(
            decode_stored_command_response(
                address.as_str(),
                &headers,
                &payload,
                &address,
                &operation_id,
                &command_message_id,
            )
            .unwrap_err()
            .kind(),
            CommandResponseReadErrorKind::InvalidResponse
        );
    }

    #[test]
    fn stored_response_requires_outer_message_id_header() {
        let (address, operation_id, command_message_id, response) = response_fixture();
        let payload = serde_json::to_vec(&response).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE_HEADER, JSON_CONTENT_TYPE);

        assert_eq!(
            decode_stored_command_response(
                address.as_str(),
                &headers,
                &payload,
                &address,
                &operation_id,
                &command_message_id,
            )
            .unwrap_err()
            .kind(),
            CommandResponseReadErrorKind::InvalidResponse
        );
    }

    #[test]
    fn response_polling_backs_off_to_the_bounded_maximum() {
        let mut interval = Duration::from_millis(25);
        let mut observed = Vec::new();
        for _ in 0..7 {
            observed.push(interval);
            interval = next_poll_interval(interval);
        }

        assert_eq!(
            observed,
            vec![
                Duration::from_millis(25),
                Duration::from_millis(50),
                Duration::from_millis(100),
                Duration::from_millis(200),
                Duration::from_millis(400),
                Duration::from_millis(800),
                MAX_COMMAND_RESPONSE_POLL_INTERVAL,
            ]
        );
        assert_eq!(
            next_poll_interval(MAX_COMMAND_RESPONSE_POLL_INTERVAL),
            MAX_COMMAND_RESPONSE_POLL_INTERVAL
        );
        assert_eq!(
            response_fixture().3.schema_version().get(),
            COMMAND_RESPONSE_SCHEMA_VERSION
        );
    }

    #[test]
    fn read_configuration_bounds_scope_and_timeout() {
        let (address, _, _, _) = response_fixture();
        assert!(validate_read_configuration(&address, "acme", Duration::from_secs(1)).is_ok());
        assert!(validate_read_configuration(&address, "other", Duration::from_secs(1)).is_err());
        assert!(validate_read_configuration(&address, "acme", Duration::ZERO).is_err());
        assert!(
            validate_read_configuration(
                &address,
                "acme",
                MAX_COMMAND_RESPONSE_TIMEOUT + Duration::from_nanos(1),
            )
            .is_err()
        );
    }
}
