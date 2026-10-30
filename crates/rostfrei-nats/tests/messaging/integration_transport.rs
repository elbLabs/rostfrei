use super::*;
use rostfrei_messaging_core::TrafficScope;

#[tokio::test]
async fn scoped_publication_and_topology_filters_use_canonical_physical_subjects() -> TestResult<()>
{
    let fixture = Fixture::new_in_scope(
        rostfrei_testing::integration::nats_url()?,
        TrafficScope::Test,
    )
    .await?;
    let address = fixture.context.integration_event_address("public-fact")?;
    let envelope = event("external-test-message", "test.*", Some("parent.>"))?;
    let ack = fixture
        .connection
        .publisher(fixture.topology.clone())
        .publish_integration_event_with_ack(outbound(&address, &envelope)?, Duration::from_secs(5))
        .await?;
    let stream = integration_stream(&fixture).await?;
    let stored = stream.get_raw_message(ack.sequence()).await?;
    let correlation_filter = fixture.topology.integration_correlation_filter("test.*");
    let causation_filter = fixture.topology.integration_causation_filter("parent.>");
    let correlated = native_filtered_ids(&fixture, &correlation_filter).await?;
    let caused = native_filtered_ids(&fixture, &causation_filter).await?;
    let prefix = format!("{}.test", fixture.context.application().as_str());
    let config = stream.cached_info().config.clone();
    fixture.cleanup().await?;
    assert_eq!(
        stored.subject.as_str(),
        format!("{}.test%2E%2A.parent%2E%3E", address.as_str())
    );
    assert_eq!(
        correlation_filter,
        format!("{prefix}.integration.*.*.test%2E%2A.*")
    );
    assert_eq!(
        causation_filter,
        format!("{prefix}.integration.*.*.*.parent%2E%3E")
    );
    assert_eq!(correlated, ["external-test-message"]);
    assert_eq!(caused, ["external-test-message"]);
    assert_eq!(config.subjects, [format!("{prefix}.integration.*.*.>")]);
    assert_eq!(
        config.retention,
        async_nats::jetstream::stream::RetentionPolicy::Limits
    );
    assert_eq!(config.max_age, Duration::from_hours(720));
    Ok(())
}

#[tokio::test]
async fn used_old_integration_capture_filter_is_rejected_before_any_provisioning_mutation()
-> TestResult<()> {
    reject_old_integration_layout(true).await
}

#[tokio::test]
async fn unused_old_integration_capture_filter_is_rejected_before_any_provisioning_mutation()
-> TestResult<()> {
    reject_old_integration_layout(false).await
}

async fn reject_old_integration_layout(has_message: bool) -> TestResult<()> {
    let fixture = Fixture::new(rostfrei_testing::integration::nats_url()?).await?;
    let application = fixture.context.application();
    let mut old = integration_stream(&fixture)
        .await?
        .cached_info()
        .config
        .clone();
    old.subjects = vec![format!("{}.integration.>", application.as_str())];
    fixture
        .connection
        .jetstream()
        .update_stream(old.clone())
        .await?;
    if has_message {
        fixture
            .connection
            .jetstream()
            .publish(
                fixture
                    .context
                    .integration_event_address("old-fact")?
                    .as_str()
                    .to_owned(),
                "old retained payload".into(),
            )
            .await?
            .await?;
    }
    // A policy change to an earlier stream would expose a partial mutation if
    // the fresh-namespace guard ran only inside the integration provisioning step.
    let proposed =
        ApplicationMessagingConfig::new(application)?.with_max_bytes(32 * 1024 * 1024)?;
    let before_commands = fixture
        .connection
        .jetstream()
        .get_stream(fixture.topology.command_stream().as_str())
        .await?
        .cached_info()
        .config
        .clone();
    let provisioned =
        provision_application_messaging(fixture.connection.jetstream(), &proposed).await;
    let expected =
        ApplicationMessagingConfig::new(application)?.with_max_bytes(64 * 1024 * 1024)?;
    let verified = fixture
        .connection
        .verify_application_messaging(&expected)
        .await;
    let after_commands = fixture
        .connection
        .jetstream()
        .get_stream(fixture.topology.command_stream().as_str())
        .await?
        .cached_info()
        .config
        .clone();
    let after = integration_stream(&fixture).await?;
    let after_config = after.cached_info().config.clone();
    let retained = if has_message {
        Some(after.get_raw_message(1).await?)
    } else {
        None
    };
    fixture.cleanup().await?;
    assert!(
        provisioned.is_err(),
        "old layout must require a fresh namespace, including an unused stream"
    );
    assert!(verified.is_err());
    for result in [provisioned, verified] {
        assert!(
            result.is_err_and(|error| error.to_string().contains("fresh application namespace"))
        );
    }
    assert_eq!(after_config, old);
    assert_eq!(after_commands, before_commands);
    if let Some(retained) = retained {
        assert_eq!(retained.payload.as_ref(), b"old retained payload");
    }
    Ok(())
}

#[tokio::test]
async fn missing_and_duplicate_incoming_control_headers_are_quarantined() -> TestResult<()> {
    let fixture = Fixture::new(rostfrei_testing::integration::nats_url()?).await?;
    let address = fixture.context.integration_event_address("public-fact")?;
    let (task, mut received) = logical_consumer(&fixture, &address).await?;
    for id in [
        "no-correlation",
        "duplicate-correlation",
        "no-message-id",
        "no-content-type",
    ] {
        let envelope = event(id, "flow", None)?;
        let mut headers = async_nats::HeaderMap::new();
        if id != "no-content-type" {
            headers.insert(CONTENT_TYPE_HEADER, JSON_CONTENT_TYPE);
        }
        if id != "no-message-id" {
            headers.insert("Nats-Msg-Id", id);
        }
        if id != "no-correlation" {
            headers.insert(publish::CORRELATION_ID_HEADER, "flow");
        }
        if id == "duplicate-correlation" {
            headers.append(publish::CORRELATION_ID_HEADER, "flow");
        }
        fixture
            .connection
            .jetstream()
            .publish_with_headers(
                format!("{}.flow.~", address.as_str()),
                headers,
                serde_json::to_vec(&envelope)?.into(),
            )
            .await?
            .await?;
    }
    let mut durable: async_nats::jetstream::consumer::PullConsumer = integration_stream(&fixture)
        .await?
        .get_consumer(fixture.context.durable_name("public-facts", 1)?.as_str())
        .await?;
    let settled = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let info = durable.info().await?;
            if info.ack_floor.stream_sequence == 4
                && info.num_pending == 0
                && info.num_ack_pending == 0
            {
                return TestResult::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    task.abort();
    let _ = task.await;
    let handled = received.try_recv().ok();
    let quarantined = stream_message_count(
        fixture.connection.jetstream(),
        fixture.topology.quarantine_stream(),
    )
    .await?;
    fixture.cleanup().await?;
    assert!(matches!(settled, Ok(Ok(()))), "{settled:?}");
    assert!(handled.is_none());
    assert_eq!(quarantined, 4);
    Ok(())
}
