//! Forward successful webhook delivery payloads from the Flute API to a
//! user-supplied local listener URL.
//!
//! Used in two paths:
//!  - One-shot manual: the user presses `t` on a Delivery Logs row.
//!  - Auto-forward: the toggle on the listener-config modal is ON, and the
//!    poller has produced a snapshot containing a new successful log.
//!
//! Forwarding is informational and best-effort. A listener that's offline,
//! returns a 4xx/5xx, or rejects the request must NOT block the rest of the
//! TUI — we log via tracing and move on.

use crate::api::ApiClient;
use anyhow::{anyhow, Result};
use reqwest::Client;
use tracing::{debug, warn};

/// Headers that don't make sense to copy verbatim onto a forwarded request:
/// they're tied to the original transport hop, not the payload.
const STRIPPED_HEADERS: &[&str] = &["host", "content-length", "connection", "transfer-encoding"];

/// Look up the full request headers + body for `log_id` and POST them to
/// `target_url`. The resulting POST mirrors what the Flute API originally
/// sent to the registered webhook endpoint, so a local listener sees an
/// equivalent payload (including the Aurora-Webhook-Signature header).
pub async fn forward_log(
    http: &Client,
    api: &ApiClient,
    log_id: &str,
    target_url: &str,
) -> Result<()> {
    if target_url.is_empty() {
        return Err(anyhow!("listener URL is empty"));
    }
    let detail = api.get_delivery_log(log_id).await?;
    let body = detail.request_body.unwrap_or_default();
    let mut req = http.post(target_url).body(body);
    if let Some(headers) = detail.request_headers {
        for (k, v_opt) in headers {
            if STRIPPED_HEADERS.iter().any(|h| h.eq_ignore_ascii_case(&k)) {
                continue;
            }
            if let Some(v) = v_opt {
                req = req.header(k, v);
            }
        }
    }
    match req.send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                debug!(log_id, target_url, status = status.as_u16(), "forwarded webhook delivery");
            } else {
                warn!(log_id, target_url, status = status.as_u16(), "listener returned non-2xx");
            }
        }
        Err(e) => {
            warn!(log_id, target_url, error = %e, "listener delivery failed");
        }
    }
    Ok(())
}
