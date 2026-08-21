use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct TokenStore {
    inner: Arc<Mutex<Option<CachedToken>>>,
    fetcher: Arc<dyn Fetcher + Send + Sync>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    bearer: String,
    expires_at: Instant,
}

#[async_trait::async_trait]
pub trait Fetcher {
    async fn fetch(&self) -> anyhow::Result<(String, Duration)>;
}

impl TokenStore {
    pub fn new(fetcher: Arc<dyn Fetcher + Send + Sync>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            fetcher,
        }
    }

    pub async fn bearer(&self) -> anyhow::Result<String> {
        let mut guard = self.inner.lock().await;
        if let Some(cached) = guard.as_ref() {
            // Refresh 60s before actual expiry
            if cached.expires_at.saturating_duration_since(Instant::now()) > Duration::from_secs(60)
            {
                return Ok(cached.bearer.clone());
            }
        }
        let (bearer, ttl) = self.fetcher.fetch().await?;
        let cached = CachedToken {
            bearer: bearer.clone(),
            expires_at: Instant::now() + ttl,
        };
        *guard = Some(cached);
        Ok(bearer)
    }

    /// Discard the cached token. The next [`bearer`](Self::bearer) call will
    /// fetch fresh credentials. Used for reactive refresh when the server
    /// returns 401 (clock skew, revocation, server restart) so a single stale
    /// cache entry doesn't keep failing requests.
    pub async fn invalidate(&self) {
        let mut guard = self.inner.lock().await;
        *guard = None;
    }
}

pub struct OAuth2Fetcher {
    pub oauth_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub http: reqwest::Client,
}

#[derive(serde::Deserialize)]
struct TokenResp {
    access_token: String,
    expires_in: u64,
}

/// RFC 6749 §5.2 error response envelope. The server MAY include
/// `error_description` and `error_uri`; only `error` is required. We fold all
/// three into the surfaced message so `auth token` (and any 401-retry path)
/// tells the user *why* the credentials were rejected instead of an opaque
/// "HTTP 401".
#[derive(serde::Deserialize)]
struct OAuthErrorResp {
    error: String,
    error_description: Option<String>,
    error_uri: Option<String>,
}

/// Cap the plain-text fallback so a misconfigured OAuth server that returns
/// an HTML error page (or a multi-MB body) can't blow up log lines / the
/// terminal. The failure message is a diagnostic aid, not a full transcript.
const OAUTH_ERROR_BODY_TRUNCATE: usize = 512;

#[async_trait::async_trait]
impl Fetcher for OAuth2Fetcher {
    async fn fetch(&self) -> anyhow::Result<(String, Duration)> {
        let resp = self
            .http
            .post(&self.oauth_url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
            ])
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "OAuth token request to {} failed: {}",
                self.oauth_url,
                format_oauth_error(status, &body)
            ));
        }
        let parsed: TokenResp = serde_json::from_str(&body).map_err(|e| {
            anyhow::anyhow!(
                "OAuth token response was not the expected shape ({e}); body was: {}",
                truncate_for_log(&body)
            )
        })?;
        Ok((parsed.access_token, Duration::from_secs(parsed.expires_in)))
    }
}

/// Build a diagnostic message from a non-2xx OAuth response. Prefers the
/// RFC 6749 structured error DTO when the server returns one; falls back to a
/// bounded body snippet for HTML / plain-text error pages.
fn format_oauth_error(status: reqwest::StatusCode, body: &str) -> String {
    if let Ok(err) = serde_json::from_str::<OAuthErrorResp>(body) {
        let mut msg = format!("HTTP {} {}", status.as_u16(), err.error);
        if let Some(desc) = err.error_description.as_deref() {
            let desc = desc.trim();
            if !desc.is_empty() {
                msg.push_str(" — ");
                msg.push_str(desc);
            }
        }
        if let Some(uri) = err.error_uri.as_deref() {
            let uri = uri.trim();
            if !uri.is_empty() {
                msg.push_str(" (see ");
                msg.push_str(uri);
                msg.push(')');
            }
        }
        msg
    } else {
        let snippet = truncate_for_log(body);
        if snippet.is_empty() {
            format!("HTTP {}", status.as_u16())
        } else {
            format!("HTTP {}: {snippet}", status.as_u16())
        }
    }
}

