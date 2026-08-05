use crate::api::error::{ApiError, from_aspnet};
use crate::api::models::*;
use crate::auth::token::TokenStore;
use reqwest::header::ACCEPT;
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use tracing::{debug, info};

const JSON: &str = "application/json";

#[derive(Clone)]
pub struct ApiClient {
    pub base_url: String,
    pub http: Client,
    pub tokens: TokenStore,
}

impl ApiClient {
    /// Build the request with bearer auth + Accept header + optional JSON body.
    /// Extracted so the same request can be issued twice (once with the cached
    /// token, once with a fresh token after a 401).
    fn build_request(
        &self,
        method: &Method,
        url: &str,
        body: Option<&serde_json::Value>,
        token: &str,
    ) -> RequestBuilder {
        let mut req = self
            .http
            .request(method.clone(), url)
            .bearer_auth(token)
            // Accept: application/json on every request so the ASP.NET content
            // negotiation pipeline always returns JSON (and never falls into a
            // different format-handler that has its own bugs). Content-Type is
            // set for us by .json() when a body is present.
            .header(ACCEPT, JSON);
        match (body, method) {
            (Some(b), _) => {
                req = req.json(b);
            }
            // Bodyless POST/PUT/PATCH: explicitly send an empty body so reqwest
            // emits Content-Length: 0. The Flute API rejects bodyless POSTs
            // without it ("POST requests require a Content-length"), which hit
            // the ping and retry endpoints.
            (None, m) if matches!(*m, Method::POST | Method::PUT | Method::PATCH) => {
                req = req.body("").header(reqwest::header::CONTENT_LENGTH, "0");
            }
            (None, _) => {}
        }
        req
    }

    /// Issue the request once, returning (status, body_text). Used by both
    /// send() and send_no_body() so the 401-retry logic stays in one place.
    async fn issue(
        &self,
        method: &Method,
        url: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<(StatusCode, String), ApiError> {
        let token = self
            .tokens
            .bearer()
            .await
            .map_err(|e| ApiError::Auth(e.to_string()))?;
        let resp = self.build_request(method, url, body, &token).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        Ok((status, text))
    }

    async fn send<R: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<R, ApiError> {
        let url = format!("{}{}", self.base_url, path);
        // Body is logged at debug level with `hmacSecret`/`secret` values
        // masked; bearer token is intentionally not logged.
        let body_for_log = body.as_ref().map(|b| redact_secrets(&b.to_string()));
        debug!(method = %method, url = %url, body = ?body_for_log, "HTTP request");

        let (mut status, mut text) = self.issue(&method, &url, body.as_ref()).await?;
        debug!(method = %method, url = %url, status = status.as_u16(), body = %redact_secrets(&text), "HTTP response");

        // Reactive token refresh: a 401 may mean our cached token is stale
        // (clock skew, server restart, revocation). Drop the cache, fetch a
        // fresh token, and retry the same request once.
        if status == StatusCode::UNAUTHORIZED {
            info!("HTTP 401 — invalidating cached token and retrying once");
            self.tokens.invalidate().await;
            let (s2, t2) = self.issue(&method, &url, body.as_ref()).await?;
            debug!(method = %method, url = %url, status = s2.as_u16(), body = %redact_secrets(&t2), "HTTP response (after refresh)");
            status = s2;
            text = t2;
        }

        if status.is_success() {
            serde_json::from_str::<R>(&text).map_err(|e| ApiError::Decode(e.to_string()))
        } else {
            Err(from_aspnet(status.as_u16(), &text))
        }
    }

    async fn send_no_body(&self, method: Method, path: &str) -> Result<(), ApiError> {
        let url = format!("{}{}", self.base_url, path);
        debug!(method = %method, url = %url, "HTTP request");

        let (mut status, mut text) = self.issue(&method, &url, None).await?;

        if status == StatusCode::UNAUTHORIZED {
            info!("HTTP 401 — invalidating cached token and retrying once");
            self.tokens.invalidate().await;
            let (s2, t2) = self.issue(&method, &url, None).await?;
            status = s2;
            text = t2;
        }

        if status.is_success() {
            debug!(method = %method, url = %url, status = status.as_u16(), "HTTP response (no body)");
            Ok(())
        } else {
            debug!(
                method = %method, url = %url, status = status.as_u16(),
                body = %redact_secrets(&text),
                "HTTP response"
            );
            Err(from_aspnet(status.as_u16(), &text))
        }
    }

    /// List webhook endpoints. Sends `pageSize=100` so a single call returns
    /// the full first page under the server's cap; the current CLI surface
    /// does not yet page beyond that. `pageInfo.hasMore == true` in the
    /// response signals a caller with >100 endpoints — a rare tail case a
    /// follow-up can address by exposing pagination flags on the CLI.
    pub async fn list_endpoints(&self) -> Result<ListWebhookEndpointsDto, ApiError> {
        self.list_endpoints_query("?pageSize=100").await
    }

    /// List webhook endpoints with a pre-built query string (must include
    /// the leading `?` when non-empty). Used by future filtered queries;
    /// goes through the shared `send()` helper so 401 retries and
    /// `from_aspnet` error parsing kick in.
    pub async fn list_endpoints_query(
        &self,
        query: &str,
    ) -> Result<ListWebhookEndpointsDto, ApiError> {
        self.send(Method::GET, &format!("/v2/webhooks/endpoints{query}"), None)
            .await
    }

    pub async fn get_endpoint(&self, id: &str) -> Result<GetWebhookEndpointDto, ApiError> {
        self.send(Method::GET, &format!("/v2/webhooks/endpoints/{id}"), None)
            .await
    }

    pub async fn create_endpoint(
        &self,
        req: &CreateWebhookEndpointRequest,
    ) -> Result<CreateWebhookEndpointResponse, ApiError> {
        let body = serde_json::to_value(req)
            .map_err(|e| ApiError::Decode(format!("encode create_endpoint body: {e}")))?;
        self.send(Method::POST, "/v2/webhooks/endpoints", Some(body))
            .await
    }

    /// Update an endpoint via JSON Merge Patch. The server accepts PATCH
    /// (not PUT), and any field absent from `req` is left unchanged
    /// server-side — the `Option<T>` fields with `skip_serializing_if` on
    /// `UpdateWebhookEndpointRequest` produce a body containing only the
    /// keys the caller actually set.
    pub async fn update_endpoint(
        &self,
        id: &str,
        req: &UpdateWebhookEndpointRequest,
    ) -> Result<GetWebhookEndpointDto, ApiError> {
        let body = serde_json::to_value(req)
            .map_err(|e| ApiError::Decode(format!("encode update_endpoint body: {e}")))?;
        self.send(
            Method::PATCH,
            &format!("/v2/webhooks/endpoints/{id}"),
            Some(body),
        )
        .await
    }

    pub async fn delete_endpoint(&self, id: &str) -> Result<(), ApiError> {
        self.send_no_body(Method::DELETE, &format!("/v2/webhooks/endpoints/{id}"))
            .await
    }

    pub async fn ping_endpoint(&self, id: &str) -> Result<PingResponseDto, ApiError> {
        self.send(
            Method::POST,
            &format!("/v2/webhooks/endpoints/{id}/ping"),
            None,
        )
        .await
    }

    pub async fn list_event_types(&self) -> Result<ListEventTypesDto, ApiError> {
        self.send(Method::GET, "/v2/webhooks/event-types", None)
            .await
    }

    pub async fn list_delivery_logs(&self, limit: u32) -> Result<ListDeliveryLogsDto, ApiError> {
        self.list_delivery_logs_query(&format!("?pageSize={limit}"))
            .await
    }

    /// List delivery logs with a pre-built query string (must include the
    /// leading `?` when non-empty). Used by the CLI for filtered queries —
    /// goes through the shared `send()` helper so 401 retries and
    /// `from_aspnet` error parsing kick in, instead of the previous
    /// bare-metal HTTP path that lost both.
    pub async fn list_delivery_logs_query(
        &self,
        query: &str,
    ) -> Result<ListDeliveryLogsDto, ApiError> {
        self.send(
            Method::GET,
            &format!("/v2/webhooks/delivery-logs{query}"),
            None,
        )
        .await
    }

    pub async fn get_delivery_log(&self, id: &str) -> Result<DeliveryLogDetailDto, ApiError> {
        self.send(
            Method::GET,
            &format!("/v2/webhooks/delivery-logs/{id}"),
            None,
        )
        .await
    }

    pub async fn retry_delivery(&self, id: &str) -> Result<serde_json::Value, ApiError> {
        self.send(
            Method::POST,
            &format!("/v2/webhooks/delivery-logs/{id}/retry"),
            None,
        )
        .await
    }
}

/// Mask `hmacSecret` / `secret` values in a JSON body before it lands in a
/// debug log. `hmacSecret` is a one-shot HMAC returned by `endpoints create`
/// and never re-issued, so keeping it out of shell scrollback and the TUI log
/// file matters even under `--debug`. Non-JSON input is returned unchanged so
/// server error pages (HTML, plain text) still render in the log.
fn redact_secrets(body: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.to_string();
    };
    redact_json_value(&mut v);
    v.to_string()
}

