#[allow(dead_code)]
#[path = "../../rostfrei-testing/src/event_store_contract.rs"]
mod contract;

#[path = "../../rostfrei-testing/src/command_acceptance_contract.rs"]
mod command_acceptance_contract;

use rostfrei_core::InMemoryEventStore;

#[tokio::test]
async fn in_memory_store_satisfies_the_event_store_contract() {
    contract::run(InMemoryEventStore::new).await;
    contract::run_atomic_multi_stream_transactions(InMemoryEventStore::new).await;
}

#[tokio::test]
async fn in_memory_store_persists_command_acceptance() {
    command_acceptance_contract::run(std::sync::Arc::new(InMemoryEventStore::new()))
        .await
        .unwrap();
}
