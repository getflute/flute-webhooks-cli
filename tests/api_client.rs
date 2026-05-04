use std::sync::Arc;
use std::time::Duration;
#[allow(unused_imports)]
use flute_webhook::api::{ApiClient, models::*};
use flute_webhook::auth::token::{Fetcher, TokenStore};
use wiremock::{matchers::{method, path}, MockServer, Mock, ResponseTemplate};

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
async fn list_endpoints_round_trips() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v2/webhooks/endpoints"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"id":"00000000-0000-0000-0000-000000000001","name":"X","endpointUrl":"https://x","status":"Active","eventTypes":["ping"],"createdOn":"2026-04-30T12:00:00Z","modifiedOn":"2026-04-30T12:00:00Z"}]
        })))
        .mount(&server).await;

    let api = client(server.uri());
    let r = api.list_endpoints().await.unwrap();
    assert_eq!(r.data.unwrap().len(), 1);
}

#[tokio::test]
async fn delete_endpoint_propagates_404() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE")).and(path("/v2/webhooks/endpoints/abc"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "details":"missing","title":"Not found","statusCode":404,"correlationId":"cid-1","errorCode":"N0000"
        })))
        .mount(&server).await;

    let api = client(server.uri());
    let err = api.delete_endpoint("abc").await.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("cid-1"), "got: {msg}");
}

#[tokio::test]
async fn list_delivery_logs_round_trips() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v2/webhooks/delivery-logs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "id":"00000000-0000-0000-0000-00000000000a",
                "webhookEndpointId":"00000000-0000-0000-0000-00000000000b",
                "webhookName":"X","endpointUrl":"https://x",
                "eventId":"00000000-0000-0000-0000-00000000000c",
                "eventType":"transaction.card.captured",
                "attemptNumber":1,"status":"Success","responseStatusCode":200,
                "durationMs":12,"errorMessage":null,
                "createdOn":"2026-04-30T12:00:00Z"
            }],
            "pagination":{"hasMore":false,"cursor":null,"totalCount":1}
        })))
        .mount(&server).await;

    let api = client(server.uri());
    let r = api.list_delivery_logs(500).await.unwrap();
    assert_eq!(r.data.unwrap().len(), 1);
    assert_eq!(r.pagination.unwrap().total_count, Some(1));
}

#[tokio::test]
async fn count_delivery_logs_reads_total_count_for_a_specific_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/v2/webhooks/delivery-logs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [],
            // The server returns the global count for the filtered query —
            // this is what we actually care about, not the (truncated) page.
            "pagination": {"hasMore": true, "cursor": null, "totalCount": 4242}
        })))
        .mount(&server).await;

    let api = client(server.uri());
    let count = api.count_delivery_logs("00000000-0000-0000-0000-00000000000b").await.unwrap();
    assert_eq!(count, 4242);
}
