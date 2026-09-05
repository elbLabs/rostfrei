//! ACT-01: behavioral RED on the frozen actor-less implementation, no future APIs.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
mod modular_support;
use modular_support::*;
rostfrei::install_macro_support!();

#[test]
fn injected_actor_cannot_validate_under_an_actorless_command_identity() -> TestResult {
    let command = bus(baseline_processor(
        Arc::new(InMemoryEventStore::new()),
        Arc::new(AtomicUsize::new(0)),
    )?)?
    .encode(request("actor-injection", false)?)?;
    command.validate()?;
    let mut wire: serde_json::Value = serde_json::from_slice(command.payload())?;
    wire["metadata_version"] = serde_json::json!(2);
    wire["actor"] = serde_json::json!({"namespace":"people", "id":"alice"});
    let injected = EncodedCommand::from_delivery(
        command.address().clone(),
        command.message_id().clone(),
        serde_json::to_vec(&wire)?,
    );
    assert!(
        injected.and_then(|command| command.validate()).is_err(),
        "valid actor injection was silently ignored under the original actor-less hash/ID"
    );
    Ok(())
}
