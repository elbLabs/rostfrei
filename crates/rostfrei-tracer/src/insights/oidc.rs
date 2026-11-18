use std::{collections::BTreeSet, time::Duration};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use super::{AccessScope, Identity, InsightsError, Principal, http::InsightsAuthenticator, now_ms};

/// Confidential resource-server credentials for online token validation.
/// Keep the secret server-side; Debug intentionally omits credentials.
#[derive(Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub audience: String,
    pub client_id: String,
    pub client_secret: String,
    pub scope: AccessScope,
    pub allowed_clients: BTreeSet<String>,
    /// Enables HTTP exclusively for disposable loopback identity-provider tests.
    pub allow_loopback_http: bool,
}

pub struct OidcAuthenticator {
    config: OidcConfig,
    introspection: String,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    introspection_endpoint: String,
}

impl OidcAuthenticator {
    pub async fn discover(config: OidcConfig) -> Result<Self, InsightsError> {
        config.scope.validate()?;
        let issuer = validate_url(&config.issuer, config.allow_loopback_http)?;
        if config.audience.is_empty()
            || config.client_id.is_empty()
            || config.client_secret.is_empty()
            || config.allowed_clients.is_empty()
        {
            return Err(InsightsError::Invalid);
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| InsightsError::Unavailable)?;
        let response = client
            .get(format!(
                "{}/.well-known/openid-configuration",
                config.issuer.trim_end_matches('/')
            ))
            .send()
            .await
            .map_err(|_| InsightsError::Unavailable)?;
        let discovery: Discovery = serde_json::from_slice(&bounded_body(response).await?)
            .map_err(|_| InsightsError::Unavailable)?;
        let endpoint = validate_url(
            &discovery.introspection_endpoint,
            config.allow_loopback_http,
        )?;
        if discovery.issuer != config.issuer || endpoint.origin() != issuer.origin() {
            return Err(InsightsError::Invalid);
        }
        Ok(Self {
            config,
            introspection: endpoint.to_string(),
            client,
        })
    }

    pub fn issuer(&self) -> &str {
        &self.config.issuer
    }
}

fn validate_url(url: &str, loopback: bool) -> Result<reqwest::Url, InsightsError> {
    let url = reqwest::Url::parse(url).map_err(|_| InsightsError::Invalid)?;
    let local = matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "::1"));
    if (url.scheme() != "https" && !(loopback && local && url.scheme() == "http"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(InsightsError::Invalid);
    }
    Ok(url)
}

async fn bounded_body(mut response: reqwest::Response) -> Result<Vec<u8>, InsightsError> {
    if !response.status().is_success() {
        return Err(InsightsError::Unavailable);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| InsightsError::Unavailable)?
    {
        if bytes.len().saturating_add(chunk.len()) > 64 * 1024 {
            return Err(InsightsError::Unavailable);
        }
        bytes.extend(chunk);
    }
    Ok(bytes)
}

fn claim<'a>(claims: &'a Value, key: &str) -> Result<&'a str, InsightsError> {
    claims
        .get(key)
        .and_then(Value::as_str)
        .ok_or(InsightsError::Unauthenticated)
}

#[async_trait]
impl InsightsAuthenticator for OidcAuthenticator {
    async fn authenticate(&self, token: &str) -> Result<Principal, InsightsError> {
        if token.is_empty() || token.len() > 16 * 1024 {
            return Err(InsightsError::Unauthenticated);
        }
        // Keycloak validates signature, validity and session state through its
        // authenticated introspection endpoint. No caller-supplied claims are trusted.
        let response = self
            .client
            .post(&self.introspection)
            .basic_auth(&self.config.client_id, Some(&self.config.client_secret))
            .form(&[("token", token), ("token_type_hint", "access_token")])
            .send()
            .await
            .map_err(|_| InsightsError::Unavailable)?;
        let claims: Value = serde_json::from_slice(&bounded_body(response).await?)
            .map_err(|_| InsightsError::Unauthenticated)?;
        let now = now_ms()? / 1000;
        let audience = claims.get("aud").is_some_and(|audience| {
            audience.as_str() == Some(self.config.audience.as_str())
                || audience.as_array().is_some_and(|audiences| {
                    audiences
                        .iter()
                        .any(|audience| audience.as_str() == Some(self.config.audience.as_str()))
                })
        });
        if claims.get("active").and_then(Value::as_bool) != Some(true)
            || claim(&claims, "iss")? != self.config.issuer
            || !audience
            || claims
                .get("exp")
                .and_then(Value::as_u64)
                .is_none_or(|exp| exp <= now)
            || claims
                .get("nbf")
                .is_some_and(|nbf| nbf.as_u64().is_none_or(|nbf| nbf > now))
            || claim(&claims, "typ")? != "Bearer"
        {
            return Err(InsightsError::Unauthenticated);
        }
        let client_id = claim(&claims, "client_id").or_else(|_| claim(&claims, "azp"))?;
        if !self.config.allowed_clients.contains(client_id)
            || claim(&claims, "tenant")? != self.config.scope.tenant
            || claim(&claims, "application")? != self.config.scope.application
            || claim(&claims, "environment")? != self.config.scope.environment
        {
            return Err(InsightsError::Forbidden);
        }
        let roles = claims
            .get("resource_access")
            .and_then(|resources| resources.get(&self.config.audience))
            .and_then(|resource| resource.get("roles"))
            .and_then(Value::as_array)
            .ok_or(InsightsError::Forbidden)?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let expires = claims
            .get("exp")
            .and_then(Value::as_u64)
            .and_then(|seconds| seconds.checked_mul(1000))
            .ok_or(InsightsError::Unauthenticated)?;
        Ok(Principal::from_trusted_identity(
            Identity {
                issuer: self.config.issuer.clone(),
                subject: claim(&claims, "sub")?.into(),
                client_id: client_id.into(),
            },
            self.config.scope.clone(),
            roles,
        )?
        .with_valid_until_ms(expires))
    }
}
