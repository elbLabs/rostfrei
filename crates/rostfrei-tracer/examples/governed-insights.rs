//! Read an existing application's JSON event history through Keycloak-governed
//! HTTP/MCP. Setup: docs/continuous-observation.md#governed-insights.
use std::{env, error::Error, fs, io, sync::Arc, time::Duration};

use rostfrei_messaging_core::{ApplicationName, BoundedContext, BoundedContextName};
use rostfrei_nats::{NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig};
use rostfrei_tracer::insights::{
    AccessScope, EventHistorySource, Insights, NatsInsightsConfig, NatsInsightsJournal,
    OidcAuthenticator, OidcConfig,
    http::{InsightsHttp, router},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let scope = AccessScope {
        tenant: env::var("INSIGHTS_TENANT")?,
        application: env::var("INSIGHTS_APPLICATION")?,
        environment: env::var("INSIGHTS_ENVIRONMENT")?,
    };
    // Use credentials for this tenant/environment's NATS account.
    let connection = rostfrei_nats::connect(&NatsConnectionConfig::new(
        "governed-insights",
        env::var("NATS_URL")?,
    ))
    .await?;
    let js = connection.jetstream().clone();
    let name = env::var("INSIGHTS_JOURNAL")?;
    let journal = if env::args().any(|arg| arg == "--provision") {
        NatsInsightsJournal::provision(
            js.clone(),
            &name,
            scope.clone(),
            NatsInsightsConfig {
                replicas: env::var("INSIGHTS_REPLICAS")?.parse()?,
                max_bytes: env::var("INSIGHTS_MAX_BYTES")?.parse()?,
                retention: Duration::from_secs(env::var("INSIGHTS_RETENTION_SECONDS")?.parse()?),
            },
        )
        .await?
    } else {
        NatsInsightsJournal::connect(js.clone(), &name, scope.clone()).await?
    };
    let context = BoundedContext::new(
        ApplicationName::new(scope.application.clone())?,
        BoundedContextName::new(env::var("INSIGHTS_CONTEXT")?)?,
    );
    let history = NatsEventStore::connect_history(
        js,
        NatsEventStoreConfig::for_bounded_context(&context)?.with_fresh_outcomes()?,
    )
    .await?;
    let key: [u8; 32] = fs::read(env::var("INSIGHTS_BINDING_KEY_FILE")?)?
        .try_into()
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "INSIGHTS_BINDING_KEY_FILE must contain 32 random bytes",
            )
        })?;
    let service = Insights::new(scope.clone(), Arc::new(journal), &key)?.with_source(
        "events",
        Arc::new(EventHistorySource::new(scope.clone(), Arc::new(history))?),
    )?;
    let issuer = env::var("INSIGHTS_ISSUER")?;
    let auth = OidcAuthenticator::discover(OidcConfig {
        issuer: issuer.clone(),
        audience: env::var("INSIGHTS_AUDIENCE")?,
        client_id: env::var("INSIGHTS_INTROSPECTION_CLIENT")?,
        client_secret: fs::read_to_string(env::var("INSIGHTS_CLIENT_SECRET_FILE")?)?
            .trim()
            .into(),
        scope,
        allowed_clients: env::var("INSIGHTS_ALLOWED_CLIENTS")?
            .split(',')
            .map(|client| client.trim().to_owned())
            .collect(),
        allow_loopback_http: env::var("INSIGHTS_LOOPBACK_HTTP").as_deref() == Ok("true"),
    })
    .await?;
    let http = InsightsHttp::new(
        service,
        Arc::new(auth),
        &env::var("INSIGHTS_RESOURCE")?,
        &issuer,
    )?
    .with_browser_origins(
        env::var("INSIGHTS_BROWSER_ORIGINS")?
            .split(',')
            .map(|origin| origin.trim().to_owned())
            .collect(),
    );
    let listener = tokio::net::TcpListener::bind(
        env::var("INSIGHTS_LISTEN").unwrap_or_else(|_| "127.0.0.1:1309".into()),
    )
    .await?;
    axum::serve(listener, router(http)).await?;
    Ok(())
}
