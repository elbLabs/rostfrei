#![allow(clippy::panic_in_result_fn)]

use std::{convert::Infallible, error::Error, sync::Arc};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt as _;
use rostfrei::{
    Aggregate, AggregateInstance, ApplicationErrorCode, ApplicationName, Command, CommandBus,
    CommandBusError, CommandBusErrorKind, CommandBusObserver, CommandBusReceipt, CommandHandler,
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

#[derive(Command, Debug, Deserialize, Serialize)]
#[domain(id = "update-product", label = "Update product")]
struct UpdateProduct;

impl CommandHandler<UpdateProduct> for ProductAggregate {
    type Rejection = Infallible;

    fn handle(
        _command: &UpdateProduct,
        _aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        Ok(())
    }
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
    criteria: Value,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct ProductView {
    product_id: String,
    limit: u32,
    criteria: Value,
    traced: bool,
}

struct FindProductHandler;

#[async_trait]
impl QueryHandler<FindProduct, ProductView> for FindProductHandler {
    async fn handle(
        &self,
        request: QueryHandlerRequest<FindProduct>,
    ) -> Result<ProductView, QueryErrorPayload> {
        let classification = match request.payload().product_id.as_str() {
            "invalid" => Some(QueryErrorClassification::InvalidRequest),
            "private" => Some(QueryErrorClassification::Unauthorized),
            "forbidden" => Some(QueryErrorClassification::Forbidden),
            "missing" => Some(QueryErrorClassification::NotFound),
            "conflict" => Some(QueryErrorClassification::Conflict),
            "rate-limited" => Some(QueryErrorClassification::RateLimited),
            "unavailable" => Some(QueryErrorClassification::Unavailable),
            "timeout" => Some(QueryErrorClassification::Timeout),
            "internal" => Some(QueryErrorClassification::Internal),
            _ => None,
        };
        if let Some(classification) = classification {
            let Ok(code) = ApplicationErrorCode::new("catalog.query-error") else {
                return Err(QueryErrorPayload::internal_error());
            };
            return Err(
                QueryErrorPayload::new(classification, code, "The query failed.")
                    .unwrap_or_else(|_| QueryErrorPayload::internal_error()),
            );
        }
        Ok(ProductView {
            product_id: request.payload().product_id.clone(),
            limit: request.payload().limit,
            criteria: request.payload().criteria.clone(),
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
    registry.register_command::<ProductAggregate, UpdateProduct>()?;
    registry.register_query::<FindProduct>()?;
    Ok(router(Arc::new(registry), command_bus, query_bus, config))
}

async fn response_json(response: axum::response::Response) -> TestResult<Value> {
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok(serde_json::from_slice(&bytes)?)
}

#[tokio::test]
async fn registered_query_is_available_through_standard_post() -> TestResult {
    let response = app()?
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/contexts/catalog/queries/find-product/schemas/1")
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    "traceparent",
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                )
                .body(Body::from(
                    json!({
                        "product_id": "product-1",
                        "limit": 2,
                        "criteria": null
                    })
                    .to_string(),
                ))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL),
        Some(&header::HeaderValue::from_static("private, no-store"))
    );
    assert_eq!(
        response_json(response).await?,
        json!({
            "product_id": "product-1",
            "limit": 2,
            "criteria": null,
            "traced": true
        })
    );
    Ok(())
}

#[tokio::test]
async fn query_preserves_structured_and_nested_json_payloads() -> TestResult {
    let criteria = json!({
        "categories": ["bicycles", "accessories"],
        "availability": {
            "warehouses": [1, 3],
            "include_backorder": false
        },
        "minimum_rating": 4.5
    });
    let response = app()?
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/contexts/catalog/queries/find-product/schemas/1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "product_id": "product-2",
                        "limit": 10,
                        "criteria": criteria
                    })
                    .to_string(),
                ))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await?["criteria"], criteria);
    Ok(())
}

#[tokio::test]
async fn invalid_query_json_returns_structured_http_error() -> TestResult {
    let response = app()?
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/contexts/catalog/queries/find-product/schemas/1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"product_id":"product-1""#))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response_json(response).await?["code"],
        "rostfrei.http.invalid-json"
    );
    Ok(())
}

#[tokio::test]
async fn query_requires_application_json_content_type() -> TestResult {
    for content_type in [None, Some("text/plain")] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/contexts/catalog/queries/find-product/schemas/1");
        if let Some(content_type) = content_type {
            request = request.header(header::CONTENT_TYPE, content_type);
        }
        let response = app()?.oneshot(request.body(Body::from("{}"))?).await?;

        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(
            response_json(response).await?["code"],
            "rostfrei.http.invalid-json"
        );
    }
    Ok(())
}

#[tokio::test]
async fn get_is_not_accepted_for_query_routes() -> TestResult {
    let response = app()?
        .oneshot(
            Request::builder()
                .uri("/contexts/catalog/queries/find-product/schemas/1")
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
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
                .method("POST")
                .uri("/contexts/catalog/queries/unknown/schemas/1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))?,
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
                .method("POST")
                .uri("/contexts/catalog/queries/find-product/schemas/1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"product_id":"private","limit":1,"criteria":null}"#,
                ))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers().get(header::WWW_AUTHENTICATE),
        Some(&header::HeaderValue::from_static(
            "Bearer realm=\"catalog\""
        ))
    );
    assert_eq!(
        response_json(response).await?,
        json!({
            "error": {
                "classification": "unauthorized",
                "code": "catalog.query-error",
                "message": "The query failed."
            }
        })
    );
    Ok(())
}

#[tokio::test]
async fn query_application_error_mappings_are_unchanged() -> TestResult {
    for (product_id, status, classification) in [
        ("invalid", StatusCode::BAD_REQUEST, "invalid_request"),
        ("forbidden", StatusCode::FORBIDDEN, "forbidden"),
        ("missing", StatusCode::NOT_FOUND, "not_found"),
        ("conflict", StatusCode::CONFLICT, "conflict"),
        (
            "rate-limited",
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
        ),
        (
            "unavailable",
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
        ),
        ("timeout", StatusCode::GATEWAY_TIMEOUT, "timeout"),
        ("internal", StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    ] {
        let response = app()?
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/contexts/catalog/queries/find-product/schemas/1")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "product_id": product_id,
                            "limit": 1,
                            "criteria": null
                        })
                        .to_string(),
                    ))?,
            )
            .await?;

        assert_eq!(response.status(), status);
        let body = response_json(response).await?;
        assert_eq!(body["error"]["classification"], classification);
        assert_eq!(body["error"]["code"], "catalog.query-error");
    }
    Ok(())
}

#[tokio::test]
async fn configured_query_body_limit_returns_payload_too_large() -> TestResult {
    let response = app_with_config(HttpApiConfig::new(QueryOptions::default(), 1)?)?
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/contexts/catalog/queries/find-product/schemas/1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        response_json(response).await?["code"],
        "rostfrei.http.payload-too-large"
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
rostfrei::install_macro_support!();
