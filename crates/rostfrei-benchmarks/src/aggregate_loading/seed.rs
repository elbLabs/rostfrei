use rostfrei_core::{
    AppendOutcome, CommandExecutor, CommandOutcome, CommandReceipt, ContentFingerprint, Event,
    EventBatch, EventStore, ExpectedVersion, MAX_BATCH_PAYLOAD_LEN, MAX_EVENTS_PER_BATCH, NewEvent,
    OperationId, StreamVersion, derive_commit_id, derive_event_id,
};

use super::{BenchResult, config::HistoryKind, model};

pub async fn seed<S: EventStore + Clone>(
    store: S,
    id: &str,
    kind: HistoryKind,
    count: u32,
    note_bytes: u32,
) -> BenchResult {
    match kind {
        HistoryKind::Direct => direct(&store, id, count, note_bytes).await,
        HistoryKind::Commands => {
            let executor = CommandExecutor::new(store);
            for index in 0..count {
                let command = command(id, u64::from(index), note_bytes)?;
                let metadata = model::metadata(&format!("{id}:seed:{index}"), &command)?;
                appended(
                    executor
                        .execute(&model::ReceiveStockHandler, metadata, &command)
                        .await?,
                )?;
                let completed = index.saturating_add(1);
                if completed.is_multiple_of(100) || completed == count {
                    eprintln!("seeding {id}: {completed}/{count} commands");
                }
            }
            Ok(())
        }
    }
}

async fn direct<S: EventStore>(store: &S, id: &str, count: u32, note_bytes: u32) -> BenchResult {
    let stream = model::stream(id)?;
    let mut position = 0_u32;
    while position < count {
        let base = position;
        let operation = OperationId::new(format!("{id}:batch:{base}"))?;
        let commit = derive_commit_id(&stream, &operation);
        let mut events = Vec::new();
        let mut bytes = 0_usize;
        while position < count && events.len() < MAX_EVENTS_PER_BATCH {
            let event = model::StockReceived::at(u64::from(position), note_bytes)?;
            let payload = event.encode_json()?;
            let next_bytes = bytes
                .checked_add(payload.len())
                .ok_or("batch bytes overflow")?;
            if next_bytes > MAX_BATCH_PAYLOAD_LEN {
                break;
            }
            events.push(NewEvent::new(
                derive_event_id(&commit, u32::try_from(events.len())?),
                event.event_type(),
                event.schema_version(),
                payload,
            )?);
            bytes = next_bytes;
            position = position.checked_add(1).ok_or("seed position overflow")?;
        }
        if events.is_empty() {
            return Err("benchmark event exceeds the batch payload budget".into());
        }
        let batch = EventBatch::new(
            commit,
            operation,
            ContentFingerprint::digest(format!("{id}:{base}:{note_bytes}")),
            events,
        )?;
        let expected = if base == 0 {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(StreamVersion::new(u64::from(base)))
        };
        if !matches!(
            store.append(&stream, expected, batch).await?,
            AppendOutcome::Appended(_)
        ) {
            return Err("direct fixture unexpectedly replayed an existing operation".into());
        }
    }
    Ok(())
}

pub fn command(id: &str, index: u64, note_bytes: u32) -> BenchResult<model::ReceiveStock> {
    Ok(model::ReceiveStock {
        inventory_id: id.to_owned(),
        event: model::StockReceived::at(index, note_bytes)?,
    })
}

pub fn appended(outcome: CommandOutcome<std::convert::Infallible>) -> BenchResult {
    if matches!(outcome, CommandOutcome::Accepted(CommandReceipt::Appended(events)) if events.len() == 1)
    {
        Ok(())
    } else {
        Err("benchmark write did not append exactly one new event".into())
    }
}
