#[path = "../../rostfrei-testing/src/command_acceptance_contract.rs"]
mod command_acceptance_contract;

use rostfrei_core::InMemoryEventStore;

#[tokio::test]
async fn in_memory_store_persists_command_acceptance() {
    command_acceptance_contract::run(std::sync::Arc::new(InMemoryEventStore::new()))
        .await
        .unwrap();
}
