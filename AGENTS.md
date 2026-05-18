# AGENTS.md — running `flute-webhook` from an AI agent

This file documents the machine-readable contract for autonomous agents (Claude Code, GPT function-calling subprocesses, Cursor, custom orchestrators). Humans should read `readme.md` instead.

## TL;DR

```bash
# Auth (one-time, before any other command)
FLUTE_CLIENT_ID=… FLUTE_CLIENT_SECRET=… flute-webhook --output json webhooks endpoints list

# Or persist credentials in the OS keychain once, then drop the env vars:
flute-webhook auth login                       # interactive prompt — not agent-friendly
flute-webhook --output json webhooks endpoints list
```

Every non-TUI subcommand accepts `--output json`. On success the response body is pretty-printed JSON on **stdout**. On failure a structured error envelope (see below) is printed to **stdout** and the process exits non-zero — agents parse one stream, never both.

## Output contract

### Success

| Command | Stdout shape |
|---|---|
| `webhooks endpoints list` | `[GetWebhookEndpointDto, …]` — bare JSON array |
| `webhooks endpoints get <id>` | `GetWebhookEndpointDto` |
| `webhooks endpoints create …` | `CreateWebhookEndpointResponse` (includes one-shot `secret` field — store it; the API never returns it again) |
| `webhooks endpoints update <id> …` | `GetWebhookEndpointDto` (the merged state after PUT) |
| `webhooks endpoints delete <id> --yes` | Under `--output json`: empty stdout, exit 0. Under `--output table` (default): human-readable `Deleted endpoint <id>.` line. Agents should always pass `--output json`. |
| `webhooks endpoints ping <id>` | `PingResponseDto { success, status_code, duration_ms, error_message? }` |
| `webhooks event-types list` | `[EventTypeDto, …]` — bare JSON array |
| `webhooks deliveries list …` | `ListDeliveryLogsDto { items, total }` |
| `webhooks deliveries get <id>` | `DeliveryLogDetailDto` (full request + response bodies) |
| `webhooks deliveries retry <id>` | `DeliveryRetryResponseDto { attemptNumber, eventId, eventType, status, webhookEndpointId }` — a distinct shape, not `DeliveryLogSummaryDto`. No `id` field. |
| `auth token` | bearer JWT as a single line of text — useful for `curl` smoke tests, not JSON |
| `update` | text status line; exit 0 = up-to-date or updated successfully |

Field types are defined in [`src/api/models.rs`](src/api/models.rs).

### Wire-format casing

The casing is **not uniform across surfaces** — DTOs need to be modeled per response:

| Surface | Casing | Example fields |
|---|---|---|
| `endpoints` (list / get / create / update) | camelCase | `endpointUrl`, `eventTypes`, `createdOn`, `modifiedOn` |
| `deliveries list` items (`DeliveryLogSummaryDto`) | snake_case | `endpoint_id`, `event_id`, `event_type`, `attempt_number`, `response_status_code`, `created_on` |
| `deliveries get` (`DeliveryLogDetailDto`) | camelCase | `webhookEndpointId`, `eventType`, `requestBody`, `responseBody`, `nextRetryAt` |
| `deliveries retry` (`DeliveryRetryResponseDto`) | camelCase | `attemptNumber`, `eventId`, `eventType`, `webhookEndpointId` |
| `event-types` list | camelCase-ish (all single-word fields) | `name`, `description`, `group` |
| Error envelope (any failure) | snake_case | `kind`, `message`, `status`, `correlation_id` |

### Status enum values

Returned values are **title-case** while filter inputs are **lowercase**. Agents must map both directions explicitly:

| Surface | Filter values (CLI input) | Returned values (server) |
|---|---|---|
| `endpoints.status` | `active`, `inactive` (via `--status`) | `"Active"`, `"Inactive"` |
| `deliveries.status` | `success`, `failed` (via `--status`) | `"Success"`, `"Failure"`, `"Pending"` (the latter for newly-scheduled retries) |

A case-insensitive comparison handles `success ↔ Success` but NOT `failed ↔ Failure` — that pair needs an explicit table.

### Failure (under `--output json`)

```json
{
  "kind": "api" | "transport" | "auth" | "decode" | "client",
  "message": "human-readable reason",
  "status": 422,                       // present only when kind="api"
  "correlation_id": "abc-123"          // present only when kind="api" and the server returned one
}
```

Process exit code is `1` on every failure path. The plain-text anyhow dump on stderr is suppressed under `--output json` so the agent's JSON parser doesn't see mixed streams.

Branch on `kind` first, then `status` for retry/backoff decisions:

