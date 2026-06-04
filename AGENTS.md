# AGENTS.md — running `flute-webhooks-cli` from an AI agent

This file documents the machine-readable contract for autonomous agents (Claude Code, GPT function-calling subprocesses, Cursor, custom orchestrators). Humans should read `readme.md` instead.

## TL;DR

```bash
# Auth (one-time, before any other command)
FLUTE_CLIENT_ID=… FLUTE_CLIENT_SECRET=… flute-webhooks-cli --output json webhooks endpoints list

# Or persist credentials in the OS keychain once, then drop the env vars:
flute-webhooks-cli auth login                       # interactive prompt — not agent-friendly
flute-webhooks-cli --output json webhooks endpoints list
```

Every non-TUI subcommand accepts `--output json`. On success the response body is pretty-printed JSON on **stdout**. On failure a structured error envelope (see below) is printed to **stdout** and the process exits non-zero — agents parse one stream, never both.

## Output contract

### Success

| Command | Stdout shape |
|---|---|
| `webhooks endpoints list` | `[GetWebhookEndpointDto, …]` — bare JSON array |
| `webhooks endpoints get <id>` | `GetWebhookEndpointDto` |
| `webhooks endpoints create …` | `CreateWebhookEndpointResponse` (includes one-shot `hmacSecret` field — store it; the API never returns it again) |
| `webhooks endpoints update <id> …` | `GetWebhookEndpointDto` (the merged state after PUT) |
| `webhooks endpoints delete <id> --yes` | Under `--output json`: empty stdout, exit 0. Under `--output table` (default): human-readable `Deleted endpoint <id>.` line. Agents should always pass `--output json`. |
| `webhooks endpoints ping <id>` | `PingResponseDto { success, statusCode, roundTripDurationMs, errorMessage? }` |
| `webhooks event-types list` | Bare JSON array of `{name, description, group}` objects. The wire `eventTypeId` is stripped during conversion — match event types by `name`, not numeric id. |
| `webhooks deliveries list …` | `{ items: [DeliveryLog…], total }`. Items use the Rust **domain** form (snake_case) — see Casing table. |
| `webhooks deliveries get <id>` | `DeliveryLogDetailDto` (full request + response bodies) |
| `webhooks deliveries retry <id>` | `DeliveryRetryResponseDto { attemptNumber, eventId, eventType, status, webhookEndpointId }` — a distinct shape, not `DeliveryLogSummaryDto`. No `id` field. |
| `auth token` | bearer JWT as a single line of text — useful for `curl` smoke tests, not JSON |
| `update` | text status line; exit 0 = up-to-date or updated successfully |

Field types are defined in [`src/api/models.rs`](src/api/models.rs).

### Wire-format casing

The casing is **not uniform across surfaces** — DTOs need to be modeled per response:

| Surface | Casing | Example fields |
|---|---|---|
| `endpoints` (list / get / create / update) | camelCase, namespaced | `endpointId`, `webhookName`, `endpointUrl`, `eventTypes`, `status`, `createdOn`, `modifiedOn` |
| `endpoints create` response | camelCase | adds `hmacSecret` (one-shot; the API only returns it on the create call) |
| `deliveries list` items | snake_case (Rust domain form) | `id`, `endpoint_id`, `endpoint_name`, `endpoint_url`, `event_id`, `event_type`, `status`, `attempt_number`, `response_status_code`, `duration_ms`, `error_message`, `created_on` |
| `deliveries get` (`DeliveryLogDetailDto`) | camelCase, namespaced | `deliveryLogId`, `webhookEndpointId`, `webhookName`, `endpointUrl`, `eventId`, `eventType`, `deliveryAttemptStatus`, `attemptNumber`, `responseStatusCode`, `roundTripDurationMs`, `requestBody`, `responseBody`, `nextRetryAt` |
| `deliveries retry` (`DeliveryRetryResponseDto`) | camelCase | `attemptNumber`, `eventId`, `eventType`, `status`, `webhookEndpointId` (no `id` field) |
| `event-types` list | bare lowercase | `name`, `description`, `group` (no id) |
| `ping` response | camelCase | `success`, `statusCode`, `roundTripDurationMs`, `errorMessage` |
| Error envelope (any failure) | snake_case | `kind`, `message`, `status`, `correlation_id` |

Two surfaces describe the same logical entity (delivery log) but emit different field names depending on whether you used `list` (domain form, snake_case `id`/`status`/`duration_ms`) or `get` (wire form, camelCase `deliveryLogId`/`deliveryAttemptStatus`/`roundTripDurationMs`). Plan to either branch on the command or unwrap to the wire fields manually. Tracked as a follow-up to unify.

### Status enum values

Returned values are **title-case** while filter inputs are **lowercase**. Agents must map both directions explicitly:

| Surface | Filter values (CLI input) | Returned values (server) |
|---|---|---|
| `endpoints.status` | `active`, `inactive` (via `--status`) | `"Active"`, `"Inactive"` |
| `deliveries.status` | `success`, `failed`, `pending` (via `--status`) | `"Success"`, `"Failure"`, `"Pending"` (the last for newly-scheduled retries) |

A case-insensitive comparison handles `success ↔ Success` and `pending ↔ Pending` but NOT `failed ↔ Failure` — that pair needs an explicit table.

### Pagination cap on `deliveries list`

The Flute server caps `pageSize` at **100**. The CLI accepts `--limit N` up to any value, but anything > 100 returns `{ "kind": "api", "status": 400, "message": "Validation failed: PageSize must be 100 or less." }`. Agents that want more than 100 rows currently need to call `deliveries list` in pages (the CLI does not yet expose pagination cursors — separate follow-up).

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
FLUTE_PROFILE=sandbox \
FLUTE_CLIENT_ID=… \
FLUTE_CLIENT_SECRET=… \
flute-webhooks-cli --output json webhooks endpoints list
```

These env vars are checked by `auth::keychain::load_with_env_fallback` before the keychain. They're the recommended path for any non-interactive caller (CI, agent runtime, container). The keychain path requires an interactive `auth login` first and depends on platform-specific session state — fragile for agents.

A bearer token is fetched automatically from `oauth_url` on demand, cached for the advertised TTL (minus a 60 s safety margin), and refreshed once on a 401. The agent does not see or need to handle tokens directly.

## Profiles and global flags

- `--profile sandbox` (default) or `--profile production` (alias `prod`)
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

- **Don't invoke the TUI from an agent.** `flute-webhooks-cli tui` enters an alternate-screen ratatui loop with no JSON surface.
- **Don't combine `--output json` with `auth login` or `listen`.** Those are interactive/long-running and don't emit JSON.
- **Don't rely on stderr.** The structured error envelope is on stdout. Stderr may contain tracing output, update notices, or anyhow debug formatting depending on flags.
- **Don't poll faster than 5 s.** The configured floor and adaptive backoff exist for a reason; agent loops should respect `poll_interval_seconds` in `~/.flute/config.toml` (default 5).
- **Don't pass `http://` URLs to `endpoints create`** — the API requires HTTPS.
- **Don't retry ping deliveries** — they're synthetic and the server refuses them.

## See also

- [`readme.md`](readme.md) — human-readable overview
- [`src/api/models.rs`](src/api/models.rs) — wire-format DTO definitions
- [`src/cli/mod.rs`](src/cli/mod.rs) — clap subcommand tree (source of truth for flags)
