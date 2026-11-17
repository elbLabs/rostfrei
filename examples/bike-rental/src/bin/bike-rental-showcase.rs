use std::{env, error::Error, fs, io, path::Path, sync::Arc, time::Duration};

use bike_rental::{
    BikeRentalNatsConfig, BikeRentalNatsResourceLimits,
    demo::{demo_stream, reservable_branch_fixture},
    tracer,
};
use rostfrei::nats::{NatsConnection, NatsConnectionConfig, ServerVersion, connect};
use rostfrei::tracer::{
    CommandRequest, ExposeTracePayloadsForLocalDevelopment, NatsApplication, NatsRuntime,
    OperationMode, OperationResult, OperationSnapshot, Tracer,
};
use rostfrei::{Aggregate as _, RecordedEvent, StreamAggregateId, StreamAggregateType, StreamId};
use serde_json::{Value, json};
use uuid::Uuid;

type DemoResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const DAMAGE_REPORT: &str = "Rear brake does not engage";

#[tokio::main]
async fn main() -> DemoResult {
    let run = Uuid::now_v7().simple().to_string();
    let application = format!("bike-showcase-{run}");
    let output = env::current_dir()?
        .join("target/bike-rental-showcase")
        .join(&run);
    fs::create_dir_all(&output)?;
    let connection = connect(
        &NatsConnectionConfig::new("bike-rental-showcase", env::var("ROSTFREI_NATS_URL")?)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;
    let bindings = Arc::new(tracer::bindings()?);
    let config = BikeRentalNatsConfig::for_bindings(
        &application,
        rostfrei::TrafficScope::Test,
        BikeRentalNatsResourceLimits::from_env()?,
        &bindings,
    )?
    .with_fresh_outcomes()?;
    println!("Rostfrei: reservation, payment, rental and workshop");
    println!("Isolated Test application: {application}");
    println!(
        "Given: city-fleet has one serviceable city bike, a €5/hour tariff capped at €25/day, and a €100 deposit.\n"
    );
    let started = NatsApplication::builder(config.context().clone(), connection.clone(), bindings)?
        .with_runtime_config(config.host_config().clone())
        .with_default_test_fixture(reservable_branch_fixture()?)
        .provision_and_start()
        .await;
    let result = match started {
        Ok(host) => {
            let result = demonstrate(&host, &output).await;
            let stopped = host.shutdown().await;
            result.and_then(|()| stopped.map_err(Into::into))
        }
        Err(error) => Err(error.into()),
    };
    let cleaned = cleanup(&connection, &config).await;
    let drained = connection.drain().await;
    result?;
    cleaned?;
    drained?;
    println!("\nEvidence: {}", output.display());
    println!("Demo workers stopped; owned NATS streams removed.");
    Ok(())
}

async fn demonstrate(host: &NatsApplication, output: &Path) -> DemoResult {
    let runtime = host.runtime();
    let tracer = runtime
        .tracer_builder()
        .with_test_transport(runtime.transport())
        .with_test_scenario_reset(runtime.clone())
        .with_default_test_fixture(reservable_branch_fixture()?)
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment))
        .build()?;
    runtime
        .attach_observer(tracer.correlation_observer(OperationMode::Test))
        .await?;
    reserve_and_collect(runtime, &tracer, output).await?;
    let request = || CommandRequest {
        schema_version: 2,
        payload: json!({
            "fleet_id": "city-fleet",
            "bicycle_id": "bike-42",
            "damage_report": DAMAGE_REPORT,
            "returned_at": "2026-12-01T17:00:00Z"
        }),
    };
    let submitted = tracer
        .submit_test(
            "bike-rental",
            "return-bicycle",
            request(),
            Some("damaged-return"),
        )
        .await?;
    let returned = completed(&tracer, &submitted.operation_id).await?;
    ensure(
        matches!(returned.result, Some(OperationResult::Accepted { .. })),
        "return was not accepted",
    )?;
    println!("4. Return accepted: \"{DAMAGE_REPORT}\".");

    let history = runtime.store().load(&demo_stream()).await?;
    let damage = history
        .iter()
        .find(|event| event.event_type() == "bicycle-damage-reported")
        .ok_or("return did not persist the damage report")?;
    let job_stream = observe_repair(runtime, damage, output).await?;
    println!("5. Workshop automatically recorded a repair job with the same report.");
    save_capture(&tracer, &returned, output, "return.json").await?;

    let replay = tracer
        .submit_test(
            "bike-rental",
            "return-bicycle",
            request(),
            Some("damaged-return"),
        )
        .await?;
    ensure(
        replay.operation_id == returned.operation_id,
        "retry created a new operation",
    )?;
    ensure(
        runtime.store().load(&demo_stream()).await?.len() == history.len(),
        "retry changed fleet history",
    )?;
    ensure(
        runtime.store().load(&job_stream).await?.len() == 1,
        "retry created another repair job",
    )?;
    println!("6. Repeat the request: same operation, still one repair job.");

    let submitted = tracer
        .submit_test(
            "bike-rental",
            "rent-bicycle",
            CommandRequest {
                schema_version: 2,
                payload: json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42"}),
            },
            Some("hire-damaged-bike"),
        )
        .await?;
    let refused = completed(&tracer, &submitted.operation_id).await?;
    save_capture(&tracer, &refused, output, "blocked-rental.json").await?;
    ensure(
        matches!(&refused.result, Some(OperationResult::Rejected { rejection, .. }) if rejection.get("code").and_then(Value::as_str) == Some("BICYCLE_UNAVAILABLE")),
        "damaged bike was not refused for hire",
    )?;
    println!("7. Try to hire it again: BICYCLE_UNAVAILABLE. The business rule protects the rider.");
    rebalance(host, &tracer, output).await?;
    demonstrate_refund(runtime, &tracer, output).await?;
    Ok(())
}

