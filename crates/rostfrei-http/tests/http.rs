#![allow(clippy::panic_in_result_fn)]

use std::{error::Error, sync::Arc};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt as _;
use rostfrei::{
    Aggregate, ApplicationErrorCode, ApplicationName, CommandBus, CommandBusError,
    CommandBusErrorKind, CommandBusObserver, CommandBusReceipt, CommandDefinition,
    CommandMessageAdapter, CommandPublication, DomainRegistry, EncodedCommand,
    InMemoryQueryAdapter, MessageId, QueryDefinition, QueryErrorClassification, QueryErrorPayload,
    QueryHandler, QueryHandlerRequest, QueryMessageAdapter, QueryOptions, QueryProcessor, StreamId,
    TraceContext, command_response_message_id,
};
use rostfrei_http::{HttpApiConfig, HttpApiConfigError, router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower::ServiceExt as _;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

struct ProductAggregate;

impl Aggregate for ProductAggregate {
    type State = ();
    type Event = ();

    const AGGREGATE_TYPE: &'static str = "catalog/product";

    fn initial(_stream_id: &StreamId) -> Self::State {}

    fn apply(_state: &mut Self::State, _event: &Self::Event) {}
}

struct UpdateProduct;

impl CommandDefinition for UpdateProduct {
    type Aggregate = ProductAggregate;

    const COMMAND_NAME: &'static str = "update-product";
    const SCHEMA_VERSION: u32 = 1;
}

#[derive(Debug, Deserialize, QueryDefinition, Serialize)]
#[rostfrei(
    context = "catalog",
    name = "find-product",
    version = 1,
    response = ProductView
)]
struct FindProduct {
    product_id: String,
    limit: u32,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ProductView {
    product_id: String,
    limit: u32,
    traced: bool,
}

struct FindProductHandler;

#[async_trait]
impl QueryHandler<FindProduct, ProductView> for FindProductHandler {
    async fn handle(
        &self,
        request: QueryHandlerRequest<FindProduct>,
    ) -> Result<ProductView, QueryErrorPayload> {
        if request.payload().product_id == "private" {
            let Ok(code) = ApplicationErrorCode::new("catalog.authentication-required") else {
                return Err(QueryErrorPayload::internal_error());
            };
            return Err(QueryErrorPayload::new(
                QueryErrorClassification::Unauthorized,
                code,
                "Authentication is required.",
            )
            .unwrap_or_else(|_| QueryErrorPayload::internal_error()));
        }
        Ok(ProductView {
            product_id: request.payload().product_id.clone(),
            limit: request.payload().limit,
            traced: request.trace_context().is_some_and(|trace| {
                trace.trace_parent() == "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
            }),
        })
    }
}

struct AcceptCommandAdapter;

#[async_trait]
impl CommandMessageAdapter for AcceptCommandAdapter {
    async fn dispatch(
        &self,
        command: EncodedCommand,
        observer: Arc<dyn CommandBusObserver>,
    ) -> Result<CommandBusReceipt, CommandBusError> {
        observer
            .published(CommandPublication::new(command.message_id().clone(), false))
            .await;
        let response = rostfrei::CommandResponse::accepted(
            command_response_message_id(command.message_id()).map_err(|error| {
                CommandBusError::new(CommandBusErrorKind::InvalidConfiguration, error.to_string())
            })?,
            command.message_id().clone(),
            command.address().clone(),
            command.operation_id().clone(),
            command.correlation_id().clone(),
        )
        .map_err(|error| {
            CommandBusError::new(CommandBusErrorKind::InvalidConfiguration, error.to_string())
        })?;
        Ok(CommandBusReceipt::new(false, response))
    }
}

fn app() -> TestResult<Router> {
    app_with_config(HttpApiConfig::default())
}

fn app_with_config(config: HttpApiConfig) -> TestResult<Router> {
    let application = ApplicationName::new("http-test")?;
    let context = application.bounded_context("catalog")?;
    let command_adapter: Arc<dyn CommandMessageAdapter> = Arc::new(AcceptCommandAdapter);
    let command_bus = CommandBus::new(context.clone(), command_adapter);

    let mut query_processor = QueryProcessor::new();
    query_processor.register::<FindProduct>(Arc::new(FindProductHandler))?;
    let query_adapter = Arc::new(InMemoryQueryAdapter::new(Arc::new(query_processor)));
    let query_adapter: Arc<dyn QueryMessageAdapter> = query_adapter;
    let query_bus = rostfrei::QueryBus::new(context, query_adapter);

    let mut registry = DomainRegistry::new();
    registry.register_command::<UpdateProduct>()?;
    registry.register_query::<FindProduct>()?;
    Ok(router(Arc::new(registry), command_bus, query_bus, config))
}

async fn response_json(response: axum::response::Response) -> TestResult<Value> {
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok(serde_json::from_slice(&bytes)?)
}

#[tokio::test]
async fn registered_query_is_available_through_standard_get() -> TestResult {
    let response = app()?
        .oneshot(
            Request::builder()
                .uri(
                    "/contexts/catalog/queries/find-product/schemas/1?product_id=product-1&limit=2",
                )
                .header(
                    "traceparent",
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                )
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL),
        Some(&header::HeaderValue::from_static("private, no-store"))
    );
    assert_eq!(
        response_json(response).await?,
        json!({ "product_id": "product-1", "limit": 2, "traced": true })
    );
    Ok(())
}

