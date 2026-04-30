# Flute Webhooks TUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust + Ratatui terminal application that lets developers create, edit, delete, and inspect Flute webhook endpoints, and watch live delivery logs polled from the Flute UAT/production REST API.

**Architecture:** A single binary `flute-webhook` with a `tui` subcommand (default) that launches a Ratatui interface backed by an async tokio runtime. A background poller task fetches `/v2/webhooks/endpoints` and `/v2/webhooks/delivery-logs`, derives per-endpoint trigger counts, and pushes snapshots into the TUI via an mpsc channel. Polling cadence is adaptive: a configurable 5–60 s default (validated, falls back to 5 s on out-of-range), backed off to 20 s while a create/edit form is open. Auth is OAuth2 client-credentials against `oauth.{uat,}arise.risewithaurora.com`, token cached in memory, client secret stored in the OS keychain (with env-var fallback for CI).

**Tech Stack:** Rust 2024 edition, ratatui 0.29 + crossterm 0.28, tokio 1 (multi-thread runtime), reqwest 0.12 (rustls-tls + json), serde + serde_json, chrono 0.4 (serde), clap 4 (derive), keyring 3, toml 0.8, dirs 5, thiserror 2, anyhow 1, tracing 0.1. Dev-deps: wiremock 0.6, tempfile 3, pretty_assertions 1.

**Out of scope (future plans):** `flute-webhook listen`, `webhooks trigger`, `webhooks deliveries` CLI subcommands, `version`/`update`/self-update, Homebrew/installer distribution, golden-file CLI tests, table/json/quiet output flags. This plan delivers the TUI experience and the minimum auth/config/API layer that makes it work end-to-end against UAT.

---

## File Structure

```
Cargo.toml
src/
├── main.rs                    # Entry point: clap parser, launches `tui` (default) or `auth login` stub
├── config.rs                  # config.toml + profile loader, polling-interval validator
├── auth/
│   ├── mod.rs                 # OAuth2 token cache + auto-refresh (TokenStore)
│   └── keychain.rs            # OS keychain get/set/delete via `keyring` crate
├── api/
│   ├── mod.rs                 # ApiClient (reqwest) + endpoint methods
│   ├── models.rs              # serde DTOs matching swagger schemas
│   └── error.rs               # ApiError + correlation-id propagation
├── domain.rs                  # TUI-facing types (Endpoint, DeliveryLog, EventTypeMeta) + counts
├── poller.rs                  # Tokio background task, adaptive cadence, mpsc channel
└── tui/
    ├── mod.rs                 # run() — terminal setup, event loop, channel select
    ├── app.rs                 # App state, FormState, ModalState, key dispatch
    ├── ui.rs                  # render() — tab bar, endpoints table, logs table, help bar, toast
    └── modals.rs              # render_create_modal / edit / delete / created / details (with scroll)

tests/
├── api_client.rs              # wiremock integration tests for ApiClient
└── tui_render.rs              # ratatui TestBackend snapshot-style tests for key screens

docs/superpowers/plans/2026-04-30-flute-webhooks-tui.md  # this file
```

Each file owns exactly one responsibility. `tui/app.rs` is the only file that mutates app state; `tui/ui.rs` and `tui/modals.rs` are pure render functions. The poller never touches the App struct directly — it only sends snapshots over a channel.

---

## Reference Materials

The engineer should keep these open while implementing:

- **Reference TUI (visual + UX template):** `/Users/chad.lung/Rust-Projects/webhook-tui/src/` — `app.rs`, `data.rs`, `ui/mod.rs`, `ui/modals.rs`. Borrow layout, colors, key bindings; replace mock data with live API.
- **Spec:** `/Users/chad.lung/Documents/Webhook-CLI/Flute CLI Spec.md`
- **OpenAPI:** `/Users/chad.lung/Documents/Webhook-CLI/swagger.json` — webhook paths are `/v2/webhooks/{endpoints,event-types,delivery-logs}` plus `…/{id}/ping` and `…/{id}/retry`.

**API base URLs:**
- UAT: `https://api.uat.arise.risewithaurora.com`, OAuth `https://oauth.uat.arise.risewithaurora.com/oauth2/token`
- Production: `https://api.arise.risewithaurora.com`, OAuth `https://oauth.arise.risewithaurora.com/oauth2/token`

**Status enums (from swagger):**
- `WebhookEndpointStatus`: `Active | Inactive`
- `WebhookDeliveryLogStatus`: `Success | Failure` *(only two states — the reference project's `Pending` is not in the API)*

---

## Task 1: Bootstrap Cargo project

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs`
- Create: `src/lib.rs`

- [ ] **Step 1: Replace `Cargo.toml` contents**

```toml
[package]
name = "flute-webhooks"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "flute-webhook"
path = "src/main.rs"

[lib]
name = "flute_webhook"
path = "src/lib.rs"

[dependencies]
ratatui = "0.29"
crossterm = "0.28"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync", "time", "signal"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
clap = { version = "4", features = ["derive", "env"] }
keyring = "3"
toml = "0.8"
dirs = "5"
thiserror = "2"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
url = "2"

[dev-dependencies]
wiremock = "0.6"
tempfile = "3"
pretty_assertions = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync", "time", "test-util"] }
```

- [ ] **Step 2: Replace `src/main.rs`**

```rust
fn main() -> anyhow::Result<()> {
    flute_webhook::run()
}
```

- [ ] **Step 3: Create `src/lib.rs`**

```rust
pub fn run() -> anyhow::Result<()> {
    Ok(())
}
```

- [ ] **Step 4: Verify build**

Run: `cargo build`
Expected: compiles cleanly, produces `target/debug/flute-webhook`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/
git commit -m "chore: bootstrap flute-webhook crate scaffold"
```

---

## Task 2: Config loader + polling-interval validation

**Files:**
- Create: `src/config.rs`
- Modify: `src/lib.rs` (add `pub mod config;`)
- Test: inline `#[cfg(test)] mod tests` in `src/config.rs`

- [ ] **Step 1: Add module declaration**

In `src/lib.rs` add: `pub mod config;`

- [ ] **Step 2: Write failing tests in `src/config.rs`**

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Config {
    pub default_profile: String,
    pub poll_interval_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_profile: "uat".into(),
            poll_interval_seconds: 5,
        }
    }
}

pub const POLL_MIN: u64 = 5;
pub const POLL_MAX: u64 = 60;
pub const POLL_DEFAULT: u64 = 5;
pub const POLL_BACKOFF_SECS: u64 = 20;

#[derive(Debug, Clone)]
pub struct ValidatedPoll {
    pub seconds: u64,
    pub warning: Option<String>,
}

