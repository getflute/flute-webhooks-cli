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
    /// In-flight retry that has been scheduled but not yet attempted.
    Pending,
}

/// Pagination metadata that accompanies `Items` in every `PagedResponse<T>`.
/// The Flute v2 spec added this envelope in ARISE-4321 (endpoints list) and
/// ARISE-4319 (delivery-logs list); both endpoints now return `{ items,
/// pageInfo }` regardless of whether callers passed paging query params.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub page_index: i32,
    pub page_size: i32,
    pub total_items: i32,
    pub total_pages: i32,
    pub has_more: bool,
}

/// Standard paginated response envelope. `Items` may be empty; `PageInfo`
/// is always present.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedResponse<T> {
    pub items: Option<Vec<T>>,
    pub page_info: PageInfo,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetWebhookEndpointDto {
    // Wire names (post ARISE-4204 / #1283 spec conformance): `endpointId`,
    // `endpointName`, `endpointStatus`. Rust identifiers stay short for
    // ergonomics. `endpointUrl`, `eventTypes`, `createdOn`, `modifiedOn` are
    // camelCase-natural and don't need explicit renames.
    #[serde(rename = "endpointId")]
    pub id: String,
    #[serde(rename = "endpointName")]
    pub name: Option<String>,
    pub endpoint_url: Option<String>,
    #[serde(rename = "endpointStatus")]
    pub status: WebhookEndpointStatus,
    pub event_types: Option<Vec<String>>,
    pub created_on: Option<DateTime<Utc>>,
    pub modified_on: Option<DateTime<Utc>>,
}

