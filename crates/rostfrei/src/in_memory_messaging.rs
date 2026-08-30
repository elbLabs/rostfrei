use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use rostfrei_messaging_core::{MessageId, PublishReceipt};
use tokio::sync::Mutex;

use crate::{
    CommandBusError, CommandBusErrorKind, CommandBusObserver, CommandBusReceipt,
    CommandMessageAdapter, CommandProcessor, CommandProcessorErrorKind, CommandPublication,
    EncodedCommand, EncodedIntegrationMessage, IntegrationEventBusError, IntegrationMessageAdapter,
};

#[derive(Clone)]
struct StoredCommand {
    response: rostfrei_messaging_core::CommandResponse,
}

#[derive(Default)]
struct InMemoryMessagingState {
    commands: HashMap<String, StoredCommand>,
    command_messages: Vec<EncodedCommand>,
    integration_events: HashMap<String, EncodedIntegrationMessage>,
    integration_messages: Vec<EncodedIntegrationMessage>,
}

pub struct InMemoryMessagingAdapter {
    processor: Arc<CommandProcessor>,
    state: Mutex<InMemoryMessagingState>,
    command_gates: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl InMemoryMessagingAdapter {
    pub fn new(processor: Arc<CommandProcessor>) -> Self {
        Self {
            processor,
            state: Mutex::new(InMemoryMessagingState::default()),
            command_gates: Mutex::new(HashMap::new()),
        }
    }

    pub async fn command_messages(&self) -> Vec<EncodedCommand> {
        self.state.lock().await.command_messages.clone()
    }

    pub async fn integration_messages(&self) -> Vec<EncodedIntegrationMessage> {
        self.state.lock().await.integration_messages.clone()
    }

    async fn command_gate(&self, message_id: &MessageId) -> Arc<Mutex<()>> {
        let mut gates = self.command_gates.lock().await;
        Arc::clone(
            gates
                .entry(message_id.as_str().to_owned())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }
}

#[async_trait]
impl CommandMessageAdapter for InMemoryMessagingAdapter {
    async fn dispatch(
        &self,
        command: EncodedCommand,
        observer: Arc<dyn CommandBusObserver>,
    ) -> Result<CommandBusReceipt, CommandBusError> {
        let gate = self.command_gate(command.message_id()).await;
        let _guard = gate.lock().await;
        let existing = self
            .state
            .lock()
            .await
            .commands
            .get(command.message_id().as_str())
            .cloned();
        if let Some(existing) = existing {
            observer
                .published(CommandPublication::new(command.message_id().clone(), true))
                .await;
            return Ok(CommandBusReceipt::new(true, existing.response));
        }

        observer
            .published(CommandPublication::new(command.message_id().clone(), false))
            .await;
        let response = self.processor.process(&command).await.map_err(|error| {
            let kind = match error.kind() {
                CommandProcessorErrorKind::InvalidMessage => CommandBusErrorKind::InvalidMessage,
                CommandProcessorErrorKind::Unavailable => CommandBusErrorKind::Unavailable,
                CommandProcessorErrorKind::InvalidConfiguration => {
                    CommandBusErrorKind::InvalidConfiguration
                }
            };
            CommandBusError::new(kind, error.to_string())
        })?;
        let mut state = self.state.lock().await;
        state.command_messages.push(command.clone());
        state.commands.insert(
            command.message_id().as_str().to_owned(),
            StoredCommand {
                response: response.clone(),
            },
        );
        drop(state);
        Ok(CommandBusReceipt::new(false, response))
    }
}

#[async_trait]
impl IntegrationMessageAdapter for InMemoryMessagingAdapter {
    async fn publish(
        &self,
        message: EncodedIntegrationMessage,
    ) -> Result<PublishReceipt, IntegrationEventBusError> {
        let mut state = self.state.lock().await;
        if state
            .integration_events
            .contains_key(message.message_id().as_str())
        {
            return Ok(PublishReceipt::new(true));
        }
        state.integration_messages.push(message.clone());
        state
            .integration_events
            .insert(message.message_id().as_str().to_owned(), message);
        drop(state);
        Ok(PublishReceipt::new(false))
    }
}
