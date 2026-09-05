#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
use rostfrei_core::*;
use rostfrei_messaging_core::{BoundedContextName, CausationId, CorrelationId};

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one manifest fixture with the complete missing/mutated binding matrix"
)]
fn manifest_restoration_requires_every_binding_and_preserves_expired_integrity() {
    let context = BoundedContextName::new("restore").unwrap();
    let operation = OperationId::new("original").unwrap();
    let fingerprint = ContentFingerprint::digest("request");
    let correlation = CorrelationId::new("correlation").unwrap();
    let cause = CausationId::new("cause").unwrap();
    let scope = SecurityScope::new("scope").unwrap();
    let evidence = CommandExecutionMetadata::new(operation.clone(), fingerprint)
        .with_default_outcome_evidence::<()>()
        .outcome_evidence()
        .cloned();
    let proposal = EventTransaction::new(operation.clone(), fingerprint, vec![])
        .with_bounded_context(context.clone())
        .with_correlation_id(correlation.clone())
        .with_causation_id(cause.clone())
        .with_security_scope(scope.clone())
        .with_completion(CommandCompletion::new(
            evidence,
            Some(
                JsonRejectionCodec::new("reason", 1)
                    .encode(&"secret".to_owned())
                    .unwrap(),
            ),
        ));
    let receipt = TransactionReceipt::new(operation, fingerprint, vec![])
        .with_bounded_context(context)
        .with_correlation_id(correlation)
        .with_causation_id(cause)
        .with_security_scope(scope);
    let (marker, bytes) = OutcomeMarker::prepare(
        CommandAuthority::new("store").unwrap(),
        &proposal,
        receipt.clone(),
    )
    .unwrap();
    let manifest = marker.encode_manifest().unwrap();
    let key = marker.detail_reference().key();
    let restored = OutcomeMarker::restore_manifest(key, &manifest, receipt.clone()).unwrap();
    assert_eq!(restored, marker);
    let details = EncodedOutcomeDetails::decode(restored.detail_reference(), &bytes).unwrap();
    CompletedOutcome::new(restored.clone(), OutcomeDetails::Available(details))
        .unwrap()
        .to_inline()
        .unwrap();
    let expired =
        CompletedOutcome::<EncodedOutcomeDetails>::new(restored, OutcomeDetails::Expired).unwrap();
    assert_eq!(expired.marker(), &marker);
    assert!(expired.to_inline().is_err());
    let wire: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    for field in wire.as_object().unwrap().keys() {
        let mut missing = wire.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            OutcomeMarker::restore_manifest(
                key,
                &serde_json::to_vec(&missing).unwrap(),
                receipt.clone()
            )
            .is_err(),
            "missing {field}"
        );
    }
    for (field, replacement) in [
        ("authority", serde_json::json!("other")),
        ("operation", serde_json::json!("other")),
        ("context", serde_json::json!("other")),
        ("scope", serde_json::json!(null)),
        ("version", serde_json::json!(2)),
        ("format", serde_json::json!(2)),
        ("proposal_version", serde_json::json!(2)),
        (
            "revision",
            serde_json::json!(ContentFingerprint::digest("other").to_hex()),
        ),
        ("length", serde_json::json!(100)),
        ("rejected", serde_json::json!(false)),
        ("evidence", serde_json::json!(null)),
        (
            "digest",
            serde_json::json!(ContentFingerprint::digest("other").to_hex()),
        ),
        (
            "integrity",
            serde_json::json!(ContentFingerprint::digest("other").to_hex()),
        ),
    ] {
        let mut changed = wire.clone();
        *changed.get_mut(field).unwrap() = replacement;
        assert!(
            OutcomeMarker::restore_manifest(
                key,
                &serde_json::to_vec(&changed).unwrap(),
                receipt.clone()
            )
            .is_err(),
            "changed {field}"
        );
    }
    for changed in [
        receipt
            .clone()
            .with_correlation_id(CorrelationId::new("other").unwrap()),
        receipt
            .clone()
            .with_causation_id(CausationId::new("other").unwrap()),
        receipt.with_security_scope(SecurityScope::new("other").unwrap()),
    ] {
        assert!(OutcomeMarker::restore_manifest(key, &manifest, changed).is_err());
    }
}
