use crate::api::error::{from_aspnet, ApiError};
use crate::api::models::*;
use crate::auth::token::TokenStore;
use reqwest::{Client, Method};

#[derive(Clone)]
pub struct ApiClient {
    pub base_url: String,
    pub http: Client,
    pub tokens: TokenStore,
}

impl ApiClient {
    async fn send<R: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<R, ApiError> {
        let token = self.tokens.bearer().await.map_err(|e| ApiError::Auth(e.to_string()))?;
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.http.request(method, &url).bearer_auth(token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if status.is_success() {
            serde_json::from_str::<R>(&text).map_err(|e| ApiError::Decode(e.to_string()))
        } else {
            Err(from_aspnet(status.as_u16(), &text))
        }
    }

    async fn send_no_body(&self, method: Method, path: &str) -> Result<(), ApiError> {
        let token = self.tokens.bearer().await.map_err(|e| ApiError::Auth(e.to_string()))?;
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.request(method, &url).bearer_auth(token).send().await?;
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(from_aspnet(status.as_u16(), &text))
        }
    }

    pub async fn list_endpoints(&self) -> Result<ListWebhookEndpointsDto, ApiError> {
        self.send(Method::GET, "/v2/webhooks/endpoints", None).await
    }

    pub async fn create_endpoint(
        &self,
        req: &CreateWebhookEndpointRequest,
    ) -> Result<CreateWebhookEndpointResponse, ApiError> {
        self.send(
            Method::POST,
            "/v2/webhooks/endpoints",
            Some(serde_json::to_value(req).unwrap()),
        )
        .await
    }

    pub async fn update_endpoint(
        &self,
        id: &str,
        req: &UpdateWebhookEndpointRequest,
    ) -> Result<GetWebhookEndpointDto, ApiError> {
        self.send(
            Method::PUT,
            &format!("/v2/webhooks/endpoints/{id}"),
            Some(serde_json::to_value(req).unwrap()),
        )
        .await
    }

    pub async fn delete_endpoint(&self, id: &str) -> Result<(), ApiError> {
        self.send_no_body(Method::DELETE, &format!("/v2/webhooks/endpoints/{id}")).await
    }

    pub async fn ping_endpoint(&self, id: &str) -> Result<PingResponseDto, ApiError> {
        self.send(Method::POST, &format!("/v2/webhooks/endpoints/{id}/ping"), None).await
    }

    pub async fn list_event_types(&self) -> Result<ListEventTypesDto, ApiError> {
        self.send(Method::GET, "/v2/webhooks/event-types", None).await
    }
}