#[tokio::test]
async fn registered_command_is_available_through_standard_post() -> TestResult {
    let response = app()?
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(
                    "/contexts/catalog/aggregates/product/product-1/commands/update-product/schemas/1",
                )
                .header(header::CONTENT_TYPE, "application/json")
                .header("idempotency-key", "update-product-1")
                .body(Body::from(r#"{"name":"Road bicycle"}"#))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_json(response).await?,
        json!({
            "status": "accepted",
            "operation_id": "update-product-1",
            "correlation_id": "update-product-1"
        })
    );
    Ok(())
}

#[tokio::test]
async fn unregistered_routes_and_missing_idempotency_are_rejected() -> TestResult {
    let unknown = app()?
        .oneshot(
            Request::builder()
                .uri("/contexts/catalog/queries/unknown/schemas/1")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    let encoded_aggregate_separator = app()?
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(
                    "/contexts/catalog/aggregates/sales%2Fproduct/product-1/commands/update-product/schemas/1",
                )
                .header(header::CONTENT_TYPE, "application/json")
                .header("idempotency-key", "encoded-context-bypass")
                .body(Body::from("{}"))?,
        )
        .await?;
    assert_eq!(
        encoded_aggregate_separator.status(),
        StatusCode::BAD_REQUEST
    );

    let duplicate_parameter = app()?
        .oneshot(
            Request::builder()
                .uri(
                    "/contexts/catalog/queries/find-product/schemas/1?product_id=product-1&product_id=product-2&limit=1",
                )
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(duplicate_parameter.status(), StatusCode::BAD_REQUEST);

    let malformed_encoding = app()?
        .oneshot(
            Request::builder()
                .uri("/contexts/catalog/queries/find-product/schemas/1?product_id=%FF&limit=1")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(malformed_encoding.status(), StatusCode::BAD_REQUEST);

    let missing_key = app()?
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(
                    "/contexts/catalog/aggregates/product/product-1/commands/update-product/schemas/1",
                )
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))?,
        )
        .await?;
    assert_eq!(missing_key.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response_json(missing_key).await?,
        json!({
            "code": "rostfrei.http.missing-header",
            "message": "idempotency-key is required"
        })
    );
    Ok(())
}

#[tokio::test]
async fn unauthorized_outcomes_include_the_configured_challenge() -> TestResult {
    let config = HttpApiConfig::default().with_authentication_challenge(
        header::HeaderValue::from_static("Bearer realm=\"catalog\""),
    )?;
    let response = app_with_config(config)?
        .oneshot(
            Request::builder()
                .uri("/contexts/catalog/queries/find-product/schemas/1?product_id=private&limit=1")
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers().get(header::WWW_AUTHENTICATE),
        Some(&header::HeaderValue::from_static(
            "Bearer realm=\"catalog\""
        ))
    );
    Ok(())
}

#[tokio::test]
async fn configured_command_body_limit_returns_payload_too_large() -> TestResult {
    let response = app_with_config(HttpApiConfig::new(QueryOptions::default(), 1)?)?
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(
                    "/contexts/catalog/aggregates/product/product-1/commands/update-product/schemas/1",
                )
                .header(header::CONTENT_TYPE, "application/json")
                .header("idempotency-key", "update-product-small-limit")
                .body(Body::from("{}"))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    Ok(())
}

#[test]
fn trace_context_fixture_is_valid() -> TestResult {
    TraceContext::new("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")?;
    MessageId::new("http-query")?;
    Ok(())
}

#[test]
fn invalid_authentication_challenge_is_rejected() {
    assert_eq!(
        HttpApiConfig::default()
            .with_authentication_challenge(header::HeaderValue::from_static("")),
        Err(HttpApiConfigError::InvalidAuthenticationChallenge)
    );
}
