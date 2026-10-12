#![allow(
    clippy::panic_in_result_fn,
    reason = "test assertions report comparison contract failures"
)]

use rostfrei_benchmarks::{BenchResult, comparison::compare};
use serde_json::{Value, json};

fn report() -> Value {
    json!({"release_build":true,"options":{"samples":10,"transactional":true,"history_auditing":false},
        "nats_server_version":"2.12.1","runtime_workers":2,"architecture":"x86_64","os":"linux",
        "cases":[{"events_per_aggregate":50,"total_events":100,"source_payload_bytes":1000,
            "encoded_snapshot_bytes":100,"encoded_response_bytes":50,
            "results":[{"path":"replay_sequential","samples":10,"p50_us":100,"p95_us":200,"published_messages_per_query":104}]}]})
}

#[test]
fn calculates_speedup_and_requests() -> BenchResult {
    let before = report();
    let mut after = report();
    *after
        .pointer_mut("/cases/0/results/0/p50_us")
        .ok_or("median")? = json!(10);
    *after
        .pointer_mut("/cases/0/results/0/published_messages_per_query")
        .ok_or("requests")? = json!(10);
    let result = compare(&before, &after)?;
    let result = result.first().ok_or("comparison")?;
    assert!((result.p50_speedup - 10.0).abs() < f64::EPSILON);
    assert!((result.before_requests - 104.0).abs() < f64::EPSILON);
    assert!((result.after_requests - 10.0).abs() < f64::EPSILON);
    Ok(())
}

#[test]
fn mismatched_workloads_policies_environments_and_samples_are_rejected() -> BenchResult {
    for (path, value) in [
        ("/release_build", json!(false)),
        ("/options/history_auditing", json!(true)),
        ("/options/samples", json!(20)),
        ("/nats_server_version", json!("2.12.15")),
        ("/runtime_workers", json!(4)),
        ("/architecture", json!("aarch64")),
        ("/os", json!("macos")),
        ("/cases/0/total_events", json!(101)),
        ("/cases/0/source_payload_bytes", json!(1001)),
        ("/cases/0/encoded_response_bytes", json!(51)),
        ("/cases/0/results/0/path", json!("another-path")),
        ("/cases/0/results/0/samples", json!(20)),
    ] {
        let mut after = report();
        *after.pointer_mut(path).ok_or("fixture field")? = value;
        assert!(
            compare(&report(), &after).is_err(),
            "mismatch accepted: {path}"
        );
    }
    Ok(())
}

#[test]
fn missing_duplicate_and_invalid_measurements_are_rejected() -> BenchResult {
    for (path, value) in [
        ("/cases", json!([])),
        ("/cases/0/results", json!([])),
        ("/cases/0/results/0/p50_us", json!(0)),
        ("/cases/0/results/0/p95_us", json!(-1)),
        ("/cases/0/results/0/samples", Value::Null),
    ] {
        let mut after = report();
        *after.pointer_mut(path).ok_or("fixture field")? = value;
        assert!(compare(&report(), &after).is_err());
    }
    assert!(compare(&json!({}), &json!({})).is_err());
    let mut duplicate = report();
    let results = duplicate
        .pointer_mut("/cases/0/results")
        .and_then(Value::as_array_mut)
        .ok_or("results")?;
    results.push(results.first().ok_or("path")?.clone());
    assert!(compare(&duplicate, &duplicate).is_err());
    Ok(())
}

#[test]
fn rust_comparison_reproduces_every_recorded_python_comparison() -> BenchResult {
    for contents in [
        include_str!("../../../docs/benchmarks/history-replay-2026-10-06.json"),
        include_str!("../../../docs/benchmarks/history-replay-main-2026-10-07.json"),
    ] {
        let report: Value = serde_json::from_str(contents)?;
        for workload in report
            .get("workloads")
            .and_then(Value::as_array)
            .ok_or("workloads")?
        {
            let comparisons = compare(
                workload.get("before").ok_or("before")?,
                workload.get("after").ok_or("after")?,
            )?;
            let expected = workload
                .get("comparison")
                .and_then(Value::as_array)
                .ok_or("comparison")?;
            assert_eq!(comparisons.len(), expected.len());
            for (actual, expected) in comparisons.iter().zip(expected) {
                assert_eq!(
                    actual.path,
                    expected.get("path").and_then(Value::as_str).ok_or("path")?
                );
                let ratio = expected
                    .get("p50_speedup")
                    .and_then(Value::as_f64)
                    .ok_or("speedup")?;
                assert!((actual.p50_speedup - ratio).abs() < 1e-10);
                for name in [
                    "before_p50_us",
                    "after_p50_us",
                    "before_p95_us",
                    "after_p95_us",
                    "before_requests",
                    "after_requests",
                ] {
                    let actual = serde_json::to_value(actual)?;
                    let actual = actual
                        .get(name)
                        .and_then(Value::as_f64)
                        .ok_or("actual metric")?;
                    let expected = expected
                        .get(name)
                        .and_then(Value::as_f64)
                        .ok_or("expected metric")?;
                    assert!((actual - expected).abs() < 1e-10);
                }
            }
        }
    }
    Ok(())
}
