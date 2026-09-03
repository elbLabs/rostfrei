#![allow(clippy::panic_in_result_fn)]

use std::{error::Error, sync::Arc};

use async_trait::async_trait;
use rostfrei::{
    DynamicQueryRequest, InMemoryQueryAdapter, MessageId, QueryBindingRegistrationError, QueryBus,
    QueryBusErrorKind, QueryDefinition, QueryErrorClassification, QueryErrorPayload, QueryHandler,
    QueryMessageAdapter, QueryOptions, QueryOutcome, QueryProcessor, QueryRequest,
    QueryRequestError, QueryRequestErrorKind, QueryResponse, QueryResult,
};
use rostfrei_messaging_core::{
    ApplicationName, BoundedContext, QueryRequest as MessageQueryRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, QueryDefinition)]
#[rostfrei(
    context = "catalog",
    name = "find-product",
    version = 1,
    response = Option<Product>
)]
struct FindProduct {
    product_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Product {
    product_id: String,
    name: String,
}

struct FindProductHandler;

#[async_trait]
impl QueryHandler<FindProduct, Option<Product>> for FindProductHandler {
    async fn handle(
        &self,
        request: MessageQueryRequest<FindProduct>,
    ) -> Result<Option<Product>, QueryErrorPayload> {
        Ok(
            (request.payload().product_id == "product-1").then(|| Product {
                product_id: request.payload().product_id.clone(),
                name: "Road bicycle".to_owned(),
            }),
        )
    }
}

struct WrongIdentityAdapter {
    request_id: MessageId,
}

#[async_trait]
impl QueryMessageAdapter for WrongIdentityAdapter {
    async fn request(
        &self,
        query: rostfrei::EncodedQuery,
        _options: QueryOptions,
    ) -> QueryResult<serde_json::Value> {
        let request = query.into_request();
        QueryResponse::success(
            self.request_id.clone(),
            request.schema_version(),
            request.created_at(),
            request.correlation_id().clone(),
            request.trace_context().cloned(),
            json!({ "product_id": "product-1", "name": "Wrong response" }),
        )
        .map_err(|_| QueryRequestError::new(QueryRequestErrorKind::InvalidResponse))
    }
}

fn context() -> TestResult<BoundedContext> {
    Ok(ApplicationName::new("query-bus-test")?.bounded_context("catalog")?)
}

fn registered_processor() -> TestResult<Arc<QueryProcessor>> {
    let mut processor = QueryProcessor::new();
    processor.register::<FindProduct>(Arc::new(FindProductHandler))?;
    Ok(Arc::new(processor))
}

fn bus(processor: Arc<QueryProcessor>) -> TestResult<QueryBus> {
    let adapter = Arc::new(InMemoryQueryAdapter::new(processor));
    let erased: Arc<dyn QueryMessageAdapter> = adapter;
    Ok(QueryBus::new(context()?, erased))
}

#[tokio::test]
async fn registered_query_types_request_without_name_branching() -> TestResult {
    let bus = bus(registered_processor()?)?;
    let response = bus
        .request(
            QueryRequest::new(FindProduct {
                product_id: "product-1".to_owned(),
            })
            .with_message_id(MessageId::new("find-product-1")?),
            QueryOptions::default(),
        )
        .await?;

    assert_eq!(
        response.into_outcome(),
        QueryOutcome::Success(Some(Product {
            product_id: "product-1".to_owned(),
            name: "Road bicycle".to_owned(),
        }))
    );
    Ok(())
}

#[tokio::test]
async fn dynamic_queries_share_typed_validation_and_error_contracts() -> TestResult {
    let bus = bus(registered_processor()?)?;
    let invalid = bus
        .request_dynamic(
            DynamicQueryRequest::new("find-product", 1, json!({ "unknown": true }))?
                .with_message_id(MessageId::new("invalid-query")?),
            QueryOptions::default(),
        )
        .await?;
    let unknown = bus
        .request_dynamic(
            DynamicQueryRequest::new("unknown-query", 1, json!({}))?
                .with_message_id(MessageId::new("unknown-query")?),
            QueryOptions::default(),
        )
        .await?;

    let QueryOutcome::Error(invalid) = invalid.into_outcome() else {
        return Err("invalid query should return an application error".into());
    };
    assert_eq!(
        invalid.classification(),
        QueryErrorClassification::InvalidRequest
    );
    assert_eq!(invalid.code().as_str(), "rostfrei.query.invalid-payload");

    let QueryOutcome::Error(unknown) = unknown.into_outcome() else {
        return Err("unknown query should return an application error".into());
    };
    assert_eq!(unknown.code().as_str(), "rostfrei.query.unknown");
    Ok(())
}

#[tokio::test]
async fn bus_rejects_adapter_responses_for_another_request() -> TestResult {
    let adapter: Arc<dyn QueryMessageAdapter> = Arc::new(WrongIdentityAdapter {
        request_id: MessageId::new("another-request")?,
    });
    let bus = QueryBus::new(context()?, adapter);
    let error = bus
        .request_dynamic(
            DynamicQueryRequest::new("find-product", 1, json!({ "product_id": "product-1" }))?
                .with_message_id(MessageId::new("expected-request")?),
            QueryOptions::default(),
        )
        .await
        .expect_err("response identity mismatch should fail");

    assert_eq!(error.kind(), QueryBusErrorKind::InvalidResponse);
    Ok(())
}

#[tokio::test]
async fn duplicate_bindings_and_context_mismatches_fail_explicitly() -> TestResult {
    let mut processor = QueryProcessor::new();
    processor.register::<FindProduct>(Arc::new(FindProductHandler))?;
    let error = processor
        .register::<FindProduct>(Arc::new(FindProductHandler))
        .err()
        .ok_or("duplicate query binding should fail")?;
    assert_eq!(
        error,
        QueryBindingRegistrationError::Duplicate {
            bounded_context: "catalog",
            query: "find-product",
            schema_version: 1,
        }
    );

    let adapter = Arc::new(InMemoryQueryAdapter::new(Arc::new(processor)));
    let erased: Arc<dyn QueryMessageAdapter> = adapter;
    let wrong_context = ApplicationName::new("query-bus-test")?.bounded_context("sales")?;
    let bus = QueryBus::new(wrong_context, erased);
    let error = bus
        .request(
            QueryRequest::new(FindProduct {
                product_id: "product-1".to_owned(),
            }),
            QueryOptions::default(),
        )
        .await
        .expect_err("typed query should reject a mismatched bus context");
    assert_eq!(error.kind(), QueryBusErrorKind::Encoding);
    Ok(())
}
rostfrei::install_macro_support!();
