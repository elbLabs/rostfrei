//! One real identity-provider + NATS + HTTP/MCP journey; owns its Keycloak container.
use std::{process::Command, time::Duration};

use super::*;
use rostfrei_tracer::insights::{OidcAuthenticator, OidcConfig, http::InsightsHttp};

const IMAGE: &str = "quay.io/keycloak/keycloak:26.8.2";

struct Keycloak {
    name: String,
    issuer: String,
    _directory: tempfile::TempDir,
}

impl Drop for Keycloak {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.name])
            .output();
    }
}

fn client(id: &str, roles: &[&str]) -> (Value, Value) {
    let mappers = [
        ("tenant", "garden"),
        ("application", "shop"),
        ("environment", "production"),
    ]
    .into_iter()
    .map(|(name, value)| {
        json!({
            "name":name,"protocol":"openid-connect","protocolMapper":"oidc-hardcoded-claim-mapper",
            "config":{"claim.name":name,"claim.value":value,"jsonType.label":"String",
                "access.token.claim":"true","introspection.token.claim":"true"}
        })
    })
    .chain(std::iter::once(json!({
        "name":"audience","protocol":"openid-connect","protocolMapper":"oidc-audience-mapper",
        "config":{"included.client.audience":"insights-api","access.token.claim":"true",
            "introspection.token.claim":"true"}
    })))
    .collect::<Vec<_>>();
    (
        json!({
            "clientId":id,"secret":"isolated-test-secret","protocol":"openid-connect",
            "enabled":true,"publicClient":false,"serviceAccountsEnabled":true,
            "standardFlowEnabled":false,"directAccessGrantsEnabled":false,
            "defaultClientScopes":["roles"],"protocolMappers":mappers
        }),
        json!({
            "username":format!("service-account-{id}"),"enabled":true,
            "serviceAccountClientId":id,"clientRoles":{"insights-api":roles}
        }),
    )
}

impl Keycloak {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let (reader_client, reader_user) =
            client("reader-client", &["insights.read", "insights.request"]);
        let (admin_client, admin_user) = client(
            "approver-client",
            &[
                "insights.approve",
                "insights.policy.manage",
                "insights.audit.read",
            ],
        );
        let realm = json!({
            "realm":"insights","enabled":true,"sslRequired":"none",
            "roles":{"client":{"insights-api":[
                {"name":"insights.read"},{"name":"insights.request"},{"name":"insights.approve"},
                {"name":"insights.policy.manage"},{"name":"insights.audit.read"}
            ]}},
            "clients":[
                {"clientId":"insights-api","secret":"isolated-test-secret","enabled":true,
                 "protocol":"openid-connect","publicClient":false,"standardFlowEnabled":false},
                reader_client,admin_client
            ],
            "users":[reader_user,admin_user]
        });
        std::fs::write(
            directory.path().join("realm.json"),
            serde_json::to_vec(&realm).unwrap(),
        )
        .unwrap();
        // Import is readable by the container's unprivileged Keycloak user.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let name = format!("rostfrei-insights-{}", uuid::Uuid::now_v7().simple());
        let output = Command::new("docker")
            .args([
                "run",
                "--detach",
                "--name",
                &name,
                "--publish",
                "127.0.0.1::8080",
                "--volume",
                &format!(
                    "{}:/opt/keycloak/data/import:ro",
                    directory.path().display()
                ),
                IMAGE,
                "start-dev",
                "--import-realm",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Keycloak setup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut keycloak = Self {
            name,
            issuer: String::new(),
            _directory: directory,
        };
        let port = Command::new("docker")
            .args(["port", &keycloak.name, "8080/tcp"])
            .output()
            .unwrap();
        let address = String::from_utf8(port.stdout).unwrap();
        keycloak.issuer = format!("http://{}/realms/insights", address.trim());
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(90), async {
            loop {
                if client
                    .get(format!(
                        "{}/.well-known/openid-configuration",
                        keycloak.issuer
                    ))
                    .send()
                    .await
                    .is_ok_and(|response| response.status().is_success())
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("Keycloak discovery did not become ready");
        keycloak
    }

    async fn token(&self, client_id: &str) -> String {
        let result: Value = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("{}/protocol/openid-connect/token", self.issuer))
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", client_id),
                ("client_secret", "isolated-test-secret"),
            ])
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        result.get("access_token").unwrap().as_str().unwrap().into()
    }
}

