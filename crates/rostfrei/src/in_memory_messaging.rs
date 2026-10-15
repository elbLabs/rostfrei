use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use rostfrei_messaging_core::{MessageId, PublishReceipt};
use tokio::sync::Mutex;

use crate::{
    CommandBusError, CommandBusErrorKind, CommandBusObserver, CommandBusReceipt,
    CommandMessageAdapter, CommandProcessingService, CommandProcessor, CommandProcessorError,
    CommandPublication, EncodedCommand, EncodedIntegrationMessage, IntegrationEventBusError,
    IntegrationMessageAdapter, TrustedCommandContext,
};

#[derive(Default)]
struct InMemoryMessagingState {
    commands: std::collections::HashSet<String>,
    command_messages: Vec<EncodedCommand>,
    integration_events: HashMap<String, EncodedIntegrationMessage>,
    integration_messages: Vec<EncodedIntegrationMessage>,
}

pub struct InMemoryMessagingAdapter {
    processor: Option<Arc<CommandProcessor>>,
    service: Option<Arc<dyn CommandProcessingService>>,
    state: Mutex<InMemoryMessagingState>,
    command_gates: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl InMemoryMessagingAdapter {
    pub fn new(processor: Arc<CommandProcessor>) -> Self {
        Self {
            processor: Some(processor),
            service: None,
            state: Mutex::new(InMemoryMessagingState::default()),
            command_gates: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_service(service: Arc<dyn CommandProcessingService>) -> Self {
        Self {
            processor: None,
            service: Some(service),
            state: Mutex::new(InMemoryMessagingState::default()),
            command_gates: Mutex::new(HashMap::new()),
        }
    }

    pub async fn dispatch_as(
        &self,
        caller: &TrustedCommandContext,
        command: EncodedCommand,
    ) -> Result<CommandBusReceipt, CommandBusError> {
        let service = self.service.as_ref().ok_or_else(|| {
            CommandBusError::new(
                CommandBusErrorKind::InvalidConfiguration,
                "context-aware dispatch requires a processing service",
            )
        })?;
        let gate = self.command_gate(command.message_id()).await;
        let _guard = gate.lock().await;
        let processed = service
            .process(caller, &command)
            .await
            .into_result()
            .map_err(|e| CommandBusError::from_processor(&e))?;
        let response = processed
            .outcome()
            .details()
            .available()
            .map_err(|e| CommandBusError::from_processor(&CommandProcessorError::recovery(&e)))?
            .clone();
        let mut state = self.state.lock().await;
        let duplicate = !state
            .commands
            .insert(command.message_id().as_str().to_owned());
        if !duplicate {
            state.command_messages.push(command);
        }
        drop(state);
        Ok(CommandBusReceipt::new(duplicate, response))
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
        let processor = self.processor.as_ref().ok_or_else(|| {
            CommandBusError::new(
                CommandBusErrorKind::InvalidConfiguration,
                "context-aware service requires dispatch_as",
            )
        })?;
        let duplicate = self
            .state
            .lock()
            .await
            .commands
            .contains(command.message_id().as_str());
        observer
            .published(CommandPublication::new(
                command.message_id().clone(),
                duplicate,
            ))
            .await;
        let response = processor
            .process(&command)
            .await
            .map_err(|error| CommandBusError::from_processor(&error))?;
        let mut state = self.state.lock().await;
        if !duplicate {
            state.command_messages.push(command.clone());
        }
        state
            .commands
            .insert(command.message_id().as_str().to_owned());
        drop(state);
        Ok(CommandBusReceipt::new(duplicate, response))
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
