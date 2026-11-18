//! HTTP and stateless Streamable HTTP MCP adapters for the same Insights service.
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query as QueryParams, Request, State, rejection::JsonRejection,
    },
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{AccessDecision, Insights, InsightsError, Policy, Principal, Query};

#[async_trait]
pub trait InsightsAuthenticator: Send + Sync {
    async fn authenticate(&self, bearer: &str) -> Result<Principal, InsightsError>;
}

#[derive(Clone)]
pub struct InsightsHttp {
    service: Insights,
    authenticator: Arc<dyn InsightsAuthenticator>,
    resource: String,
    issuer: String,
    origins: Vec<String>,
}

impl InsightsHttp {
    pub fn new(
        service: Insights,
        authenticator: Arc<dyn InsightsAuthenticator>,
        resource: &str,
        issuer: &str,
    ) -> Result<Self, InsightsError> {
        let resource_url = reqwest::Url::parse(resource).map_err(|_| InsightsError::Invalid)?;
        let issuer_url = reqwest::Url::parse(issuer).map_err(|_| InsightsError::Invalid)?;
        if !matches!(resource_url.scheme(), "https" | "http")
            || !matches!(issuer_url.scheme(), "https" | "http")
            || resource_url.query().is_some()
            || resource_url.fragment().is_some()
            || !resource_url.username().is_empty()
            || resource_url.password().is_some()
        {
            return Err(InsightsError::Invalid);
        }
        Ok(Self {
            service,
            authenticator,
            resource: resource.into(),
            issuer: issuer.into(),
            origins: vec![resource_url.origin().ascii_serialization()],
        })
    }

    #[must_use]
    pub fn with_browser_origins(mut self, origins: Vec<String>) -> Self {
        self.origins = origins;
        self
    }
}

pub fn router(config: InsightsHttp) -> Router {
    let protected = Router::new()
        .route("/insights/query", post(query))
        .route(
            "/insights/access-requests",
            post(request_access).get(requests),
        )
        .route("/insights/access-requests/{id}", get(access_request))
        .route("/insights/access-requests/{id}/decision", post(decide))
        .route("/insights/grants/{id}/revoke", post(revoke))
        .route("/insights/policies", put(set_policy).get(policy))
        .route("/insights/audit", get(audit))
        .route(
            "/mcp",
            post(mcp).get(|| async { StatusCode::METHOD_NOT_ALLOWED }),
        )
        .route_layer(middleware::from_fn_with_state(config.clone(), authenticate));
    Router::new()
        .merge(protected)
        .route("/.well-known/oauth-protected-resource", get(metadata))
        .route("/.well-known/oauth-protected-resource/mcp", get(metadata))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn(no_store))
        .with_state(config)
}

async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("private, no-store"),
    );
    response
}

async fn authenticate(
    State(config): State<InsightsHttp>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(origin) = request.headers().get(header::ORIGIN)
        && origin
            .to_str()
            .ok()
            .is_none_or(|origin| !config.origins.iter().any(|allowed| allowed == origin))
    {
        return error(InsightsError::Forbidden);
    }
    let bearer = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "));
    let result = match bearer {
        Some(bearer) => config.authenticator.authenticate(bearer).await,
        None => Err(InsightsError::Unauthenticated),
    };
    match result {
        Ok(principal) => {
            // Retain verified attempts even when a body cannot be decoded or a
            // protocol request fails before it reaches a service operation.
            if let Err(failure) = config
                .service
                .record(&principal, "http.attempt", "authenticated")
                .await
            {
                return error(failure);
            }
            request.extensions_mut().insert(principal);
            next.run(request).await
        }
        Err(failure) => {
            let mut response = error(failure);
            if response.status() == StatusCode::UNAUTHORIZED
                && let Ok(resource) = reqwest::Url::parse(&config.resource)
            {
                let challenge = format!(
                    "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\"",
                    resource.origin().ascii_serialization()
                );
                if let Ok(value) = challenge.parse() {
                    response
                        .headers_mut()
                        .insert(header::WWW_AUTHENTICATE, value);
                }
            }
            response
        }
    }
}