fn redact_json_value(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map.iter_mut() {
                if matches!(k.as_str(), "hmacSecret" | "secret") {
                    *val = serde_json::Value::String("[REDACTED]".into());
                } else {
                    redact_json_value(val);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                redact_json_value(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod redact_tests {
    use super::redact_secrets;
    use serde_json::Value;

    #[test]
    fn redacts_hmac_secret_field_value() {
        let input = r#"{"endpointId":"abc","hmacSecret":"whsec_supersecret"}"#;
        let out = redact_secrets(input);
        assert!(!out.contains("whsec_supersecret"), "secret leaked: {out}");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["hmacSecret"], "[REDACTED]");
        assert_eq!(parsed["endpointId"], "abc");
    }

    #[test]
    fn redacts_legacy_secret_alias() {
        let input = r#"{"secret":"whsec_old"}"#;
        let out = redact_secrets(input);
        assert!(!out.contains("whsec_old"), "secret leaked: {out}");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["secret"], "[REDACTED]");
    }

    #[test]
    fn redacts_nested_secret_inside_array() {
        let input = r#"{"items":[{"hmacSecret":"a"},{"hmacSecret":"b"}]}"#;
        let out = redact_secrets(input);
        assert!(!out.contains("\"a\""));
        assert!(!out.contains("\"b\""));
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["items"][0]["hmacSecret"], "[REDACTED]");
        assert_eq!(parsed["items"][1]["hmacSecret"], "[REDACTED]");
    }

    #[test]
    fn non_json_input_is_returned_unchanged() {
        let input = "<html>bad gateway</html>";
        assert_eq!(redact_secrets(input), input);
    }

    #[test]
    fn json_without_secret_fields_is_semantically_unchanged() {
        let input = r#"{"id":"1","name":"n","nested":{"k":"v"}}"#;
        let a: Value = serde_json::from_str(input).unwrap();
        let b: Value = serde_json::from_str(&redact_secrets(input)).unwrap();
        assert_eq!(a, b);
    }
}