pub fn validate_poll_interval(raw: u64) -> ValidatedPoll {
    if (POLL_MIN..=POLL_MAX).contains(&raw) {
        ValidatedPoll { seconds: raw, warning: None }
    } else {
        ValidatedPoll {
            seconds: POLL_DEFAULT,
            warning: Some(format!(
                "poll_interval_seconds={raw} is outside {POLL_MIN}-{POLL_MAX}; defaulting to {POLL_DEFAULT}"
            )),
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".flute")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn load_or_default() -> Config {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_in_range_accepted() {
        let v = validate_poll_interval(10);
        assert_eq!(v.seconds, 10);
        assert!(v.warning.is_none());
    }

    #[test]
    fn poll_below_min_warns_and_defaults() {
        let v = validate_poll_interval(1);
        assert_eq!(v.seconds, POLL_DEFAULT);
        assert!(v.warning.unwrap().contains("outside"));
    }

    #[test]
    fn poll_above_max_warns_and_defaults() {
        let v = validate_poll_interval(120);
        assert_eq!(v.seconds, POLL_DEFAULT);
        assert!(v.warning.is_some());
    }

    #[test]
    fn boundary_min_accepted() {
        assert_eq!(validate_poll_interval(POLL_MIN).seconds, POLL_MIN);
    }

    #[test]
    fn boundary_max_accepted() {
        assert_eq!(validate_poll_interval(POLL_MAX).seconds, POLL_MAX);
    }

    #[test]
    fn config_default_uses_uat_profile_and_5s() {
        let c = Config::default();
        assert_eq!(c.default_profile, "uat");
        assert_eq!(c.poll_interval_seconds, 5);
    }
}
```

- [ ] **Step 3: Run tests to confirm they pass**

Run: `cargo test --lib config::`
Expected: all 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/lib.rs src/config.rs
git commit -m "feat(config): add Config + polling interval validator"
```

---

## Task 3: Profile loader (UAT/production base URLs)

**Files:**
- Create: `src/config.rs` (extend)
- Test: same file

- [ ] **Step 1: Append to `src/config.rs`**

```rust
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub api_base_url: String,
    pub oauth_url: String,
}

impl Profile {
    pub fn uat() -> Self {
        Self {
            name: "uat".into(),
            api_base_url: "https://api.uat.arise.risewithaurora.com".into(),
            oauth_url: "https://oauth.uat.arise.risewithaurora.com/oauth2/token".into(),
        }
    }

    pub fn production() -> Self {
        Self {
            name: "production".into(),
            api_base_url: "https://api.arise.risewithaurora.com".into(),
            oauth_url: "https://oauth.arise.risewithaurora.com/oauth2/token".into(),
        }
    }

    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "uat" => Some(Self::uat()),
            "production" | "prod" => Some(Self::production()),
            _ => None,
        }
    }
}
```

- [ ] **Step 2: Append tests**

```rust
#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn uat_profile_has_uat_hosts() {
        let p = Profile::uat();
        assert!(p.api_base_url.contains("api.uat.arise"));
        assert!(p.oauth_url.contains("oauth.uat.arise"));
    }

    #[test]
    fn production_profile_has_prod_hosts() {
        let p = Profile::production();
        assert!(!p.api_base_url.contains(".uat."));
        assert!(!p.oauth_url.contains(".uat."));
    }

    #[test]
    fn by_name_resolves_known_profiles() {
        assert_eq!(Profile::by_name("uat").unwrap().name, "uat");
        assert_eq!(Profile::by_name("production").unwrap().name, "production");
        assert_eq!(Profile::by_name("prod").unwrap().name, "production");
        assert!(Profile::by_name("garbage").is_none());
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib config::profile_tests`
Expected: all 3 pass.

- [ ] **Step 4: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): add Profile with UAT and production base URLs"
```

---

## Task 4: Keychain wrapper

**Files:**
- Create: `src/auth/mod.rs`
- Create: `src/auth/keychain.rs`
- Modify: `src/lib.rs` (add `pub mod auth;`)

- [ ] **Step 1: Add module declaration**

In `src/lib.rs` add: `pub mod auth;`

- [ ] **Step 2: Create `src/auth/mod.rs`**

```rust
pub mod keychain;
```

- [ ] **Step 3: Create `src/auth/keychain.rs`**

```rust
use anyhow::{Context, Result};
use keyring::Entry;

const SERVICE: &str = "flute-webhook";

fn entry(profile: &str, kind: &str) -> Result<Entry> {
    Entry::new(SERVICE, &format!("{profile}:{kind}"))
        .with_context(|| format!("creating keyring entry {profile}:{kind}"))
}

pub fn store_client_credentials(profile: &str, client_id: &str, client_secret: &str) -> Result<()> {
    entry(profile, "client_id")?.set_password(client_id)?;
    entry(profile, "client_secret")?.set_password(client_secret)?;
    Ok(())
}

pub fn load_client_credentials(profile: &str) -> Result<Option<(String, String)>> {
    let id_entry = entry(profile, "client_id")?;
    let secret_entry = entry(profile, "client_secret")?;
    match (id_entry.get_password(), secret_entry.get_password()) {
        (Ok(id), Ok(secret)) => Ok(Some((id, secret))),
        (Err(keyring::Error::NoEntry), _) | (_, Err(keyring::Error::NoEntry)) => Ok(None),
        (Err(e), _) | (_, Err(e)) => Err(e.into()),
    }
}

pub fn delete_client_credentials(profile: &str) -> Result<()> {
    let _ = entry(profile, "client_id")?.delete_credential();
    let _ = entry(profile, "client_secret")?.delete_credential();
    Ok(())
}

pub fn load_with_env_fallback(profile: &str) -> Result<Option<(String, String)>> {
    if let (Ok(id), Ok(secret)) = (std::env::var("FLUTE_CLIENT_ID"), std::env::var("FLUTE_CLIENT_SECRET")) {
        return Ok(Some((id, secret)));
    }
    load_client_credentials(profile)
}
```

- [ ] **Step 4: Verify build (no test — keychain access is not unit-testable in CI)**

Run: `cargo build --lib`
Expected: compiles cleanly.

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/auth/
git commit -m "feat(auth): add keychain wrapper with env-var fallback"
```

---

## Task 5: OAuth2 token cache + auto-refresh

**Files:**
- Create: `src/auth/token.rs`
- Modify: `src/auth/mod.rs`

- [ ] **Step 1: Append to `src/auth/mod.rs`**

```rust
pub mod token;
```

- [ ] **Step 2: Write failing tests in `src/auth/token.rs`**

```rust
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
```

- [ ] **Step 3: Add `async-trait` to deps and run tests**

Add to `[dependencies]` in `Cargo.toml`: `async-trait = "0.1"`

Run: `cargo test --lib auth::token`
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/auth/
git commit -m "feat(auth): add token cache with 60s refresh window"
```

---

## Task 6: OAuth2 fetcher implementation

**Files:**
- Modify: `src/auth/token.rs`

- [ ] **Step 1: Append to `src/auth/token.rs`**

```rust
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

#[async_trait::async_trait]
impl Fetcher for OAuth2Fetcher {
    async fn fetch(&self) -> anyhow::Result<(String, Duration)> {
        let resp: TokenResp = self.http
            .post(&self.oauth_url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
            ])
            .send().await?
            .error_for_status()?
            .json().await?;
        Ok((resp.access_token, Duration::from_secs(resp.expires_in)))
    }
}
```

- [ ] **Step 2: Add wiremock-based test**

Append to the `tests` module in `src/auth/token.rs`:

```rust
    #[tokio::test]
    async fn oauth2_fetcher_parses_token_response() {
        use wiremock::{matchers::method, MockServer, Mock, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "abc.def.ghi",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&server).await;

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
```

- [ ] **Step 3: Run test**

Run: `cargo test --lib auth::token::tests::oauth2_fetcher_parses_token_response`
Expected: passes.

- [ ] **Step 4: Commit**

```bash
git add src/auth/token.rs
git commit -m "feat(auth): add OAuth2 client-credentials fetcher"
```

---

## Task 7: API error type + correlation ID

**Files:**
- Create: `src/api/mod.rs`
- Create: `src/api/error.rs`
- Modify: `src/lib.rs` (add `pub mod api;`)

- [ ] **Step 1: Add `pub mod api;` to `src/lib.rs`**

- [ ] **Step 2: Create `src/api/mod.rs`**

```rust
pub mod error;
pub mod models;

pub use error::ApiError;
```

- [ ] **Step 3: Write failing tests in `src/api/error.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    #[error("API {status} (correlation_id={correlation_id:?}): {message}")]
    Api { status: u16, correlation_id: Option<String>, message: String },

    #[error("auth error: {0}")]
    Auth(String),

    #[error("invalid response: {0}")]
    Decode(String),
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct AspNetError {
    pub details: Option<String>,
    pub title: Option<String>,
    #[serde(rename = "correlationId")]
    pub correlation_id: Option<String>,
    #[serde(rename = "errorCode")]
    pub error_code: Option<String>,
}

pub fn from_aspnet(status: u16, body: &str) -> ApiError {
    match serde_json::from_str::<AspNetError>(body) {
        Ok(e) => ApiError::Api {
            status,
            correlation_id: e.correlation_id,
            message: e.title.or(e.details).unwrap_or_else(|| body.to_string()),
        },
        Err(_) => ApiError::Api { status, correlation_id: None, message: body.to_string() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_correlation_id_from_aspnet_body() {
        let body = r#"{"details":"X","title":"Validation failed","statusCode":400,"correlationId":"abc-123","errorCode":"V0000"}"#;
        match from_aspnet(400, body) {
            ApiError::Api { status, correlation_id, message } => {
                assert_eq!(status, 400);
                assert_eq!(correlation_id.as_deref(), Some("abc-123"));
                assert_eq!(message, "Validation failed");
            }
            _ => panic!("expected Api"),
        }
    }

    #[test]
    fn falls_back_when_body_is_not_aspnet() {
        match from_aspnet(500, "oops") {
            ApiError::Api { status, correlation_id, message } => {
                assert_eq!(status, 500);
                assert!(correlation_id.is_none());
                assert_eq!(message, "oops");
            }
            _ => panic!(),
        }
    }
}
```

- [ ] **Step 4: Create empty `src/api/models.rs`**

```rust
// Populated in subsequent tasks.
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib api::error`
Expected: 2 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/api/
git commit -m "feat(api): add ApiError with AspNet correlation-id extraction"
```

---

## Task 8: API DTOs for endpoints + event types

**Files:**
- Modify: `src/api/models.rs`

- [ ] **Step 1: Replace `src/api/models.rs`**

```rust
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
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib api::models`
Expected: 2 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/api/models.rs
git commit -m "feat(api): add DTOs for webhook endpoints and event types"
```

---

## Task 9: API DTOs for delivery logs + retry + ping

**Files:**
- Modify: `src/api/models.rs`

- [ ] **Step 1: Append to `src/api/models.rs`**

```rust
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginationDto {
    pub has_more: bool,
    pub cursor: Option<String>,
    pub total_count: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDeliveryLogsDto {
    pub data: Option<Vec<DeliveryLogSummaryDto>>,
    pub pagination: Option<PaginationDto>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResponseDto {
    pub success: bool,
    pub status_code: Option<i32>,
    pub duration_ms: i32,
    pub error_message: Option<String>,
}
```

- [ ] **Step 2: Add tests**

Append to the existing `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn deserializes_delivery_log_summary() {
        let json = r#"{"id":"00000000-0000-0000-0000-0000000000aa","webhookEndpointId":"00000000-0000-0000-0000-0000000000bb","webhookName":"X","endpointUrl":"https://x","eventId":"00000000-0000-0000-0000-0000000000cc","eventType":"transaction.card.captured","attemptNumber":1,"status":"Success","responseStatusCode":200,"durationMs":120,"errorMessage":null,"createdOn":"2026-04-30T12:00:00Z"}"#;
        let v: DeliveryLogSummaryDto = serde_json::from_str(json).unwrap();
        assert_eq!(v.status, WebhookDeliveryLogStatus::Success);
        assert_eq!(v.response_status_code, Some(200));
    }

    #[test]
    fn deserializes_pagination() {
        let json = r#"{"hasMore":true,"cursor":"abc","totalCount":42}"#;
        let v: PaginationDto = serde_json::from_str(json).unwrap();
        assert!(v.has_more);
        assert_eq!(v.cursor.as_deref(), Some("abc"));
        assert_eq!(v.total_count, Some(42));
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib api::models`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/api/models.rs
git commit -m "feat(api): add DTOs for delivery logs, pagination, and ping"
```

---

## Task 10: ApiClient — endpoints CRUD + ping + event types

**Files:**
- Create: `src/api/client.rs`
- Modify: `src/api/mod.rs`
- Test: `tests/api_client.rs`

- [ ] **Step 1: Append to `src/api/mod.rs`**

```rust
pub mod client;
pub use client::ApiClient;
```

- [ ] **Step 2: Create `src/api/client.rs`**

```rust
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
    async fn send<R: serde::de::DeserializeOwned>(&self, method: Method, path: &str, body: Option<serde_json::Value>) -> Result<R, ApiError> {
        let token = self.tokens.bearer().await.map_err(|e| ApiError::Auth(e.to_string()))?;
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.http.request(method, &url).bearer_auth(token);
        if let Some(b) = body { req = req.json(&b); }
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
        if status.is_success() { Ok(()) } else {
            let text = resp.text().await.unwrap_or_default();
            Err(from_aspnet(status.as_u16(), &text))
        }
    }

    pub async fn list_endpoints(&self) -> Result<ListWebhookEndpointsDto, ApiError> {
        self.send(Method::GET, "/v2/webhooks/endpoints", None).await
    }

    pub async fn create_endpoint(&self, req: &CreateWebhookEndpointRequest) -> Result<CreateWebhookEndpointResponse, ApiError> {
        self.send(Method::POST, "/v2/webhooks/endpoints", Some(serde_json::to_value(req).unwrap())).await
    }

    pub async fn update_endpoint(&self, id: &str, req: &UpdateWebhookEndpointRequest) -> Result<GetWebhookEndpointDto, ApiError> {
        self.send(Method::PUT, &format!("/v2/webhooks/endpoints/{id}"), Some(serde_json::to_value(req).unwrap())).await
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
```

- [ ] **Step 3: Write integration test in `tests/api_client.rs`**

```rust
use std::sync::Arc;
use std::time::Duration;
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
```

Add `async-trait` to `[dev-dependencies]` in `Cargo.toml`: `async-trait = "0.1"`

- [ ] **Step 4: Make `auth::token::Fetcher` and `auth::token` types `pub`**

Already public in Task 5; verify `#[async_trait::async_trait] pub trait Fetcher` and `pub struct TokenStore` carry `pub` visibility.

- [ ] **Step 5: Run tests**

Run: `cargo test --test api_client`
Expected: 2 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/api/ tests/api_client.rs Cargo.toml
git commit -m "feat(api): add ApiClient for endpoints, ping, and event types"
```

---

## Task 11: ApiClient — delivery logs list + detail + retry

**Files:**
- Modify: `src/api/client.rs`
- Modify: `tests/api_client.rs`

- [ ] **Step 1: Append to `impl ApiClient` in `src/api/client.rs`**

```rust
    pub async fn list_delivery_logs(&self, limit: u32) -> Result<ListDeliveryLogsDto, ApiError> {
        self.send(Method::GET, &format!("/v2/webhooks/delivery-logs?limit={limit}"), None).await
    }

    pub async fn get_delivery_log(&self, id: &str) -> Result<DeliveryLogDetailDto, ApiError> {
        self.send(Method::GET, &format!("/v2/webhooks/delivery-logs/{id}"), None).await
    }

    pub async fn retry_delivery(&self, id: &str) -> Result<serde_json::Value, ApiError> {
        self.send(Method::POST, &format!("/v2/webhooks/delivery-logs/{id}/retry"), None).await
    }
```

- [ ] **Step 2: Add test to `tests/api_client.rs`**

```rust
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
```

- [ ] **Step 3: Run tests**

Run: `cargo test --test api_client`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/api/client.rs tests/api_client.rs
git commit -m "feat(api): add delivery-log list, detail, and retry methods"
```

---

## Task 12: Domain types + DTO conversions + event-count aggregation

**Files:**
- Create: `src/domain.rs`
- Modify: `src/lib.rs` (add `pub mod domain;`)

- [ ] **Step 1: Add `pub mod domain;` to `src/lib.rs`**

- [ ] **Step 2: Create `src/domain.rs`**

```rust
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
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib domain`
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/lib.rs src/domain.rs
git commit -m "feat(domain): add Endpoint/DeliveryLog/EventType + count aggregation"
```

---

## Task 13: Poller — adaptive cadence state machine

**Files:**
- Create: `src/poller.rs`
- Modify: `src/lib.rs` (add `pub mod poller;`)

- [ ] **Step 1: Add `pub mod poller;` to `src/lib.rs`**

- [ ] **Step 2: Write failing tests in `src/poller.rs`**

```rust
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::api::ApiClient;
use crate::config::POLL_BACKOFF_SECS;
use crate::domain::{aggregate_counts, DeliveryLog, Endpoint, EventTypeMeta};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CadenceMode {
    /// Use the user-configured interval (5–60 s).
    Active,
    /// Force 20 s while a form modal is open.
    Backoff,
}

pub fn current_interval(mode: CadenceMode, configured_secs: u64) -> Duration {
    match mode {
        CadenceMode::Active => Duration::from_secs(configured_secs),
        CadenceMode::Backoff => Duration::from_secs(POLL_BACKOFF_SECS.max(configured_secs)),
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub endpoints: Vec<Endpoint>,
    pub logs: Vec<DeliveryLog>,
    pub event_types: Vec<EventTypeMeta>,
    pub fetched_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug)]
pub enum PollerEvent {
    Snapshot(Snapshot),
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::POLL_BACKOFF_SECS;

    #[test]
    fn active_mode_uses_configured_interval() {
        assert_eq!(current_interval(CadenceMode::Active, 5), Duration::from_secs(5));
        assert_eq!(current_interval(CadenceMode::Active, 60), Duration::from_secs(60));
    }

    #[test]
    fn backoff_mode_is_at_least_20s() {
        assert_eq!(current_interval(CadenceMode::Backoff, 5), Duration::from_secs(POLL_BACKOFF_SECS));
        assert_eq!(current_interval(CadenceMode::Backoff, 10), Duration::from_secs(POLL_BACKOFF_SECS));
    }

    #[test]
    fn backoff_does_not_speed_up_a_slower_configured_interval() {
        // If user configured 30 s, opening a form should not speed polling to 20 s.
        assert_eq!(current_interval(CadenceMode::Backoff, 30), Duration::from_secs(30));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib poller`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/lib.rs src/poller.rs
git commit -m "feat(poller): add cadence-mode state machine"
```

---

## Task 14: Poller — background tokio task

**Files:**
- Modify: `src/poller.rs`

- [ ] **Step 1: Append to `src/poller.rs`**

```rust
pub fn spawn(
    api: ApiClient,
    cadence_rx: watch::Receiver<CadenceMode>,
    configured_secs: u64,
    tx: mpsc::Sender<PollerEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // One-time fetch of event types — they don't change often
        let event_types = match api.list_event_types().await {
            Ok(v) => v.data.unwrap_or_default().into_iter().map(EventTypeMeta::from).collect(),
            Err(e) => {
                let _ = tx.send(PollerEvent::Error(format!("event-types: {e}"))).await;
                Vec::new()
            }
        };

        loop {
            let mode = *cadence_rx.borrow();
            let interval = current_interval(mode, configured_secs);

            match poll_once(&api, &event_types).await {
                Ok(snap) => { let _ = tx.send(PollerEvent::Snapshot(snap)).await; }
                Err(e) => { let _ = tx.send(PollerEvent::Error(e)).await; }
            }

            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = wait_for_cadence_change(cadence_rx.clone(), mode) => {
                    info!("cadence changed; re-evaluating");
                }
            }
        }
    })
}

async fn wait_for_cadence_change(mut rx: watch::Receiver<CadenceMode>, current: CadenceMode) {
    loop {
        if rx.changed().await.is_err() { return; }
        if *rx.borrow() != current { return; }
    }
}

async fn poll_once(api: &ApiClient, event_types: &[EventTypeMeta]) -> Result<Snapshot, String> {
    let endpoints_resp = api.list_endpoints().await.map_err(|e| format!("endpoints: {e}"))?;
    let logs_resp = api.list_delivery_logs(500).await.map_err(|e| format!("logs: {e}"))?;

    let mut endpoints: Vec<Endpoint> = endpoints_resp.data.unwrap_or_default()
        .into_iter().map(Endpoint::from).collect();
    let logs: Vec<DeliveryLog> = logs_resp.data.unwrap_or_default()
        .into_iter().map(DeliveryLog::from).collect();
    let has_more = logs_resp.pagination.map(|p| p.has_more).unwrap_or(false);

    aggregate_counts(&mut endpoints, &logs, has_more);

    Ok(Snapshot {
        endpoints, logs,
        event_types: event_types.to_vec(),
        fetched_at: chrono::Utc::now(),
    })
}
```

- [ ] **Step 2: Verify build**

Run: `cargo build --lib`
Expected: compiles cleanly. (No new unit tests — `spawn` is exercised end-to-end at app startup.)

- [ ] **Step 3: Commit**

```bash
git add src/poller.rs
git commit -m "feat(poller): spawn background fetch loop with cadence-aware sleep"
```

---

## Task 15: Clap CLI parser + entry point

**Files:**
- Modify: `src/lib.rs`
- Create: `src/cli.rs`

- [ ] **Step 1: Create `src/cli.rs`**

```rust
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "flute-webhook", version, about = "Flute Webhooks TUI and helpers")]
pub struct Cli {
    #[arg(long, env = "FLUTE_PROFILE", default_value = "uat")]
    pub profile: String,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Launch the interactive TUI (default if no subcommand is given).
    Tui,

    /// Auth subcommands.
    #[command(subcommand)]
    Auth(AuthCommand),
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    /// Prompt for client_id + client_secret and store them in the OS keychain.
    Login,

    /// Print the current bearer token (debugging aid).
    Token,
}
```

- [ ] **Step 2: Replace `src/lib.rs`**

```rust
pub mod api;
pub mod auth;
pub mod cli;
pub mod config;
pub mod domain;
pub mod poller;
pub mod tui;

use clap::Parser;

pub fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "warn,flute_webhook=info".into()))
        .with_writer(std::io::stderr)
        .init();

    let cli = cli::Cli::parse();
    let cmd = cli.command.unwrap_or(cli::Command::Tui);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        match cmd {
            cli::Command::Tui => tui::run(&cli.profile).await,
            cli::Command::Auth(cli::AuthCommand::Login) => auth_login(&cli.profile).await,
            cli::Command::Auth(cli::AuthCommand::Token) => auth_print_token(&cli.profile).await,
        }
    })
}

async fn auth_login(profile: &str) -> anyhow::Result<()> {
    use std::io::{self, BufRead, Write};
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    print!("client_id for [{profile}]: ");
    stdout.flush()?;
    let mut id = String::new();
    stdin.lock().read_line(&mut id)?;
    let id = id.trim().to_string();

    print!("client_secret for [{profile}]: ");
    stdout.flush()?;
    let mut secret = String::new();
    stdin.lock().read_line(&mut secret)?;
    let secret = secret.trim().to_string();

    auth::keychain::store_client_credentials(profile, &id, &secret)?;
    println!("Stored credentials for profile [{profile}] in OS keychain.");
    Ok(())
}

async fn auth_print_token(profile: &str) -> anyhow::Result<()> {
    let p = config::Profile::by_name(profile)
        .ok_or_else(|| anyhow::anyhow!("unknown profile: {profile}"))?;
    let (id, secret) = auth::keychain::load_with_env_fallback(profile)?
        .ok_or_else(|| anyhow::anyhow!("no credentials for [{profile}]; run `flute-webhook auth login`"))?;
    let fetcher = std::sync::Arc::new(auth::token::OAuth2Fetcher {
        oauth_url: p.oauth_url,
        client_id: id,
        client_secret: secret,
        http: reqwest::Client::new(),
    });
    let store = auth::token::TokenStore::new(fetcher);
    let bearer = store.bearer().await?;
    println!("{bearer}");
    Ok(())
}
```

- [ ] **Step 3: Stub `tui::run` so the crate builds**

Create `src/tui/mod.rs`:

```rust
pub mod app;
pub mod modals;
pub mod ui;

pub async fn run(_profile: &str) -> anyhow::Result<()> {
    Ok(())
}
```

Create empty `src/tui/app.rs`, `src/tui/ui.rs`, `src/tui/modals.rs` (each containing `// populated in later tasks`).

- [ ] **Step 4: Verify build**

Run: `cargo build`
Expected: compiles cleanly. `cargo run -- --help` lists `tui` and `auth` subcommands.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/lib.rs src/tui/
git commit -m "feat(cli): add clap parser with tui and auth subcommands"
```

---

## Task 16: TUI — App state struct

**Files:**
- Replace: `src/tui/app.rs`

- [ ] **Step 1: Replace `src/tui/app.rs`**

```rust
use crate::api::models::{WebhookDeliveryLogStatus, WebhookEndpointStatus};
use crate::domain::{DeliveryLog, Endpoint, EventTypeMeta};

#[derive(Clone, Debug, PartialEq)]
pub enum Screen { Endpoints, DeliveryLogs }

#[derive(Clone, Debug, PartialEq)]
pub enum ModalState {
    None,
    CreateWebhook,
    EditWebhook(usize),
    DeleteWebhook(usize),
    WebhookCreated(String), // signing secret
    DeliveryDetails(String), // delivery log id (for fetching detail on demand)
}

#[derive(Clone, Debug, PartialEq)]
pub enum FormField {
    Url,
    Name,
    Status,
    CheckAll,
    UncheckAll,
    Event(usize),
    Cancel,
    Submit,
}

#[derive(Clone, Debug)]
pub struct FormState {
    pub url: String,
    pub name: String,
    pub events: Vec<bool>, // length matches App.event_types
    pub status: WebhookEndpointStatus,
    pub active_field: FormField,
    pub is_edit: bool,
    pub scroll: u16, // for the long event list inside the modal
}

impl FormState {
    pub fn new_create(num_events: usize) -> Self {
        Self {
            url: String::new(), name: String::new(),
            events: vec![true; num_events],
            status: WebhookEndpointStatus::Active,
            active_field: FormField::Url,
            is_edit: false, scroll: 0,
        }
    }

    pub fn new_edit(ep: &Endpoint, event_types: &[EventTypeMeta]) -> Self {
        let events: Vec<bool> = event_types.iter()
            .map(|et| ep.event_types.iter().any(|n| n == &et.name))
            .collect();
        Self {
            url: ep.endpoint_url.clone(),
            name: ep.name.clone(),
            events,
            status: ep.status,
            active_field: FormField::Url,
            is_edit: true, scroll: 0,
        }
    }

    pub fn next_field(&mut self, num_events: usize) {
        self.active_field = match &self.active_field {
            FormField::Url => FormField::Name,
            FormField::Name => if self.is_edit { FormField::Status } else { FormField::CheckAll },
            FormField::Status => FormField::CheckAll,
            FormField::CheckAll => FormField::UncheckAll,
            FormField::UncheckAll if num_events > 0 => FormField::Event(0),
            FormField::UncheckAll => FormField::Cancel,
            FormField::Event(i) if *i + 1 < num_events => FormField::Event(*i + 1),
            FormField::Event(_) => FormField::Cancel,
            FormField::Cancel => FormField::Submit,
            FormField::Submit => FormField::Url,
        };
    }

    pub fn prev_field(&mut self, num_events: usize) {
        self.active_field = match &self.active_field {
            FormField::Url => FormField::Submit,
            FormField::Name => FormField::Url,
            FormField::Status => FormField::Name,
            FormField::CheckAll => if self.is_edit { FormField::Status } else { FormField::Name },
            FormField::UncheckAll => FormField::CheckAll,
            FormField::Event(0) => FormField::UncheckAll,
            FormField::Event(i) => FormField::Event(*i - 1),
            FormField::Cancel => if num_events > 0 { FormField::Event(num_events - 1) } else { FormField::UncheckAll },
            FormField::Submit => FormField::Cancel,
        };
    }
}

pub struct App {
    pub running: bool,
    pub screen: Screen,
    pub modal: ModalState,
    pub endpoints: Vec<Endpoint>,
    pub logs: Vec<DeliveryLog>,
    pub event_types: Vec<EventTypeMeta>,
    pub selected_endpoint: usize,
    pub selected_log: usize,
    pub form: FormState,
    pub detail_scroll: u16,
    pub last_error: Option<String>,
    pub poll_warning: Option<String>,

    pub filter_endpoint: usize, // 0=All, 1+=index+1
    pub filter_event: usize,
    pub filter_status: usize,   // 0=All, 1=Success, 2=Failure
    pub sort_ascending: bool,

    pub toast_message: Option<String>,
    pub toast_timer: u8,
}

impl App {
    pub fn new(poll_warning: Option<String>) -> Self {
        Self {
            running: true,
            screen: Screen::Endpoints,
            modal: ModalState::None,
            endpoints: Vec::new(),
            logs: Vec::new(),
            event_types: Vec::new(),
            selected_endpoint: 0,
            selected_log: 0,
            form: FormState::new_create(0),
            detail_scroll: 0,
            last_error: None,
            poll_warning,
            filter_endpoint: 0,
            filter_event: 0,
            filter_status: 0,
            sort_ascending: false,
            toast_message: None,
            toast_timer: 0,
        }
    }

    pub fn show_toast(&mut self, msg: impl Into<String>) {
        self.toast_message = Some(msg.into());
        self.toast_timer = 20;
    }

    pub fn tick_toast(&mut self) {
        if self.toast_timer > 0 {
            self.toast_timer -= 1;
            if self.toast_timer == 0 { self.toast_message = None; }
        }
    }

    pub fn apply_snapshot(&mut self, endpoints: Vec<Endpoint>, logs: Vec<DeliveryLog>, event_types: Vec<EventTypeMeta>) {
        self.endpoints = endpoints;
        self.logs = logs;
        if self.event_types.is_empty() && !event_types.is_empty() {
            self.event_types = event_types;
        }
        if self.selected_endpoint >= self.endpoints.len() && !self.endpoints.is_empty() {
            self.selected_endpoint = self.endpoints.len() - 1;
        }
    }

    pub fn cadence_mode(&self) -> crate::poller::CadenceMode {
        match self.modal {
            ModalState::CreateWebhook | ModalState::EditWebhook(_) => crate::poller::CadenceMode::Backoff,
            _ => crate::poller::CadenceMode::Active,
        }
    }
}
```

- [ ] **Step 2: Verify build**

Run: `cargo build`
Expected: compiles cleanly.

- [ ] **Step 3: Commit**

```bash
git add src/tui/app.rs
git commit -m "feat(tui): add App state with FormState and ModalState"
```

---

## Task 17: TUI — key-handling for main screens (with q-quit at top level)

**Files:**
- Modify: `src/tui/app.rs`

- [ ] **Step 1: Append `impl App` block to `src/tui/app.rs`**

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Debug)]
pub enum AppAction {
    None,
    Create(crate::api::models::CreateWebhookEndpointRequest),
    Update(String, crate::api::models::UpdateWebhookEndpointRequest),
    Delete(String),
    OpenDetails(String),
}

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) -> AppAction {
        if key.kind != KeyEventKind::Press { return AppAction::None; }
        // Ctrl-C always quits
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.running = false;
            return AppAction::None;
        }
        match self.modal.clone() {
            ModalState::None => self.handle_main_key(key),
            ModalState::CreateWebhook | ModalState::EditWebhook(_) => self.handle_form_key(key),
            ModalState::DeleteWebhook(idx) => self.handle_delete_key(key, idx),
            ModalState::WebhookCreated(_) => self.handle_created_key(key),
            ModalState::DeliveryDetails(_) => self.handle_details_key(key),
        }
    }

    fn handle_main_key(&mut self, key: KeyEvent) -> AppAction {
        match key.code {
            KeyCode::Char('q') => { self.running = false; AppAction::None }
            KeyCode::Tab | KeyCode::BackTab => {
                self.screen = match self.screen {
                    Screen::Endpoints => Screen::DeliveryLogs,
                    Screen::DeliveryLogs => Screen::Endpoints,
                };
                AppAction::None
            }
            _ => match self.screen {
                Screen::Endpoints => self.handle_endpoints_key(key),
                Screen::DeliveryLogs => self.handle_logs_key(key),
            }
        }
    }

    fn handle_endpoints_key(&mut self, key: KeyEvent) -> AppAction {
        let n = self.endpoints.len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') if n > 0 && self.selected_endpoint > 0 => {
                self.selected_endpoint -= 1; AppAction::None
            }
            KeyCode::Down | KeyCode::Char('j') if n > 0 && self.selected_endpoint + 1 < n => {
                self.selected_endpoint += 1; AppAction::None
            }
            KeyCode::Char('c') => {
                self.form = FormState::new_create(self.event_types.len());
                self.modal = ModalState::CreateWebhook;
                AppAction::None
            }
            KeyCode::Char('e') | KeyCode::Enter if n > 0 => {
                self.form = FormState::new_edit(&self.endpoints[self.selected_endpoint], &self.event_types);
                self.modal = ModalState::EditWebhook(self.selected_endpoint);
                AppAction::None
            }
            KeyCode::Char('d') if n > 0 => {
                self.modal = ModalState::DeleteWebhook(self.selected_endpoint);
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    fn handle_logs_key(&mut self, key: KeyEvent) -> AppAction {
        let filtered = self.filtered_log_indices();
        let n = filtered.len();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') if n > 0 && self.selected_log > 0 => {
                self.selected_log -= 1; AppAction::None
            }
            KeyCode::Down | KeyCode::Char('j') if n > 0 && self.selected_log + 1 < n => {
                self.selected_log += 1; AppAction::None
            }
            KeyCode::Enter | KeyCode::Char('v') if n > 0 => {
                let id = self.logs[filtered[self.selected_log]].id.clone();
                self.detail_scroll = 0;
                self.modal = ModalState::DeliveryDetails(id.clone());
                AppAction::OpenDetails(id)
            }
            KeyCode::Char('1') => {
                self.filter_endpoint = (self.filter_endpoint + 1) % (self.endpoints.len() + 1);
                self.selected_log = 0; AppAction::None
            }
            KeyCode::Char('2') => {
                self.filter_event = (self.filter_event + 1) % (self.event_types.len() + 1);
                self.selected_log = 0; AppAction::None
            }
            KeyCode::Char('3') => {
                self.filter_status = (self.filter_status + 1) % 3;
                self.selected_log = 0; AppAction::None
            }
            KeyCode::Char('s') => { self.sort_ascending = !self.sort_ascending; AppAction::None }
            KeyCode::Char('x') => {
                self.filter_endpoint = 0; self.filter_event = 0; self.filter_status = 0;
                self.selected_log = 0; AppAction::None
            }
            _ => AppAction::None,
        }
    }

    pub fn filtered_log_indices(&self) -> Vec<usize> {
        let mut out: Vec<usize> = (0..self.logs.len()).filter(|&i| {
            let log = &self.logs[i];
            if self.filter_endpoint > 0 {
                let ep_idx = self.filter_endpoint - 1;
                if ep_idx >= self.endpoints.len() || self.endpoints[ep_idx].id != log.endpoint_id {
                    return false;
                }
            }
            if self.filter_event > 0 {
                let evt_idx = self.filter_event - 1;
                if evt_idx >= self.event_types.len() || self.event_types[evt_idx].name != log.event_type {
                    return false;
                }
            }
            match self.filter_status {
                1 => log.status == WebhookDeliveryLogStatus::Success,
                2 => log.status == WebhookDeliveryLogStatus::Failure,
                _ => true,
            }
        }).collect();
        if self.sort_ascending { out.reverse(); }
        out
    }
}
```

- [ ] **Step 2: Add tests at the bottom of `src/tui/app.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent { code: KeyCode::Char(c), modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: Default::default() }
    }

    #[test]
    fn q_at_top_level_quits() {
        let mut app = App::new(None);
        app.handle_key(key('q'));
        assert!(!app.running);
    }

    #[test]
    fn tab_switches_screens() {
        let mut app = App::new(None);
        let kp = KeyEvent { code: KeyCode::Tab, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: Default::default() };
        app.handle_key(kp);
        assert_eq!(app.screen, Screen::DeliveryLogs);
        app.handle_key(kp);
        assert_eq!(app.screen, Screen::Endpoints);
    }

    #[test]
    fn ctrl_c_always_quits_even_inside_form() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(0);
        let kp = KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, kind: KeyEventKind::Press, state: Default::default() };
        app.handle_key(kp);
        assert!(!app.running);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib tui::app`
Expected: 3 tests pass (the form/details/delete handlers are added and tested in later tasks).

- [ ] **Step 4: Stub the unimplemented handlers**

Add to `impl App`:

```rust
    fn handle_form_key(&mut self, _key: KeyEvent) -> AppAction { AppAction::None }
    fn handle_delete_key(&mut self, _key: KeyEvent, _idx: usize) -> AppAction { AppAction::None }
    fn handle_created_key(&mut self, _key: KeyEvent) -> AppAction { AppAction::None }
    fn handle_details_key(&mut self, _key: KeyEvent) -> AppAction { AppAction::None }
```

(These will be replaced in Tasks 18 and 19.)

- [ ] **Step 5: Re-run build + tests**

Run: `cargo test --lib`
Expected: all tests pass; crate builds.

- [ ] **Step 6: Commit**

```bash
git add src/tui/app.rs
git commit -m "feat(tui): add main-screen key handling with q-quit and Ctrl-C"
```

---

## Task 18: TUI — form key-handling (q is literal in text fields)

**Files:**
- Modify: `src/tui/app.rs`

- [ ] **Step 1: Replace `handle_form_key` and `handle_delete_key` and `handle_created_key` and `handle_details_key`**

```rust
    fn handle_form_key(&mut self, key: KeyEvent) -> AppAction {
        let n = self.event_types.len();
        match key.code {
            KeyCode::Esc => { self.modal = ModalState::None; return AppAction::None; }
            KeyCode::Tab | KeyCode::Down => { self.form.next_field(n); return AppAction::None; }
            KeyCode::BackTab | KeyCode::Up => { self.form.prev_field(n); return AppAction::None; }
            KeyCode::PageDown => { self.form.scroll = self.form.scroll.saturating_add(5); return AppAction::None; }
            KeyCode::PageUp => { self.form.scroll = self.form.scroll.saturating_sub(5); return AppAction::None; }
            KeyCode::Enter => return self.activate_form_field(),
            KeyCode::Backspace => match self.form.active_field {
                FormField::Url => { self.form.url.pop(); return AppAction::None; }
                FormField::Name => { self.form.name.pop(); return AppAction::None; }
                _ => return AppAction::None,
            },
            KeyCode::Char(' ') => match self.form.active_field {
                FormField::Url => self.form.url.push(' '),
                FormField::Name => self.form.name.push(' '),
                _ => return self.activate_form_field(),
            },
            KeyCode::Char(c) => match self.form.active_field {
                FormField::Url => self.form.url.push(c),
                FormField::Name => self.form.name.push(c),
                _ => {}
            },
            _ => {}
        }
        AppAction::None
    }

    fn activate_form_field(&mut self) -> AppAction {
        match self.form.active_field.clone() {
            FormField::Cancel => { self.modal = ModalState::None; AppAction::None }
            FormField::Submit => self.submit_form(),
            FormField::CheckAll => { self.form.events.iter_mut().for_each(|e| *e = true); AppAction::None }
            FormField::UncheckAll => { self.form.events.iter_mut().for_each(|e| *e = false); AppAction::None }
            FormField::Event(i) => {
                if let Some(slot) = self.form.events.get_mut(i) { *slot = !*slot; }
                AppAction::None
            }
            FormField::Status => {
                self.form.status = match self.form.status {
                    WebhookEndpointStatus::Active => WebhookEndpointStatus::Inactive,
                    WebhookEndpointStatus::Inactive => WebhookEndpointStatus::Active,
                };
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    fn submit_form(&mut self) -> AppAction {
        let selected: Vec<String> = self.event_types.iter().enumerate()
            .filter(|(i, _)| self.form.events.get(*i).copied().unwrap_or(false))
            .map(|(_, et)| et.name.clone())
            .collect();
        if self.form.url.trim().is_empty() {
            self.show_toast("Endpoint URL is required");
            return AppAction::None;
        }
        if selected.is_empty() {
            self.show_toast("Select at least one event type");
            return AppAction::None;
        }
        let name = if self.form.name.trim().is_empty() {
            "Untitled Webhook".to_string()
        } else {
            self.form.name.clone()
        };
        match self.modal.clone() {
            ModalState::CreateWebhook => {
                AppAction::Create(crate::api::models::CreateWebhookEndpointRequest {
                    name, endpoint_url: self.form.url.clone(), event_types: selected,
                })
            }
            ModalState::EditWebhook(idx) => {
                let id = self.endpoints[idx].id.clone();
                AppAction::Update(id, crate::api::models::UpdateWebhookEndpointRequest {
                    name, endpoint_url: self.form.url.clone(),
                    status: self.form.status, event_types: selected,
                })
            }
            _ => AppAction::None,
        }
    }

    fn handle_delete_key(&mut self, key: KeyEvent, idx: usize) -> AppAction {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => { self.modal = ModalState::None; AppAction::None }
            KeyCode::Enter | KeyCode::Char('y') => {
                if let Some(ep) = self.endpoints.get(idx) {
                    let id = ep.id.clone();
                    self.modal = ModalState::None;
                    return AppAction::Delete(id);
                }
                AppAction::None
            }
            _ => AppAction::None,
        }
    }

    fn handle_created_key(&mut self, key: KeyEvent) -> AppAction {
        if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
            self.modal = ModalState::None;
        }
        AppAction::None
    }

    fn handle_details_key(&mut self, key: KeyEvent) -> AppAction {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => { self.modal = ModalState::None; }
            KeyCode::Down | KeyCode::Char('j') => self.detail_scroll = self.detail_scroll.saturating_add(2),
            KeyCode::Up | KeyCode::Char('k') => self.detail_scroll = self.detail_scroll.saturating_sub(2),
            KeyCode::PageDown => self.detail_scroll = self.detail_scroll.saturating_add(10),
            KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(10),
            _ => {}
        }
        AppAction::None
    }
```

- [ ] **Step 2: Add behavior tests**

Append to the `mod tests` block in `src/tui/app.rs`:

```rust
    #[test]
    fn typing_q_in_url_field_appends_q_and_does_not_quit() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(0);
        // FormField::Url is the default starting field
        app.handle_key(key('q'));
        app.handle_key(key('a'));
        assert_eq!(app.form.url, "qa");
        assert!(app.running, "q in URL field must not quit the app");
    }

    #[test]
    fn typing_q_c_d_e_in_name_field_are_all_literal() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(0);
        app.form.active_field = FormField::Name;
        for c in ['q', 'c', 'd', 'e'] { app.handle_key(key(c)); }
        assert_eq!(app.form.name, "qcde");
        assert!(app.running);
    }

    #[test]
    fn esc_in_form_closes_modal() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(0);
        let kp = KeyEvent { code: KeyCode::Esc, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: Default::default() };
        app.handle_key(kp);
        assert_eq!(app.modal, ModalState::None);
    }

    #[test]
    fn pgdown_in_form_scrolls_event_list() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        app.form = FormState::new_create(35);
        let kp = KeyEvent { code: KeyCode::PageDown, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: Default::default() };
        app.handle_key(kp);
        app.handle_key(kp);
        assert_eq!(app.form.scroll, 10);
    }

    #[test]
    fn cadence_mode_is_backoff_when_form_open() {
        let mut app = App::new(None);
        app.modal = ModalState::CreateWebhook;
        assert_eq!(app.cadence_mode(), crate::poller::CadenceMode::Backoff);
        app.modal = ModalState::None;
        assert_eq!(app.cadence_mode(), crate::poller::CadenceMode::Active);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib tui::app`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/tui/app.rs
git commit -m "feat(tui): add form/delete/details key handling, q literal in text fields"
```

---

## Task 19: TUI — main render (tab bar, endpoints table with trigger count, help bar)

**Files:**
- Replace: `src/tui/ui.rs`

- [ ] **Step 1: Replace `src/tui/ui.rs`**

Use `/Users/chad.lung/Rust-Projects/webhook-tui/src/ui/mod.rs` as a structural template; the differences are:
- Endpoints table's "Events" column shows the trigger count, not the strings.
- Filter/help labels reference our enums.

```rust
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs},
    Frame,
};

use crate::api::models::{WebhookDeliveryLogStatus, WebhookEndpointStatus};
use crate::tui::app::{App, ModalState, Screen};
use crate::tui::modals;

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ]).split(frame.area());

    render_tab_bar(frame, app, chunks[0]);
    match app.screen {
        Screen::Endpoints => render_endpoints(frame, app, chunks[1]),
        Screen::DeliveryLogs => render_logs(frame, app, chunks[1]),
    }
    render_help_bar(frame, app, chunks[2]);

    match &app.modal {
        ModalState::None => {}
        ModalState::CreateWebhook => modals::render_create_modal(frame, app),
        ModalState::EditWebhook(_) => modals::render_edit_modal(frame, app),
        ModalState::DeleteWebhook(idx) => modals::render_delete_modal(frame, app, *idx),
        ModalState::WebhookCreated(secret) => modals::render_created_modal(frame, secret),
        ModalState::DeliveryDetails(id) => modals::render_details_modal(frame, app, id),
    }

    if let Some(msg) = &app.toast_message { render_toast(frame, msg); }
    if let Some(w) = &app.poll_warning { render_poll_warning(frame, w); }
}

fn render_tab_bar(frame: &mut Frame, app: &App, area: Rect) {
    let titles = vec![" Endpoints ", " Delivery Logs "];
    let selected = match app.screen { Screen::Endpoints => 0, Screen::DeliveryLogs => 1 };
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL)
            .title(" Flute Webhook Dashboard ").title_style(Style::default().fg(Color::Cyan).bold()))
        .select(selected)
        .style(Style::default().fg(Color::Green))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Green).bold());
    frame.render_widget(tabs, area);
}

