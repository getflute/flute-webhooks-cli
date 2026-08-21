# AGENTS.md — running `flute-webhooks` from an AI agent

This file documents the machine-readable contract for autonomous agents (Claude Code, GPT function-calling subprocesses, Cursor, custom orchestrators). Humans should read `readme.md` instead.

## TL;DR

```bash
# Auth (one-time, before any other command)
FLUTE_CLIENT_ID=… FLUTE_CLIENT_SECRET=… flute-webhooks --output json webhooks endpoints list

# Or persist credentials in the OS keychain once, then drop the env vars:
flute-webhooks auth login                       # interactive prompt — not agent-friendly
flute-webhooks --output json webhooks endpoints list
```

Every non-TUI subcommand accepts `--output json`. For `webhooks …` subcommands, success emits pretty-printed JSON on **stdout** (per-command shape listed below; `auth keys` and `update` are text-only — see the table). On failure a structured error envelope (see below) is printed to **stdout** and the process exits non-zero — agents parse one stream, never both.

> **v0.7.0 note.** The Flute v2 webhooks surface underwent a spec-conformance pass (ARISE-4204 / -4321 / -4319) that renamed most wire fields and moved both `endpoints list` and `deliveries list` onto a paginated `{ items, pageInfo }` envelope. If you have code targeting v0.6.x, see the "Migration from v0.6.x" section at the bottom before parsing.

## Output contract

### Success

| Command | Stdout shape |
|---|---|
| `webhooks endpoints list` | `PagedResponse<GetWebhookEndpointDto>` — `{ items: [...], pageInfo: {...} }`. The CLI sends `pageSize=100` per call; check `pageInfo.hasMore` for accounts with more than 100 endpoints. |
| `webhooks endpoints get <id>` | `GetWebhookEndpointDto` |
| `webhooks endpoints create …` | `CreateWebhookEndpointResponse` (includes one-shot `hmacSecret` field — store it; the API never returns it again) |
| `webhooks endpoints update <id> …` | `GetWebhookEndpointDto` (the endpoint state after the PATCH is applied). CLI sends **PATCH** with a JSON Merge Patch (RFC 7396) body — only the fields the caller passed on the command line appear in the wire body; every other field is left unchanged server-side. |
| `webhooks endpoints delete <id> --yes` | Under `--output json`: `{"deleted":"<id>"}` on success, exit 0. Under `--output table` (default): human-readable `Deleted endpoint <id>.` line. Agents should always pass `--output json`. |
| `webhooks endpoints ping <id>` | `PingResponseDto { isDelivered, endpointHTTPResponseCode, roundTripDurationMs, errorMessage? }` |
| `webhooks event-types list` | Bare JSON array of `EventTypeDto` objects: `{eventType, description, group}`. Match by the wire-format `eventType` string. |
| `webhooks deliveries list …` | `PagedResponse<DeliveryLogSummaryDto>` — `{ items: [...], pageInfo: {...} }`. Items use the same wire field names as `deliveries get` — agents can reuse field paths between the two calls. |
| `webhooks deliveries get <id>` | `DeliveryLogDetailDto` (full request + response bodies) |
| `webhooks deliveries retry <id>` | `DeliveryLogDetailDto` — the same shape as `deliveries get`. HTTP 200. Represents the new delivery attempt's log record with full request + response bodies. |
| `auth keys` | bearer JWT as a single line of text — useful for `curl` smoke tests, not JSON. `auth token` is a deprecated hidden alias that still works. |
| `update` | text status line; exit 0 = up-to-date or updated successfully |

Field types are defined in [`src/api/models.rs`](src/api/models.rs).

### PageInfo envelope (used by `endpoints list` and `deliveries list`)

```json
{
  "items": [ ... ],
  "pageInfo": {
    "pageIndex": 0,      // zero-based, echoed from request
    "pageSize": 20,      // echoed from request; CLI sends 100 for endpoints list, --limit N for deliveries list
    "totalItems": 17,    // across all pages
    "totalPages": 1,
    "hasMore": false     // true when at least one more page exists
  }
}
```

### Wire-format casing

Everything is camelCase. The v0.7.0 spec pass normalized field naming across surfaces, so most identifiers are now uniform:

