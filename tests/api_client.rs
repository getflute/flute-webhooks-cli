use std::sync::Arc;
use std::time::Duration;
#[allow(unused_imports)]
use flute_webhook::api::{ApiClient, models::*};
use flute_webhook::auth::token::{Fetcher, TokenStore};
use wiremock::{matchers::{header, method, path}, MockServer, Mock, ResponseTemplate};

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
            "items": [{
                "id":"00000000-0000-0000-0000-00000000000a",
                "webhookEndpointId":"00000000-0000-0000-0000-00000000000b",
                "webhookName":"X","endpointUrl":"https://x",
                "eventId":"00000000-0000-0000-0000-00000000000c",
                "eventType":"transaction.card.captured",
                "attemptNumber":1,"status":"Success","responseStatusCode":200,
                "durationMs":12,"errorMessage":null,
                "createdOn":"2026-04-30T12:00:00Z"
            }],
            "total": 1
        })))
        .mount(&server).await;

    let api = client(server.uri());
    let r = api.list_delivery_logs(500).await.unwrap();
    assert_eq!(r.items.unwrap().len(), 1);
    assert_eq!(r.total, Some(1));
}

/// On HTTP 401, the client must invalidate its cached token, fetch a fresh
/// one, and retry the same request. After the retry it should return success
/// (or whatever the second response is) without surfacing the 401 to callers.
#[tokio::test]
async fn refreshes_token_and_retries_once_on_401() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingFetcher { calls: AtomicUsize }
    #[async_trait::async_trait]
    impl Fetcher for CountingFetcher {
        async fn fetch(&self) -> anyhow::Result<(String, Duration)> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            Ok((format!("token-{n}"), Duration::from_secs(3600)))
        }
    }

    let server = MockServer::start().await;

    // First call to GET /v2/webhooks/endpoints returns 401.
    Mock::given(method("GET")).and(path("/v2/webhooks/endpoints"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&server).await;

    // Subsequent calls return 200 with one endpoint. Mounted second so
    // wiremock matches the up_to_n_times(1) entry first.
    Mock::given(method("GET")).and(path("/v2/webhooks/endpoints"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "id":"00000000-0000-0000-0000-000000000099","name":"after-refresh",
                "endpointUrl":"https://x","status":"Active","eventTypes":["ping"],
                "createdOn":"2026-05-04T12:00:00Z","modifiedOn":"2026-05-04T12:00:00Z"
            }]
        })))
        .mount(&server).await;

    let counter = Arc::new(CountingFetcher { calls: AtomicUsize::new(0) });
    let api = ApiClient {
        base_url: server.uri(),
        http: reqwest::Client::new(),
        tokens: TokenStore::new(counter.clone()),
    };

    // The 401 must NOT be surfaced — the retry succeeded.
    let r = api.list_endpoints().await.expect("list_endpoints should succeed via retry");
    let data = r.data.expect("data should be present after retry");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0].name.as_deref(), Some("after-refresh"));

    // Token was fetched twice: once originally, once after the 401 invalidation.
    assert_eq!(counter.calls.load(Ordering::SeqCst), 2,
        "expected exactly 2 token fetches (initial + post-401 refresh)");
}


/// The Flute API rejects bodyless POSTs without `Content-Length: 0`. The ping
/// and retry endpoints both hit this — make sure we always emit the header
/// even when there's no JSON payload.
#[tokio::test]
async fn bodyless_post_sends_content_length_zero() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/webhooks/endpoints/ep-1/ping"))
        .and(header("content-length", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "statusCode": 200,
            "durationMs": 12,
            "errorMessage": null
        })))
        .expect(1)
        .mount(&server).await;

    let api = ApiClient {
        base_url: server.uri(),
        http: reqwest::Client::new(),
        tokens: TokenStore::new(Arc::new(StaticFetcher)),
    };
    api.ping_endpoint("ep-1").await.expect("ping should succeed with Content-Length: 0");
}
