#![allow(clippy::panic_in_result_fn)]

use std::{convert::Infallible, error::Error, sync::Arc, time::Duration};

use async_trait::async_trait;
use rostfrei::{
    Aggregate, Apply, BoundedContext, Command, CommandBus, CommandDecision, CommandExecution,
    CommandHandler, CommandHandlingResult, CommandMessageAdapter, CommandProcessor, CommandRequest,
    CommittedDomainEvent, DomainEvent, DomainEventDispatcher, Entity, EventStore,
    InMemoryEventStore, InMemoryMessagingAdapter, InfallibleCommandRejectionMapper, Initialize,
    IntegrationCommandMapper, IntegrationEvent, IntegrationEventBus,
    IntegrationEventCommandHandler, IntegrationEventDispatcherExt, IntegrationEventMapper,
    IntegrationMessageAdapter, OperationId, RoutedCommand, StreamAggregateId, StreamAggregateType,
    StreamId,
};
use rostfrei_messaging_core::{
    ApplicationName, CallerMetadata, CommandEnvelope, CommandResponseOutcome, CorrelationId,
    DeliveryDisposition, DeliveryInfo, MessageDelivery, MessageHandler, RetryDelay,
};
use serde::{Deserialize, Serialize};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(BoundedContext)]
#[domain(id = "ledger", label = "Ledger")]
struct Ledger;

#[derive(rostfrei::DomainIdentity)]
#[allow(dead_code)]
struct AccountId(String);

#[derive(Entity)]
#[domain(id = "account", label = "Account")]
#[allow(dead_code)]
struct Account {
    id: AccountId,
    balance: i64,
}

impl rostfrei::EntityDefinition for Account {
    type Owner = AccountAggregate;
    type Identity = AccountId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, DomainEvent)]
#[domain(id = "account-credited", label = "Account credited")]
struct AccountCredited {
    amount: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, DomainEvent)]
#[domain(id = "balance-observed", label = "Balance observed")]
struct BalanceObserved {
    balance: i64,
}

#[derive(rostfrei::AggregateEvents)]
enum AccountEvents {
    AccountCredited(AccountCredited),
    BalanceObserved(BalanceObserved),
}

#[derive(Aggregate)]
#[domain(id = "account", label = "Account")]
struct AccountAggregate;

impl rostfrei::AggregateDefinition for AccountAggregate {
    type Context = Ledger;
    type Root = Account;
    type Event = AccountEvents;
}

impl Initialize<AccountAggregate> for Account {
    fn initialize(stream_id: &StreamId) -> Self {
        Self {
            id: AccountId(stream_id.aggregate_id().as_str().to_owned()),
            balance: 0,
        }
    }
}

impl Apply<AccountCredited> for Account {
    fn apply(&mut self, event: &AccountCredited) {
        self.balance = self.balance.saturating_add(event.amount);
    }
}

impl Apply<BalanceObserved> for Account {
    fn apply(&mut self, _event: &BalanceObserved) {}
}

#[derive(Clone, Debug, Eq, PartialEq, Command)]
#[domain(context = Ledger, id = "credit-account", label = "Credit account")]
struct CreditAccount {
    account_id: String,
    amount: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Command)]
#[domain(context = Ledger, id = "observe-balance", label = "Observe balance")]
struct ObserveBalance {
    account_id: String,
}

struct CreditAccountHandler;

#[async_trait]
impl CommandHandler<CreditAccount> for CreditAccountHandler {
    type Rejection = Infallible;

    async fn handle(
        &self,
        command: &CreditAccount,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut account = execution
            .load::<AccountAggregate>(&command.account_id)
            .await?;
        account.aggregate_mut().raise(AccountCredited {
            amount: command.amount,
        });
        Ok(CommandDecision::Accepted)
    }
}

struct ObserveBalanceHandler;

#[async_trait]
impl CommandHandler<ObserveBalance> for ObserveBalanceHandler {
    type Rejection = Infallible;

