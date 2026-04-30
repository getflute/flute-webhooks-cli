use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::api::models::{
    DeliveryLogSummaryDto, EventTypeDto, GetWebhookEndpointDto,
    WebhookDeliveryLogStatus, WebhookEndpointStatus,
};

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub endpoint_url: String,
    pub event_types: Vec<String>,
    pub status: WebhookEndpointStatus,
    pub created_on: Option<DateTime<Utc>>,
    pub trigger_count: u32, // populated by aggregate_counts
    pub trigger_count_partial: bool, // true when there are more pages
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
            trigger_count: 0,
            trigger_count_partial: false,
        }
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

/// Aggregate trigger counts onto endpoints from a delivery-log page.
/// `has_more` becomes the `trigger_count_partial` flag for endpoints
/// whose count was non-zero (so the UI can render `42+`).
pub fn aggregate_counts(endpoints: &mut [Endpoint], logs: &[DeliveryLog], has_more: bool) {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for log in logs {
        *counts.entry(log.endpoint_id.as_str()).or_insert(0) += 1;
    }
    for ep in endpoints.iter_mut() {
        let n = counts.get(ep.id.as_str()).copied().unwrap_or(0);
        ep.trigger_count = n;
        ep.trigger_count_partial = has_more && n > 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(id: &str) -> Endpoint {
        Endpoint {
            id: id.into(), name: id.into(), endpoint_url: "".into(), event_types: vec![],
            status: WebhookEndpointStatus::Active, created_on: None,
            trigger_count: 0, trigger_count_partial: false,
        }
    }

    fn log(eid: &str) -> DeliveryLog {
        DeliveryLog {
            id: format!("log-{eid}"), endpoint_id: eid.into(),
            endpoint_name: "".into(), endpoint_url: "".into(),
            event_id: "".into(), event_type: "transaction.card.captured".into(),
            status: WebhookDeliveryLogStatus::Success,
            attempt_number: 1, response_status_code: Some(200), duration_ms: 1,
            error_message: None, created_on: Utc::now(),
        }
    }

    #[test]
    fn aggregates_counts_per_endpoint() {
        let mut eps = vec![ep("a"), ep("b"), ep("c")];
        let logs = vec![log("a"), log("a"), log("b")];
        aggregate_counts(&mut eps, &logs, false);
        assert_eq!(eps[0].trigger_count, 2);
        assert_eq!(eps[1].trigger_count, 1);
        assert_eq!(eps[2].trigger_count, 0);
        assert!(!eps[0].trigger_count_partial);
    }

    #[test]
    fn marks_partial_when_more_pages_exist_and_endpoint_had_logs() {
        let mut eps = vec![ep("a"), ep("b")];
        let logs = vec![log("a")];
        aggregate_counts(&mut eps, &logs, true);
        assert!(eps[0].trigger_count_partial);
        // b had no logs, so we don't claim it might have more
        assert!(!eps[1].trigger_count_partial);
    }
}