fn render_endpoints(frame: &mut Frame, app: &App, area: Rect) {
    if app.endpoints.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "No webhooks yet. Press [c] to create one.",
            Style::default().fg(Color::Green))).alignment(Alignment::Center))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Description"), Cell::from("Endpoint URL"),
        Cell::from("Events"), Cell::from("Status"), Cell::from("Actions"),
    ]).style(Style::default().bold().fg(Color::Green));

    let rows: Vec<Row> = app.endpoints.iter().enumerate().map(|(i, ep)| {
        let status_style = match ep.status {
            WebhookEndpointStatus::Active => Style::default().fg(Color::Green),
            WebhookEndpointStatus::Inactive => Style::default().fg(Color::Yellow),
        };
        let count = if ep.trigger_count_partial {
            format!("{}+", ep.trigger_count)
        } else {
            ep.trigger_count.to_string()
        };
        let row = Row::new(vec![
            Cell::from(ep.name.clone()),
            Cell::from(Span::styled(ep.endpoint_url.clone(), Style::default().fg(Color::Blue))),
            Cell::from(count),
            Cell::from(Span::styled(format!("{:?}", ep.status), status_style)),
            Cell::from("[e]dit [d]el"),
        ]);
        if i == app.selected_endpoint {
            row.style(Style::default().bg(Color::Rgb(0, 80, 0)).fg(Color::White).bold())
        } else { row }
    }).collect();

    let widths = [
        Constraint::Percentage(25), Constraint::Percentage(35),
        Constraint::Percentage(10), Constraint::Percentage(12), Constraint::Percentage(18),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(table, area);
}

