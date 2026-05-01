use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::api::{ApiClient, ApiError};
use crate::config::POLL_BACKOFF_SECS;
use crate::domain::{aggregate_counts, DeliveryLog, Endpoint, EventTypeMeta};

/// Hard cap on backoff sleep so a long outage doesn't leave the user waiting hours.
const MAX_BACKOFF_SECS: u64 = 300; // 5 minutes
/// Cap on the doubling exponent so a u64 overflow can't happen and the schedule
/// reaches the cap predictably (2^6 = 64x base).
const BACKOFF_EXPONENT_CAP: u32 = 6;

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

/// Returns true for API errors where retrying immediately would just bombard a
/// failing endpoint. Specifically: 401, 403, 404, any 5xx, plus transport and
/// auth-store failures (the latter usually a network blip during token refresh).
/// Decode errors are NOT backoff-eligible — they signal a client-side parser
/// bug that won't fix itself.
fn is_backoff_eligible(err: &ApiError) -> bool {
    match err {
        ApiError::Api { status, .. } => matches!(*status, 401 | 403 | 404 | 500..=599),
        ApiError::Transport(_) | ApiError::Auth(_) => true,
        ApiError::Decode(_) => false,
    }
}

/// Compute the next sleep duration given the base interval and a count of
/// consecutive failures. Doubles per failure, capped at [`MAX_BACKOFF_SECS`].
fn backoff_seconds(base: Duration, consecutive_failures: u32) -> u64 {
    if consecutive_failures == 0 { return base.as_secs(); }
    let exp = consecutive_failures.min(BACKOFF_EXPONENT_CAP);
    base.as_secs()
        .saturating_mul(2_u64.pow(exp))
        .min(MAX_BACKOFF_SECS)
}

struct PollError {
    message: String,
    backoff_eligible: bool,
}

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

        let mut consecutive_failures: u32 = 0;
        loop {
            let mode = *cadence_rx.borrow();
            let base_interval = current_interval(mode, configured_secs);

            match poll_once(&api, &event_types).await {
                Ok(snap) => {
                    consecutive_failures = 0;
                    let _ = tx.send(PollerEvent::Snapshot(snap)).await;
                }
                Err(pe) => {
                    if pe.backoff_eligible {
                        consecutive_failures = consecutive_failures.saturating_add(1);
                    }
                    let _ = tx.send(PollerEvent::Error(pe.message)).await;
                }
            }

            let sleep_secs = backoff_seconds(base_interval, consecutive_failures);
            if consecutive_failures > 0 {
                warn!(
                    "poll failed (consecutive={consecutive_failures}); backing off to {sleep_secs}s"
                );
            }
            let interval = Duration::from_secs(sleep_secs);

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

async fn poll_once(api: &ApiClient, event_types: &[EventTypeMeta]) -> Result<Snapshot, PollError> {
    let endpoints_resp = api.list_endpoints().await.map_err(|e| PollError {
        backoff_eligible: is_backoff_eligible(&e),
        message: format!("endpoints: {e}"),
    })?;
    let logs_resp = api.list_delivery_logs(500).await.map_err(|e| PollError {
        backoff_eligible: is_backoff_eligible(&e),
        message: format!("logs: {e}"),
    })?;

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

    #[test]
    fn backoff_eligible_for_listed_statuses() {
        let api = |status: u16| ApiError::Api { status, correlation_id: None, message: String::new() };
        assert!(is_backoff_eligible(&api(401)));
        assert!(is_backoff_eligible(&api(403)));
        assert!(is_backoff_eligible(&api(404)));
        assert!(is_backoff_eligible(&api(500)));
        // Other 5xx behave the same — server is having a moment.
        assert!(is_backoff_eligible(&api(502)));
        assert!(is_backoff_eligible(&api(503)));
        // 4xx outside the listed set should NOT trigger backoff (e.g. 400 = client bug).
        assert!(!is_backoff_eligible(&api(400)));
        assert!(!is_backoff_eligible(&api(422)));
    }

    #[test]
    fn backoff_eligible_for_auth_and_decode() {
        // Auth/transport are flaky-network-eligible; Decode is a parser bug.
        assert!(is_backoff_eligible(&ApiError::Auth("token fetch failed".into())));
        assert!(!is_backoff_eligible(&ApiError::Decode("bad json".into())));
    }

    #[test]
    fn backoff_seconds_doubles_then_caps() {
        let base = Duration::from_secs(5);
        assert_eq!(backoff_seconds(base, 0), 5);   // no failures: base interval
        assert_eq!(backoff_seconds(base, 1), 10);  // 5 * 2
        assert_eq!(backoff_seconds(base, 2), 20);  // 5 * 4
        assert_eq!(backoff_seconds(base, 3), 40);
        assert_eq!(backoff_seconds(base, 4), 80);
        assert_eq!(backoff_seconds(base, 5), 160);
        assert_eq!(backoff_seconds(base, 6), MAX_BACKOFF_SECS); // 5 * 64 = 320, capped at 300
        assert_eq!(backoff_seconds(base, 100), MAX_BACKOFF_SECS); // saturates
    }

    #[test]
    fn backoff_seconds_respects_larger_base_interval() {
        // If user configured 60 s, even one failure should be 120 s — but cap at MAX.
        let base = Duration::from_secs(60);
        assert_eq!(backoff_seconds(base, 1), 120);
        assert_eq!(backoff_seconds(base, 3), MAX_BACKOFF_SECS); // 60 * 8 = 480, capped
    }
}
