//! Use the supported URL-authentication path with the owned authenticated test broker.
pub async fn client(
    url: impl Into<String>,
) -> Result<async_nats::Client, rostfrei_nats::NatsError> {
    let connection = rostfrei_nats::connect(&rostfrei_nats::NatsConnectionConfig::new(
        "native-regression-fixture",
        url,
    ))
    .await?;
    Ok(connection.client().clone())
}
