use async_nats::jetstream::{
    self,
    context::{GetStreamError, GetStreamErrorKind},
    stream::{Compression, Config},
};

pub(crate) fn stream_config_mismatches(expected: &Config, actual: &Config) -> Vec<&'static str> {
    let mut mismatches = Vec::new();
    macro_rules! compare {
        ($($field:ident),+ $(,)?) => {
            $(
                if actual.$field != expected.$field {
                    mismatches.push(stringify!($field));
                }
            )+
        };
    }
    compare!(
        name,
        max_bytes,
        max_messages,
        max_messages_per_subject,
        discard,
        discard_new_per_subject,
        subjects,
        retention,
        max_consumers,
        max_age,
        max_message_size,
        storage,
        num_replicas,
        no_ack,
        duplicate_window,
        template_owner,
        sealed,
        description,
        allow_rollup,
        deny_delete,
        deny_purge,
        republish,
        allow_direct,
        mirror_direct,
        mirror,
        sources,
    );
    if !metadata_matches(expected, actual) {
        mismatches.push("metadata");
    }
    compare!(subject_transform,);
    if !compression_matches(expected.compression.as_ref(), actual.compression.as_ref()) {
        mismatches.push("compression");
    }
    compare!(
        consumer_limits,
        first_sequence,
        placement,
        persist_mode,
        pause_until,
        allow_message_ttl,
        subject_delete_marker_ttl,
        allow_atomic_publish,
        allow_message_schedules,
        allow_message_counter,
        allow_batch_publish,
    );
    mismatches
}

pub(crate) fn is_stream_not_found(error: &GetStreamError) -> bool {
    matches!(
        error.kind(),
        GetStreamErrorKind::JetStream(error)
            if error.error_code() == jetstream::ErrorCode::STREAM_NOT_FOUND
    )
}

fn metadata_matches(expected: &Config, actual: &Config) -> bool {
    expected
        .metadata
        .iter()
        .filter(|(key, _)| !is_server_metadata(key))
        .all(|(key, value)| actual.metadata.get(key) == Some(value))
        && actual
            .metadata
            .iter()
            .filter(|(key, _)| !is_server_metadata(key))
            .all(|(key, value)| expected.metadata.get(key) == Some(value))
}

fn is_server_metadata(key: &str) -> bool {
    key.starts_with("_nats.")
}

fn compression_matches(expected: Option<&Compression>, actual: Option<&Compression>) -> bool {
    expected == actual
        || matches!(
            (expected, actual),
            (None, Some(Compression::None)) | (Some(Compression::None), None)
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_server_normalized_metadata_and_compression() {
        let expected = Config::default();
        let mut actual = expected.clone();
        actual
            .metadata
            .insert("_nats.level".to_owned(), "2".to_owned());
        actual
            .metadata
            .insert("_nats.req.level".to_owned(), "1".to_owned());
        actual
            .metadata
            .insert("_nats.ver".to_owned(), "2.12.0".to_owned());
        actual.compression = Some(Compression::None);

        assert!(stream_config_mismatches(&expected, &actual).is_empty());
    }

    #[test]
    fn rejects_different_user_metadata() {
        let mut expected = Config::default();
        expected
            .metadata
            .insert("owner".to_owned(), "rostfrei".to_owned());
        let mut actual = expected.clone();
        actual
            .metadata
            .insert("owner".to_owned(), "another-owner".to_owned());
        actual
            .metadata
            .insert("_nats.ver".to_owned(), "2.12.0".to_owned());

        assert_eq!(
            stream_config_mismatches(&expected, &actual),
            vec!["metadata"]
        );
    }
}
