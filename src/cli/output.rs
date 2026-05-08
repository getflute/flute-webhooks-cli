//! Pretty-printers for CLI subcommand responses.
//!
//! Two output modes (selected via `--output`):
//!   - `OutputFormat::Json` — `serde_json::to_string_pretty` of the response.
//!   - `OutputFormat::Table` — hand-rolled aligned columns. Kept dep-free so
//!     we don't pull in `comfy-table` for a few CLI helpers.

use crate::api::models::{
    DeliveryLogDetailDto, GetWebhookEndpointDto, PingResponseDto, WebhookDeliveryLogStatus,
    WebhookEndpointStatus,
};
use crate::cli::OutputFormat;
use crate::domain::{DeliveryLog, EventTypeMeta};
use serde::Serialize;
use std::io::Write;

/// Print a JSON-serializable value if the format is Json. Returns true if
/// printed, so the caller can fall through to the Table branch.
fn maybe_print_json<T: Serialize + ?Sized>(value: &T, fmt: OutputFormat) -> anyhow::Result<bool> {
    if fmt == OutputFormat::Json {
        let s = serde_json::to_string_pretty(value)?;
        println!("{s}");
        return Ok(true);
    }
    Ok(false)
}

/// Trim a string to fit a column width — appends `…` if truncation happened.
fn fit(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= width {
        format!("{:<width$}", s, width = width)
    } else if width >= 1 {
        let kept: String = chars.into_iter().take(width.saturating_sub(1)).collect();
        format!("{kept}…")
    } else {
        String::new()
    }
}

pub fn print_endpoints(eps: &[GetWebhookEndpointDto], fmt: OutputFormat) -> anyhow::Result<()> {
    if maybe_print_json(eps, fmt)? {
        return Ok(());
    }
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "{}  {}  {}  {}",
        fit("ID", 36),
        fit("NAME", 24),
        fit("URL", 50),
        fit("STATUS", 8)
    )?;
    writeln!(out, "{}", "-".repeat(36 + 24 + 50 + 8 + 6))?;
    for ep in eps {
        let status = match ep.status {
            WebhookEndpointStatus::Active => "Active",
            WebhookEndpointStatus::Inactive => "Inactive",
        };
        writeln!(
            out,
            "{}  {}  {}  {}",
            fit(&ep.id, 36),
            fit(ep.name.as_deref().unwrap_or(""), 24),
            fit(ep.endpoint_url.as_deref().unwrap_or(""), 50),
            fit(status, 8)
        )?;
    }
    Ok(())
}

pub fn print_endpoint(ep: &GetWebhookEndpointDto, fmt: OutputFormat) -> anyhow::Result<()> {
    if maybe_print_json(ep, fmt)? {
        return Ok(());
    }
    println!("ID:        {}", ep.id);
    println!("Name:      {}", ep.name.as_deref().unwrap_or(""));
    println!("URL:       {}", ep.endpoint_url.as_deref().unwrap_or(""));
    println!("Status:    {:?}", ep.status);
    if let Some(types) = &ep.event_types {
        println!("Events:    {}", types.join(", "));
    }
    if let Some(t) = ep.created_on {
        println!("Created:   {t}");
    }
    if let Some(t) = ep.modified_on {
        println!("Modified:  {t}");
    }
    Ok(())
}

pub fn print_event_types(types: &[EventTypeMeta], fmt: OutputFormat) -> anyhow::Result<()> {
    if maybe_print_json(types, fmt)? {
        return Ok(());
    }
    let mut by_group: std::collections::BTreeMap<&str, Vec<&EventTypeMeta>> = Default::default();
    for et in types {
        by_group.entry(et.group.as_str()).or_default().push(et);
    }
    let mut out = std::io::stdout().lock();
    for (group, members) in by_group {
        writeln!(out, "[{group}]")?;
        for m in members {
            if m.description.is_empty() {
                writeln!(out, "  {}", m.name)?;
            } else {
                writeln!(out, "  {:<35}  {}", m.name, m.description)?;
            }
        }
    }
    Ok(())
}

