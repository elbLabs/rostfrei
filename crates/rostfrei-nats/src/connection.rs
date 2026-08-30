use std::time::Duration;

use async_nats::{Client, ConnectOptions, Event, jetstream};
use rostfrei_messaging_core::ApplicationName;
use tokio::{sync::watch, time::timeout};

use crate::{
    command_response::NatsCommandResponseReader,
    consumer::NatsConsumerFactory,
    error::NatsError,
    messaging_adapter::NatsMessagingAdapter,
    messaging_config::{MessagingTopology, NatsConnectionConfig},
    provisioning::{ApplicationMessagingConfig, verify_application_messaging, verify_stream},
    publish::NatsPublisher,
    query::{NatsQueryRequester, NatsQueryServer, NatsQueryServerConfig},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionHealth {
    Connecting,
    Connected,
    Disconnected,
    Closed,
}

#[derive(Clone)]
pub struct NatsConnection {
    client: Client,
    jetstream: jetstream::Context,
    closed: watch::Receiver<bool>,
    operation_timeout: Duration,
    drain_timeout: Duration,
}

impl NatsConnection {
    pub const fn client(&self) -> &Client {
        &self.client
    }

    pub const fn jetstream(&self) -> &jetstream::Context {
        &self.jetstream
    }

    pub fn publisher(&self, topology: MessagingTopology) -> NatsPublisher {
        NatsPublisher::new(self.jetstream.clone(), topology)
    }

    pub fn consumer_factory(&self, topology: MessagingTopology) -> NatsConsumerFactory {
        NatsConsumerFactory::new(self.jetstream.clone(), topology)
    }

    pub fn command_response_reader(
        &self,
        topology: MessagingTopology,
    ) -> NatsCommandResponseReader {
        NatsCommandResponseReader::new(self.jetstream.clone(), topology)
    }

    pub fn messaging_adapter(&self, topology: MessagingTopology) -> NatsMessagingAdapter {
        NatsMessagingAdapter::new(
            self.publisher(topology.clone()),
            self.command_response_reader(topology),
        )
    }

    pub fn query_requester(&self, application: &ApplicationName) -> NatsQueryRequester {
        NatsQueryRequester::new(self.client.clone(), application.clone())
    }

    pub fn query_server(
        &self,
        application: &ApplicationName,
        config: NatsQueryServerConfig,
    ) -> Result<NatsQueryServer, NatsError> {
        NatsQueryServer::new(self.client.clone(), application.clone(), config)
    }

    pub fn health(&self) -> ConnectionHealth {
        if *self.closed.borrow() {
            return ConnectionHealth::Closed;
        }
        match self.client.connection_state() {
            async_nats::connection::State::Pending => ConnectionHealth::Connecting,
            async_nats::connection::State::Connected => ConnectionHealth::Connected,
            async_nats::connection::State::Disconnected => ConnectionHealth::Disconnected,
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.health() == ConnectionHealth::Connected
    }

    pub async fn check_health(&self) -> Result<(), NatsError> {
        if !self.is_healthy() {
            return Err(NatsError::Connection);
        }
        self.flush().await
    }

    pub async fn verify_topology(&self, topology: &MessagingTopology) -> Result<(), NatsError> {
        verify_stream(&self.jetstream, topology.command_stream()).await?;
        verify_stream(&self.jetstream, topology.command_response_stream()).await?;
        verify_stream(&self.jetstream, topology.integration_event_stream()).await?;
        verify_stream(&self.jetstream, topology.quarantine_stream()).await
    }

    pub async fn verify_application_messaging(
        &self,
        config: &ApplicationMessagingConfig,
    ) -> Result<(), NatsError> {
        verify_application_messaging(&self.jetstream, config).await
    }

    pub async fn flush(&self) -> Result<(), NatsError> {
        timeout(self.operation_timeout, self.client.flush())
            .await
            .map_err(|_| NatsError::Flush)?
            .map_err(|_| NatsError::Flush)
    }

    pub async fn drain(&self) -> Result<(), NatsError> {
        let mut closed = self.closed.clone();
        timeout(self.drain_timeout, async {
            self.client.drain().await.map_err(|_| NatsError::Drain)?;
            closed
                .wait_for(|is_closed| *is_closed)
                .await
                .map_err(|_| NatsError::Drain)?;
            Ok(())
        })
        .await
        .map_err(|_| NatsError::DrainTimeout)?
    }
}

pub async fn connect(config: &NatsConnectionConfig) -> Result<NatsConnection, NatsError> {
    let servers = config.server_addrs()?;
    let client_name = config.client_name().to_owned();
    let event_client_name = client_name.clone();
    let (closed_tx, closed) = watch::channel(false);
    let client = ConnectOptions::new()
        .name(client_name)
        .connection_timeout(config.connection_timeout())
        .max_reconnects(None)
        .event_callback(move |event| {
            let closed_tx = closed_tx.clone();
            let client_name = event_client_name.clone();
            async move {
                match event {
                    Event::Connected => tracing::info!(%client_name, "NATS connected"),
                    Event::Disconnected => tracing::warn!(%client_name, "NATS disconnected"),
                    Event::LameDuckMode => {
                        tracing::warn!(%client_name, "NATS server entered lame duck mode");
                    }
                    Event::Draining => tracing::info!(%client_name, "NATS connection draining"),
                    Event::Closed => {
                        tracing::info!(%client_name, "NATS connection closed");
                        let _ = closed_tx.send(true);
                    }
                    Event::SlowConsumer(dropped) => {
                        tracing::error!(%client_name, dropped, "NATS slow consumer");
                    }
                    Event::ServerError(error) => {
                        tracing::error!(%client_name, %error, "NATS server error");
                    }
                    Event::ClientError(error) => {
                        tracing::warn!(%client_name, %error, "NATS client error");
                    }
                }
            }
        })
        .connect(servers)
        .await
        .map_err(|_| NatsError::Connection)?;
    let minimum = config.minimum_server_version();
    if !client.is_server_compatible(minimum.major(), minimum.minor(), minimum.patch()) {
        return Err(NatsError::MinimumServerVersion { required: minimum });
    }

    Ok(NatsConnection {
        jetstream: jetstream::new(client.clone()),
        client,
        closed,
        operation_timeout: config.connection_timeout(),
        drain_timeout: config.drain_timeout(),
    })
}