    async fn handle(
        &self,
        command: &ObserveBalance,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut account = execution
            .load::<AccountAggregate>(&command.account_id)
            .await?;
        let balance = account.aggregate().state().balance;
        account.aggregate_mut().raise(BalanceObserved { balance });
        Ok(CommandDecision::Accepted)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct AccountWasCredited {
    account_id: String,
    amount: i64,
}

impl IntegrationEvent for AccountWasCredited {
    const EVENT_NAME: &'static str = "account-was-credited";
    const SCHEMA_VERSION: u32 = 1;
}

struct AccountCreditedMapper;

impl IntegrationEventMapper<AccountCredited> for AccountCreditedMapper {
    type Output = AccountWasCredited;

    fn map(&self, event: &CommittedDomainEvent<'_, AccountCredited>) -> Self::Output {
        AccountWasCredited {
            account_id: event
                .recorded()
                .stream_id()
                .aggregate_id()
                .as_str()
                .to_owned(),
            amount: event.event().amount,
        }
    }
}

struct ObserveBalanceAfterCredit;

impl IntegrationCommandMapper<AccountWasCredited> for ObserveBalanceAfterCredit {
    type Command = ObserveBalance;
    type Error = &'static str;

    fn map(&self, event: &AccountWasCredited) -> Result<Self::Command, Self::Error> {
        Ok(ObserveBalance {
            account_id: event.account_id.clone(),
        })
    }
}

fn stream_id() -> TestResult<StreamId> {
    Ok(StreamId::new(
        StreamAggregateType::new(AccountAggregate::aggregate_type().into_owned())?,
        StreamAggregateId::new("account-1")?,
    ))
}

fn registered_processor(store: &InMemoryEventStore) -> TestResult<CommandProcessor> {
    let erased_store: Arc<dyn EventStore> = Arc::new(store.clone());
    let processor_context = ApplicationName::new("integration-flow-test")?
        .bounded_context("ledger")?
        .name()
        .clone();
    let mut processor = CommandProcessor::new(processor_context, erased_store);
    processor.register::<CreditAccount, CreditAccountHandler>(
        CreditAccountHandler,
        InfallibleCommandRejectionMapper,
    )?;
    processor.register::<ObserveBalance, ObserveBalanceHandler>(
        ObserveBalanceHandler,
        InfallibleCommandRejectionMapper,
    )?;
    Ok(processor)
}

#[tokio::test]
async fn committed_domain_event_drives_integration_command_mapping() -> TestResult {
    let store = InMemoryEventStore::new();
    let processor = registered_processor(&store)?;
    let adapter = Arc::new(InMemoryMessagingAdapter::new(Arc::new(processor)));
    let context = ApplicationName::new("integration-flow-test")?.bounded_context("ledger")?;
    let command_adapter: Arc<dyn CommandMessageAdapter> = adapter.clone();
    let command_bus = CommandBus::new(context.clone(), command_adapter);
    let correlation_id = CorrelationId::new("credit-correlation")?;

    let credit = command_bus
        .dispatch::<CreditAccount>(
            CommandRequest::new(
                OperationId::new("credit-operation")?,
                CreditAccount {
                    account_id: "account-1".to_owned(),
                    amount: 7,
                },
            )
            .with_correlation_id(correlation_id.clone()),
        )
        .await?;
    assert!(matches!(
        credit.response().outcome(),
        CommandResponseOutcome::Accepted
    ));

    let history = store.load(&stream_id()?).await?;
    let credited = history.first().ok_or("credit event was not committed")?;
    let integration_adapter: Arc<dyn IntegrationMessageAdapter> = adapter.clone();
    let integration_bus = IntegrationEventBus::new(context.clone(), integration_adapter);
    let mut dispatcher = DomainEventDispatcher::new();
    dispatcher.register_integration_event::<AccountAggregate, AccountCredited, _>(
        integration_bus,
        AccountCreditedMapper,
    )?;
    dispatcher.dispatch(credited).await?;

    let messages = adapter.integration_messages().await;
    let message = messages
        .first()
        .ok_or("integration event was not published")?;
    let delivery = MessageDelivery::new_with_transport_context(
        message.address().clone(),
        message.message_id().clone(),
        message.payload().to_vec(),
        CallerMetadata::new(),
        message.message().correlation_id().cloned(),
        None,
        DeliveryInfo::new(1, 0, 1, 1)?,
    )?;
    let reaction = IntegrationEventCommandHandler::<AccountWasCredited, _>::new(
        command_bus,
        context.durable_name("observe-balance-after-credit", 1)?,
        RetryDelay::new(Duration::from_secs(1))?,
        ObserveBalanceAfterCredit,
    );

    assert_eq!(
        reaction.handle(delivery.clone()).await,
        DeliveryDisposition::Acknowledge
    );
    assert_eq!(
        reaction.handle(delivery).await,
        DeliveryDisposition::Acknowledge
    );

    let history = store.load(&stream_id()?).await?;
    assert_eq!(history.len(), 2);
    assert_eq!(
        history.get(1).map(rostfrei::RecordedEvent::event_type),
        Some(BalanceObserved::LOCAL_ID)
    );
    assert_eq!(
        history
            .get(1)
            .and_then(rostfrei::RecordedEvent::correlation_id),
        Some(&correlation_id)
    );
    let commands = adapter.command_messages().await;
    assert_eq!(commands.len(), 2);
    let generated = commands.get(1).ok_or("mapped command was not published")?;
    let wire: serde_json::Value = serde_json::from_slice(generated.payload())?;
    assert_eq!(
        wire.pointer("/payload/events_caused_by_command"),
        Some(&serde_json::Value::Bool(true))
    );
    let envelope: CommandEnvelope<RoutedCommand> = serde_json::from_slice(generated.payload())?;
    assert_eq!(
        envelope.causation_id().map(rostfrei::CausationId::as_str),
        Some(message.message_id().as_str())
    );
    assert_eq!(
        history
            .get(1)
            .and_then(rostfrei::RecordedEvent::causation_id)
            .map(rostfrei::CausationId::as_str),
        Some(generated.message_id().as_str())
    );
    Ok(())
}

rostfrei::install_macro_support!();