fn truncate_for_log(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= OAUTH_ERROR_BODY_TRUNCATE {
        trimmed.to_string()
    } else {
        let mut out = String::with_capacity(OAUTH_ERROR_BODY_TRUNCATE + 1);
        // Slice on a char boundary so a truncated UTF-8 body doesn't panic.
        let mut end = OAUTH_ERROR_BODY_TRUNCATE;
        while !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        out.push_str(&trimmed[..end]);
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingFetcher {
        calls: AtomicUsize,
        ttl: Duration,
    }

    #[async_trait::async_trait]
    impl Fetcher for CountingFetcher {
        async fn fetch(&self) -> anyhow::Result<(String, Duration)> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            Ok((format!("token-{n}"), self.ttl))
        }
    }

    #[tokio::test]
    async fn caches_token_within_validity() {
        let fetcher = Arc::new(CountingFetcher {
            calls: AtomicUsize::new(0),
            ttl: Duration::from_secs(3600),
        });
        let store = TokenStore::new(fetcher.clone());
        assert_eq!(store.bearer().await.unwrap(), "token-0");
        assert_eq!(store.bearer().await.unwrap(), "token-0");
        assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn invalidate_forces_a_refetch_on_next_bearer_call() {
        let fetcher = Arc::new(CountingFetcher {
            calls: AtomicUsize::new(0),
            ttl: Duration::from_secs(3600),
        });
        let store = TokenStore::new(fetcher.clone());
        assert_eq!(store.bearer().await.unwrap(), "token-0");
        // Cache is still valid by TTL — without invalidate this would reuse token-0.
        store.invalidate().await;
        assert_eq!(store.bearer().await.unwrap(), "token-1");
        assert_eq!(fetcher.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn refreshes_when_within_60s_of_expiry() {
        let fetcher = Arc::new(CountingFetcher {
            calls: AtomicUsize::new(0),
            ttl: Duration::from_secs(30),
        });
        let store = TokenStore::new(fetcher.clone());
        assert_eq!(store.bearer().await.unwrap(), "token-0");
        // 30s ttl is below the 60s safety margin, so the next call refreshes
        assert_eq!(store.bearer().await.unwrap(), "token-1");
    }

    #[tokio::test]
    async fn oauth2_fetcher_parses_token_response() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "abc.def.ghi",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&server)
            .await;

        let fetcher = OAuth2Fetcher {
            oauth_url: format!("{}/oauth2/token", server.uri()),
            client_id: "id".into(),
            client_secret: "secret".into(),
            http: reqwest::Client::new(),
        };
        let (bearer, ttl) = fetcher.fetch().await.unwrap();
        assert_eq!(bearer, "abc.def.ghi");
        assert_eq!(ttl, Duration::from_secs(3600));
    }

    #[tokio::test]
    async fn oauth2_fetcher_surfaces_rfc6749_error_description_on_401() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": "invalid_client",
                "error_description": "Client authentication failed",
                "error_uri": "https://tools.ietf.org/html/rfc6749#section-5.2"
            })))
            .mount(&server)
            .await;

        let fetcher = OAuth2Fetcher {
            oauth_url: format!("{}/oauth2/token", server.uri()),
            client_id: "id".into(),
            client_secret: "wrong".into(),
            http: reqwest::Client::new(),
        };
        let err = fetcher.fetch().await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("HTTP 401"), "must surface status code: {msg}");
        assert!(
            msg.contains("invalid_client"),
            "must surface `error`: {msg}"
        );
        assert!(
            msg.contains("Client authentication failed"),
            "must surface `error_description`: {msg}"
        );
        assert!(msg.contains("rfc6749"), "must surface `error_uri`: {msg}");
    }

    #[tokio::test]
    async fn oauth2_fetcher_falls_back_to_body_snippet_on_non_json_error() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(502).set_body_string("<html>bad gateway</html>"))
            .mount(&server)
            .await;

        let fetcher = OAuth2Fetcher {
            oauth_url: format!("{}/oauth2/token", server.uri()),
            client_id: "id".into(),
            client_secret: "secret".into(),
            http: reqwest::Client::new(),
        };
        let err = fetcher.fetch().await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("HTTP 502"), "must surface status code: {msg}");
        assert!(
            msg.contains("bad gateway"),
            "must include a bounded body snippet: {msg}"
        );
    }

    #[test]
    fn truncate_for_log_respects_utf8_char_boundaries() {
        // Build a body that would slice mid-char at the truncation boundary.
        let mut s = "a".repeat(OAUTH_ERROR_BODY_TRUNCATE - 1);
        s.push('é'); // 2 bytes; naive slice at boundary would panic.
        s.push_str(&"b".repeat(10));
        let out = truncate_for_log(&s);
        assert!(
            out.ends_with('…'),
            "truncated output should end with ellipsis: {out}"
        );
        assert!(out.len() <= OAUTH_ERROR_BODY_TRUNCATE + 4);
    }
}
