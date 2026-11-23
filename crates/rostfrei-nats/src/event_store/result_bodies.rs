//! Split layout 3. Only the journal decides completion and withdrawal. An
//! acknowledged body can disappear at any point, including before journal commit.
use super::*;
use async_nats::jetstream::{message::StreamMessage, stream::Stream};

// Leader-served last-by-subject, not direct get. Only an explicit no-message
// response is absence. Both kinds of record are immutable CAS-zero writes.
async fn last_record(
    stream: &Stream,
    subject: &str,
) -> Result<Option<StreamMessage>, EventStoreError> {
    let record = match stream.get_last_raw_message_by_subject(subject).await {
        Ok(record) => record,
        Err(e) if e.kind() == LastRawMessageErrorKind::NoMessageFound => return Ok(None),
        Err(e) => return Err(unavailable(format!("failed to read {subject}: {e}"))),
    };
    if record.subject.as_str() != subject
        || record.sequence == 0
        || required_single_header(&record.headers, "Nats-Expected-Stream")?
            != stream.cached_info().config.name
        || required_single_header(&record.headers, "Nats-Expected-Last-Subject-Sequence")? != "0"
        || [
            "Nats-Msg-Id",
            "Nats-Rollup",
            "KV-Operation",
            NATS_EXPECTED_LAST_SUBJECT_SEQUENCE_SUBJECT,
        ]
        .iter()
        .any(|key| record.headers.get(*key).is_some())
    {
        return Err(corrupt(
            "invalid immutable outcome record coordinates or CAS headers",
        ));
    }
    Ok(Some(record))
}

impl NatsEventStore {
    async fn checked_body_stream(&self) -> Result<Stream, EventStoreError> {
        self.verify_authority()
            .await?
            .ok_or_else(|| configuration("missing bound result stream"))
    }

    pub(super) async fn prepare_body(
        &self,
        marker: &OutcomeMarker,
        bytes: &[u8],
    ) -> Result<(), EventStoreError> {
        let stream = self.checked_body_stream().await?;
        let subject = self
            .config
            .outcome_state_subject(&marker.detail_reference().encode()?);
        let Err(error) = self
            .publish_cas(
                self.config.outcome_state_stream_name(),
                subject.clone(),
                bytes.to_vec(),
                0,
                "application/octet-stream",
            )
            .await
        else {
            // A valid PubAck suffices. A readback here could already miss under pressure.
            return Ok(());
        };
        if !matches!(
            error.kind(),
            EventStoreErrorKind::Conflict | EventStoreErrorKind::Unavailable
        ) {
            return Err(error);
        }
        match last_record(&stream, &subject).await? {
            Some(record) if record.payload.as_ref() == bytes => Ok(()),
            Some(_) => Err(corrupt("stored result body differs from exact candidate")),
            None => Err(error), // No positive evidence of preparation.
        }
    }

    async fn journal_withdrawn(&self, marker: &OutcomeMarker) -> Result<bool, EventStoreError> {
        let journal = self.verify_journal().await?;
        let subject = self.config.expiry_subject(marker.revision());
        let Some(proof) = last_record(&journal, &subject).await? else {
            return Ok(false);
        };
        // The revision covers authority, identity, proposal, body digest and receipt.
        if proof.payload.as_ref() != marker.revision().to_hex().as_bytes() {
            return Err(corrupt(
                "journal withdrawal does not match the winning revision",
            ));
        }
        Ok(true)
    }

    pub(super) async fn read_admitted_body(
        &self,
        marker: &OutcomeMarker,
    ) -> Result<AdmittedBody, EventStoreError> {
        let withdrawn = self.journal_withdrawn(marker).await?;
        // Validate the observed journal incarnation *after* the name-addressed
        // proof GET, before treating a missing proof as admission to any reader.
        let stream = self.checked_body_stream().await?;
        if withdrawn {
            return Ok(AdmittedBody::Expired);
        }
        let subject = self
            .config
            .outcome_state_subject(&marker.detail_reference().encode()?);
        let Some(record) = last_record(&stream, &subject).await? else {
            // A GET addresses a stream by name. Do not classify absence on a
            // replacement resource, or admit a cold reader against a broken binding.
            self.verify_authority().await?;
            return Ok(AdmittedBody::NotRetained);
        };
        EncodedOutcomeDetails::decode(marker.detail_reference(), &record.payload)?;
        Ok(AdmittedBody::Available(record.payload.to_vec()))
    }

    pub(super) async fn expire_body(
        &self,
        marker: &OutcomeMarker,
    ) -> Result<OutcomeExpiry, EventStoreError> {
        if !self.journal_withdrawn(marker).await? {
            let subject = self.config.expiry_subject(marker.revision());
            let result = self
                .publish_cas(
                    self.config.stream_name(),
                    subject,
                    marker.revision().to_hex().into_bytes(),
                    0,
                    "text/plain",
                )
                .await;
            // Also reconciles an interrupted or concurrent CAS. A full journal
            // cannot record withdrawal; never claim expiry on an unconfirmed write.
            if !self.journal_withdrawn(marker).await? {
                return Err(result
                    .err()
                    .unwrap_or_else(|| corrupt("journal withdrawal not visible")));
            }
        }
        let subject = self
            .config
            .outcome_state_subject(&marker.detail_reference().encode()?);
        let stream = self.checked_body_stream().await?;
        let reclamation = match last_record(&stream, &subject).await {
            Ok(None) => {
                self.verify_authority().await?;
                ReclamationStatus::Reclaimed
            }
            _ => ReclamationStatus::Pending,
        };
        Ok(OutcomeExpiry::new(marker.revision().clone(), reclamation))
    }
}
