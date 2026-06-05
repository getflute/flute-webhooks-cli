//! Dispatch for the `flute-webhooks webhooks …` subcommands. Each handler
//! constructs a request from the parsed clap args, hits the live API via
//! `ApiClient`, and prints the response through the format-aware helpers in
//! `crate::cli::output`.

use crate::api::ApiClient;
use crate::api::models::{
    CreateWebhookEndpointRequest, UpdateWebhookEndpointRequest, WebhookEndpointStatus,
};
use crate::cli::output;
use crate::cli::{
    DeliveriesCommand, DeliveryStatusArg, EndpointStatusArg, EndpointsCommand, EventTypesCommand,
    OutputFormat, WebhooksCommand,
};
use crate::domain::{DeliveryLog, EventTypeMeta};
use anyhow::{Context, Result, anyhow};

pub async fn run(api: &ApiClient, fmt: OutputFormat, cmd: WebhooksCommand) -> Result<()> {
    match cmd {
        WebhooksCommand::Endpoints(c) => run_endpoints(api, fmt, c).await,
        WebhooksCommand::EventTypes(c) => run_event_types(api, fmt, c).await,
        WebhooksCommand::Deliveries(c) => run_deliveries(api, fmt, c).await,
    }
}

async fn run_endpoints(api: &ApiClient, fmt: OutputFormat, cmd: EndpointsCommand) -> Result<()> {
    match cmd {
        EndpointsCommand::List => {
            let resp = api.list_endpoints().await.context("list endpoints")?;
            let data = resp.data.unwrap_or_default();
            output::print_endpoints(&data, fmt)
        }
        EndpointsCommand::Get { id } => {
            let ep = api
                .get_endpoint(&id)
                .await
                .with_context(|| format!("get endpoint {id}"))?;
            output::print_endpoint(&ep, fmt)
        }
        EndpointsCommand::Create { url, events, name } => {
            if events.is_empty() {
                return Err(anyhow!(
                    "--events is required (e.g. --events transaction.card.captured,refund.completed)"
                ));
            }
            let req = CreateWebhookEndpointRequest {
                name: name.unwrap_or_else(|| "Untitled Webhook".into()),
                endpoint_url: url,
                event_types: events,
            };
            let resp = api.create_endpoint(&req).await.context("create endpoint")?;
            // Always emit JSON-pretty for create so the user gets the secret —
            // the table form would truncate it. The --output flag chooses
            // between `--output json` (full struct) and `--output table` which
            // we render as a labeled key/value block including the secret.
            if fmt == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("ID:        {}", resp.id);
                println!("Name:      {}", resp.name.as_deref().unwrap_or(""));
                println!("URL:       {}", resp.endpoint_url.as_deref().unwrap_or(""));
                println!("Status:    {:?}", resp.status);
                if let Some(types) = &resp.event_types {
                    println!("Events:    {}", types.join(", "));
                }
                if let Some(t) = resp.created_at {
                    println!("Created:   {t}");
                }
                println!();
                println!("⚠ Save the signing secret now — it will not be shown again.");
                println!(
                    "Secret:    {}",
                    resp.secret.as_deref().unwrap_or("(none returned)")
                );
            }
            Ok(())
        }
        EndpointsCommand::Update {
            id,
            url,
            events,
            name,
            status,
        } => {
            // Get-merge-put: GET the current state, overlay only the supplied
            // flags, PUT the merged version. Avoids accidentally clearing
            // event_types when the user only wanted to rename.
            let current = api
                .get_endpoint(&id)
                .await
                .with_context(|| format!("get endpoint {id}"))?;
            let merged = UpdateWebhookEndpointRequest {
                name: name.unwrap_or_else(|| current.name.clone().unwrap_or_default()),
                endpoint_url: url
                    .unwrap_or_else(|| current.endpoint_url.clone().unwrap_or_default()),
                status: match status {
                    Some(EndpointStatusArg::Active) => WebhookEndpointStatus::Active,
                    Some(EndpointStatusArg::Inactive) => WebhookEndpointStatus::Inactive,
                    None => current.status,
                },
                event_types: events
                    .unwrap_or_else(|| current.event_types.clone().unwrap_or_default()),
            };
            let resp = api
                .update_endpoint(&id, &merged)
                .await
                .with_context(|| format!("update endpoint {id}"))?;
            output::print_endpoint(&resp, fmt)
        }
        EndpointsCommand::Delete { id, yes } => {
            if !yes {
                use std::io::{self, BufRead, Write};
                print!("Delete endpoint {id}? Type 'yes' to confirm: ");
                io::stdout().flush().ok();
                let mut line = String::new();
                io::stdin()
                    .lock()
                    .read_line(&mut line)
                    .context("reading confirmation")?;
                if line.trim() != "yes" {
                    eprintln!("Aborted.");
                    return Err(anyhow!("delete cancelled"));
                }
            }
            api.delete_endpoint(&id)
                .await
                .with_context(|| format!("delete endpoint {id}"))?;
            if fmt == OutputFormat::Json {
                println!(r#"{{"deleted":"{id}"}}"#);
            } else {
                println!("Deleted endpoint {id}.");
            }
            Ok(())
        }
        EndpointsCommand::Ping { id } => {
            let resp = api
                .ping_endpoint(&id)
                .await
                .with_context(|| format!("ping endpoint {id}"))?;
            output::print_ping(&resp, fmt)
        }
    }
}