/// Endpoints list response — paginated envelope per ARISE-4321.
pub type ListWebhookEndpointsDto = PagedResponse<GetWebhookEndpointDto>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookEndpointRequest {
    #[serde(rename = "endpointName")]
    pub name: String,
    pub endpoint_url: String,
    pub event_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookEndpointResponse {
    #[serde(rename = "endpointId")]
    pub id: String,
    #[serde(rename = "endpointName")]
    pub name: Option<String>,
    pub endpoint_url: Option<String>,
    #[serde(rename = "endpointStatus")]
    pub status: WebhookEndpointStatus,
    /// One-shot HMAC signing secret. The server only returns it on the
    /// create call; any subsequent GET omits it. Wire name is `hmacSecret`;
    /// `--debug` HTTP body logs redact this field's value.
    pub hmac_secret: Option<String>,
    pub event_types: Option<Vec<String>>,
    pub created_on: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWebhookEndpointRequest {
    #[serde(rename = "endpointName")]
    pub name: String,
    pub endpoint_url: String,
    #[serde(rename = "endpointStatus")]
    pub status: WebhookEndpointStatus,
    pub event_types: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeDto {
    // Post-ARISE-4204 the wire dropped `eventTypeId` and renamed `name` to
    // `eventType`. The catalog is matched by the wire-format event-type
    // string on both sides; consumers no longer need an integer id.
    #[serde(rename = "eventType")]
    pub name: Option<String>,
    pub description: Option<String>,
    pub group: Option<String>,
}

/// Event-types catalog — flat `{ items: [...] }` envelope (not paginated).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEventTypesDto {
    pub items: Option<Vec<EventTypeDto>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryLogSummaryDto {
    // Wire names (post ARISE-4204): `deliveryLogId`, `endpointId`,
    // `endpointName`, `deliveryLogStatus`, `endpointHTTPResponseCode`,
    // `roundTripDurationMs`. Rust identifiers stay short.
    #[serde(rename = "deliveryLogId")]
    pub id: String,
    #[serde(rename = "endpointId")]
    pub webhook_endpoint_id: String,
    #[serde(rename = "endpointName")]
    pub webhook_name: Option<String>,
    pub endpoint_url: Option<String>,
    pub event_id: String,
    pub event_type: Option<String>,
    pub attempt_number: i32,
    #[serde(rename = "deliveryLogStatus")]
    pub status: WebhookDeliveryLogStatus,
    #[serde(rename = "endpointHTTPResponseCode")]
    pub response_status_code: Option<i32>,
    #[serde(rename = "roundTripDurationMs")]
    pub duration_ms: i32,
    pub error_message: Option<String>,
    pub created_on: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryLogDetailDto {
    #[serde(rename = "deliveryLogId")]
    pub id: String,
    #[serde(rename = "endpointId")]
    pub webhook_endpoint_id: String,
    #[serde(rename = "endpointName")]
    pub webhook_name: Option<String>,
    pub endpoint_url: Option<String>,
    pub event_id: String,
    pub event_type: Option<String>,
    pub attempt_number: i32,
    #[serde(rename = "deliveryLogStatus")]
    pub status: WebhookDeliveryLogStatus,
    #[serde(rename = "endpointHTTPResponseCode")]
    pub response_status_code: Option<i32>,
    #[serde(rename = "roundTripDurationMs")]
    pub duration_ms: i32,
    pub error_message: Option<String>,
    pub created_on: DateTime<Utc>,
    pub request_headers: Option<std::collections::HashMap<String, Option<String>>>,
    pub request_body: Option<String>,
    pub response_headers: Option<std::collections::HashMap<String, Option<String>>>,
    pub response_body: Option<String>,
    pub next_retry_at: Option<DateTime<Utc>>,
}

/// Delivery-logs list response — paginated envelope per ARISE-4319.
pub type ListDeliveryLogsDto = PagedResponse<DeliveryLogSummaryDto>;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResponseDto {
    // Wire names (post ARISE-4204): `isDelivered`, `endpointHTTPResponseCode`,
    // `roundTripDurationMs`.
    #[serde(rename = "isDelivered")]
    pub success: bool,
    #[serde(rename = "endpointHTTPResponseCode")]
    pub status_code: Option<i32>,
    #[serde(rename = "roundTripDurationMs")]
    pub duration_ms: i32,
    pub error_message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_endpoint_with_v2_field_names() {
        let json = r#"{"endpointId":"00000000-0000-0000-0000-000000000001","endpointName":"My EP","endpointUrl":"https://x","endpointStatus":"Active","eventTypes":["transaction.card.captured"],"createdOn":"2026-04-30T12:00:00Z","modifiedOn":"2026-04-30T12:00:00Z"}"#;
        let v: GetWebhookEndpointDto = serde_json::from_str(json).unwrap();
        assert_eq!(v.id, "00000000-0000-0000-0000-000000000001");
        assert_eq!(v.name.as_deref(), Some("My EP"));
        assert_eq!(v.endpoint_url.as_deref(), Some("https://x"));
        assert_eq!(v.status, WebhookEndpointStatus::Active);
        assert_eq!(v.event_types.unwrap(), vec!["transaction.card.captured"]);
    }

    #[test]
    fn deserializes_endpoints_list_paginated_envelope() {
        let json = r#"{"items":[{"endpointId":"ep-1","endpointName":"n","endpointUrl":"https://x","endpointStatus":"Active","eventTypes":[],"createdOn":"2026-04-30T12:00:00Z","modifiedOn":"2026-04-30T12:00:00Z"}],"pageInfo":{"pageIndex":0,"pageSize":20,"totalItems":1,"totalPages":1,"hasMore":false}}"#;
        let v: ListWebhookEndpointsDto = serde_json::from_str(json).unwrap();
        assert_eq!(v.items.unwrap().len(), 1);
        assert_eq!(v.page_info.total_items, 1);
        assert!(!v.page_info.has_more);
    }

    #[test]
    fn serializes_create_request_with_endpoint_name() {
        let req = CreateWebhookEndpointRequest {
            name: "My EP".into(),
            endpoint_url: "https://x".into(),
            event_types: vec!["transaction.card.captured".into()],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(
            json.get("endpointName").is_some(),
            "wire field should be endpointName, got: {json}"
        );
        assert!(
            json.get("webhookName").is_none(),
            "legacy `webhookName` must not appear: {json}"
        );
    }

    #[test]
    fn deserializes_create_response_with_hmac_secret_and_created_on() {
        let json = r#"{"endpointId":"ep-1","endpointName":"n","endpointUrl":"https://x","endpointStatus":"Active","hmacSecret":"abc","eventTypes":["ping"],"createdOn":"2026-04-30T12:00:00Z"}"#;
        let v: CreateWebhookEndpointResponse = serde_json::from_str(json).unwrap();
        assert_eq!(v.hmac_secret.as_deref(), Some("abc"));
        assert!(v.created_on.is_some());
    }

    #[test]
    fn serializes_update_request_with_endpoint_status() {
        let req = UpdateWebhookEndpointRequest {
            name: "n".into(),
            endpoint_url: "https://x".into(),
            status: WebhookEndpointStatus::Inactive,
            event_types: vec!["ping".into()],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("endpointStatus").is_some());
        assert!(json.get("status").is_none());
    }

    #[test]
    fn deserializes_event_type_without_id_field() {
        // Post-ARISE-4204 wire drops `eventTypeId` and renames `name` to
        // `eventType`. Consumers match by the wire event-type string.
        let json = r#"{"eventType":"transaction.card.captured","description":"d","group":"Card Transactions"}"#;
        let v: EventTypeDto = serde_json::from_str(json).unwrap();
        assert_eq!(v.name.unwrap(), "transaction.card.captured");
        assert_eq!(v.group.unwrap(), "Card Transactions");
    }

    #[test]
    fn deserializes_event_types_list_with_items_envelope() {
        let json = r#"{"items":[{"eventType":"payment_session.created","description":"created","group":"Payment Sessions"}]}"#;
        let v: ListEventTypesDto = serde_json::from_str(json).unwrap();
        let items = v.items.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name.as_deref(), Some("payment_session.created"));
    }

    #[test]
    fn deserializes_delivery_log_summary_with_v2_field_names() {
        let json = r#"{"deliveryLogId":"00000000-0000-0000-0000-0000000000aa","endpointId":"00000000-0000-0000-0000-0000000000bb","endpointName":"X","endpointUrl":"https://x","eventId":"00000000-0000-0000-0000-0000000000cc","eventType":"transaction.card.captured","attemptNumber":1,"deliveryLogStatus":"Success","endpointHTTPResponseCode":200,"roundTripDurationMs":120,"errorMessage":null,"createdOn":"2026-04-30T12:00:00Z"}"#;
        let v: DeliveryLogSummaryDto = serde_json::from_str(json).unwrap();
        assert_eq!(v.status, WebhookDeliveryLogStatus::Success);
        assert_eq!(v.response_status_code, Some(200));
        assert_eq!(
            v.webhook_endpoint_id,
            "00000000-0000-0000-0000-0000000000bb"
        );
    }

    #[test]
    fn deserializes_delivery_log_summary_with_pending_status() {
        let json = r#"{"deliveryLogId":"00000000-0000-0000-0000-0000000000aa","endpointId":"00000000-0000-0000-0000-0000000000bb","endpointName":null,"endpointUrl":null,"eventId":"00000000-0000-0000-0000-0000000000cc","eventType":"transaction.card.captured","attemptNumber":2,"deliveryLogStatus":"Pending","endpointHTTPResponseCode":null,"roundTripDurationMs":0,"errorMessage":null,"createdOn":"2026-06-04T12:00:00Z"}"#;
        let v: DeliveryLogSummaryDto = serde_json::from_str(json).unwrap();
        assert_eq!(v.status, WebhookDeliveryLogStatus::Pending);
    }

    #[test]
    fn deserializes_delivery_logs_list_paginated_envelope() {
        let json = r#"{"items":[{"deliveryLogId":"00000000-0000-0000-0000-0000000000aa","endpointId":"00000000-0000-0000-0000-0000000000bb","eventId":"00000000-0000-0000-0000-0000000000cc","eventType":"transaction.card.captured","attemptNumber":1,"deliveryLogStatus":"Success","endpointHTTPResponseCode":200,"roundTripDurationMs":12,"errorMessage":null,"createdOn":"2026-04-30T12:00:00Z","endpointName":null,"endpointUrl":null}],"pageInfo":{"pageIndex":0,"pageSize":50,"totalItems":4242,"totalPages":85,"hasMore":true}}"#;
        let v: ListDeliveryLogsDto = serde_json::from_str(json).unwrap();
        assert_eq!(v.items.unwrap().len(), 1);
        assert_eq!(v.page_info.total_items, 4242);
        assert!(v.page_info.has_more);
    }

    #[test]
    fn deserializes_ping_response_with_is_delivered_and_endpoint_http_code() {
        let json = r#"{"isDelivered":true,"endpointHTTPResponseCode":200,"roundTripDurationMs":42,"errorMessage":null}"#;
        let v: PingResponseDto = serde_json::from_str(json).unwrap();
        assert!(v.success);
        assert_eq!(v.status_code, Some(200));
        assert_eq!(v.duration_ms, 42);
    }

    #[test]
    fn delivery_log_summary_and_detail_agree_on_field_names() {
        // Every wire field on the summary DTO must exist on the detail DTO
        // with the same name — consumers reuse field paths between
        // `deliveries list` and `deliveries get`. Regression for the
        // proposal.md concern (fixed in v0.5.6, preserved here).
        let summary = DeliveryLogSummaryDto {
            id: "x".into(),
            webhook_endpoint_id: "y".into(),
            webhook_name: None,
            endpoint_url: None,
            event_id: "z".into(),
            event_type: None,
            attempt_number: 1,
            status: WebhookDeliveryLogStatus::Success,
            response_status_code: None,
            duration_ms: 0,
            error_message: None,
            created_on: chrono::Utc::now(),
        };
        let detail = DeliveryLogDetailDto {
            id: "x".into(),
            webhook_endpoint_id: "y".into(),
            webhook_name: None,
            endpoint_url: None,
            event_id: "z".into(),
            event_type: None,
            attempt_number: 1,
            status: WebhookDeliveryLogStatus::Success,
            response_status_code: None,
            duration_ms: 0,
            error_message: None,
            created_on: chrono::Utc::now(),
            request_headers: None,
            request_body: None,
            response_headers: None,
            response_body: None,
            next_retry_at: None,
        };
        let sj = serde_json::to_value(&summary).unwrap();
        let dj = serde_json::to_value(&detail).unwrap();
        let sk: std::collections::BTreeSet<&str> =
            sj.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        let dk: std::collections::BTreeSet<&str> =
            dj.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        let missing: Vec<&&str> = sk.difference(&dk).collect();
        assert!(
            missing.is_empty(),
            "summary keys not present on detail: {missing:?}",
        );
        // Spot-check that the v2-renamed fields use the namespaced names on
        // BOTH sides — i.e. neither leaks the bare Rust identifier.
        for k in [
            "deliveryLogId",
            "endpointId",
            "endpointName",
            "deliveryLogStatus",
            "endpointHTTPResponseCode",
            "roundTripDurationMs",
        ] {
            assert!(sk.contains(k), "summary missing wire key {k}");
            assert!(dk.contains(k), "detail missing wire key {k}");
        }
        for k in ["id", "status", "duration_ms", "webhook_endpoint_id"] {
            assert!(!sk.contains(k), "summary leaked raw Rust key {k}");
            assert!(!dk.contains(k), "detail leaked raw Rust key {k}");
        }
    }
}
