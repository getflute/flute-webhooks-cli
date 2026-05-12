//! Lightweight startup check that pings GitHub Releases at most once per
//! 24 h and reports whether a newer version of flute-webhook is available.
//!
//! Three opt-out paths so this never gets in the way:
//!   1. `auto_update_check = false` in `~/.flute/config.toml`
//!   2. `FLUTE_NO_UPDATE_CHECK` env var set to anything
//!   3. `CI` env var set (typical Actions/Buildkite/Jenkins indicator)
//!
//! Callers are also expected to bail when stderr isn't a TTY — there is no
//! point printing an update notice to a piped or redirected stream — but
//! `IsTerminal` is a per-call concern so that check lives at the call site.
//!
//! The cache file lives at `~/.flute/update-check.json`. Anything we can't
//! parse, write, or fetch is treated as "no notice" rather than a hard
//! error: this code path is best-effort, not part of the user's task.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::{Config, config_dir};

const CACHE_FILE: &str = "update-check.json";
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;
const NETWORK_TIMEOUT_SECS: u64 = 3;

#[derive(serde::Serialize, serde::Deserialize)]
struct Cache {
    checked_at_unix_secs: u64,
    /// Latest version seen at the time of the check; `None` means we
    /// confirmed the binary was on latest. We persist the answer either way
    /// so a "no update" check still suppresses the next 24 h of network
    /// calls.
    latest_version: Option<String>,
}

/// True if any of the documented opt-outs apply.
pub fn opt_out(cfg: &Config) -> bool {
    if !cfg.auto_update_check {
        return true;
    }
    if std::env::var_os("FLUTE_NO_UPDATE_CHECK").is_some() {
        return true;
    }
    if std::env::var_os("CI").is_some() {
        return true;
    }
    false
}

/// Returns `Some(latest_version)` if a newer version exists, otherwise None.
/// Uses the on-disk cache when fresh; falls back to a network query (bounded
/// by `NETWORK_TIMEOUT_SECS`) when the cache is stale or unreadable.
pub async fn check_for_update() -> Option<String> {
    if let Some(cached) = read_fresh_cache() {
        return cached.latest_version.filter(|v| is_newer_than_current(v));
    }

    let latest: Option<String> = tokio::time::timeout(
        Duration::from_secs(NETWORK_TIMEOUT_SECS),
        crate::update::query_latest_silently(),
    )
    .await
    .unwrap_or_default();

    let _ = write_cache(&Cache {
        checked_at_unix_secs: now_unix(),
        latest_version: latest.clone(),
    });

    latest.filter(|v| is_newer_than_current(v))
}

fn is_newer_than_current(v: &str) -> bool {
    v != env!("CARGO_PKG_VERSION")
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> std::path::PathBuf {
    config_dir().join(CACHE_FILE)
}

fn read_fresh_cache() -> Option<Cache> {
    let raw = std::fs::read_to_string(cache_path()).ok()?;
    let cache: Cache = serde_json::from_str(&raw).ok()?;
    let age = now_unix().saturating_sub(cache.checked_at_unix_secs);
    if age < CACHE_TTL_SECS {
        Some(cache)
    } else {
        None
    }
}

fn write_cache(c: &Cache) -> std::io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(CACHE_FILE), serde_json::to_string(c)?)
}

/// Compose the one-line notice we print in CLI mode. Kept here so the TUI
/// modal and the CLI banner stay in sync.
pub fn notice_for(version: &str) -> String {
    format!(
        "A newer version ({version}) of flute-webhook is available — run `flute-webhook update` to install."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opt_out_respects_config_flag() {
        let cfg = Config {
            auto_update_check: false,
            ..Config::default()
        };
        assert!(opt_out(&cfg));
    }

    #[test]
    fn current_version_is_not_considered_newer() {
        assert!(!is_newer_than_current(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn anything_other_than_current_counts_as_newer() {
        // is_newer_than_current is a simple string compare; semver ordering
        // is the GitHub API's job, not ours.
        assert!(is_newer_than_current("999.999.999"));
    }

    #[test]
    fn notice_contains_version_and_update_command() {
        let n = notice_for("9.9.9");
        assert!(n.contains("9.9.9"), "notice should embed the version: {n}");
        assert!(
            n.contains("flute-webhook update"),
            "notice should tell the user how to update: {n}"
        );
    }
}