fn render_logs(frame: &mut Frame, app: &App, area: Rect) {
    let filtered = app.filtered_log_indices();
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    let filter_line = Line::from(vec![
        Span::styled(format!(" [1] Endpoint: {} ", endpoint_filter_label(app)), Style::default().fg(Color::Cyan)),
        Span::styled(format!(" [2] Event: {} ", event_filter_label(app)), Style::default().fg(Color::Cyan)),
        Span::styled(format!(" [3] Status: {} ", status_filter_label(app)), Style::default().fg(Color::Cyan)),
        Span::styled(format!(" [s] Sort: {} ", if app.sort_ascending { "Asc" } else { "Desc" }), Style::default().fg(Color::Cyan)),
        Span::styled(" [x] Clear ", Style::default().fg(Color::Yellow)),
    ]);
    frame.render_widget(Paragraph::new(filter_line).block(Block::default().borders(Borders::ALL)), chunks[0]);

    if filtered.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "No delivery logs match these filters.",
            Style::default().fg(Color::Green))).alignment(Alignment::Center))
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(p, chunks[1]);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Timestamp"), Cell::from("Event Type"), Cell::from("Status"),
        Cell::from("HTTP"), Cell::from("Duration"), Cell::from("Webhook"), Cell::from("Actions"),
    ]).style(Style::default().bold().fg(Color::Green));

    let rows: Vec<Row> = filtered.iter().enumerate().map(|(display_i, &log_i)| {
        let log = &app.logs[log_i];
        let (text, color) = match log.status {
            WebhookDeliveryLogStatus::Success => ("Success", Color::Green),
            WebhookDeliveryLogStatus::Failure => ("Failed", Color::Red),
        };
        let http = log.response_status_code.map(|s| s.to_string()).unwrap_or_else(|| "—".into());
        let row = Row::new(vec![
            Cell::from(log.created_on.format("%m/%d/%y %H:%M:%S").to_string()),
            Cell::from(Span::styled(log.event_type.clone(), Style::default().fg(Color::Blue))),
            Cell::from(Span::styled(text, Style::default().fg(color).bold())),
            Cell::from(http),
            Cell::from(format!("{}ms", log.duration_ms)),
            Cell::from(log.endpoint_name.clone()),
            Cell::from("[v]iew"),
        ]);
        if display_i == app.selected_log {
            row.style(Style::default().bg(Color::Rgb(0, 80, 0)).fg(Color::White).bold())
        } else { row }
    }).collect();

    let widths = [
        Constraint::Length(18), Constraint::Percentage(22), Constraint::Length(9),
        Constraint::Length(6), Constraint::Length(10), Constraint::Percentage(20), Constraint::Length(10),
    ];
    frame.render_widget(Table::new(rows, widths).header(header).block(Block::default().borders(Borders::ALL)), chunks[1]);
}