struct HttpServer {
    url: String,
    client: reqwest::Client,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl HttpServer {
    async fn start(app: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        Self { url, client, task }
    }
}

async fn post(app: &HttpServer, token: &str, path: &str, value: Value) -> (StatusCode, Value) {
    let response = app
        .client
        .post(format!("{}{path}", app.url))
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-11-25")
        .json(&value)
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let status = response.status();
    (status, response.json().await.unwrap())
}

#[tokio::test]
async fn keycloak_identity_and_approval_are_shared_by_http_and_mcp() {
    let keycloak = Keycloak::start().await;
    let (js, journal, name) = journal().await;
    let history = nats_history(&js).await;
    append_customer(&history, json!({"age":34,"name":"Alice"})).await;
    let service = Insights::new(scope(), journal.clone(), &[42; 32])
        .unwrap()
        .with_source(
            "people",
            Arc::new(EventHistorySource::new(scope(), history).unwrap()),
        )
        .unwrap();
    service.set_policy(&admin(), 0, policy()).await.unwrap();
    let config = OidcConfig {
        issuer: keycloak.issuer.clone(),
        audience: "insights-api".into(),
        client_id: "insights-api".into(),
        client_secret: "isolated-test-secret".into(),
        scope: scope(),
        allowed_clients: ["reader-client".into(), "approver-client".into()].into(),
        allow_loopback_http: true,
    };
    let auth = Arc::new(OidcAuthenticator::discover(config).await.unwrap());
    let http = InsightsHttp::new(
        service.clone(),
        auth,
        "http://127.0.0.1/mcp",
        &keycloak.issuer,
    )
    .unwrap();
    let scenario = scenario().await;
    let app = HttpServer::start(http::router(
        tracer(&scenario, false),
        HttpConfig::governed(http),
    ))
    .await;
    let token = keycloak.token("reader-client").await;
    let admin_token = keycloak.token("approver-client").await;
    let (status, first) = post(
        &app,
        &token,
        "/insights/query",
        serde_json::to_value(query()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["records"][0]["fields"], json!({"/age":34}));
    let (_, initialized) = post(&app, &token, "/mcp", json!({
        "jsonrpc":"2.0","id":0,"method":"initialize",
        "params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}
    })).await;
    assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
    let (_, malformed) = post(&app, &token, "/mcp", json!({"jsonrpc":"2.0","id":0})).await;
    assert_eq!(malformed["error"]["code"], -32600);
    let request_id = first["requestId"].as_str().unwrap();
    let decision_path = format!("/insights/access-requests/{request_id}/decision");
    let decision = json!({"approve":true,"durationSeconds":300,"fields":null,"records":null});
    assert_eq!(
        post(&app, &token, &decision_path, decision.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(&app, &admin_token, &decision_path, decision).await.0,
        StatusCode::OK
    );
    let mut query = query();
    query.request_id = Some(request_id.into());
    let (_, result) = post(
        &app,
        &token,
        "/mcp",
        json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"insights.query","arguments":query}
        }),
    )
    .await;
    assert_eq!(result["result"]["isError"], false, "{result}");
    assert_eq!(
        result["result"]["structuredContent"]["records"][0]["fields"]["/name"],
        "Alice"
    );
    assert_eq!(
        post(&app, INSPECTION, "/insights/query", json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
    for path in [
        "/catalog",
        "/aggregates/ordering%2Fcustomer/C7/correlations",
        "/operations/any",
    ] {
        for bearer in [INSPECTION, &token] {
            let response = app
                .client
                .get(format!("{}{path}", app.url))
                .bearer_auth(bearer)
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "governed host must exclude {path}"
            );
        }
    }
    let audit = journal.audit(None, 100).await.unwrap();
    assert!(
        audit.entries.iter().any(|entry| {
            entry.action == "access.decision.denied" && entry.identity.client_id == "reader-client"
        }),
        "denied administrative access must retain the verified caller"
    );
    assert!(audit.entries.iter().any(|entry| {
        entry.action == "query.disclosure"
            && entry.identity.issuer == keycloak.issuer
            && entry.identity.client_id == "reader-client"
            && !entry.identity.subject.is_empty()
    }));
    js.delete_stream(name).await.unwrap();
}