| Surface | Example fields |
|---|---|
| `endpoints` (list items / get / update) | `endpointId`, `endpointName`, `endpointUrl`, `eventTypes`, `endpointStatus`, `createdOn`, `modifiedOn` |
| `endpoints create` response | adds `hmacSecret` (one-shot; the API only returns it on the create call) and returns `createdOn` (not `createdAt`) |
| `deliveries list` items (`DeliveryLogSummaryDto`) | `deliveryLogId`, `endpointId`, `endpointName`, `endpointUrl`, `eventId`, `eventType`, `deliveryLogStatus`, `attemptNumber`, `endpointHTTPResponseCode`, `roundTripDurationMs`, `errorMessage`, `createdOn` |
| `deliveries get` (`DeliveryLogDetailDto`) | superset of the summary: adds `requestHeaders`, `requestBody`, `responseHeaders`, `responseBody`, `nextRetryAt` |
| `deliveries retry` | Returns `DeliveryLogDetailDto` (same shape as `deliveries get`). No custom retry envelope. |
| `event-types` list (`EventTypeDto`) | `eventType`, `description`, `group` (no numeric `eventTypeId` — the wire dropped it in ARISE-4204; match by the string `eventType`) |
| `ping` response | `isDelivered`, `endpointHTTPResponseCode`, `roundTripDurationMs`, `errorMessage` |
| `pageInfo` (in `endpoints list` and `deliveries list`) | `pageIndex`, `pageSize`, `totalItems`, `totalPages`, `hasMore` |
| Error envelope (any failure) | `kind`, `message`, `status`, `correlation_id` — snake_case, unchanged |

`deliveries list` items and `deliveries get` share every field name — agents can call `list` then `get(item.deliveryLogId)` and reuse field paths across both responses.

Note the odd casing of `endpointHTTPResponseCode` — `HTTP` is preserved uppercase (three letters), while the rest is camelCase. Serde `rename_all = "camelCase"` won't produce this pattern; the CLI's DTOs pin it explicitly with `#[serde(rename)]`.

### Status enum values

Returned values are **title-case** while filter inputs are **lowercase**. Agents must map both directions explicitly:

| Surface | Filter values (CLI input) | Wire query-string key | Returned values (server) |
|---|---|---|---|
| `endpoints.endpointStatus` | `active`, `inactive` (via `--status`) | `endpointStatus` | `"Active"`, `"Inactive"` |
| `deliveries.deliveryLogStatus` | `success`, `failed` (via `--status`) | `deliveryLogStatus` | `"Success"`, `"Failure"` |

A case-insensitive comparison handles `success ↔ Success` but NOT `failed ↔ Failure` — that pair needs an explicit table.

### Pagination cap on `deliveries list`

The Flute server caps `pageSize` at **100**. The CLI accepts `--limit N` up to any value, but anything > 100 returns `{ "kind": "api", "status": 400, "message": "Validation failed: PageSize must be 100 or less." }`. Agents that want more than 100 rows currently need to call `deliveries list` in pages using an updated CLI that surfaces `pageIndex` — the v0.7.0 CLI does not yet expose it (`--limit` maps to `pageSize` only; page index defaults to 0). Use `pageInfo.hasMore` to detect the tail.

### Event-type catalog additions

Both cohorts of new events use the same reference-only delivered envelope: `{ id, data.object.{id, resourceType}, type, created, apiVersion }`. The receiver refetches the resource by its id for the full outcome. The Flute server emits that envelope to your endpoint — this CLI is not involved in the delivery, only in subscription management.

| Added in | Event type | Group | Refetch endpoint |
|---|---|---|---|
| ARISE-3639 (v0.7.0) | `payment_session.created` | Payment Sessions | `GET /v2/payment-sessions/{id}` |
| ARISE-3639 (v0.7.0) | `payment_session.completed` | Payment Sessions | `GET /v2/payment-sessions/{id}` |
| ARISE-4501 (v0.7.2) | `payment_link.created` | Payment Links | `GET /v2/payment-links/{id}` |
| ARISE-4501 (v0.7.2) | `payment_link.updated` | Payment Links | `GET /v2/payment-links/{id}` |

Subscribe like any other event:

```bash
flute-webhooks webhooks endpoints create \
  --url https://example.com/hook \
  --events payment_session.completed,payment_link.created,payment_link.updated \
  --name "Checkout + links listener"
```

Note on payment-link paid flows (per the ARISE-4501 PR): a paid **single-use** link flips to `Completed` **without** emitting `payment_link.updated` — the status change is a consequence of payment and is already announced via the corresponding `payment_session.completed` and `transaction.*` webhooks. Session→link linkage is resolvable server-side via `paymentLinkId` on the payment session; transaction→link linkage via `source.sourceId` on the transaction.

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
- `"client"` → bad CLI args (clap parse failure), unknown profile, or a CLI-side precondition that fired before any HTTP request. Current preconditions: `endpoints create` rejects an empty `--events` list, and `endpoints delete` requires `--yes` under `--output json` or when stdin is not a TTY (an interactive prompt would corrupt the JSON stream or hang a headless caller). The CLI does **not** locally validate UUID format, HTTPS URL shape, or "retryable" delivery status — those checks happen server-side and surface as `kind:"api"` with the server's validation `message`. Treat `kind:"client"` as a programming error in the agent's invocation; `kind:"api"` carries the operator-actionable diagnostic.