fn error(error: InsightsError) -> Response {
    let status = match error {
        InsightsError::Invalid => StatusCode::BAD_REQUEST,
        InsightsError::Unauthenticated => StatusCode::UNAUTHORIZED,
        InsightsError::Forbidden => StatusCode::FORBIDDEN,
        InsightsError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        InsightsError::Conflict | InsightsError::SourceChanged => StatusCode::CONFLICT,
        InsightsError::Capacity => StatusCode::PAYLOAD_TOO_LARGE,
    };
    (status, Json(json!({"error": error.to_string()}))).into_response()
}

fn response<T: serde::Serialize>(result: Result<T, InsightsError>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(failure) => error(failure),
    }
}

async fn query(
    State(c): State<InsightsHttp>,
    axum::Extension(p): axum::Extension<Principal>,
    Json(q): Json<Query>,
) -> Response {
    response(c.service.query(&p, q).await)
}
async fn request_access(
    State(c): State<InsightsHttp>,
    axum::Extension(p): axum::Extension<Principal>,
    Json(mut q): Json<Query>,
) -> Response {
    q.request_missing = true;
    response(c.service.query(&p, q).await)
}
async fn requests(
    State(c): State<InsightsHttp>,
    axum::Extension(p): axum::Extension<Principal>,
) -> Response {
    response(c.service.requests(&p).await)
}
async fn access_request(
    State(c): State<InsightsHttp>,
    axum::Extension(p): axum::Extension<Principal>,
    Path(id): Path<String>,
) -> Response {
    response(c.service.access_request(&p, &id).await)
}
async fn decide(
    State(c): State<InsightsHttp>,
    axum::Extension(p): axum::Extension<Principal>,
    Path(id): Path<String>,
    Json(d): Json<AccessDecision>,
) -> Response {
    response(c.service.decide(&p, &id, d).await)
}
async fn revoke(
    State(c): State<InsightsHttp>,
    axum::Extension(p): axum::Extension<Principal>,
    Path(id): Path<String>,
) -> Response {
    response(c.service.revoke(&p, &id).await)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyUpdate {
    expected_revision: u64,
    policy: Policy,
}

async fn set_policy(
    State(c): State<InsightsHttp>,
    axum::Extension(p): axum::Extension<Principal>,
    Json(update): Json<PolicyUpdate>,
) -> Response {
    response(
        c.service
            .set_policy(&p, update.expected_revision, update.policy)
            .await,
    )
}
async fn policy(
    State(c): State<InsightsHttp>,
    axum::Extension(p): axum::Extension<Principal>,
) -> Response {
    response(
        c.service
            .policy(&p)
            .await
            .map(|(revision, policy)| json!({"revision":revision,"policy":policy})),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuditQuery {
    before: Option<u64>,
    limit: Option<usize>,
}

async fn audit(
    State(c): State<InsightsHttp>,
    axum::Extension(p): axum::Extension<Principal>,
    QueryParams(q): QueryParams<AuditQuery>,
) -> Response {
    response(c.service.audit(&p, q.before, q.limit.unwrap_or(50)).await)
}

async fn metadata(State(c): State<InsightsHttp>) -> Json<Value> {
    Json(json!({
        "resource": c.resource,
        "authorization_servers": [c.issuer],
        "bearer_methods_supported": ["header"]
    }))
}

#[derive(Deserialize)]
struct Rpc {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

async fn mcp(
    State(c): State<InsightsHttp>,
    axum::Extension(p): axum::Extension<Principal>,
    headers: HeaderMap,
    payload: Result<Json<Value>, JsonRejection>,
) -> Response {
    let value = match payload {
        Ok(Json(value)) => value,
        Err(JsonRejection::JsonSyntaxError(_)) => {
            return rpc_error(&Value::Null, -32700, "Parse error");
        }
        Err(rejection) => return rejection.into_response(),
    };
    let Ok(rpc) = serde_json::from_value::<Rpc>(value) else {
        return rpc_error(&Value::Null, -32600, "Invalid Request");
    };
    if rpc.jsonrpc != "2.0"
        || rpc
            .id
            .as_ref()
            .is_some_and(|id| !id.is_string() && !id.is_number())
    {
        return rpc_error(&Value::Null, -32600, "Invalid Request");
    }
    if rpc.method != "initialize"
        && headers
            .get("mcp-protocol-version")
            .is_none_or(|v| v != "2025-11-25")
    {
        return error(InsightsError::Invalid);
    }
    let Some(id) = rpc.id else {
        return if rpc.method.starts_with("notifications/") {
            StatusCode::ACCEPTED.into_response()
        } else {
            rpc_error(&Value::Null, -32600, "Invalid Request")
        };
    };
    if rpc.method == "tools/call"
        && (!matches!(
            rpc.params.get("name").and_then(Value::as_str),
            Some("insights.query" | "insights.request_access" | "insights.get_access_request")
        ) || !rpc.params.get("arguments").is_some_and(Value::is_object))
    {
        return rpc_error(&id, -32602, "Invalid params");
    }
    let result = match rpc.method.as_str() {
        "initialize" => json!({
            "protocolVersion":"2025-11-25",
            "capabilities":{"tools":{}},
            "serverInfo":{"name":"rostfrei-insights","version":env!("CARGO_PKG_VERSION")}
        }),
        "ping" => json!({}),
        "tools/list" => tool_catalog(),
        "tools/call" => match call(&c.service, &p, rpc.params).await {
            Ok(result) => json!({
                "content":[{"type":"text","text":result.to_string()}],
                "structuredContent":result,
                "isError":false
            }),
            Err(failure) => json!({
                "content":[{"type":"text","text":failure.to_string()}],
                "isError":true
            }),
        },
        _ => return rpc_error(&id, -32601, "Method not found"),
    };
    Json(json!({"jsonrpc":"2.0","id":id,"result":result})).into_response()
}

fn rpc_error(id: &Value, code: i32, message: &str) -> Response {
    Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})).into_response()
}

fn tool_catalog() -> Value {
    let query_schema = json!({
        "type":"object",
        "properties":{
            "selection":{"type":"object","properties":{
                "source":{"type":"string"},
                "resource":{"type":"object","properties":{
                    "kind":{"type":"string","enum":["event","command","aggregate","operation","readModel","diagnostic"]},
                    "type":{"type":"string"},"schemaVersion":{"type":"integer","minimum":1}
                },"required":["kind","type","schemaVersion"],"additionalProperties":false},
                "aggregateType":{"type":"string"},"aggregateId":{"type":"string"},
                "ids":{"type":"array","items":{"type":"string"},"maxItems":256}
            },"required":["source","resource"],"additionalProperties":false},
            "fields":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":64},
            "filters":{"type":"array","maxItems":8,"items":{"type":"object","properties":{
                "path":{"type":"string"},"op":{"type":"string","enum":["eq","lt","gt"]},
                "value":{"type":["string","number","boolean"]}
            },"required":["path","op","value"],"additionalProperties":false}},
            "purpose":{"type":"string"},
            "requestMissing":{"type":"boolean"},
            "requestId":{"type":"string"}
        },"required":["selection","fields","purpose"],"additionalProperties":false
    });
    json!({"tools":[
        {"name":"insights.query","description":"Query authorized fields. Every disclosure is audited. requestMissing creates an approval request for restricted data.",
         "inputSchema":query_schema,"annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}},
        {"name":"insights.request_access","description":"Request administrator approval for protected fields; return independently permitted fields.",
         "inputSchema":query_schema,"annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}},
        {"name":"insights.get_access_request","description":"Check the authenticated caller's access request status.",
         "inputSchema":{"type":"object","properties":{"requestId":{"type":"string"}},"required":["requestId"],"additionalProperties":false},
         "annotations":{"readOnlyHint":true,"openWorldHint":false}}
    ]})
}

async fn call(
    service: &Insights,
    principal: &Principal,
    params: Value,
) -> Result<Value, InsightsError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or(InsightsError::Invalid)?;
    let arguments = params
        .get("arguments")
        .cloned()
        .ok_or(InsightsError::Invalid)?;
    match name {
        "insights.query" | "insights.request_access" => {
            let mut query: Query =
                serde_json::from_value(arguments).map_err(|_| InsightsError::Invalid)?;
            if name == "insights.request_access" {
                query.request_missing = true;
            }
            serde_json::to_value(service.query(principal, query).await?)
                .map_err(|_| InsightsError::Unavailable)
        }
        "insights.get_access_request" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Status {
                request_id: String,
            }
            let status: Status =
                serde_json::from_value(arguments).map_err(|_| InsightsError::Invalid)?;
            serde_json::to_value(
                service
                    .access_request(principal, &status.request_id)
                    .await?,
            )
            .map_err(|_| InsightsError::Unavailable)
        }
        _ => Err(InsightsError::Invalid),
    }
}
