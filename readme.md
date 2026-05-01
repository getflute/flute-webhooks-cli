# flute-webhook

Interactive terminal UI for managing Flute webhook endpoints and watching delivery logs against the Flute REST API. Built in Rust with [ratatui](https://ratatui.rs).

![status](https://img.shields.io/badge/status-v0.1.0-blue)

## What it does

- **Manage endpoints** — create, edit, delete, and inspect webhook endpoints from a keyboard-driven UI.
- **Watch deliveries** — poll the live API in the background and stream delivery logs into a filterable table.
- **Per-endpoint trigger counts** — the Endpoints view shows how many times each webhook fired in the most recent log window.
- **Adaptive polling** — 5 s by default, configurable 5–60 s, automatically backs off to 20 s while you're editing a form.
- **Errors with correlation IDs** — failed API calls show a sticky banner with the full server error, including the correlation ID, until you dismiss it.

## Requirements

- **Rust 1.85+** (edition 2024). Install with [rustup](https://rustup.rs/).
- **macOS, Linux, or Windows** — uses the OS keychain (Keychain on macOS, Secret Service on Linux, Credential Manager on Windows).
- A **Flute API client_id and client_secret** for the environment you want to use (UAT or production).

## Build

```bash
git clone <this-repo>
cd flute-webhooks
cargo build --release
# Binary lands at target/release/flute-webhook
```

For development, `cargo build` (debug profile) is faster and works the same way.

## First run

### 1. Authenticate

```bash
cargo run -- auth login
```

You'll be prompted for `client_id` and `client_secret`. The secret prompt is hidden (no echo). Credentials are stored in your OS keychain — never in plaintext on disk.

By default this stores credentials for the **uat** profile. To set up production:

```bash
cargo run -- --profile production auth login
```

### 2. Verify

```bash
cargo run -- auth token
```

Prints the current bearer JWT (useful for `curl` smoke tests). If credentials aren't stored, you'll get an actionable error pointing you back to `auth login`.

### 3. Launch the TUI

```bash
cargo run -- tui
# or against production
cargo run -- --profile production tui
```

`cargo run` (no subcommand) prints the help text — it does not launch the TUI silently. Use `tui` explicitly.

## Key bindings

| Context | Keys |
|---|---|
| **Top level** | `Tab` switch tabs · `q` quit · `Ctrl-C` quit anywhere |
| **Endpoints tab** | `↑↓`/`jk` navigate · `c` create · `e`/`Enter` edit · `d` delete |
| **Delivery Logs tab** | `↑↓`/`jk` navigate · `v`/`Enter` view details · `1` cycle endpoint filter · `2` cycle event-type filter · `3` cycle status filter · `s` toggle sort · `x` clear filters |
| **Form modal (create/edit)** | `Tab`/`↑↓` move between fields · `Space`/`Enter` toggle controls · `PgUp`/`PgDn` scroll the event list · `Esc` cancel |
| **Delete confirm** | `y`/`Enter` delete · `n`/`Esc` cancel |
| **Details modal** | `↑↓`/`jk` scroll · `PgUp`/`PgDn` page · `Esc`/`Enter`/`q` close |
| **Error modal** | `Enter`/`Esc` dismiss (every other key is absorbed) |

While typing in a text field (URL or Name), single-character keys like `q`, `c`, `d`, `e` are treated as literal characters — they will not trigger the corresponding TUI commands.

## Configuration

Optional `~/.flute/config.toml`:

```toml
default_profile = "uat"          # uat | production
poll_interval_seconds = 5        # 5–60; out of range falls back to 5 with a warning
```

If `poll_interval_seconds` is outside `5..=60`, the TUI shows a yellow warning in the dashboard title and uses the default of 5 seconds.

### Environment variables

| Variable | Purpose |
|---|---|
| `FLUTE_PROFILE` | Default profile (overridden by `--profile`) |
| `FLUTE_CLIENT_ID` | Skips keychain lookup — used for CI |
| `FLUTE_CLIENT_SECRET` | Same — both must be set together |
| `RUST_LOG` | Tracing filter, e.g. `RUST_LOG=flute_webhook=debug` (overrides `--debug` defaults if set) |

### Debugging HTTP traffic

Pass `--debug` to log every HTTP request and response (status, URL, body) at debug level:

```bash
flute-webhook --debug auth token        # traces print to STDOUT
flute-webhook --debug tui               # TUI: traces go to ~/.flute/flute-webhook.log
```

For non-TUI commands, traces print to **stdout** so you can pipe them through `jq` / `grep`. For the TUI, stdout is owned by ratatui, so traces are appended to `~/.flute/flute-webhook.log` instead — open a second terminal and `tail -f ~/.flute/flute-webhook.log` to watch live. Bodies over 4 KB are truncated; the bearer token is never logged.

Without `--debug`, default tracing is INFO/WARN — non-TUI commands write to stderr, the TUI writes to the log file.

## Profiles

| Profile | API base | OAuth URL |
|---|---|---|
| `uat` (default) | `https://api.uat.arise.risewithaurora.com` | `https://oauth.uat.arise.risewithaurora.com/oauth2/token` |
| `production` (alias `prod`) | `https://api.arise.risewithaurora.com` | `https://oauth.arise.risewithaurora.com/oauth2/token` |

Use `--profile` (global flag, accepted before or after the subcommand). Active profile is shown in the dashboard title.

## Development

```bash
cargo test       # 37 tests across lib + integration
cargo clippy
cargo fmt
```

Project layout:

```
src/
├── api/        REST client, DTOs, error types
├── auth/       Keychain wrapper, OAuth2 token cache
├── config.rs   Config + Profile + polling validator
├── domain.rs   TUI-facing domain types (Endpoint, DeliveryLog)
├── poller.rs   Background tokio task with adaptive cadence
├── cli.rs      clap subcommands
├── lib.rs      Entry point: tracing, runtime, dispatch
└── tui/        Ratatui App state, key handling, render, modals
```

Implementation plan: `docs/superpowers/plans/2026-04-30-flute-webhooks-tui.md`.

## Troubleshooting

**`no credentials for [uat]`** — run `cargo run -- auth login`.

**Terminal looks broken after a crash** — the panic hook should restore it automatically; if it didn't, run `reset` or `stty sane`.

**Errors flash by too fast** — they don't anymore. Errors pop a red modal that stays put until you press `Enter` or `Esc`. While it's up the modal absorbs every other key (so `q` doesn't quit, `c` doesn't open the create form, etc.).

**`Busy — try again in a moment` toast** — the action queue is briefly saturated by an in-flight API call. The next press will go through.

**The polling cadence seems slow after an error** — that's the exponential backoff. On consecutive 401/403/404/5xx (or transport) failures the poll interval doubles each time, capped at 30 seconds (or your configured base interval if it's larger — backoff never polls faster than your normal cadence). The counter resets to zero on the first successful poll. The error modal stays up so you can see what's happening.

## License

MIT.
