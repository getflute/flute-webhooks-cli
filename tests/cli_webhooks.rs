//! Integration tests for the `flute-webhooks-cli webhooks …` subcommands.
//!
//! Each test stubs the Flute API with wiremock, builds an ApiClient pointed
//! at the mock server, and drives the dispatcher in `crate::cli::webhooks`
//! directly. We're testing flag parsing, request shaping, and the get/merge/
//! put flow on update — not the underlying HTTP transport (already covered
//! by tests/api_client.rs).

use std::sync::Arc;
use std::time::Duration;

use flute_webhooks_cli::api::ApiClient;
use flute_webhooks_cli::auth::token::{Fetcher, TokenStore};
use flute_webhooks_cli::cli::webhooks;
use flute_webhooks_cli::cli::{
    DeliveriesCommand, EndpointStatusArg, EndpointsCommand, EventTypesCommand, OutputFormat,
    WebhooksCommand,
};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path, query_param},
};

struct StaticFetcher;
#[async_trait::async_trait]
impl Fetcher for StaticFetcher {
    async fn fetch(&self) -> anyhow::Result<(String, Duration)> {
        Ok(("test-token".into(), Duration::from_secs(3600)))
    }
}

fn client(base_url: String) -> ApiClient {
    ApiClient {
        base_url,
        http: reqwest::Client::new(),
        tokens: TokenStore::new(Arc::new(StaticFetcher)),
    }
}

#[tokio::test]
async fn endpoints_list_success_uses_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/webhooks/endpoints"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{
                "endpointId": "ep-1",
                "endpointName": "first",
                "endpointUrl": "https://x",
                "endpointStatus": "Active",
                "eventTypes": ["ping"],
                "createdOn": "2026-05-04T00:00:00Z",
                "modifiedOn": "2026-05-04T00:00:00Z"
            }],
            "pageInfo": {"pageIndex":0,"pageSize":100,"totalItems":1,"totalPages":1,"hasMore":false}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let api = client(server.uri());
    webhooks::run(
        &api,
        OutputFormat::Json,
        WebhooksCommand::Endpoints(EndpointsCommand::List),
    )
    .await
    .expect("endpoints list should succeed");
}

