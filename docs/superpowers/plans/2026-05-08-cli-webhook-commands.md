# Plan: CLI commands for the Flute Webhook API

**Goal:** Expose every Webhook API call from the live spec (`/isv-api/swagger/v2/swagger.json`) as a non-interactive CLI subcommand, alongside the existing TUI. Keep the change as a single git commit (or short revertable chain) so user testing failures are reversible with `git revert`.

**Source spec:** `https://github.com/aurora-payments/luna/blob/main/context/app-specs/FEATURE-FLUTE-CLI.md` § 6 (and the live swagger).

---

## 1. Audit: what already works

### TUI (interactive)

| Spec capability                       | TUI today                              | Status |
|---------------------------------------|----------------------------------------|--------|
| List endpoints                        | Endpoints tab                          | ✅     |
| Create endpoint                       | `c` → form modal                       | ✅     |
| Update endpoint                       | `e`/Enter → form modal                 | ✅     |
| Delete endpoint                       | `d` → confirm modal                    | ✅     |
| Get one endpoint                      | (implicit — list shows everything)     | ◐ partial |
| Ping endpoint                         | _none_                                 | ❌     |
| List event types                      | (used internally to populate the form) | ◐ implicit |
| List delivery logs                    | Delivery Logs tab                      | ✅     |
| Get one delivery log (full body)      | `v`/Enter → details modal              | ✅     |
| Retry a failed delivery               | _none_                                 | ❌     |
| Listen / forward to local URL         | `[l]` modal + `t` row trigger          | ✅     |
| Export delivery logs (`/export`)      | _none_                                 | ❌     |

### CLI (non-interactive)

| Spec capability               | CLI today                                  | Status |
|------------------------------|--------------------------------------------|--------|
| Headless listen               | `flute-webhook listen --forward-to <url>`  | ✅     |
| Auth login / token            | `flute-webhook auth login` / `auth token`  | ✅     |
| Everything else               | _none_                                     | ❌     |

### `ApiClient` already has Rust methods for

`list_endpoints`, `create_endpoint`, `update_endpoint`, `delete_endpoint`, `ping_endpoint`, `list_event_types`, `list_delivery_logs`, `get_delivery_log`, `retry_delivery`. **No new `ApiClient` work is needed for the documented routes** except `delivery_logs/export`, which is a deliberate post-MVP defer (see §5).

---

## 2. Target CLI surface

Mirrors the spec layout. All output goes to stdout, errors to stderr, exit codes per the existing convention (0 success / 1 general / 2 auth / 3 validation / 4 not found).

```text
flute-webhook webhooks endpoints list                                       [--output json|table]
flute-webhook webhooks endpoints get      <id>                              [--output json|table]
flute-webhook webhooks endpoints create   --url <U> --events <E1,E2,...>    [--name <N>]
flute-webhook webhooks endpoints update   <id> [--url <U>] [--events <list>] [--name <N>] [--status active|inactive]
flute-webhook webhooks endpoints delete   <id> [--yes]
flute-webhook webhooks endpoints ping     <id>                              [--output json|table]

flute-webhook webhooks event-types list                                     [--output json|table]

flute-webhook webhooks deliveries list    [--endpoint-id <id>] [--status success|failed]
                                          [--limit <N>]                     [--output json|table]
flute-webhook webhooks deliveries get     <id>                              [--output json|table]
flute-webhook webhooks deliveries retry   <id>
```

Notes on each:

- **`endpoints get <id>`** — the live API's `GET /v2/webhooks/endpoints/{id}` is currently NOT in `ApiClient`. Add a `get_endpoint(id)` method (≈10 lines, mirrors `delete_endpoint`). One commit-internal item.
- **`endpoints create --events <e1,e2,...>`** — accept a comma-separated list. Spec mentions `transaction.*` wildcards but the API takes explicit names; if a wildcard is passed, expand client-side against `list_event_types` results before posting.
- **`endpoints update`** — every flag is optional; we do a `GET` then merge then `PUT`. Avoids accidentally clearing `event_types` when the user only wants to rename.
- **`endpoints delete --yes`** — refuse without `--yes` (or interactive `y/n` confirm on a TTY) to mirror the TUI's confirmation modal.
- **`deliveries retry <id>`** — calls `retry_delivery`; returns the resulting attempt JSON.
- **`webhooks trigger <event-type>`** from the spec is intentionally **not** on this list. The live API has no "fire a synthetic event" endpoint — only `ping`. The closest thing in the TUI today is `[t]rigger` on a successful row, which re-forwards an existing payload to the local listener. If the user wants a CLI equivalent, we add `flute-webhook webhooks deliveries forward <id> --to <url>` in a follow-up.

---

## 3. Output format

A new global flag, `--output json|table` (default `table`), wired through every webhook subcommand. Internal helpers in `src/cli/output.rs`:

```rust
pub enum OutputFormat { Table, Json }
pub fn print_endpoints(eps: &[Endpoint], fmt: OutputFormat) -> anyhow::Result<()>
pub fn print_endpoint(ep: &Endpoint, fmt: OutputFormat) -> anyhow::Result<()>
pub fn print_delivery_logs(logs: &[DeliveryLog], total: Option<i32>, fmt: OutputFormat) -> anyhow::Result<()>
pub fn print_delivery_log(log: &DeliveryLogDetailDto, fmt: OutputFormat) -> anyhow::Result<()>
pub fn print_event_types(types: &[EventTypeMeta], fmt: OutputFormat) -> anyhow::Result<()>
pub fn print_ping(p: &PingResponseDto, fmt: OutputFormat) -> anyhow::Result<()>
```

