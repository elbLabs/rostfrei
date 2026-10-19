//! Reconstructs genuine v1 history with evolved v2 code, then writes v2 events.
//!
//! Run with `cargo run -p bike-rental --example event_evolution`.

mod inventory;

use std::sync::Arc;

use inventory::{
    ExampleResult, Inventory, RetireBicycle, RetireBicycleHandler, RetirementReason, metadata,
    seed_v1_history, stream,
};
use rostfrei::{CommandExecutor, InMemoryEventStore};

rostfrei::install_macro_support!();

#[tokio::main]
async fn main() -> ExampleResult {
    let store = InMemoryEventStore::new();
    seed_v1_history(&store).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), Arc::new(inventory::codecs()?));
    let before = executor.rehydrate::<Inventory>(&stream()?).await?;
    println!("reconstructed from v1: {:?}", before.state());
    executor
        .execute(
            &RetireBicycleHandler,
            metadata("retire-bike-2")?,
            &RetireBicycle {
                bicycle_id: "bike-2".to_owned(),
                reason: RetirementReason::Damaged,
            },
        )
        .await?;
    for event in store.load(&stream()?).await? {
        println!(
            "{} v{}: {}",
            event.event_type(),
            event.schema_version(),
            String::from_utf8_lossy(event.payload())
        );
    }
    let after = executor.rehydrate::<Inventory>(&stream()?).await?;
    println!("reloaded mixed history: {:?}", after.state());
    Ok(())
}
