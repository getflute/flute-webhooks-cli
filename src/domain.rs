use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::api::models::{
    DeliveryLogSummaryDto, EventTypeDto, GetWebhookEndpointDto,
    WebhookDeliveryLogStatus, WebhookEndpointStatus,
};

#[derive(Debug, Clone, Serialize)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub endpoint_url: String,
    pub event_types: Vec<String>,
    pub status: WebhookEndpointStatus,
    pub created_on: Option<DateTime<Utc>>,
}

impl From<GetWebhookEndpointDto> for Endpoint {
    fn from(d: GetWebhookEndpointDto) -> Self {
        Self {
            id: d.id,
            name: d.name.unwrap_or_else(|| "Untitled".into()),
            endpoint_url: d.endpoint_url.unwrap_or_default(),
            event_types: d.event_types.unwrap_or_default(),
            status: d.status,
            created_on: d.created_on,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeliveryLog {
    pub id: String,
    pub endpoint_id: String,
    pub endpoint_name: String,
    pub endpoint_url: String,
    pub event_id: String,
    pub event_type: String,
    pub status: WebhookDeliveryLogStatus,
    pub attempt_number: i32,
    pub response_status_code: Option<i32>,
    pub duration_ms: i32,
    pub error_message: Option<String>,
    pub created_on: DateTime<Utc>,
}

impl From<DeliveryLogSummaryDto> for DeliveryLog {
    fn from(d: DeliveryLogSummaryDto) -> Self {
        Self {
            id: d.id,
            endpoint_id: d.webhook_endpoint_id,
            endpoint_name: d.webhook_name.unwrap_or_default(),
            endpoint_url: d.endpoint_url.unwrap_or_default(),
            event_id: d.event_id,
            event_type: d.event_type.unwrap_or_default(),
            status: d.status,
            attempt_number: d.attempt_number,
            response_status_code: d.response_status_code,
            duration_ms: d.duration_ms,
            error_message: d.error_message,
            created_on: d.created_on,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EventTypeMeta {
    pub name: String,
    pub description: String,
    pub group: String,
}

impl From<EventTypeDto> for EventTypeMeta {
    fn from(d: EventTypeDto) -> Self {
        Self {
            name: d.name.unwrap_or_default(),
            description: d.description.unwrap_or_default(),
            group: d.group.unwrap_or_else(|| "Other".into()),
        }
    }
}
