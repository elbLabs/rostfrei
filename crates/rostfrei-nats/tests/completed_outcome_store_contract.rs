#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use release_contract_support::*;
rostfrei::install_macro_support!();

#[tokio::test]
async fn real_nats_completed_outcome_store_and_session_conformance() -> TestResult {
    let fixture = Fixture::new("92-store-conformance").await?;
    let store = fixture.event_store().await?;
    rostfrei_testing::completed_outcome_contract::run(store.as_ref(), fixture.context.name()).await
}
