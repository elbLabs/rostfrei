use std::{env, io::Read, net::IpAddr, time::Duration};

use reqwest::{
    Method, Url,
    blocking::Client,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
    redirect, retry,
};
use serde_json::Value;

use super::{ErrorKind, Phase, Result, ToolError};

pub struct Tracer {
    origin: Url,
    token: HeaderValue,
    client: Client,
}

impl Tracer {
    pub fn from_env() -> Result<Self> {
        let address =
            env::var("ROSTFREI_TRACER_URL").unwrap_or_else(|_| "http://127.0.0.1:1309".to_owned());
        let configuration = || {
            ToolError::new(
                ErrorKind::SetupFailed,
                Phase::Configuration,
                "Tracer URL must be an HTTP(S) origin without credentials; plain HTTP requires loopback",
            )
        };
        let origin = Url::parse(&address).map_err(|_| configuration())?;
        let loopback = origin.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !matches!(origin.scheme(), "http" | "https")
            || origin.host_str().is_none()
            || (origin.scheme() == "http" && !loopback)
            || origin.path() != "/"
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || address.chars().any(char::is_control)
        {
            return Err(configuration());
        }
        let secret = env::var("ROSTFREI_API_TOKEN").map_err(|_| configuration())?;
        if secret.is_empty() || secret.chars().any(char::is_control) {
            return Err(ToolError::new(
                ErrorKind::SetupFailed,
                Phase::Configuration,
                "Missing or invalid control capability",
            ));
        }
        let mut token =
            HeaderValue::from_str(&format!("Bearer {secret}")).map_err(|_| configuration())?;
        token.set_sensitive(true);
        let client = Client::builder()
            .no_proxy()
            .redirect(redirect::Policy::none())
            .retry(retry::never())
            .referer(false)
            .timeout(Duration::from_secs(90))
            .connect_timeout(Duration::from_secs(3))
            .build()
            .map_err(|_| configuration())?;
        Ok(Self {
            origin,
            token,
            client,
        })
    }

    pub fn request(&self, href: &str, body: Option<&Value>, phase: Phase) -> Result<Value> {
        let invalid_link = || {
            ToolError::new(
                ErrorKind::InvalidResponse,
                phase,
                "Unsafe, cross-origin or unresolved Tracer link",
            )
        };
        if !href.starts_with('/')
            || href.starts_with("//")
            || href.chars().any(|ch| {
                ch.is_control() || ch.is_whitespace() || matches!(ch, '\\' | '#' | '{' | '}')
            })
        {
            return Err(invalid_link());
        }
        let url = self.origin.join(href).map_err(|_| invalid_link())?;
        if url.origin() != self.origin.origin() {
            return Err(invalid_link());
        }
        let method = if body.is_some() {
            Method::POST
        } else {
            Method::GET
        };
        let mut request = self
            .client
            .request(method.clone(), url)
            .header(AUTHORIZATION, self.token.clone());
        if let Some(body) = body {
            let bytes = serde_json::to_vec(body).map_err(|_| {
                ToolError::new(ErrorKind::SetupFailed, phase, "Could not encode request")
            })?;
            request = request.header(CONTENT_TYPE, "application/json").body(bytes);
        }
        let mut response = request.send().map_err(|_| {
            ToolError::new(
                ErrorKind::RequestFailed,
                phase,
                "Tracer transport failed or timed out; no retry performed",
            )
            .detail("method", method.as_str())
        })?;
        if !response.status().is_success() {
            return Err(ToolError::new(
                ErrorKind::RequestFailed,
                phase,
                "Tracer HTTP request failed; no retry performed",
            )
            .detail("httpStatus", response.status().as_u16())
            .detail("method", method.as_str()));
        }
        let expected = response.content_length();
        let mut bytes = Vec::new();
        // Classify from the HTTP read boundary, never from error-message text.
        if response.read_to_end(&mut bytes).is_err() {
            return Err(ToolError::new(
                ErrorKind::IncompleteResponse,
                phase,
                "Tracer response body was interrupted; no retry performed",
            )
            .detail("method", method.as_str())
            .detail("receivedBytes", bytes.len())
            .detail("expectedBytes", expected));
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
            ToolError::new(
                ErrorKind::InvalidResponse,
                phase,
                "Tracer response was not valid JSON",
            )
        })?;
        if !value.is_object() {
            return Err(ToolError::new(
                ErrorKind::InvalidResponse,
                phase,
                "Expected a JSON response object",
            ));
        }
        Ok(value)
    }
}