fn endpoint_filter_label(app: &App) -> String {
    if app.filter_endpoint == 0 { "All".into() }
    else { app.endpoints.get(app.filter_endpoint - 1).map(|e| e.name.clone()).unwrap_or_else(|| "All".into()) }
}
fn event_filter_label(app: &App) -> String {
    if app.filter_event == 0 { "All".into() }
    else { app.event_types.get(app.filter_event - 1).map(|e| e.name.clone()).unwrap_or_else(|| "All".into()) }
}
fn status_filter_label(app: &App) -> &'static str {
    match app.filter_status { 1 => "Success", 2 => "Failed", _ => "All" }
}

fn render_help_bar(frame: &mut Frame, app: &App, area: Rect) {
    let help = match (&app.modal, &app.screen) {
        (ModalState::None, Screen::Endpoints) => "Tab: switch | ↑↓/jk: nav | c: create | e: edit | d: delete | q: quit",
        (ModalState::None, Screen::DeliveryLogs) => "Tab: switch | ↑↓/jk: nav | v: details | 1-3: filters | s: sort | x: clear | q: quit",
        (ModalState::CreateWebhook | ModalState::EditWebhook(_), _) => "Tab/↑↓: fields | Space: toggle | Enter: activate | PgUp/PgDn: scroll | Esc: cancel",
        (ModalState::DeleteWebhook(_), _) => "y/Enter: confirm | n/Esc: cancel",
        (ModalState::WebhookCreated(_), _) => "Enter/Esc: done",
        (ModalState::DeliveryDetails(_), _) => "↑↓/jk/PgUp/PgDn: scroll | Esc/Enter/q: close",
    };
    frame.render_widget(Paragraph::new(Line::from(Span::styled(format!(" {help}"),
        Style::default().fg(Color::Green)))).style(Style::default().bg(Color::Black)), area);
}

