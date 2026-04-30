use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tracing::info;

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
