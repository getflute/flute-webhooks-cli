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
