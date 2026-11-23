#![cfg(feature = "http")]
#![allow(
    clippy::panic_in_result_fn,
    reason = "test assertions with fallible setup"
)]

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt as _;
use rostfrei_core::InMemoryEventStore;
use rostfrei_registry::DomainRegistry;
use rostfrei_tracer::{
    StudioDomainInventory, StudioReadModel, TracerBuilder,
    domain_inventory::ReadModelDeclaration,
    http::{self, HttpConfig},
    workload_memory::CgroupV2Workloads,
};
use serde_json::{Value, json};
use std::{error::Error, fs, num::NonZeroU32, sync::Arc};
use tower::ServiceExt as _;

const SNAPSHOT: &str = "/inventory/workloads";
type TestResult = Result<(), Box<dyn Error>>;

#[tokio::test]
async fn workload_inspection_is_opt_in() -> TestResult {
    let app = http::router(builder().build()?, config()?);
    let (_, catalog) = request(&app, Method::GET, "/catalog", "inspection").await?;
    assert!(catalog.get("workloads").is_none());
    assert_eq!(
        request(&app, Method::GET, SNAPSHOT, "inspection").await?.0,
        StatusCode::NOT_IMPLEMENTED
    );
    Ok(())
}

#[tokio::test]
async fn inspection_samples_only_explicitly_mapped_container_labels() -> TestResult {
    let fixture = tempfile::tempdir()?;
    let finite = fixture.path().join("private-directory");
    let unlimited = fixture.path().join("another-private-directory");
    let unsupported = fixture.path().join("unsupported-directory");
    for dir in [&finite, &unlimited, &unsupported] {
        fs::create_dir(dir)?;
    }
    fs::write(finite.join("memory.current"), "2097152\n")?;
    fs::write(finite.join("memory.max"), "8388608\n")?;
    fs::write(unlimited.join("memory.current"), "0\n")?;
    fs::write(unlimited.join("memory.max"), "max\n")?;
    fs::write(
        unsupported.join("memory.current"),
        format!("{}1", " ".repeat(64)),
    )?;
    let source = CgroupV2Workloads::new([
        ("access", &finite),
        ("billing", &unlimited),
        ("missing", &unsupported),
        ("unmapped", &finite),
    ])?;
    let inventory = StudioDomainInventory::default()
        .with_read_model(declare("unmapped"), None)
        .with_studio_read_model(StudioReadModel::new(declare("one"), None).with_workload("access"))
        .with_studio_read_model(StudioReadModel::new(declare("two"), None).with_workload("access"))
        .with_studio_read_model(
            StudioReadModel::new(declare("three"), None).with_workload("billing"),
        )
        .with_studio_read_model(
            StudioReadModel::new(declare("four"), None).with_workload("missing"),
        );
    let tracer = builder()
        .with_domain_inventory(inventory)
        .with_workload_memory(Arc::new(source))
        .build()?;
    let app = http::router(tracer, config()?);

    assert_eq!(
        request(&app, Method::GET, "/catalog", "inspection")
            .await?
            .1
            .get("workloads"),
        Some(&json!({"snapshotHref": SNAPSHOT}))
    );
    assert!(
        request(&app, Method::GET, "/catalog", "control")
            .await?
            .1
            .get("workloads")
            .is_none()
    );
    let (_, domain) = request(&app, Method::GET, "/inventory/domain", "inspection").await?;
    assert!(domain.pointer("/readModels/0/workload").is_none());
    assert_eq!(
        domain.pointer("/readModels/1/workload"),
        Some(&json!("access"))
    );
    assert_eq!(
        domain.pointer("/readModels/2/workload"),
        Some(&json!("access"))
    );
    for token in ["control", "dispatch"] {
        assert_eq!(
            request(&app, Method::GET, SNAPSHOT, token).await?.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(&app, Method::POST, SNAPSHOT, "inspection").await?.0,
        StatusCode::METHOD_NOT_ALLOWED
    );

    let (status, snapshot) = request(
        &app,
        Method::GET,
        "/inventory/workloads?label=unmapped&path=%2Fprivate",
        "inspection",
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        snapshot,
        json!({"workloads": [
            {"label": "access", "memory": {"currentBytes": 2_097_152, "limit": {"status": "limited", "bytes": 8_388_608}}},
            {"label": "billing", "memory": {"currentBytes": 0, "limit": {"status": "unlimited"}}},
            {"label": "missing", "memory": {"currentBytes": null, "limit": {"status": "unavailable"}}}
        ]})
    );
    Ok(())
}

fn declare(name: &str) -> ReadModelDeclaration {
    ReadModelDeclaration {
        context: "ordering".into(),
        name: name.into(),
        schema_version: NonZeroU32::MIN,
        storage: None,
    }
}

fn builder() -> TracerBuilder {
    TracerBuilder::new(Arc::new(InMemoryEventStore::new()), DomainRegistry::new())
}

fn config() -> Result<HttpConfig, Box<dyn Error>> {
    Ok(HttpConfig::new("control")?
        .with_dispatch_token("dispatch")?
        .with_inspection_token("inspection")?)
}

async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    token: &str,
) -> Result<(StatusCode, Value), Box<dyn Error>> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );
    let status = response.status();
    let body = response.into_body().collect().await?.to_bytes();
    Ok((
        status,
        if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body)?
        },
    ))
}