pub fn print_delivery_logs(
    logs: &[DeliveryLog],
    total: Option<i32>,
    fmt: OutputFormat,
) -> anyhow::Result<()> {
    if fmt == OutputFormat::Json {
        // Wrap into a {items, total} envelope so json output mirrors the API.
        #[derive(Serialize)]
        struct Wrapper<'a> {
            items: &'a [DeliveryLog],
            total: Option<i32>,
        }
        let s = serde_json::to_string_pretty(&Wrapper { items: logs, total })?;
        println!("{s}");
        return Ok(());
    }
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "{}  {}  {}  {}  {}  {}",
        fit("TIMESTAMP", 19),
        fit("EVENT TYPE", 30),
        fit("STATUS", 8),
        fit("HTTP", 5),
        fit("DUR", 8),
        fit("ID", 36)
    )?;
    writeln!(out, "{}", "-".repeat(19 + 30 + 8 + 5 + 8 + 36 + 10))?;
    for l in logs {
        let status = match l.status {
            WebhookDeliveryLogStatus::Success => "Success",
            WebhookDeliveryLogStatus::Failure => "Failed",
        };
        let http = l
            .response_status_code
            .map(|s| s.to_string())
            .unwrap_or_else(|| "—".into());
        writeln!(
            out,
            "{}  {}  {}  {}  {}  {}",
            fit(&l.created_on.format("%Y-%m-%d %H:%M:%S").to_string(), 19),
            fit(&l.event_type, 30),
            fit(status, 8),
            fit(&http, 5),
            fit(&format!("{}ms", l.duration_ms), 8),
            fit(&l.id, 36)
        )?;
    }
    if let Some(t) = total {
        writeln!(out, "\n{} of {} total", logs.len(), t)?;
    }
    Ok(())
}

pub fn print_delivery_log(log: &DeliveryLogDetailDto, fmt: OutputFormat) -> anyhow::Result<()> {
    if maybe_print_json(log, fmt)? {
        return Ok(());
    }
    println!("ID:        {}", log.id);
    println!(
        "Endpoint:  {} ({})",
        log.webhook_name.as_deref().unwrap_or(""),
        log.webhook_endpoint_id
    );
    println!(
        "Event:     {} (id={})",
        log.event_type.as_deref().unwrap_or(""),
        log.event_id
    );
    println!(
        "Status:    {:?}  HTTP={:?}  Duration={}ms  Attempt={}",
        log.status, log.response_status_code, log.duration_ms, log.attempt_number
    );
    println!("Created:   {}", log.created_on);
    if let Some(err) = &log.error_message {
        println!("Error:     {err}");
    }
    println!();
    println!("--- Request headers ---");
    if let Some(h) = &log.request_headers {
        let mut keys: Vec<&String> = h.keys().collect();
        keys.sort();
        for k in keys {
            let v = h.get(k).cloned().flatten().unwrap_or_default();
            println!("  {k}: {v}");
        }
    }
    if let Some(body) = &log.request_body {
        println!("\n--- Request body ---\n{body}");
    }
    println!("\n--- Response ---");
    if let Some(code) = log.response_status_code {
        println!("HTTP {code}");
    } else {
        println!("(no response received)");
    }
    if let Some(body) = &log.response_body {
        println!("{body}");
    }
    Ok(())
}

pub fn print_ping(p: &PingResponseDto, fmt: OutputFormat) -> anyhow::Result<()> {
    if maybe_print_json(p, fmt)? {
        return Ok(());
    }
    println!("Success:   {}", p.success);
    if let Some(c) = p.status_code {
        println!("Status:    {c}");
    }
    println!("Duration:  {}ms", p.duration_ms);
    if let Some(e) = &p.error_message {
        println!("Error:     {e}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_pads_short_strings_to_width() {
        assert_eq!(fit("hi", 5), "hi   ");
    }

    #[test]
    fn fit_truncates_long_strings_with_ellipsis() {
        let out = fit("hello-world", 6);
        assert_eq!(out.chars().count(), 6);
        assert!(out.ends_with('…'));
        assert!(out.starts_with("hello"));
    }
}
