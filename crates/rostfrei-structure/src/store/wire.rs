//! async-nats 0.50.0 has no stream snapshot/restore API. Use the native
//! `JetStream` protocol (`server/jetstream_api.go`) over its core client.

use std::{path::Path, time::Duration};

use async_nats::{Client, Message, Request, Subject};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};

use super::Result;

const CHUNK_SIZE: usize = 128 * 1024;
const FLOW_WAIT: Duration = Duration::from_secs(30);
// The initial checksum scan and the final restore can take longer than a chunk.
const API_WAIT: Duration = Duration::from_secs(300);
const SNAPSHOT: &str = "io.nats.jetstream.api.v1.stream_snapshot_response";
const RESTORE: &str = "io.nats.jetstream.api.v1.stream_restore_response";
const CREATE: &str = "io.nats.jetstream.api.v1.stream_create_response";

fn check_status(message: &Message) -> Result<()> {
    if let Some(status) = message.status
        && !status.is_success()
    {
        return Err(format!("transfer failed with NATS status {status}").into());
    }
    Ok(())
}

async fn request(
    client: &Client,
    subject: Subject,
    data: Vec<u8>,
    wait: Duration,
) -> Result<Message> {
    // Bound the enqueue as well as the response; don't replay ambiguous chunks.
    let req = Request::new().payload(data.into()).timeout(None);
    let message = timeout(wait, client.send_request(subject, req))
        .await
        .map_err(|_| "JetStream transfer request timed out")?
        .map_err(|error| format!("JetStream transfer request: {}", error.kind()))?;
    check_status(&message)?;
    Ok(message)
}

fn api_reply(message: &Message, expected: &str) -> Result<Value> {
    let reply: Value = serde_json::from_slice(&message.payload)?;
    if let Some(error) = reply.get("error") {
        // Broker descriptions can include user-supplied config; report stable codes.
        let code = error.get("code").and_then(Value::as_u64);
        let err_code = error.get("err_code").and_then(Value::as_u64);
        return Err(match (code, err_code) {
            (Some(code), Some(err_code)) => {
                format!("JetStream API error {code} ({err_code})").into()
            }
            _ => "JetStream API returned a malformed error".into(),
        });
    }
    if reply.get("type").and_then(Value::as_str) != Some(expected) {
        return Err(format!("expected JetStream reply of type {expected}").into());
    }
    Ok(reply)
}

fn stream_reply(message: &Message, expected: &str, name: &str) -> Result<()> {
    let reply = api_reply(message, expected)?;
    if reply.pointer("/config/name").and_then(Value::as_str) != Some(name)
        || !reply.get("state").is_some_and(Value::is_object)
    {
        return Err("JetStream reply lacks the requested stream's config or state".into());
    }
    Ok(())
}

pub(super) async fn snapshot(client: &Client, name: &str, path: &Path) -> Result<()> {
    let mut output = File::create_new(path).await?;
    let inbox = client.new_inbox();
    let mut chunks = timeout(FLOW_WAIT, client.subscribe(inbox.clone())).await??;
    let chunk_size = client.max_payload().clamp(1024, CHUNK_SIZE);
    let body = json!({
        "deliver_subject": inbox, "jsck": true, "no_consumers": true,
        "chunk_size": chunk_size, "window_size": chunk_size,
    });
    let subject = format!("$JS.API.STREAM.SNAPSHOT.{name}").into();
    let response = request(client, subject, serde_json::to_vec(&body)?, API_WAIT).await?;
    stream_reply(&response, SNAPSHOT, name)?;

    let mut received_data = false;
    loop {
        let chunk = timeout(FLOW_WAIT, chunks.next())
            .await
            .map_err(|_| "timed out waiting for snapshot data")?
            .ok_or("snapshot subscription closed before EOF")?;
        // Error EOFs (408/500) are empty too. Normal EOF is bare for a small
        // snapshot, or has status 204 after at least one full-sized chunk.
        check_status(&chunk)?;
        if chunk.payload.is_empty() {
            if !received_data || chunk.reply.is_some() {
                return Err("invalid snapshot EOF".into());
            }
            output.flush().await?;
            output.sync_all().await?;
            return Ok(());
        }
        if chunk.payload.len() > chunk_size
            || (chunk.payload.len() == chunk_size) != chunk.reply.is_some()
        {
            return Err("invalid snapshot data chunk".into());
        }
        output.write_all(&chunk.payload).await?;
        received_data = true;
        // The final short data chunk has no reply subject.
        if let Some(ack) = chunk.reply {
            timeout(FLOW_WAIT, client.publish(ack, "".into())).await??;
        }
    }
}

pub(super) async fn restore(client: &Client, name: &str, path: &Path, info: &Value) -> Result<()> {
    let mut input = File::open(path).await?;
    // Send the original config verbatim: a typed round trip can drop policy fields.
    // Creation time is restored from the snapshot, not part of the restore request.
    let body = json!({
        "config": info.get("config").ok_or("missing restore config")?,
        "state": info.get("state").ok_or("missing restore state")?,
    });
    let subject = format!("$JS.API.STREAM.RESTORE.{name}").into();
    let response = request(client, subject, serde_json::to_vec(&body)?, API_WAIT).await?;
    let handshake = api_reply(&response, RESTORE)?;
    let deliver = Subject::validated(
        handshake
            .get("deliver_subject")
            .and_then(Value::as_str)
            .ok_or("missing restore delivery subject")?,
    )?;

    let chunk_size = client.max_payload().min(CHUNK_SIZE);
    if chunk_size == 0 {
        return Err("server does not accept restore data".into());
    }
    loop {
        let mut chunk = vec![0; chunk_size];
        let size = input.read(&mut chunk).await?;
        chunk.truncate(size);
        if size == 0 {
            // EOF is a request. Only a successful stream-create reply commits it.
            let done = request(client, deliver, chunk, API_WAIT).await?;
            return stream_reply(&done, CREATE, name);
        }
        let ack = request(client, deliver.clone(), chunk, FLOW_WAIT).await?;
        if !ack.payload.is_empty() {
            // A failed pipe write returns the stream-create error on THIS chunk.
            api_reply(&ack, CREATE)?;
            return Err("unexpected nonempty restore chunk acknowledgement".into());
        }
    }
}
