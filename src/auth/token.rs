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
        Self { inner: Arc::new(Mutex::new(None)), fetcher }
    }

    pub async fn bearer(&self) -> anyhow::Result<String> {
        let mut guard = self.inner.lock().await;
        if let Some(cached) = guard.as_ref() {
            // Refresh 60s before actual expiry
            if cached.expires_at.saturating_duration_since(Instant::now()) > Duration::from_secs(60) {
                return Ok(cached.bearer.clone());
            }
        }
        let (bearer, ttl) = self.fetcher.fetch().await?;
        let cached = CachedToken { bearer: bearer.clone(), expires_at: Instant::now() + ttl };
        *guard = Some(cached);
        Ok(bearer)
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
        let fetcher = Arc::new(CountingFetcher { calls: AtomicUsize::new(0), ttl: Duration::from_secs(3600) });
        let store = TokenStore::new(fetcher.clone());
        assert_eq!(store.bearer().await.unwrap(), "token-0");
        assert_eq!(store.bearer().await.unwrap(), "token-0");
        assert_eq!(fetcher.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn refreshes_when_within_60s_of_expiry() {
        let fetcher = Arc::new(CountingFetcher { calls: AtomicUsize::new(0), ttl: Duration::from_secs(30) });
        let store = TokenStore::new(fetcher.clone());
        assert_eq!(store.bearer().await.unwrap(), "token-0");
        // 30s ttl is below the 60s safety margin, so the next call refreshes
        assert_eq!(store.bearer().await.unwrap(), "token-1");
    }
}