async fn reserve_and_collect(runtime: &NatsRuntime, tracer: &Tracer, output: &Path) -> DemoResult {
    let submitted = tracer.submit_test("bike-rental", "reserve-bicycle-category", CommandRequest {
        schema_version: 1,
        payload: json!({"fleet_id": "city-fleet", "reservation_id": "booking-42", "customer_id": "customer-alex", "category": "city",
            "period": {"starts_at": "2026-12-01T09:00:00Z", "ends_at": "2026-12-01T17:00:00Z"}}),
    }, Some("reserve-city-bike")).await?;
    let booked = completed(tracer, &submitted.operation_id).await?;
    ensure(
        matches!(booked.result, Some(OperationResult::Accepted { .. })),
        "reservation was not confirmed",
    )?;
    save_capture(tracer, &booked, output, "reservation.json").await?;
    println!(
        "1. Alex reserves a city bike for 09:00–17:00: €25 rent plus a €100 refundable deposit, due at collection."
    );
    let submitted = tracer.submit_test("bike-rental", "collect-reserved-bicycle", CommandRequest {
        schema_version: 1,
        payload: json!({"fleet_id": "city-fleet", "reservation_id": "booking-42", "bicycle_id": "bike-42", "payment_id": "payment-42", "payment_method": "sandbox-approved"}),
    }, Some("collect-city-bike")).await?;
    let collection = completed(tracer, &submitted.operation_id).await?;
    ensure(
        matches!(collection.result, Some(OperationResult::Accepted { .. })),
        "collection was not requested",
    )?;
    println!("2. Staff selects bike-42: the bike is held while the sandbox processes €125.");
    let payment = payment_history(runtime, "payment-42", 2).await?;
    let decision = payment.last().ok_or("payment decision missing")?;
    let payload: Value = serde_json::from_slice(decision.payload())?;
    ensure(
        payload.get("outcome").and_then(Value::as_str) == Some("approved")
            && payload.get("quote")
                == Some(
                    &json!({"currency": "EUR", "rental_charge_minor": 2500, "security_deposit_minor": 10000}),
                ),
        "finance did not retain the approved rent and deposit",
    )?;
    let fleet = runtime.store().load(&demo_stream()).await?;
    ensure(
        fleet.iter().any(|event| {
            event.event_type() == "bicycle-rented"
                && event.operation_id() == decision.operation_id()
        }),
        "payment and rental must commit together",
    )?;
    save(
        output,
        "payment.json",
        &json!({"aggregate": {"type": "bike-rental/rental-payment", "id": "payment-42"}, "eventId": decision.event_id().as_str(), "payload": payload}),
    )?;
    save_capture(tracer, &collection, output, "collection.json").await?;
    println!(
        "3. Sandbox payment approved: finance records rent and deposit separately, and the rental starts in the same commit."
    );
    Ok(())
}