fn render_toast(frame: &mut Frame, msg: &str) {
    let area = frame.area();
    let width = (msg.len() as u16 + 4).min(area.width);
    let x = area.width.saturating_sub(width) / 2;
    let toast_area = Rect::new(x, area.height.saturating_sub(3), width, 3);
    frame.render_widget(ratatui::widgets::Clear, toast_area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(Color::White).bold())))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).style(Style::default().bg(Color::Black))),
        toast_area,
    );
}

fn render_poll_warning(frame: &mut Frame, msg: &str) {
    let area = frame.area();
    let width = (msg.len() as u16 + 4).min(area.width);
    let bar = Rect::new(0, 0, width, 1);
    let _ = bar; // intentionally unused — warning is part of the toast system
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(format!("⚠ {msg}"), Style::default().fg(Color::Yellow)))),
        Rect::new(0, area.height.saturating_sub(2), area.width, 1),
    );
}

pub fn centered_rect(width_pct: u16, height_pct: u16, area: Rect) -> Rect {
    let v = Layout::vertical([
        Constraint::Percentage((100 - height_pct) / 2),
        Constraint::Percentage(height_pct),
        Constraint::Percentage((100 - height_pct) / 2),
    ]).split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - width_pct) / 2),
        Constraint::Percentage(width_pct),
        Constraint::Percentage((100 - width_pct) / 2),
    ]).split(v[1])[1]
}
```

- [ ] **Step 2: Verify build**

Run: `cargo build`
Expected: compiles. `tui/modals.rs` is still empty — link errors will name `modals::render_*` symbols; we add those next.

- [ ] **Step 3: Add `render_*` stubs to `src/tui/modals.rs`**

```rust
use ratatui::Frame;
use crate::tui::app::App;

pub fn render_create_modal(_frame: &mut Frame, _app: &App) {}
pub fn render_edit_modal(_frame: &mut Frame, _app: &App) {}
pub fn render_delete_modal(_frame: &mut Frame, _app: &App, _idx: usize) {}
pub fn render_created_modal(_frame: &mut Frame, _secret: &str) {}
pub fn render_details_modal(_frame: &mut Frame, _app: &App, _log_id: &str) {}
```

- [ ] **Step 4: Run build**

Run: `cargo build`
Expected: compiles cleanly.

- [ ] **Step 5: Commit**

```bash
git add src/tui/ui.rs src/tui/modals.rs
git commit -m "feat(tui): add main render with trigger-count column"
```

---

## Task 20: TUI — modal renderers (scrollable form + scrollable details)

**Files:**
- Replace: `src/tui/modals.rs`

- [ ] **Step 1: Replace `src/tui/modals.rs`**

Use `/Users/chad.lung/Rust-Projects/webhook-tui/src/ui/modals.rs` as a structural template. Adapt: the events list iterates over `app.event_types` (any length, grouped by `EventTypeMeta::group`); the form `Paragraph` uses `.scroll((app.form.scroll, 0))`; the delete modal uses `app.endpoints[idx]`; the details modal looks up by id and uses `app.detail_scroll`.

Key changes from the reference:

```rust
use ratatui::{
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use std::collections::BTreeMap;

use crate::api::models::WebhookEndpointStatus;
use crate::tui::app::{App, FormField};
use crate::tui::ui::centered_rect;

pub fn render_create_modal(frame: &mut Frame, app: &App) {
    render_form_modal(frame, app, "Create Webhook",
        "Configure an endpoint to receive event notifications", "Create Webhook");
}

pub fn render_edit_modal(frame: &mut Frame, app: &App) {
    render_form_modal(frame, app, "Edit Webhook",
        "Update endpoint URL, name, status, and event subscriptions", "Save Changes");
}

fn render_form_modal(frame: &mut Frame, app: &App, title: &str, subtitle: &str, submit_label: &str) {
    let area = centered_rect(70, 90, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL)
        .title(format!(" {title} "))
        .title_style(Style::default().fg(Color::Cyan).bold())
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(subtitle, Style::default().fg(Color::Green))));
    lines.push(Line::raw(""));

    // URL field
    push_text_field(&mut lines, "Endpoint URL *", &app.form.url,
        app.form.active_field == FormField::Url, "https://api.yourdomain.com/webhooks");
    lines.push(Line::from(Span::styled("  Must be an HTTPS endpoint", Style::default().fg(Color::Green))));
    lines.push(Line::raw(""));

    // Name field
    push_text_field(&mut lines, "Name", &app.form.name,
        app.form.active_field == FormField::Name, "e.g., Order Processing Webhook");
    lines.push(Line::raw(""));

    // Status (edit only)
    if app.form.is_edit {
        push_status_field(&mut lines, app);
        lines.push(Line::raw(""));
    }

    // Events header + Check/Uncheck All
    push_events_header(&mut lines, app);
    lines.push(Line::raw(""));

    // Events grouped by metadata.group, in the API's natural order
    let groups = group_events(app);
    for (group, indices) in &groups {
        lines.push(Line::from(Span::styled(format!("  {group}"),
            Style::default().fg(Color::White).bold())));
        for &i in indices {
            let et = &app.event_types[i];
            let checked = if app.form.events.get(i).copied().unwrap_or(false) { "☑" } else { "☐" };
            let active = app.form.active_field == FormField::Event(i);
            let pointer = if active { Span::styled("  ▸ ", Style::default().fg(Color::Cyan)) } else { Span::raw("    ") };
            let style = if active { Style::default().fg(Color::Cyan).bold() }
                else if app.form.events.get(i).copied().unwrap_or(false) { Style::default().fg(Color::Green) }
                else { Style::default().fg(Color::White) };
            lines.push(Line::from(vec![pointer, Span::styled(format!("{checked} "), style),
                Span::styled(et.name.clone(), style)]));
            if !et.description.is_empty() {
                lines.push(Line::from(Span::styled(format!("      {}", et.description),
                    Style::default().fg(Color::Green))));
            }
        }
        lines.push(Line::raw(""));
    }

    // Buttons
    push_buttons(&mut lines, app, submit_label);

    let para = Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((app.form.scroll, 0));
    frame.render_widget(para, inner);
}

fn group_events(app: &App) -> Vec<(String, Vec<usize>)> {
    let mut map: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, et) in app.event_types.iter().enumerate() {
        map.entry(et.group.clone()).or_default().push(i);
    }
    map.into_iter().collect()
}

fn push_text_field(lines: &mut Vec<Line>, label: &str, value: &str, active: bool, placeholder: &str) {
    let label_style = if active { Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::White) };
    lines.push(Line::from(Span::styled(label.to_string(), label_style)));
    let cursor = if active { "█" } else { "" };
    let display = if value.is_empty() && !active {
        Span::styled(placeholder.to_string(), Style::default().fg(Color::Green))
    } else {
        Span::styled(format!("{value}{cursor}"), Style::default().fg(Color::White))
    };
    let pointer = if active { Span::styled("▸ ", Style::default().fg(Color::Cyan)) } else { Span::raw("  ") };
    lines.push(Line::from(vec![Span::raw(" "), pointer, display]));
}

fn push_status_field(lines: &mut Vec<Line>, app: &App) {
    let active = app.form.active_field == FormField::Status;
    let style = if active { Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::White) };
    lines.push(Line::from(Span::styled("Status *".to_string(), style)));
    let active_marker = if app.form.status == WebhookEndpointStatus::Active { "(●)" } else { "( )" };
    let inactive_marker = if app.form.status == WebhookEndpointStatus::Inactive { "(●)" } else { "( )" };
    let pointer = if active { Span::styled("▸ ", Style::default().fg(Color::Cyan)) } else { Span::raw("  ") };
    lines.push(Line::from(vec![
        Span::raw(" "), pointer,
        Span::styled(format!("{active_marker} Active"), Style::default().fg(Color::Green)),
        Span::raw("  "),
        Span::styled(format!("{inactive_marker} Inactive"), Style::default().fg(Color::Yellow)),
    ]));
}

fn push_events_header(lines: &mut Vec<Line>, app: &App) {
    let check_style = if app.form.active_field == FormField::CheckAll {
        Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::Green) };
    let uncheck_style = if app.form.active_field == FormField::UncheckAll {
        Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::Green) };
    lines.push(Line::from(vec![
        Span::styled("Events *", Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled("[Check All]", check_style),
        Span::raw(" "),
        Span::styled("[Uncheck All]", uncheck_style),
    ]));
}

fn push_buttons(lines: &mut Vec<Line>, app: &App, submit_label: &str) {
    let cancel = if app.form.active_field == FormField::Cancel {
        Style::default().fg(Color::Black).bg(Color::White).bold() } else { Style::default().fg(Color::White) };
    let submit = if app.form.active_field == FormField::Submit {
        Style::default().fg(Color::Black).bg(Color::Cyan).bold() } else { Style::default().fg(Color::Cyan) };
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(" Cancel ", cancel),
        Span::raw("  "),
        Span::styled(format!(" {submit_label} "), submit),
    ]));
}

