use rostfrei_messaging_core::{
    CommandAddress, CommandResponse, CommandResponseOutcome, CorrelationId, ExecutionRecovery,
    MessageId, OperationId,
};

#[test]
fn historical_conversion_failure_round_trips_as_a_blocked_technical_response() {
    let accepted = CommandResponse::accepted(
        MessageId::new("response-1").unwrap(),
        MessageId::new("command-1").unwrap(),
        CommandAddress::new("warehouse-app", "warehouse", "retire-stock").unwrap(),
        OperationId::new("operation-1").unwrap(),
        CorrelationId::new("correlation-1").unwrap(),
    )
    .unwrap();
    let mut wire = serde_json::to_value(accepted).unwrap();
    wire["schema_version"] = serde_json::json!(2);
    wire["outcome"] = serde_json::json!({
        "status": "execution_failed",
        "value": {"source": "codec", "kind": "conversion_failed"}
    });

    let response: CommandResponse = serde_json::from_value(wire.clone())
        .expect("a historical conversion failure is a typed technical failure");
    let CommandResponseOutcome::ExecutionFailed(failure) = response.outcome() else {
        panic!("a failed historical conversion must not become a business outcome");
    };
    assert_eq!(failure.recovery(), ExecutionRecovery::Blocked);
    assert_eq!(serde_json::to_value(response).unwrap(), wire);
}