## Idempotency

| Subcommand | Safe to retry? | Notes |
|---|---|---|
| `endpoints list` / `get` | yes | pure read |
| `endpoints create` | **no** | duplicates create a second endpoint. Check `list` first if recovering from an ambiguous timeout. |
| `endpoints update` | yes | PATCH with a JSON Merge Patch (RFC 7396) body containing only user-supplied fields; the server merges. Repeated calls converge on the same state — safe to retry after an ambiguous timeout. |
| `endpoints delete` | yes, with caveat | The first call returns 204 + `{"deleted":"<id>"}`. A second call against the same id surfaces `kind:"api"` `status:404` — the CLI does **not** swallow the 404 into a success. Agents that want at-least-once idempotency should branch: treat `kind:"api"` + `status:404` on a delete as already-gone. |
| `endpoints ping` | yes | one-shot HTTP test, no side effect on Flute. |
| `event-types list` | yes | pure read |
| `deliveries list` / `get` | yes | pure read |
| `deliveries retry` | **no** | each call schedules an additional retry attempt. Check the latest log via `deliveries get` before retrying again. The server rejects retries against (a) ping-event deliveries with `"Ping deliveries are synthetic and not retryable"` and (b) already-Success deliveries with `"Cannot retry delivery log … — status is Success, only Failure is retryable."` Both surface as `kind:"api"` `status:400`. Filter to `deliveryLogStatus == "Failure"` and `eventType != "ping"` before retrying. |

## Authentication for agents

The agent-friendly path bypasses the OS keychain entirely via env vars:

```bash
FLUTE_PROFILE=sandbox \
FLUTE_CLIENT_ID=… \
FLUTE_CLIENT_SECRET=… \
flute-webhooks --output json webhooks endpoints list
```

These env vars are checked by `auth::keychain::load_with_env_fallback` before the keychain. They're the recommended path for any non-interactive caller (CI, agent runtime, container). The keychain path requires an interactive `auth login` first and depends on platform-specific session state — fragile for agents.

A bearer token is fetched automatically from `oauth_url` on demand, cached for the advertised TTL (minus a 60 s safety margin), and refreshed once on a 401. The agent does not see or need to handle tokens directly.

## Profiles and global flags

- `--profile sandbox` (default) or `--profile production` (alias `prod`)
- `--output json` — see Output contract above
- `--debug` — verbose HTTP traces. For agents, prefer `--output json` and parse `correlation_id` from the error envelope; only set `--debug` when an operator is investigating a specific failure. `hmacSecret` values are redacted in `--debug` body logs.

## Common intents → commands

| Intent | Command | Notes |
|---|---|---|
| "What webhook endpoints exist?" | `webhooks endpoints list` | Returns `{ items, pageInfo }`; iterate `items`. `pageInfo.hasMore == true` means an account with more than 100 endpoints. |
| "Create a webhook for transaction events" | `webhooks endpoints create --url <URL> --events transaction.card.captured,transaction.card.refunded --name "<name>"` | **URL must be HTTPS.** `http://` (including `http://localhost` / `http://127.0.0.1`) is rejected server-side with `kind:"api"` + validation error. Use an HTTPS tunneling service (ngrok, cloudflared) for local development. |
| "Pause this endpoint" | `webhooks endpoints update <id> --status inactive` | Wire field is `endpointStatus`; CLI flag is `--status`. |
| "Delete this endpoint" | `webhooks endpoints delete <id> --yes` | The `--yes` flag is required — no interactive prompt. Always pair with `--output json` if you want machine-parseable success. |
| "Is this endpoint reachable?" | `webhooks endpoints ping <id>` | Response fields are `isDelivered: bool` and `endpointHTTPResponseCode: int?`. |
| "Show the last 50 deliveries" | `webhooks deliveries list --limit 50` | Response is `{ items, pageInfo }`. Total available is `pageInfo.totalItems`. |
| "Show the failures for a given endpoint" | `webhooks deliveries list --endpoint-id <id> --status failed` | Filter input is lowercase; returned items have `deliveryLogStatus: "Failure"`. CLI sends `endpointId=<id>&deliveryLogStatus=Failure` on the wire. |
| "Inspect a specific delivery's payload" | `webhooks deliveries get <id>` | |
| "Re-send a failed delivery" | `webhooks deliveries retry <id>` | Skip ping-event deliveries — server rejects them. |
| "What event types can I subscribe to?" | `webhooks event-types list` | Bare array of `{eventType, description, group}` objects. Match subscriptions by the `eventType` string. |
| "Subscribe to checkout completion" | `webhooks endpoints create --url <URL> --events payment_session.completed,payment_session.created …` | Added by ARISE-3639 (v0.7.0); group is `Payment Sessions`. |
| "Subscribe to payment-link lifecycle" | `webhooks endpoints create --url <URL> --events payment_link.created,payment_link.updated …` | Added by ARISE-4501 (v0.7.2); group is `Payment Links`. Paid single-use links do not fire `payment_link.updated` — surface via `payment_session.completed` + `transaction.*`. |