- **JSON**: `serde_json::to_string_pretty` of the response DTO (or our domain type).
- **Table**: ratatui-free helper that prints aligned columns to stdout. Keep it simple — `format!` + manual padding, no extra deps. Existing reqwest/serde gets us most of the way; one more crate (`comfy-table`) is justifiable but optional. Plan: hand-rolled padding to keep the dep tree minimal.

---

## 4. File layout

Refactor the current `src/cli.rs` into a small directory for breathing room:

```
src/
├── cli.rs                # one-line `pub mod cli_impl;` re-export, OR
└── cli/
    ├── mod.rs            # clap structs (Cli, Command, AuthCommand, WebhooksCommand…)
    ├── webhooks.rs       # dispatch for `webhooks endpoints …`, `deliveries …`, `event-types …`
    └── output.rs         # OutputFormat + print_* helpers
```

`lib.rs::run()` keeps the same shape — just dispatches a new `Command::Webhooks(WebhooksCommand)` arm to `cli::webhooks::run`.

---

## 5. Out of scope for this commit (deferred)

| Item                               | Why deferred                                   |
|------------------------------------|------------------------------------------------|
| `GET /v2/webhooks/delivery-logs/export` | Returns a stream/CSV; needs a separate subcommand `deliveries export --format csv --out <path>`. Add later. |
| `webhooks trigger <event-type>`    | No live API endpoint to invoke — needs server-side support first. |
| `--output quiet` / `-q`            | Spec mentions it for scripting; trivial to add but not required for parity. |
| Pagination cursor support          | The live `delivery-logs` returns `total` + `items`; the cursor field doesn't exist. We rely on `--limit`. |
| Wildcard event matching            | `transaction.*` in `--events` — useful sugar but nontrivial; nice-to-have. |

These are tracked in this plan; each is a follow-up commit if/when the user asks.

---

## 6. Tests

- **Unit tests** (in-module) for `OutputFormat` selection and the table-formatting helpers (assert the exact column header line + a representative row).
- **Integration tests** (`tests/cli_webhooks.rs`, new file) using `wiremock` for each command:
  - `webhooks endpoints list` → asserts JSON output contains the expected ids; table output contains the names.
  - `webhooks endpoints create` → asserts the request body the wiremock saw matches the supplied flags.
  - `webhooks endpoints update` (the merge path) → assert that omitted flags don't clear server-side fields (server gets the merged PUT, not a sparse one).
  - `webhooks endpoints delete --yes` → asserts the DELETE was issued.
  - `webhooks deliveries list` with filters → asserts the right query string.
  - `webhooks deliveries retry` → asserts the POST.

The existing `tests/api_client.rs` already covers the underlying ApiClient; the new CLI tests are about flag-parsing and output, not transport.

---

## 7. Commit strategy (revertable)

**Single feature commit** titled `feat(cli): add webhooks endpoints/deliveries/event-types subcommands`. Reverting it removes the entire surface in one go without touching the TUI, listener, or auth code.

If the diff grows large enough to be uncomfortable (>800 lines), split into:

1. `feat(cli): add OutputFormat helpers and --output flag plumbing`
2. `feat(cli): add webhooks endpoints subcommands`
3. `feat(cli): add webhooks deliveries subcommands`

…and revert in reverse order. The TUI never imports any of these new modules so it stays unaffected either way.

---

## 8. Acceptance checklist

After implementation, the user can:

- [ ] `flute-webhook webhooks endpoints list` prints a table of endpoints
- [ ] `flute-webhook --output json webhooks endpoints list` prints valid JSON pipeable into `jq`
- [ ] `flute-webhook webhooks endpoints create --url https://… --events ping,transaction.card.captured` returns the new id + signing secret
- [ ] `flute-webhook webhooks endpoints update <id> --status inactive` only changes status, preserves URL/events
- [ ] `flute-webhook webhooks endpoints delete <id> --yes` returns nothing on success, exit 0
- [ ] `flute-webhook webhooks endpoints ping <id>` returns a `PingResponseDto` (success / status / duration / error)
- [ ] `flute-webhook webhooks event-types list` lists all subscribable types grouped by category
- [ ] `flute-webhook webhooks deliveries list --endpoint-id <id> --status failed` filters correctly
- [ ] `flute-webhook webhooks deliveries get <id>` prints the full request + response detail
- [ ] `flute-webhook webhooks deliveries retry <id>` returns the new attempt ack
- [ ] `--debug` (existing global) prints the underlying HTTP traces for any of the above
- [ ] All commands honor `FLUTE_PROFILE` and `--profile`

If user testing surfaces a problem in any of the above: `git revert <hash>` and the binary is back to "TUI + auth + listen" — no manual cleanup needed.

---

## 9. Estimated size

≈600–800 lines of new code, mostly clap struct definitions + `output.rs` formatting + a `webhooks.rs` dispatcher. About a third of that is tests. No new runtime dependencies; possibly add `comfy-table` if hand-rolled tables prove too ugly during implementation, otherwise zero.
