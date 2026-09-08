#[allow(dead_code)]
#[path = "../../rostfrei-testing/src/event_store_contract.rs"]
mod contract;

use rostfrei_core::InMemoryEventStore;

#[tokio::test]
async fn in_memory_store_satisfies_the_event_store_contract() {
    contract::run(InMemoryEventStore::new).await;
    contract::run_atomic_multi_stream_transactions(InMemoryEventStore::new).await;
}