## Things to avoid

- **Don't invoke the TUI from an agent.** `flute-webhooks tui` enters an alternate-screen ratatui loop with no JSON surface.
- **Don't combine `--output json` with `auth login` or `listen`.** Those are interactive/long-running and don't emit JSON.
- **Don't rely on stderr.** The structured error envelope is on stdout. Stderr may contain tracing output, update notices, or anyhow debug formatting depending on flags.
- **Don't poll faster than 5 s.** The configured floor and adaptive backoff exist for a reason; agent loops should respect `poll_interval_seconds` in `~/.flute/config.toml` (default 5).
- **Don't pass `http://` URLs to `endpoints create`** — the API requires HTTPS.
- **Don't retry ping deliveries** — they're synthetic and the server refuses them.
- **Don't send `PUT /v2/webhooks/endpoints/{id}`.** The endpoint accepts PATCH only and returns 405 for PUT. The CLI already handles this; only relevant if you're hand-rolling requests.
- **Don't filter deliveries by `pending`.** The v2 `WebhookDeliveryLogStatus` enum was reduced to `Success` / `Failure`; any `Pending` filter value the server sees now returns HTTP 400.

## Migration from v0.6.x

Every wire rename in one place — apply these to any code that parsed the v0.6.x output:

| v0.6.x wire name | v0.7.0 wire name |
|---|---|
| `endpoints.list` shape `[…]` (bare array) | `{ items, pageInfo }` |
| `endpoints.*.webhookName` | `endpoints.*.endpointName` |
| `endpoints.*.status` (Active/Inactive) | `endpoints.*.endpointStatus` |
| `endpoints.create.createdAt` | `endpoints.create.createdOn` |
| `event-types.*.eventTypeId` | *(removed — match by `eventType`)* |
| `event-types.*.name` | `event-types.*.eventType` |
| `deliveries.list` shape `{ items, total }` | `{ items, pageInfo }` (total is now `pageInfo.totalItems`) |
| `deliveries.*.webhookEndpointId` | `deliveries.*.endpointId` |
| `deliveries.*.webhookName` | `deliveries.*.endpointName` |
| `deliveries.*.deliveryAttemptStatus` | `deliveries.*.deliveryLogStatus` |
| `WebhookDeliveryLogStatus` enum `{Success, Failure, Pending}` | `{Success, Failure}` — `Pending` removed |
| `PUT /v2/webhooks/endpoints/{id}` | `PATCH /v2/webhooks/endpoints/{id}` (JSON Merge Patch body) |
| `deliveries.*.responseStatusCode` | `deliveries.*.endpointHTTPResponseCode` |
| `deliveries list` query key `webhookId` | `endpointId` |
| `deliveries list` query key `status` | `deliveryLogStatus` |
| `ping.success` | `ping.isDelivered` |
| `ping.statusCode` | `ping.endpointHTTPResponseCode` |

Unchanged in v0.7.0: `hmacSecret`, `endpointUrl`, `eventTypes`, `createdOn`/`modifiedOn` on endpoints, `deliveryLogId`, `eventId`, `eventType`, `attemptNumber`, `roundTripDurationMs`, `errorMessage`, all the request/response body fields on `deliveries get`, the error envelope shape, and every CLI flag name (`--endpoint-id`, `--status`, `--limit`, `--url`, `--events`, `--name`, `--yes`).

### Deprecations landing in v0.7.1

- `auth token` → **`auth keys`.** The command was renamed for consistency with sibling CLIs (see `getflute/flute-cli`, which did the same `tokens → keys` rename). `auth token` continues to work as a hidden alias and produces identical output; there is no runtime warning. New agent code should invoke `auth keys` — `auth token` may be removed in a later major bump.

## See also

- [`readme.md`](readme.md) — human-readable overview
- [`src/api/models.rs`](src/api/models.rs) — wire-format DTO definitions
- [`src/cli/mod.rs`](src/cli/mod.rs) — clap subcommand tree (source of truth for flags)