async fn run_event_types(api: &ApiClient, fmt: OutputFormat, cmd: EventTypesCommand) -> Result<()> {
    match cmd {
        EventTypesCommand::List => {
            let resp = api.list_event_types().await.context("list event types")?;
            let dtos = resp.data.unwrap_or_default();
            // JSON path serializes the wire DTOs directly so consumers see
            // the same field names (incl. `eventTypeId`) as the underlying
            // API. The table path uses the trimmed `EventTypeMeta` domain
            // type which is also what the TUI consumes.
            if fmt == OutputFormat::Json {
                let s = serde_json::to_string_pretty(&dtos)?;
                println!("{s}");
                return Ok(());
            }
            let metas: Vec<EventTypeMeta> = dtos.into_iter().map(EventTypeMeta::from).collect();
            output::print_event_types(&metas, fmt)
        }
    }
}

async fn run_deliveries(api: &ApiClient, fmt: OutputFormat, cmd: DeliveriesCommand) -> Result<()> {
    match cmd {
        DeliveriesCommand::List {
            endpoint_id,
            status,
            limit,
        } => {
            // Build the query string and hand it to ApiClient — that path goes
            // through send(), which gives us the 401 → refresh → retry loop and
            // structured ApiError on non-2xx responses.
            let query = build_deliveries_query(endpoint_id.as_deref(), status, limit);
            let resp = api
                .list_delivery_logs_query(&query)
                .await
                .context("list deliveries")?;
            let items_dto = resp.items.unwrap_or_default();
            // JSON path serializes the wire DTOs directly so each `items[N]`
            // uses the same field names (`deliveryLogId`, `deliveryAttemptStatus`,
            // `roundTripDurationMs`, etc.) as `webhooks deliveries get`. The
            // table path goes through the trimmed `DeliveryLog` domain type
            // shared with the TUI.
            if fmt == OutputFormat::Json {
                #[derive(serde::Serialize)]
                struct Wrapper<'a> {
                    items: &'a [crate::api::models::DeliveryLogSummaryDto],
                    total: Option<i32>,
                }
                let s = serde_json::to_string_pretty(&Wrapper {
                    items: &items_dto,
                    total: resp.total,
                })?;
                println!("{s}");
                return Ok(());
            }
            let logs: Vec<DeliveryLog> = items_dto.into_iter().map(DeliveryLog::from).collect();
            output::print_delivery_logs(&logs, resp.total, fmt)
        }
        DeliveriesCommand::Get { id } => {
            let detail = api
                .get_delivery_log(&id)
                .await
                .with_context(|| format!("get delivery log {id}"))?;
            output::print_delivery_log(&detail, fmt)
        }
        DeliveriesCommand::Retry { id } => {
            let resp = api
                .retry_delivery(&id)
                .await
                .with_context(|| format!("retry delivery {id}"))?;
            // Server returns a small JSON envelope — print as-is regardless of
            // format. Table mode just shows the JSON since there's no
            // domain-level Rust type for the retry response yet.
            if fmt == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Retry queued: {}", serde_json::to_string(&resp)?);
            }
            Ok(())
        }
    }
}

/// Build the `?pageSize=…&webhookId=…&status=…` query for the deliveries list.
///
/// Post-2026-06 rebrand the server expects `pageSize` (not `limit`) to cap
/// the response. The CLI flag is still `--limit` for historical ergonomics.
fn build_deliveries_query(
    endpoint_id: Option<&str>,
    status: Option<DeliveryStatusArg>,
    limit: u32,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("pageSize={limit}"));
    if let Some(id) = endpoint_id {
        parts.push(format!("webhookId={id}"));
    }
    if let Some(s) = status {
        let v = match s {
            DeliveryStatusArg::Success => "Success",
            DeliveryStatusArg::Failed => "Failure",
            DeliveryStatusArg::Pending => "Pending",
        };
        parts.push(format!("status={v}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deliveries_query_is_well_formed() {
        let q = build_deliveries_query(None, None, 25);
        assert_eq!(q, "?pageSize=25");

        let q = build_deliveries_query(Some("ep-1"), Some(DeliveryStatusArg::Success), 100);
        assert_eq!(q, "?pageSize=100&webhookId=ep-1&status=Success");

        // The API uses "Failure" (PascalCase) for the failed status value,
        // even though we expose `--status failed` for nicer ergonomics.
        let q = build_deliveries_query(None, Some(DeliveryStatusArg::Failed), 1);
        assert_eq!(q, "?pageSize=1&status=Failure");

        // Pending = in-flight retry; goes on the wire as the title-case
        // status value the server returns.
        let q = build_deliveries_query(None, Some(DeliveryStatusArg::Pending), 1);
        assert_eq!(q, "?pageSize=1&status=Pending");
    }
}