#[tokio::test]
async fn endpoints_create_sends_expected_request_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/webhooks/endpoints"))
        .and(body_json(json!({
            "endpointName": "my-hook",
            "endpointUrl": "https://example.com/hook",
            "eventTypes": ["transaction.card.captured", "refund.completed"]
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "endpointId": "ep-new",
            "endpointName": "my-hook",
            "endpointUrl": "https://example.com/hook",
            "endpointStatus": "Active",
            "hmacSecret": "whsec_test",
            "eventTypes": ["transaction.card.captured", "refund.completed"],
            "createdOn": "2026-05-04T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let api = client(server.uri());
    webhooks::run(
        &api,
        OutputFormat::Json,
        WebhooksCommand::Endpoints(EndpointsCommand::Create {
            url: "https://example.com/hook".into(),
            events: vec![
                "transaction.card.captured".into(),
                "refund.completed".into(),
            ],
            name: Some("my-hook".into()),
        }),
    )
    .await
    .expect("endpoints create should succeed");
}

#[tokio::test]
async fn endpoints_create_rejects_empty_events() {
    let api = client("http://unused".into());
    let r = webhooks::run(
        &api,
        OutputFormat::Table,
        WebhooksCommand::Endpoints(EndpointsCommand::Create {
            url: "https://example.com/hook".into(),
            events: vec![],
            name: None,
        }),
    )
    .await;
    let msg = format!("{}", r.unwrap_err());
    assert!(msg.contains("--events is required"), "got: {msg}");
}

#[tokio::test]
async fn endpoints_update_sends_patch_with_only_supplied_fields() {
    // The user only passes --status; the CLI must PATCH with a body
    // containing only that key. RFC 7396 merge-patch semantics leave every
    // other field unchanged server-side — no GET-first-then-PUT.
    let server = MockServer::start().await;

    Mock::given(method("PATCH"))
        .and(path("/v2/webhooks/endpoints/ep-1"))
        .and(body_json(json!({
            "endpointStatus": "Inactive"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "endpointId": "ep-1",
            "endpointName": "preserved-name",
            "endpointUrl": "https://preserved.example.com/hook",
            "endpointStatus": "Inactive",
            "eventTypes": ["a.b", "c.d"],
            "createdOn": "2026-05-04T00:00:00Z",
            "modifiedOn": "2026-05-04T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let api = client(server.uri());
    webhooks::run(
        &api,
        OutputFormat::Json,
        WebhooksCommand::Endpoints(EndpointsCommand::Update {
            id: "ep-1".into(),
            url: None,
            events: None,
            name: None,
            status: Some(EndpointStatusArg::Inactive),
        }),
    )
    .await
    .expect("endpoints update should succeed");
}

#[tokio::test]
async fn endpoints_delete_with_yes_issues_delete() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v2/webhooks/endpoints/ep-1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let api = client(server.uri());
    webhooks::run(
        &api,
        OutputFormat::Json,
        WebhooksCommand::Endpoints(EndpointsCommand::Delete {
            id: "ep-1".into(),
            yes: true,
        }),
    )
    .await
    .expect("delete --yes should succeed");
}

#[tokio::test]
async fn endpoints_ping_returns_listener_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/webhooks/endpoints/ep-1/ping"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "isDelivered": true,
            "endpointHTTPResponseCode": 200,
            "roundTripDurationMs": 42,
            "errorMessage": null
        })))
        .expect(1)
        .mount(&server)
        .await;

    let api = client(server.uri());
    webhooks::run(
        &api,
        OutputFormat::Json,
        WebhooksCommand::Endpoints(EndpointsCommand::Ping { id: "ep-1".into() }),
    )
    .await
    .expect("ping should succeed");
}

#[tokio::test]
async fn deliveries_list_attaches_filter_query_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/webhooks/delivery-logs"))
        .and(query_param("pageSize", "75"))
        .and(query_param("endpointId", "ep-7"))
        .and(query_param("deliveryLogStatus", "Failure"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [],
            "pageInfo": {"pageIndex":0,"pageSize":75,"totalItems":0,"totalPages":0,"hasMore":false}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let api = client(server.uri());
    webhooks::run(
        &api,
        OutputFormat::Json,
        WebhooksCommand::Deliveries(DeliveriesCommand::List {
            endpoint_id: Some("ep-7".into()),
            status: Some(flute_webhooks_cli::cli::DeliveryStatusArg::Failed),
            limit: 75,
        }),
    )
    .await
    .expect("deliveries list should succeed");
}

#[tokio::test]
async fn deliveries_retry_calls_post_retry() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/webhooks/delivery-logs/dl-1/retry"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "webhookEndpointId": "ep-1",
            "eventId": "evt-1",
            "eventType": "transaction.card.captured",
            "status": "Scheduled",
            "attemptNumber": 2,
            "createdOn": "2026-05-04T00:00:00Z"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let api = client(server.uri());
    webhooks::run(
        &api,
        OutputFormat::Json,
        WebhooksCommand::Deliveries(DeliveriesCommand::Retry { id: "dl-1".into() }),
    )
    .await
    .expect("retry should succeed");
}

#[tokio::test]
async fn event_types_list_pretty_prints_grouped_catalog() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/webhooks/event-types"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [
                { "eventType": "transaction.card.captured", "description": "captured", "group": "Card Transactions" },
                { "eventType": "settlement.batch.completed", "description": "settled", "group": "Settlements" },
                { "eventType": "payment_session.created", "description": "created", "group": "Payment Sessions" }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let api = client(server.uri());
    webhooks::run(
        &api,
        OutputFormat::Table,
        WebhooksCommand::EventTypes(EventTypesCommand::List),
    )
    .await
    .expect("event-types list should succeed");
}
