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
        // Body is logged at debug level in full; bearer token is intentionally not logged.
        let body_for_log = body.as_ref().map(|b| b.to_string());
        debug!(method = %method, url = %url, body = ?body_for_log, "HTTP request");

        let (mut status, mut text) = self.issue(&method, &url, body.as_ref()).await?;
        debug!(method = %method, url = %url, status = status.as_u16(), body = %text, "HTTP response");

        // Reactive token refresh: a 401 may mean our cached token is stale
        // (clock skew, server restart, revocation). Drop the cache, fetch a
        // fresh token, and retry the same request once.
        if status == StatusCode::UNAUTHORIZED {
            info!("HTTP 401 — invalidating cached token and retrying once");
            self.tokens.invalidate().await;
            let (s2, t2) = self.issue(&method, &url, body.as_ref()).await?;
            debug!(method = %method, url = %url, status = s2.as_u16(), body = %t2, "HTTP response (after refresh)");
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
                body = %text,
                "HTTP response"
            );
            Err(from_aspnet(status.as_u16(), &text))
        }
    }

    pub async fn list_endpoints(&self) -> Result<ListWebhookEndpointsDto, ApiError> {
        self.send(Method::GET, "/v2/webhooks/endpoints", None).await
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

    pub async fn update_endpoint(
        &self,
        id: &str,
        req: &UpdateWebhookEndpointRequest,
    ) -> Result<GetWebhookEndpointDto, ApiError> {
        let body = serde_json::to_value(req)
            .map_err(|e| ApiError::Decode(format!("encode update_endpoint body: {e}")))?;
        self.send(
            Method::PUT,
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
        self.send(
            Method::GET,
            &format!("/v2/webhooks/delivery-logs?pageSize={limit}"),
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
