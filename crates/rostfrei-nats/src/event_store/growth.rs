//! Maintenance on an existing bound pair, while other administrators are fenced.
use super::{
    Config, EventStoreError, EventStoreErrorKind, NatsEventStoreConfig, jetstream, outcomes,
    unavailable, verify_stream_config,
};
use jetstream::stream::{Info, Stream};

pub(super) async fn grow_pair(
    context: &jetstream::Context,
    config: &NatsEventStoreConfig,
    journal: &Stream,
) -> Result<(), EventStoreError> {
    let journal = journal.cached_info();
    let mut expected = config.stream_config();
    let journal_growth = growth_needed(&mut expected, journal)?;
    verify_stream_config(&expected, &journal.config)?;

    let state = outcomes::get_split_state(context, config).await?;
    let state = state.cached_info();
    let mut expected = config.outcome_state_config();
    let state_growth = growth_needed(&mut expected, state)?;
    outcomes::verify_split_state(expected, journal, state)?;
    if journal_growth == 0 && state_growth == 0 {
        return Ok(());
    }
    let account = context.query_account().await.map_err(|error| {
        unavailable(format!(
            "failed to inspect account headroom before capacity growth: {error}"
        ))
    })?;
    check_headroom(
        &account,
        config.replicas(),
        config
            .max_stream_bytes()
            .max(config.max_result_stream_bytes()),
        journal_growth,
        state_growth,
    )?;
    for (current, target) in [
        (journal, config.max_stream_bytes()),
        (state, config.max_result_stream_bytes()),
    ] {
        if current.config.max_bytes != target {
            let mut policy = current.config.clone();
            policy.max_bytes = target;
            let updated = context.update_stream(policy).await.map_err(|error| {
                unavailable(format!("failed to grow {}; rerun with the same capacity target after checking the pair: {error}", current.config.name))
            })?;
            if updated.created != current.created || updated.config.max_bytes != target {
                return Err(mismatch(
                    "stream identity or capacity changed during growth",
                ));
            }
        }
    }
    let after = context
        .get_stream(config.stream_name())
        .await
        .map_err(|e| unavailable(e.to_string()))?;
    let after = after.cached_info();
    let companion = outcomes::get_split_state(context, config).await?;
    let companion = companion.cached_info();
    if after.created != journal.created || companion.created != state.created {
        return Err(mismatch("stream pair was replaced during growth"));
    }
    verify_stream_config(&config.stream_config(), &after.config)?;
    outcomes::verify_split_state(config.outcome_state_config(), after, companion)
}

fn growth_needed(expected: &mut Config, current: &Info) -> Result<u64, EventStoreError> {
    let target = expected.max_bytes;
    expected.max_bytes = current.config.max_bytes;
    if expected.max_bytes <= 0 || expected.max_bytes > target {
        return Err(mismatch(format!(
            "{} max_bytes must be finite and cannot be reduced",
            current.config.name
        )));
    }
    Ok(target.abs_diff(expected.max_bytes))
}

// Account info is a snapshot, not a reservation. The server also checks placement and
// concurrent changes on each stream update. Flat accounts charge growth per replica;
// named replication tiers charge once against the selected tier's limit.
fn check_headroom(
    account: &jetstream::account::Account,
    replicas: usize,
    target: i64,
    journal_growth: u64,
    state_growth: u64,
) -> Result<(), EventStoreError> {
    let (limits, reserved, factor) = if account.tiers.is_empty() {
        (account.limits, account.reserved_storage, replicas)
    } else {
        let tier = account
            .tiers
            .get(&format!("R{replicas}"))
            .ok_or_else(|| mismatch("account is missing the store's replica tier"))?;
        (tier.limits, tier.reserved_storage, 1)
    };
    let additional = journal_growth
        .saturating_add(state_growth)
        .saturating_mul(u64::try_from(factor).map_err(|_| mismatch("invalid replica count"))?);
    if limits
        .storage_max_stream_bytes
        .is_some_and(|limit| limit > 0 && target > limit)
        || limits
            .max_storage
            .is_some_and(|limit| reserved.saturating_add(additional) > limit.unsigned_abs())
    {
        return Err(EventStoreError::new(
            EventStoreErrorKind::CapacityExhausted,
            "insufficient JetStream account file-storage headroom for the target stream pair",
        ));
    }
    Ok(())
}

fn mismatch(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::ConfigurationMismatch, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetstream::account::{Account, Limits, Tier};

    #[test]
    fn combined_growth_must_fit_the_applicable_replica_budget() {
        let limits = Limits {
            max_storage: Some(100),
            ..Limits::default()
        };
        let mut account = Account {
            reserved_storage: 70,
            limits,
            ..Account::default()
        };
        // Thirty bytes free: only ten more logical bytes in a flat account at R3.
        assert!(check_headroom(&account, 3, 32, 6, 4).is_ok());
        assert_eq!(
            check_headroom(&account, 3, 32, 6, 5).unwrap_err().kind(),
            EventStoreErrorKind::CapacityExhausted
        );
        // An R3 tier expresses the same limit in logical bytes.
        account.tiers.insert(
            "R3".to_owned(),
            Tier {
                reserved_storage: 70,
                limits,
                ..Tier::default()
            },
        );
        assert!(check_headroom(&account, 3, 32, 0, 30).is_ok());
        assert_eq!(
            check_headroom(&account, 3, 32, 20, 11).unwrap_err().kind(),
            EventStoreErrorKind::CapacityExhausted
        );
    }
}
