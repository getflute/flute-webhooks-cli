use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum WebhookEndpointStatus {
    Active,
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum WebhookDeliveryLogStatus {
    Success,
    Failure,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetWebhookEndpointDto {
    pub id: String,
    pub name: Option<String>,
    pub endpoint_url: Option<String>,
    pub status: WebhookEndpointStatus,
    pub event_types: Option<Vec<String>>,
    pub created_on: Option<DateTime<Utc>>,
    pub modified_on: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListWebhookEndpointsDto {
    pub data: Option<Vec<GetWebhookEndpointDto>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookEndpointRequest {
    pub name: String,
    pub endpoint_url: String,
    pub event_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookEndpointResponse {
    pub id: String,
    pub name: Option<String>,
    pub endpoint_url: Option<String>,
    pub status: WebhookEndpointStatus,
    pub secret: Option<String>,
    pub event_types: Option<Vec<String>>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWebhookEndpointRequest {
    pub name: String,
    pub endpoint_url: String,
    pub status: WebhookEndpointStatus,
    pub event_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeDto {
    pub id: i32,
    pub name: Option<String>,
    pub description: Option<String>,
    pub group: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEventTypesDto {
    pub data: Option<Vec<EventTypeDto>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryLogSummaryDto {
    pub id: String,
    pub webhook_endpoint_id: String,
    pub webhook_name: Option<String>,
    pub endpoint_url: Option<String>,
    pub event_id: String,
    pub event_type: Option<String>,
    pub attempt_number: i32,
    pub status: WebhookDeliveryLogStatus,
    pub response_status_code: Option<i32>,
    pub duration_ms: i32,
    pub error_message: Option<String>,
    pub created_on: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryLogDetailDto {
    pub id: String,
    pub webhook_endpoint_id: String,
    pub webhook_name: Option<String>,
    pub endpoint_url: Option<String>,
    pub event_id: String,
    pub event_type: Option<String>,
    pub attempt_number: i32,
    pub status: WebhookDeliveryLogStatus,
    pub response_status_code: Option<i32>,
    pub duration_ms: i32,
    pub error_message: Option<String>,
    pub created_on: DateTime<Utc>,
    pub request_headers: Option<std::collections::HashMap<String, Option<String>>>,
    pub request_body: Option<String>,
    pub response_headers: Option<std::collections::HashMap<String, Option<String>>>,
    pub response_body: Option<String>,
    pub next_retry_at: Option<DateTime<Utc>>,
}

/// Live API delivery-logs response shape: `{ "items": [...], "total": N }`.
/// (The earlier `{ "data": [...], "pagination": {...} }` shape was from a
/// stale local swagger.json — confirmed against the live spec at
/// /isv-api/swagger/v2/swagger.json.)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDeliveryLogsDto {
    pub items: Option<Vec<DeliveryLogSummaryDto>>,
    pub total: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResponseDto {
    pub success: bool,
    pub status_code: Option<i32>,
    pub duration_ms: i32,
    pub error_message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_endpoint_camel_case() {
        let json = r#"{"id":"00000000-0000-0000-0000-000000000001","name":"My EP","endpointUrl":"https://x","status":"Active","eventTypes":["transaction.card.captured"],"createdOn":"2026-04-30T12:00:00Z","modifiedOn":"2026-04-30T12:00:00Z"}"#;
        let v: GetWebhookEndpointDto = serde_json::from_str(json).unwrap();
        assert_eq!(v.id, "00000000-0000-0000-0000-000000000001");
        assert_eq!(v.endpoint_url.as_deref(), Some("https://x"));
        assert_eq!(v.status, WebhookEndpointStatus::Active);
        assert_eq!(v.event_types.unwrap(), vec!["transaction.card.captured"]);
    }

    #[test]
    fn deserializes_event_type_grouping() {
        let json = r#"{"id":1,"name":"transaction.card.captured","description":"d","group":"Card Transactions"}"#;
        let v: EventTypeDto = serde_json::from_str(json).unwrap();
        assert_eq!(v.name.unwrap(), "transaction.card.captured");
        assert_eq!(v.group.unwrap(), "Card Transactions");
    }

    #[test]
    fn deserializes_delivery_log_summary() {
        let json = r#"{"id":"00000000-0000-0000-0000-0000000000aa","webhookEndpointId":"00000000-0000-0000-0000-0000000000bb","webhookName":"X","endpointUrl":"https://x","eventId":"00000000-0000-0000-0000-0000000000cc","eventType":"transaction.card.captured","attemptNumber":1,"status":"Success","responseStatusCode":200,"durationMs":120,"errorMessage":null,"createdOn":"2026-04-30T12:00:00Z"}"#;
        let v: DeliveryLogSummaryDto = serde_json::from_str(json).unwrap();
        assert_eq!(v.status, WebhookDeliveryLogStatus::Success);
        assert_eq!(v.response_status_code, Some(200));
    }

    #[test]
    fn deserializes_delivery_logs_list_with_items_and_total() {
        // Matches the live API shape: { items: [...], total: N }.
        let json = r#"{"items":[{"id":"00000000-0000-0000-0000-0000000000aa","webhookEndpointId":"00000000-0000-0000-0000-0000000000bb","eventId":"00000000-0000-0000-0000-0000000000cc","eventType":"transaction.card.captured","attemptNumber":1,"status":"Success","responseStatusCode":200,"durationMs":12,"errorMessage":null,"createdOn":"2026-04-30T12:00:00Z","webhookName":null,"endpointUrl":null}],"total":4242}"#;
        let v: ListDeliveryLogsDto = serde_json::from_str(json).unwrap();
        assert_eq!(v.items.unwrap().len(), 1);
        assert_eq!(v.total, Some(4242));
    }
}
