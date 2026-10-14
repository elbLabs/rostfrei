//! Response-only raw reads: preserve header multiplicity before protocol validation.
// async-nats 0.50's StreamMessage conversion inserts (rather than appends) repeated
// stored header lines. Use the same broker API, retaining those lines in HeaderMap.
use async_nats::{
    HeaderMap, HeaderName, HeaderValue,
    jetstream::{self, message::StreamMessage, response::Response, stream::RawMessage},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;

use crate::error::NatsError;

#[derive(Deserialize)]
struct StoredMessage {
    message: RawMessage,
}

pub async fn latest(
    context: &jetstream::Context,
    stream: &str,
    subject: &str,
) -> Result<Option<StreamMessage>, NatsError> {
    fetch(
        context,
        stream,
        &serde_json::json!({"last_by_subj": subject}),
    )
    .await
}

pub async fn sequence(
    context: &jetstream::Context,
    stream: &str,
    sequence: u64,
) -> Result<Option<StreamMessage>, NatsError> {
    fetch(context, stream, &serde_json::json!({"seq": sequence})).await
}

async fn fetch(
    context: &jetstream::Context,
    stream: &str,
    request: &serde_json::Value,
) -> Result<Option<StreamMessage>, NatsError> {
    let response: Response<StoredMessage> = context
        .request(format!("STREAM.MSG.GET.{stream}"), request)
        .await
        .map_err(|_| NatsError::Publish)?;
    match response {
        Response::Ok(stored) => {
            let headers = decode_headers(stored.message.headers.as_deref())?;
            let mut decoded: StreamMessage = stored
                .message
                .try_into()
                .map_err(|_| NatsError::InvalidMessage)?;
            decoded.headers = headers;
            Ok(Some(decoded))
        }
        Response::Err { error }
            if error.error_code() == jetstream::ErrorCode::NO_MESSAGE_FOUND =>
        {
            Ok(None)
        }
        Response::Err { .. } => Err(NatsError::Publish),
    }
}

fn decode_headers(encoded: Option<&str>) -> Result<HeaderMap, NatsError> {
    let Some(encoded) = encoded else {
        return Ok(HeaderMap::new());
    };
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| NatsError::InvalidMessage)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| NatsError::InvalidMessage)?;
    let mut lines = text.lines().peekable();
    // The pinned client's conversion below still validates any optional status line.
    if !lines.next().is_some_and(|line| line.starts_with("NATS/1.0")) {
        return Err(NatsError::InvalidMessage);
    }
    let mut headers = HeaderMap::new();
    while let Some(line) = lines.next() {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or(NatsError::InvalidMessage)?;
        let mut value = value.trim().to_owned();
        while let Some(continuation) = lines.next_if(|line| line.starts_with([' ', '\t'])) {
            value.push(' ');
            value.push_str(continuation.trim());
        }
        headers.append(
            name.parse::<HeaderName>()
                .map_err(|_| NatsError::InvalidMessage)?,
            value
                .parse::<HeaderValue>()
                .map_err(|_| NatsError::InvalidMessage)?,
        );
    }
    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_header_lines_and_continuations_do_not_erase_duplicates() {
        let encoded = STANDARD.encode(b"NATS/1.0\r\nrostfrei-Control-Correlation-Id: first\r\nrostfrei-Control-Correlation-Id: second\r\nX-Trace: first\r\n continuation\r\n\r\n");
        let headers = decode_headers(Some(&encoded)).unwrap();
        assert_eq!(
            headers
                .get_all("rostfrei-Control-Correlation-Id")
                .map(HeaderValue::as_str)
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        assert_eq!(
            headers.get("X-Trace").map(HeaderValue::as_str),
            Some("first continuation")
        );
    }
}