async fn rebalance(host: &NatsApplication, tracer: &Tracer, output: &Path) -> DemoResult {
    let added = tracer.submit_test("bike-rental", "add-bicycle", CommandRequest {
        schema_version: 2,
        payload: json!({"fleet_id": "harbor-fleet", "bicycle_id": "bike-77", "condition": "serviceable", "category": "city"}),
    }, Some("register-harbor-spare")).await?;
    let added = completed(tracer, &added.operation_id).await?;
    ensure(
        matches!(added.result, Some(OperationResult::Accepted { .. })),
        "harbor spare was not registered",
    )?;
    let submitted = tracer.submit_test(
        "bike-rental",
        "transfer-bicycle",
        CommandRequest {
            schema_version: 2,
            payload: json!({"from_fleet_id": "harbor-fleet", "to_fleet_id": "city-fleet", "bicycle_id": "bike-77"}),
        },
        Some("bring-spare-to-city"),
    ).await?;
    let transferred = completed(tracer, &submitted.operation_id).await?;
    save_capture(tracer, &transferred, output, "transfer.json").await?;
    ensure(
        matches!(transferred.result, Some(OperationResult::Accepted { .. })),
        "spare transfer was not accepted",
    )?;
    let store = host.runtime().store();
    let harbor = store
        .load(&bike_rental::rental_fleet::stream_id("harbor-fleet")?)
        .await?;
    let city = store.load(&demo_stream()).await?;
    let out = harbor.last().ok_or("harbor transfer is missing")?;
    let incoming = city.last().ok_or("city transfer is missing")?;
    ensure(
        out.event_type() == "bicycle-transferred-out"
            && incoming.event_type() == "bicycle-transferred-in"
            && out.operation_id() == incoming.operation_id()
            && out.operation_id().as_str() == transferred.operation_id,
        "both stations must belong to the same transfer operation",
    )?;
    println!(
        "8. Register spare bike-77 at harbor and bring it to city: both fleets updated in one transaction."
    );
    Ok(())
}

async fn demonstrate_refund(runtime: &NatsRuntime, tracer: &Tracer, output: &Path) -> DemoResult {
    let booked = tracer.submit_test("bike-rental", "reserve-bicycle-category", CommandRequest {
        schema_version: 1,
        payload: json!({"fleet_id":"city-fleet", "reservation_id":"booking-77", "customer_id":"customer-sam", "category":"city",
            "period":{"starts_at":"2026-12-02T09:00:00Z", "ends_at":"2026-12-02T17:00:00Z"}}),
    }, Some("reserve-spare")).await?;
    ensure(
        matches!(
            completed(tracer, &booked.operation_id).await?.result,
            Some(OperationResult::Accepted { .. })
        ),
        "second reservation failed",
    )?;
    let collected = tracer.submit_test("bike-rental", "collect-reserved-bicycle", CommandRequest {
        schema_version: 1,
        payload: json!({"fleet_id":"city-fleet", "reservation_id":"booking-77", "bicycle_id":"bike-77", "payment_id":"payment-77", "payment_method":"sandbox-approved"}),
    }, Some("collect-spare")).await?;
    ensure(
        matches!(
            completed(tracer, &collected.operation_id).await?.result,
            Some(OperationResult::Accepted { .. })
        ),
        "second collection failed",
    )?;
    payment_history(runtime, "payment-77", 2).await?;
    println!(
        "9. Sam books and collects bike-77 for the following day, paying €25 rent and a €100 deposit."
    );
    let returned = tracer.submit_test("bike-rental", "return-bicycle", CommandRequest {
        schema_version: 2,
        payload: json!({"fleet_id":"city-fleet", "bicycle_id":"bike-77", "returned_at":"2026-12-02T17:00:00Z"}),
    }, Some("satisfactory-return")).await?;
    let returned = completed(tracer, &returned.operation_id).await?;
    ensure(
        matches!(returned.result, Some(OperationResult::Accepted { .. })),
        "satisfactory return failed",
    )?;
    let history = payment_history(runtime, "payment-77", 3).await?;
    let refund = history.last().ok_or("refund missing")?;
    let payload: Value = serde_json::from_slice(refund.payload())?;
    ensure(
        refund.event_type() == "security-deposit-refunded"
            && payload.get("amount_minor").and_then(Value::as_u64) == Some(10000)
            && payload.get("charge_reference").and_then(Value::as_str)
                == Some("sandbox:payment-77"),
        "refund must return only the deposit to the original charge",
    )?;
    save_capture(tracer, &returned, output, "satisfactory-return.json").await?;
    save(
        output,
        "refund.json",
        &json!({"eventId":refund.event_id().as_str(), "payload":payload}),
    )?;
    println!(
        "10. Sam returns on time with no damage: the sandbox automatically refunds €100; the €25 rental charge stays paid."
    );
    let settled = tracer
        .submit_test(
            "bike-rental",
            "settle-rental-deposit",
            CommandRequest {
                schema_version: 1,
                payload: json!({"payment_id":"payment-42"}),
            },
            Some("review-damaged-deposit"),
        )
        .await?;
    ensure(
        matches!(
            completed(tracer, &settled.operation_id).await?.result,
            Some(OperationResult::Accepted { .. })
        ),
        "damaged deposit assessment failed",
    )?;
    let held = payment_history(runtime, "payment-42", 2).await?;
    ensure(
        held.iter()
            .all(|event| event.event_type() != "security-deposit-refunded"),
        "damaged rental deposit must remain held",
    )?;
    println!(
        "11. Alex's damaged-return deposit remains held for workshop assessment and later settlement."
    );
    Ok(())
}