pub fn render_delete_modal(frame: &mut Frame, app: &App, idx: usize) {
    let area = centered_rect(50, 40, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL)
        .title(" ⚠ Delete Webhook ")
        .title_style(Style::default().fg(Color::Red).bold())
        .border_style(Style::default().fg(Color::Red))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(ep) = app.endpoints.get(idx) else { return; };
    let lines = vec![
        Line::from(Span::styled("Are you sure you want to delete this webhook?", Style::default().fg(Color::White))),
        Line::raw(""),
        Line::from(Span::styled(ep.name.clone(), Style::default().fg(Color::White).bold())),
        Line::from(Span::styled(ep.endpoint_url.clone(), Style::default().fg(Color::Blue))),
        Line::raw(""),
        Line::from(Span::styled(
            "This action cannot be undone. The endpoint will stop receiving events immediately.",
            Style::default().fg(Color::Red))),
        Line::raw(""),
        Line::from(vec![
            Span::styled(" [n/Esc] Cancel ", Style::default().fg(Color::White)),
            Span::raw("  "),
            Span::styled(" [y/Enter] Delete Webhook ", Style::default().fg(Color::Red).bold()),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

pub fn render_created_modal(frame: &mut Frame, secret: &str) {
    let area = centered_rect(60, 50, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL)
        .title(" ✓ Webhook Created ")
        .title_style(Style::default().fg(Color::Green).bold())
        .border_style(Style::default().fg(Color::Green))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = vec![
        Line::from(Span::styled("Your webhook has been created.", Style::default().fg(Color::White))),
        Line::from(Span::styled("Copy the signing secret now — it won't be shown again.",
            Style::default().fg(Color::Green))),
        Line::raw(""),
        Line::from(Span::styled("Your Signing Secret:", Style::default().fg(Color::White).bold())),
        Line::from(Span::styled(secret.to_string(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        Line::raw(""),
        Line::from(Span::styled(" [Enter/Esc] Done ", Style::default().fg(Color::Cyan).bold())),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

pub fn render_details_modal(frame: &mut Frame, app: &App, log_id: &str) {
    let area = centered_rect(75, 90, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL)
        .title(" Delivery Details ")
        .title_style(Style::default().fg(Color::Cyan).bold())
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(log) = app.logs.iter().find(|l| l.id == log_id) else {
        let p = Paragraph::new(Span::styled("Loading details…", Style::default().fg(Color::Yellow)));
        frame.render_widget(p, inner);
        return;
    };
    let lines = vec![
        Line::from(Span::styled(format!("Event: {}", log.event_type), Style::default().fg(Color::Blue))),
        Line::from(Span::styled(format!("Endpoint: {}", log.endpoint_name), Style::default().fg(Color::White).bold())),
        Line::from(Span::styled(format!("URL: {}", log.endpoint_url), Style::default().fg(Color::Blue))),
        Line::from(Span::styled(format!("Status: {:?}  HTTP: {:?}  Duration: {}ms  Attempt: {}",
            log.status, log.response_status_code, log.duration_ms, log.attempt_number),
            Style::default().fg(Color::White))),
        Line::raw(""),
        Line::from(Span::styled("Press v on a log row to fetch full request/response from the API",
            Style::default().fg(Color::Green))),
    ];
    let para = Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((app.detail_scroll, 0));
    frame.render_widget(para, inner);
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles cleanly.

- [ ] **Step 3: Commit**

```bash
git add src/tui/modals.rs
git commit -m "feat(tui): add scrollable form, delete, created, details modals"
```

---

## Task 21: TUI — terminal lifecycle + event loop wired to poller

**Files:**
- Replace: `src/tui/mod.rs`

- [ ] **Step 1: Replace `src/tui/mod.rs`**

```rust
pub mod app;
pub mod modals;
pub mod ui;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use tokio::sync::{mpsc, watch};

use crate::api::ApiClient;
use crate::auth::{keychain, token::{OAuth2Fetcher, TokenStore}};
use crate::config::{self, validate_poll_interval, Profile};
use crate::poller::{self, CadenceMode, PollerEvent};
use crate::tui::app::{App, AppAction, ModalState};

pub async fn run(profile_name: &str) -> anyhow::Result<()> {
    let profile = Profile::by_name(profile_name)
        .ok_or_else(|| anyhow!("unknown profile: {profile_name}"))?;
    let cfg = config::load_or_default();
    let validated = validate_poll_interval(cfg.poll_interval_seconds);

    let (id, secret) = keychain::load_with_env_fallback(profile_name)?
        .ok_or_else(|| anyhow!("no credentials for [{profile_name}]; run `flute-webhook auth login`"))?;

    let http = reqwest::Client::builder().timeout(Duration::from_secs(15)).build()?;
    let fetcher = Arc::new(OAuth2Fetcher {
        oauth_url: profile.oauth_url.clone(),
        client_id: id, client_secret: secret, http: http.clone(),
    });
    let api = ApiClient {
        base_url: profile.api_base_url.clone(),
        http, tokens: TokenStore::new(fetcher),
    };

    let (cadence_tx, cadence_rx) = watch::channel(CadenceMode::Active);
    let (events_tx, mut events_rx) = mpsc::channel::<PollerEvent>(8);
    let (action_tx, mut action_rx) = mpsc::channel::<AppAction>(8);

    let _poller_handle = poller::spawn(api.clone(), cadence_rx, validated.seconds, events_tx);

    // Action executor task: runs Create/Update/Delete/Details requests off the UI thread.
    let api_for_actions = api.clone();
    let result_tx = events_tx_clone(); // helper defined below
    tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            execute_action(&api_for_actions, action, &result_tx).await;
        }
    });

    enable_raw_mode().context("enable_raw_mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("EnterAlternateScreen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(validated.warning);

    let res = event_loop(&mut terminal, &mut app, &mut events_rx, &cadence_tx, &action_tx).await;

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    res
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    events_rx: &mut mpsc::Receiver<PollerEvent>,
    cadence_tx: &watch::Sender<CadenceMode>,
    action_tx: &mpsc::Sender<AppAction>,
) -> anyhow::Result<()> {
    let mut last_mode = app.cadence_mode();
    while app.running {
        terminal.draw(|f| ui::render(f, app))?;

        // Drain any poller events without blocking
        while let Ok(ev) = events_rx.try_recv() {
            match ev {
                PollerEvent::Snapshot(s) => app.apply_snapshot(s.endpoints, s.logs, s.event_types),
                PollerEvent::Error(e) => app.last_error = Some(e),
            }
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let action = app.handle_key(key);
                if !matches!(action, AppAction::None) {
                    let _ = action_tx.send(action).await;
                }
            }
        }
        app.tick_toast();

        let mode = app.cadence_mode();
        if mode != last_mode {
            let _ = cadence_tx.send(mode);
            last_mode = mode;
        }
    }
    Ok(())
}

// helper: route Create/Update/Delete results back via toast/PollerEvent stream
fn events_tx_clone() -> mpsc::Sender<PollerEvent> {
    // The action executor needs its own channel handle. The simplest path is to
    // have execute_action trigger an immediate poll by returning AppAction::None
    // and letting the next poll tick refresh state. We deliver outcome strings
    // through a separate side-channel (see Task 22 for wiring toasts).
    let (tx, _rx) = mpsc::channel(1);
    tx
}

async fn execute_action(api: &ApiClient, action: AppAction, _tx: &mpsc::Sender<PollerEvent>) {
    match action {
        AppAction::Create(req) => { let _ = api.create_endpoint(&req).await; }
        AppAction::Update(id, req) => { let _ = api.update_endpoint(&id, &req).await; }
        AppAction::Delete(id) => { let _ = api.delete_endpoint(&id).await; }
        AppAction::OpenDetails(id) => { let _ = api.get_delivery_log(&id).await; }
        AppAction::None => {}
    }
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles cleanly. (Action results don't surface to the UI yet — Task 22 fixes that.)

- [ ] **Step 3: Commit**

```bash
git add src/tui/mod.rs
git commit -m "feat(tui): wire terminal lifecycle, poller, and action channels"
```

---

## Task 22: Wire action outcomes back to the UI as toasts + Created modal

**Files:**
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/app.rs`

This task closes the gap left in Task 21: action results need to flow back to the App so create/update/delete show a toast and create can pop the secret modal.

- [ ] **Step 1: Add `ActionOutcome` to `src/tui/app.rs`**

```rust
#[derive(Debug)]
pub enum ActionOutcome {
    Toast(String),
    Created { secret: String },
    Error(String),
}

impl App {
    pub fn apply_outcome(&mut self, outcome: ActionOutcome) {
        match outcome {
            ActionOutcome::Toast(msg) => self.show_toast(msg),
            ActionOutcome::Created { secret } => {
                self.modal = ModalState::WebhookCreated(secret);
            }
            ActionOutcome::Error(msg) => self.last_error = Some(msg),
        }
    }
}
```

- [ ] **Step 2: Replace `events_tx_clone` and `execute_action` in `src/tui/mod.rs`**

```rust
// Remove the placeholder events_tx_clone helper.
async fn execute_action(api: &ApiClient, action: AppAction,
    outcome_tx: &mpsc::Sender<crate::tui::app::ActionOutcome>) {
    use crate::tui::app::ActionOutcome;
    match action {
        AppAction::Create(req) => match api.create_endpoint(&req).await {
            Ok(resp) => {
                let secret = resp.secret.unwrap_or_else(|| "(none returned)".into());
                let _ = outcome_tx.send(ActionOutcome::Created { secret }).await;
            }
            Err(e) => { let _ = outcome_tx.send(ActionOutcome::Error(e.to_string())).await; }
        },
        AppAction::Update(id, req) => match api.update_endpoint(&id, &req).await {
            Ok(_) => { let _ = outcome_tx.send(ActionOutcome::Toast("Webhook updated".into())).await; }
            Err(e) => { let _ = outcome_tx.send(ActionOutcome::Error(e.to_string())).await; }
        },
        AppAction::Delete(id) => match api.delete_endpoint(&id).await {
            Ok(_) => { let _ = outcome_tx.send(ActionOutcome::Toast("Webhook deleted".into())).await; }
            Err(e) => { let _ = outcome_tx.send(ActionOutcome::Error(e.to_string())).await; }
        },
        AppAction::OpenDetails(_) => {} // detail fetch deferred — Task 23
        AppAction::None => {}
    }
}
```

- [ ] **Step 3: Replace channel wiring in `run()`**

Replace the `(action_tx, mut action_rx)` block and the action-executor spawn:

```rust
    let (action_tx, mut action_rx) = mpsc::channel::<AppAction>(8);
    let (outcome_tx, mut outcome_rx) = mpsc::channel::<crate::tui::app::ActionOutcome>(8);

    let api_for_actions = api.clone();
    let outcome_tx_for_executor = outcome_tx.clone();
    tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            execute_action(&api_for_actions, action, &outcome_tx_for_executor).await;
        }
    });
```

Update the `event_loop` signature and body to take `outcome_rx` and drain it each tick:

```rust
async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    events_rx: &mut mpsc::Receiver<PollerEvent>,
    outcome_rx: &mut mpsc::Receiver<crate::tui::app::ActionOutcome>,
    cadence_tx: &watch::Sender<CadenceMode>,
    action_tx: &mpsc::Sender<AppAction>,
) -> anyhow::Result<()> {
    let mut last_mode = app.cadence_mode();
    while app.running {
        terminal.draw(|f| ui::render(f, app))?;

        while let Ok(ev) = events_rx.try_recv() {
            match ev {
                PollerEvent::Snapshot(s) => app.apply_snapshot(s.endpoints, s.logs, s.event_types),
                PollerEvent::Error(e) => app.last_error = Some(e),
            }
        }
        while let Ok(o) = outcome_rx.try_recv() { app.apply_outcome(o); }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let action = app.handle_key(key);
                if !matches!(action, AppAction::None) { let _ = action_tx.send(action).await; }
            }
        }
        app.tick_toast();

        let mode = app.cadence_mode();
        if mode != last_mode { let _ = cadence_tx.send(mode); last_mode = mode; }
    }
    Ok(())
}
```

And update the call site in `run()`:

```rust
    let res = event_loop(&mut terminal, &mut app, &mut events_rx, &mut outcome_rx, &cadence_tx, &action_tx).await;
```

- [ ] **Step 4: Build**

Run: `cargo build`
Expected: compiles cleanly.

- [ ] **Step 5: Commit**

```bash
git add src/tui/
git commit -m "feat(tui): surface action outcomes as toasts and Created modal"
```

---

## Task 23: Fetch full delivery details on demand

**Files:**
- Modify: `src/tui/app.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/modals.rs`

- [ ] **Step 1: Extend `App` with a detail cache in `src/tui/app.rs`**

Add to the `App` struct: `pub delivery_detail: Option<crate::api::models::DeliveryLogDetailDto>,`
Initialise to `None` in `App::new`.
Add a method:

```rust
    pub fn set_delivery_detail(&mut self, d: crate::api::models::DeliveryLogDetailDto) {
        self.delivery_detail = Some(d);
    }

    pub fn clear_delivery_detail(&mut self) { self.delivery_detail = None; }
```

Update `handle_details_key` so `Esc/Enter/q` also calls `self.clear_delivery_detail()`.

Add a third variant to `ActionOutcome`:

```rust
    DeliveryDetail(crate::api::models::DeliveryLogDetailDto),
```

…and handle it in `apply_outcome`:

```rust
            ActionOutcome::DeliveryDetail(d) => self.set_delivery_detail(d),
```

- [ ] **Step 2: Update `execute_action` for `OpenDetails`**

```rust
        AppAction::OpenDetails(id) => match api.get_delivery_log(&id).await {
            Ok(d) => { let _ = outcome_tx.send(ActionOutcome::DeliveryDetail(d)).await; }
            Err(e) => { let _ = outcome_tx.send(ActionOutcome::Error(e.to_string())).await; }
        },
```

- [ ] **Step 3: Use the cached detail in `render_details_modal`**

Replace the body of `render_details_modal` so that, when `app.delivery_detail` is `Some` and matches `log_id`, it renders headers + payload + response (similar to the reference `render_details_modal`). Keep `.scroll((app.detail_scroll, 0))` so the modal remains scrollable.

```rust
pub fn render_details_modal(frame: &mut Frame, app: &App, log_id: &str) {
    let area = centered_rect(75, 90, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL)
        .title(" Delivery Details ")
        .title_style(Style::default().fg(Color::Cyan).bold())
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    let detail = app.delivery_detail.as_ref().filter(|d| d.id == log_id);
    if detail.is_none() {
        lines.push(Line::from(Span::styled("Loading details…", Style::default().fg(Color::Yellow))));
        let p = Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((app.detail_scroll, 0));
        frame.render_widget(p, inner);
        return;
    }
    let d = detail.unwrap();

    lines.push(Line::from(vec![
        Span::styled(format!(" {:?} ", d.status),
            Style::default().fg(Color::Black).bg(match d.status {
                crate::api::models::WebhookDeliveryLogStatus::Success => Color::Green,
                crate::api::models::WebhookDeliveryLogStatus::Failure => Color::Red,
            }).bold()),
        Span::raw("  "),
        Span::styled(d.created_on.format("%m/%d/%y %H:%M:%S").to_string(),
            Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled(format!("{}ms", d.duration_ms), Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled(format!("Attempt {}", d.attempt_number), Style::default().fg(Color::Green)),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("━━ Request ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
        Style::default().fg(Color::Cyan))));
    lines.push(Line::from(vec![
        Span::styled("POST ", Style::default().fg(Color::Green).bold()),
        Span::styled(d.endpoint_url.clone().unwrap_or_default(), Style::default().fg(Color::Blue)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Event: ", Style::default().fg(Color::Green)),
        Span::styled(d.event_type.clone().unwrap_or_default(), Style::default().fg(Color::Blue)),
    ]));
    lines.push(Line::raw(""));

    if let Some(headers) = &d.request_headers {
        lines.push(Line::from(Span::styled("Headers:", Style::default().fg(Color::White).bold())));
        let mut keys: Vec<&String> = headers.keys().collect();
        keys.sort();
        for k in keys {
            let v = headers.get(k).cloned().flatten().unwrap_or_default();
            lines.push(Line::from(Span::styled(format!("  {k}: {v}"), Style::default().fg(Color::Green))));
        }
        lines.push(Line::raw(""));
    }
    if let Some(body) = &d.request_body {
        lines.push(Line::from(Span::styled("Payload:", Style::default().fg(Color::White).bold())));
        for ln in body.lines() {
            lines.push(Line::from(Span::styled(format!("  {ln}"), Style::default().fg(Color::Green))));
        }
        lines.push(Line::raw(""));
    }
    lines.push(Line::from(Span::styled("━━ Response ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
        Style::default().fg(Color::Cyan))));
    if let Some(code) = d.response_status_code {
        lines.push(Line::from(Span::styled(format!("HTTP {code}"), Style::default().fg(Color::White).bold())));
    } else {
        lines.push(Line::from(Span::styled("No response received", Style::default().fg(Color::Red))));
    }
    if let Some(body) = &d.response_body {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled("Body:", Style::default().fg(Color::White).bold())));
        for ln in body.lines() {
            lines.push(Line::from(Span::styled(format!("  {ln}"), Style::default().fg(Color::Green))));
        }
    }
    if let Some(err) = &d.error_message {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(format!("Error: {err}"), Style::default().fg(Color::Red))));
    }

    let para = Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((app.detail_scroll, 0));
    frame.render_widget(para, inner);
}
```

- [ ] **Step 4: Build + run lib tests**

Run: `cargo test --lib`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/tui/
git commit -m "feat(tui): fetch full delivery detail on demand and render scrollable modal"
```

---

## Task 24: TestBackend snapshot test for endpoints screen

**Files:**
- Create: `tests/tui_render.rs`

- [ ] **Step 1: Create `tests/tui_render.rs`**

```rust
use chrono::Utc;
use flute_webhook::api::models::WebhookEndpointStatus;
use flute_webhook::domain::Endpoint;
use flute_webhook::tui::app::App;
use flute_webhook::tui::ui::render;
use ratatui::{backend::TestBackend, Terminal};

fn endpoint(name: &str, count: u32, partial: bool) -> Endpoint {
    Endpoint {
        id: name.into(), name: name.into(),
        endpoint_url: format!("https://example.com/{name}"),
        event_types: vec!["transaction.card.captured".into()],
        status: WebhookEndpointStatus::Active,
        created_on: Some(Utc::now()),
        trigger_count: count, trigger_count_partial: partial,
    }
}

#[test]
fn endpoints_table_shows_trigger_count_not_strings() {
    let mut app = App::new(None);
    app.endpoints = vec![endpoint("Order Processing", 17, false), endpoint("Hot path", 200, true)];

    let backend = TestBackend::new(120, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| render(f, &app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let text = buf_to_string(&buf);

    assert!(text.contains("Order Processing"), "row missing: {text}");
    assert!(text.contains("17"), "count missing: {text}");
    assert!(text.contains("200+"), "partial-count marker missing: {text}");
    // The column should NOT show a comma-joined event-type list
    assert!(!text.contains("transaction.card.captured"), "events should be a count, not strings: {text}");
}

fn buf_to_string(buf: &ratatui::buffer::Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(""));
        }
        out.push('\n');
    }
    out
}
```

Make `tui::ui::render` and `tui::app::App` and `domain::Endpoint` `pub` (already exposed via `lib.rs` re-exports — verify).

- [ ] **Step 2: Run test**

Run: `cargo test --test tui_render`
Expected: passes; the assertions check for `"17"`, `"200+"`, and *absence* of the event-type string in the Events column.

- [ ] **Step 3: Commit**

```bash
git add tests/tui_render.rs
git commit -m "test(tui): assert endpoints column shows trigger count, not event names"
```

---

## Task 25: End-to-end smoke test against UAT

**Files:** none (manual verification step).

- [ ] **Step 1: Authenticate against UAT**

```bash
cargo run -- --profile uat auth login
# enter your UAT client_id and client_secret
```

- [ ] **Step 2: Verify token retrieval**

```bash
cargo run -- --profile uat auth token
```

Expected: prints a JWT (no errors). If this fails, fix auth before continuing.

- [ ] **Step 3: Launch the TUI**

```bash
cargo run
# or explicitly:
cargo run -- --profile uat tui
```

Verify in order:
1. The Endpoints tab populates within ~5 s of launch.
2. Press `c`, fill in URL and Name, type the letter `q` in each field — confirm the program does **not** quit and the `q` is captured.
3. Tab to Events, scroll with `PgDown` through all 35 event types, toggle a few with Space, press Enter on `Create Webhook`. The Created modal should show the signing secret.
4. Press `e` on a row to edit; change status; verify the change persists after closing.
5. Press `d` then `y` to delete; verify the row disappears and a toast confirms.
6. Switch to Delivery Logs (`Tab`); cycle filters with `1` `2` `3`; press `v` on a row to fetch and view full request/response (scroll with `j`/`k`).
7. Set `poll_interval_seconds = 999` in `~/.flute/config.toml`, relaunch, and verify the warning bar appears at the bottom and polling falls back to 5 s (confirm via the timestamp on snapshot updates).
8. Set `poll_interval_seconds = 30`, open a create form, and verify polling does not speed up (still 30 s, since 30 ≥ 20 s backoff floor).

- [ ] **Step 4: If everything passes, commit a docs/notes update**

Optional — only if you want to record the verification:

```bash
git commit --allow-empty -m "test: smoke-tested TUI against UAT (manual)"
```

---

## Self-Review Notes

Spec coverage cross-check (spec § → task):

- §1 Hard requirements (Rust + Ratatui + single binary + sub-200ms startup) → Task 1, 21
- §2 Authentication, environments, profile system → Tasks 3, 4, 5, 6, 15
- §6 Webhook endpoint management (create/list/get/update/delete/ping) → Tasks 8, 10, 22
- §6 Delivery inspection (list/get/retry) → Tasks 9, 11, 23
- §6 Event types catalog → Tasks 8, 10, 14
- §7 Configuration precedence + ~/.flute layout → Tasks 2, 3, 4, 15
- User-stated TUI requirements:
  - Events column shows trigger count, not strings → Tasks 12, 19, 24
  - Modals scroll → Tasks 18 (form scroll), 18 (details scroll), 20, 23
  - `q` is literal in text-input fields → Tasks 18 (test cases)
  - Polling default 5 s, backoff to 20 s during input, configurable 5–60 s with warning → Tasks 2, 13, 14, 18 (cadence_mode test)
  - Use REST API to power the TUI → Tasks 10, 11, 14

Out-of-scope items the user did **not** ask for (defer to future plans): self-update, Homebrew/installer, `flute-webhook listen`/`trigger`/`deliveries` CLI subcommands, JSON/quiet output modes, golden-file CLI tests, OAuth `tokens create/list/revoke`, `--merchant-id` ISV flag.