- `"api"` + `status ∈ {500, 502, 503, 504}` → transient; safe to retry with backoff.
- `"api"` + `status ∈ {401, 403}` → auth state is broken; do **not** retry the same call. Re-issue `auth login` (or refresh credentials) and try once more.
- `"api"` + `status ∈ {400, 404, 409, 422}` → permanent for this request shape; surface to the operator with the `correlation_id`.
- `"transport"` → connection failure; retry with backoff.
- `"auth"` → keychain or OAuth handshake failed; needs operator intervention (no credentials configured).
- `"decode"` → bug in this CLI or a server contract change; surface for investigation.
- `"client"` → bad CLI args, unknown profile, or **client-side input validation failed** (e.g. non-UUID id passed to `delete`/`get`, non-HTTPS URL passed to `create`, retry against a synthetic delivery). Often a programming error in the agent's invocation, but sometimes a data constraint the operator must reconcile. Inspect `message` for the specific validation that fired.

## Idempotency

| Subcommand | Safe to retry? | Notes |
|---|---|---|
| `endpoints list` / `get` | yes | pure read |
| `endpoints create` | **no** | duplicates create a second endpoint. Check `list` first if recovering from an ambiguous timeout. |
| `endpoints update` | yes | full-state PUT — the CLI re-GETs, merges, and re-PUTs every call. |
| `endpoints delete` | yes | second call returns 404; treat as idempotent success. |
| `endpoints ping` | yes | one-shot HTTP test, no side effect on Flute. |
| `event-types list` | yes | pure read |
| `deliveries list` / `get` | yes | pure read |
| `deliveries retry` | **no** | each call schedules an additional retry attempt. Check the latest log via `deliveries get` before retrying again. **Ping-event deliveries are rejected**: the server returns `kind:"client"` + `"Ping deliveries are synthetic and not retryable"`. Filter `event_type != "ping"` before retrying. |

## Authentication for agents

The agent-friendly path bypasses the OS keychain entirely via env vars:

```bash
FLUTE_PROFILE=uat \
FLUTE_CLIENT_ID=… \
FLUTE_CLIENT_SECRET=… \
flute-webhook --output json webhooks endpoints list
```

These env vars are checked by `auth::keychain::load_with_env_fallback` before the keychain. They're the recommended path for any non-interactive caller (CI, agent runtime, container). The keychain path requires an interactive `auth login` first and depends on platform-specific session state — fragile for agents.

A bearer token is fetched automatically from `oauth_url` on demand, cached for the advertised TTL (minus a 60 s safety margin), and refreshed once on a 401. The agent does not see or need to handle tokens directly.

## Profiles and global flags

- `--profile uat` (default) or `--profile production` (alias `prod`)
- `--output json` — see Output contract above
- `--debug` — verbose HTTP traces. For agents, prefer `--output json` and parse `correlation_id` from the error envelope; only set `--debug` when an operator is investigating a specific failure.

## Common intents → commands

| Intent | Command | Notes |
|---|---|---|
| "What webhook endpoints exist?" | `webhooks endpoints list` | |
| "Create a webhook for transaction events" | `webhooks endpoints create --url <URL> --events transaction.card.captured,refund.completed --name "<name>"` | **URL must be HTTPS.** `http://` (including `http://localhost` / `http://127.0.0.1`) is rejected with `kind:"client"` + validation error. Use an HTTPS tunneling service (ngrok, cloudflared) for local development. |
| "Pause this endpoint" | `webhooks endpoints update <id> --status inactive` | |
| "Delete this endpoint" | `webhooks endpoints delete <id> --yes` | The `--yes` flag is required — no interactive prompt. Always pair with `--output json` if you want machine-parseable success (empty stdout); table mode prints a confirmation line. |
| "Is this endpoint reachable?" | `webhooks endpoints ping <id>` | Returns `success: bool` + `status_code`. |
| "Show the last 50 deliveries" | `webhooks deliveries list --limit 50` | `--limit` is sent to the server but may not be honored — agents should not rely on the exact returned count. Total available is in `total`. |
| "Show the failures for a given endpoint" | `webhooks deliveries list --endpoint-id <id> --status failed` | Filter input is lowercase; returned items have `status: "Failure"`. |
| "Inspect a specific delivery's payload" | `webhooks deliveries get <id>` | |
| "Re-send a failed delivery" | `webhooks deliveries retry <id>` | Skip ping-event deliveries — server rejects them. |
| "What event types can I subscribe to?" | `webhooks event-types list` | |

## Things to avoid

- **Don't invoke the TUI from an agent.** `flute-webhook tui` enters an alternate-screen ratatui loop with no JSON surface.
- **Don't combine `--output json` with `auth login` or `listen`.** Those are interactive/long-running and don't emit JSON.
- **Don't rely on stderr.** The structured error envelope is on stdout. Stderr may contain tracing output, update notices, or anyhow debug formatting depending on flags.
- **Don't poll faster than 5 s.** The configured floor and adaptive backoff exist for a reason; agent loops should respect `poll_interval_seconds` in `~/.flute/config.toml` (default 5).
- **Don't pass `http://` URLs to `endpoints create`** — the API requires HTTPS.
- **Don't retry ping deliveries** — they're synthetic and the server refuses them.

## See also

- [`readme.md`](readme.md) — human-readable overview
- [`src/api/models.rs`](src/api/models.rs) — wire-format DTO definitions
- [`src/cli/mod.rs`](src/cli/mod.rs) — clap subcommand tree (source of truth for flags)