async fn payment_history(
    runtime: &NatsRuntime,
    payment_id: &str,
    count: usize,
) -> DemoResult<Vec<RecordedEvent>> {
    let stream = StreamId::new(
        StreamAggregateType::new("bike-rental/rental-payment")?,
        StreamAggregateId::new(payment_id)?,
    );
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let history = runtime.store().load(&stream).await?;
            if history.len() >= count {
                return DemoResult::Ok(history);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?
}

async fn observe_repair(
    runtime: &NatsRuntime,
    damage: &RecordedEvent,
    output: &Path,
) -> DemoResult<StreamId> {
    let job_stream = StreamId::new(
        StreamAggregateType::new(bike_rental::repair_job::RepairJobAggregate::aggregate_type())?,
        StreamAggregateId::new(damage.event_id().as_str())?,
    );
    let job_history = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let history = runtime.store().load(&job_stream).await?;
            if !history.is_empty() {
                return DemoResult::Ok(history);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    let job = job_history
        .first()
        .ok_or("workshop job was not persisted")?;
    ensure(
        job.event_type() == "repair-job-opened" && job_history.len() == 1,
        "expected one workshop job",
    )?;
    let job_payload: Value = serde_json::from_slice(job.payload())?;
    ensure(
        job_payload.get("damage_report").and_then(Value::as_str) == Some(DAMAGE_REPORT),
        "workshop lost the damage report",
    )?;
    ensure(
        job.correlation_id() == damage.correlation_id(),
        "workshop lost the return correlation",
    )?;
    save(
        output,
        "repair-job.json",
        &json!({
            "aggregate": {"type": job_stream.aggregate_type().as_str(), "id": job_stream.aggregate_id().as_str()},
            "eventId": job.event_id().as_str(),
            "name": job.event_type(),
            "correlationId": job.correlation_id().map(rostfrei::CorrelationId::as_str),
            "causationId": job.causation_id().map(rostfrei::CausationId::as_str),
            "payload": job_payload
        }),
    )?;
    Ok(job_stream)
}

async fn completed(tracer: &Tracer, operation_id: &str) -> DemoResult<OperationSnapshot> {
    tokio::time::timeout(Duration::from_secs(35), async {
        loop {
            let operation = tracer.operation(operation_id).await?;
            if operation.status.is_terminal() {
                return DemoResult::Ok(operation);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?
}

async fn save_capture(
    tracer: &Tracer,
    operation: &OperationSnapshot,
    output: &Path,
    name: &str,
) -> DemoResult {
    let capture = tracer
        .operation_message_series(&operation.operation_id, "10s".parse()?, "500ms".parse()?)
        .await?;
    save(
        output,
        name,
        &json!({"operation": operation, "messageSeries": capture}),
    )?;
    println!(
        "   Tracer capture: settled={}, fidelity={:?}.",
        capture.capture.settled, capture.capture.fidelity
    );
    Ok(())
}

fn save(output: &Path, name: &str, value: &Value) -> DemoResult {
    fs::write(output.join(name), serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn ensure(condition: bool, message: &str) -> DemoResult {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}

async fn cleanup(connection: &NatsConnection, config: &BikeRentalNatsConfig) -> DemoResult {
    let topology = config.messaging().topology();
    let mut failure = None;
    for stream in [
        topology.command_stream().as_str(),
        topology.command_response_stream().as_str(),
        topology.integration_event_stream().as_str(),
        topology.quarantine_stream().as_str(),
        config.event_store().stream_name(),
        config.event_store().outcome_state_stream_name(),
    ] {
        if let Err(error) = connection.delete_stream_if_exists(stream).await {
            failure.get_or_insert(error);
        }
    }
    failure.map_or(Ok(()), |error| Err(error.into()))
}
