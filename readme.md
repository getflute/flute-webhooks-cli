# flute-webhook

A Rust CLI **and** terminal UI for working with Flute webhooks: manage endpoints, watch delivery logs in real time, retry failures, and forward incoming successful events to a local listener URL. Built with [ratatui](https://ratatui.rs), [reqwest](https://docs.rs/reqwest), [clap](https://docs.rs/clap), and tokio.

![status](https://img.shields.io/badge/status-v0.2.0-blue)
[![build](https://github.com/getflute/flute-webhooks/actions/workflows/build.yaml/badge.svg)](https://github.com/getflute/flute-webhooks/actions/workflows/build.yaml)

## What it does

- **Manage endpoints** — create, edit, delete, and ping webhook endpoints from the keyboard or scriptable CLI.
- **Watch deliveries** — poll the live API in the background, stream delivery logs into a filterable table, and view full request/response bodies on demand.
- **Retry failed deliveries** — single-shot from the TUI (`r` key) or CLI (`webhooks deliveries retry <id>`).
- **Local listener** — forward every NEW successful delivery to `http://127.0.0.1:port/...` in TUI mode (toggleable modal) or as a long-running CLI command (`flute-webhook listen --forward-to ...`).
- **Adaptive polling** — 5 s default, configurable 5–60 s, backs off to 20 s during form input, exponential 30-second cap on consecutive failures.
- **Resilient auth** — bearer tokens cached + proactively refreshed 60 s before expiry; reactive retry once on a 401.
- **Errors with correlation IDs** — failed API calls show a sticky red modal with the server's `Title`, `Details`, `ExceptionType`, and correlation ID until you dismiss it.
- **`--debug` for HTTP traces** — every request + response (status, URL, body) at debug level, to stdout (CLI) or `~/.flute/flute-webhook.log` (TUI).

## Coverage: TUI ↔ CLI

Every documented Webhook API call is reachable from both modes:

| Capability                  | TUI                                  | CLI                                            |
|-----------------------------|--------------------------------------|------------------------------------------------|
| List endpoints              | Endpoints tab                        | `webhooks endpoints list`                      |
| Get one endpoint            | implicit (table shows all fields)    | `webhooks endpoints get <id>`                  |
| Create endpoint             | `c` → form modal                     | `webhooks endpoints create`                    |
| Update endpoint             | `e`/`Enter` → form modal             | `webhooks endpoints update <id>`               |
| Delete endpoint             | `d` → confirm modal                  | `webhooks endpoints delete <id> --yes`         |
| Ping endpoint               | `p` (toast on result)                | `webhooks endpoints ping <id>`                 |
| List event types            | used to populate the form            | `webhooks event-types list`                    |
| List delivery logs          | Delivery Logs tab                    | `webhooks deliveries list`                     |
| Get delivery log detail     | `v`/`Enter` → details modal          | `webhooks deliveries get <id>`                 |
| Retry failed delivery       | `r` on a failed row                  | `webhooks deliveries retry <id>`               |
| Listen + forward locally    | `l` → listener modal                 | `flute-webhook listen --forward-to <url>`      |
| Manual one-shot forward     | `t` on a successful row              | (`listen` covers it; manual one-shot deferred) |
| Self-update                 | modal on startup; dismissable        | `flute-webhook update`                         |

`--output json` works on every CLI subcommand, producing pretty-printed JSON for piping into `jq`.

## Requirements

- **Rust 1.85+** (edition 2024). Install via [rustup](https://rustup.rs/).
- **macOS, Linux, or Windows** — uses the OS keychain (Keychain on macOS, Secret Service on Linux, Credential Manager on Windows).
- A **Flute API client_id and client_secret** for the environment you want to use (UAT or production).

## Install

Pick whichever installer matches your platform — each one drops a `flute-webhook` binary on your `PATH` plus an install receipt that the in-app `update` command reads when checking for new versions.

```bash
# macOS / Linux (curl + sh)
curl -LsSf https://github.com/getflute/flute-webhooks/releases/latest/download/flute-webhooks-installer.sh | sh

# macOS / Linux (Homebrew)
brew install getflute/flute-webhooks/flute-webhooks

# Windows (PowerShell)
irm https://github.com/getflute/flute-webhooks/releases/latest/download/flute-webhooks-installer.ps1 | iex
```

Installers, archives, and SHA-256 sums are produced by [`cargo-dist`](https://opensource.axo.dev/cargo-dist/) on every `v*` tag and attached to the [GitHub Release page](https://github.com/getflute/flute-webhooks/releases). Build targets: macOS Apple Silicon, Linux x86_64, Windows x86_64.

## Build from source

```bash
git clone https://github.com/getflute/flute-webhooks.git
cd flute-webhooks
cargo build --release
# Binary lands at target/release/flute-webhook
```

For development, `cargo build` (debug profile) is faster and works the same way. Source-built binaries do not carry an install receipt, so `flute-webhook update` cannot self-replace them — it will check for a newer version and tell you to reinstall via one of the installers above.

## First run

### 1. Authenticate

```bash
flute-webhook auth login
# (or: cargo run -- auth login)
```

You'll be prompted for `client_id` and `client_secret`. The secret prompt is hidden (no echo). Credentials are stored in your OS keychain — never in plaintext on disk.

By default this stores credentials for the **uat** profile. To set up production:

```bash
flute-webhook --profile production auth login
```

### 2. Verify

```bash
flute-webhook auth token
```

Prints the current bearer JWT (useful for `curl` smoke tests).

### 3. Use it

**Interactive (TUI):**
```bash
flute-webhook tui
```
`flute-webhook` with no subcommand prints help — it does not launch the TUI silently.

**Scriptable (CLI):**
```bash
flute-webhook webhooks endpoints list
flute-webhook --output json webhooks deliveries list --limit 5 | jq .
```

## CLI reference

```bash
# Endpoints
flute-webhook webhooks endpoints list
flute-webhook webhooks endpoints get <id>
flute-webhook webhooks endpoints create --url https://… --events transaction.card.captured,refund.completed [--name "My Hook"]
flute-webhook webhooks endpoints update <id> [--url …] [--events …] [--name …] [--status active|inactive]
flute-webhook webhooks endpoints delete <id> --yes
flute-webhook webhooks endpoints ping <id>

# Event-types catalog
flute-webhook webhooks event-types list

# Delivery logs
flute-webhook webhooks deliveries list [--endpoint-id <id>] [--status success|failed] [--limit 50]
flute-webhook webhooks deliveries get <id>
flute-webhook webhooks deliveries retry <id>

# Headless listener — POSTs every NEW successful delivery's headers + body
# to a local URL. Runs in the foreground until Ctrl-C.
flute-webhook listen --forward-to http://127.0.0.1:3000/webhook

# Check GitHub Releases and self-update in place (only works when installed
# via one of the cargo-dist installers above; from-source builds get an
# informational message pointing at the installers).
flute-webhook update
```

Global flags (work on every subcommand): `--profile <uat|production>`, `--debug`, `--output table|json`.

## TUI key bindings

| Context | Keys |
|---|---|
| **Top level** | `Tab` switch tabs · `q` quit · `Ctrl-C` quit anywhere |
| **Endpoints tab** | `↑↓`/`jk` navigate · `c` create · `e`/`Enter` edit · `d` delete · `p` ping |
| **Delivery Logs tab** | `↑↓`/`jk` navigate · `PgUp`/`PgDn`/`Home`/`End` jump · `v`/`Enter` view details · `t` trigger forward · `r` retry (failed deliveries only) · `l` listener config · `1` cycle endpoint filter · `2` cycle event-type filter · `3` cycle status filter · `s` toggle sort · `x` clear filters |
| **Form modal (create/edit)** | `Tab`/`↑↓` move between fields · `←/→` swap Cancel/Submit · `Space`/`Enter` toggle controls · `PgUp`/`PgDn` scroll the event list · `Esc` cancel |
| **Listener modal** | `Tab`/`↑↓` move between fields · type the URL · `Space` toggle Enabled · `Enter` activate · `Esc` cancel |
| **Delete confirm** | `y`/`Enter` delete · `n`/`Esc` cancel |
| **Details modal** | `↑↓`/`jk` scroll · `PgUp`/`PgDn` page · `Esc`/`Enter`/`q` close |
| **Error modal** | `Enter`/`Esc` dismiss (every other key is absorbed) |

While typing in a text field (URL or Name), single-character keys like `q`, `c`, `d`, `e`, `p`, `r`, `l`, `t` are treated as literal characters — they will **not** trigger the corresponding TUI commands.

## Configuration

Optional `~/.flute/config.toml`:

```toml
default_profile = "uat"          # uat | production
poll_interval_seconds = 5        # 5–60; out of range falls back to 5 with a warning
auto_update_check = true         # check GitHub Releases at most once / 24h
```

If `poll_interval_seconds` is outside `5..=60`, the TUI shows a yellow warning in the dashboard title and uses the default of 5 seconds.

### Environment variables

| Variable | Purpose |
|---|---|
| `FLUTE_PROFILE` | Default profile (overridden by `--profile`) |
| `FLUTE_CLIENT_ID` | Skips keychain lookup — used for CI |
| `FLUTE_CLIENT_SECRET` | Same — both must be set together |
| `RUST_LOG` | Tracing filter, e.g. `RUST_LOG=flute_webhook=debug` (overrides `--debug` defaults if set) |
| `FLUTE_NO_UPDATE_CHECK` | Set to anything to suppress the startup update check |
| `FLUTE_GITHUB_TOKEN` | Optional GitHub token (raises the 60/hr unauth limit; required if the release repo is private) |
| `CI` | When set (Actions, Buildkite, etc.), the startup update check is automatically skipped |

### Updating

On every run, `flute-webhook` checks GitHub Releases at most once per 24 hours (cached at `~/.flute/update-check.json`) and surfaces a non-blocking notice if a newer version is available:

- **CLI**: a one-line `eprintln!` after the command finishes, so stdout (including `--output json`) stays clean.
- **TUI**: a dismissable green modal on the first frame; Enter or Esc dismisses it.

The check is skipped entirely when any of the following apply: the subcommand is `update` or `auth`, `auto_update_check = false`, `FLUTE_NO_UPDATE_CHECK` is set, `CI` is set, or stderr isn't a TTY (piped output).

When a newer version exists, run `flute-webhook update` to install it. Binaries installed via one of the cargo-dist installers (curl, brew, irm) carry an install receipt and can self-replace in place. Source-built binaries (`cargo install`, `cargo build --release`) instead receive a printed instruction to reinstall via an installer.

### Debugging HTTP traffic

Pass `--debug` to log every HTTP request and response (status, URL, body) at debug level:

```bash
flute-webhook --debug auth token        # traces print to STDOUT
flute-webhook --debug tui               # TUI: traces go to ~/.flute/flute-webhook.log
```

For non-TUI commands, traces print to **stdout** so you can pipe them through `jq` / `grep`. For the TUI, stdout is owned by ratatui, so traces are appended to `~/.flute/flute-webhook.log` instead — open a second terminal and `tail -f ~/.flute/flute-webhook.log` to watch live. Response bodies are logged in full (no truncation) so server stack traces are captured intact; the bearer token is never logged.

Without `--debug`, default tracing is INFO/WARN — non-TUI commands write to stderr, the TUI writes to the log file.

## Profiles

| Profile | API base | OAuth URL |
|---|---|---|
| `uat` (default) | `https://api.uat.arise.risewithaurora.com` | `https://oauth.uat.arise.risewithaurora.com/oauth2/token` |
| `production` (alias `prod`) | `https://api.arise.risewithaurora.com` | `https://oauth.arise.risewithaurora.com/oauth2/token` |

Use `--profile` (global flag, accepted before or after the subcommand). Active profile is shown in the dashboard title.

## Development

```bash
cargo test       # 78 tests across lib + integration suites
cargo clippy --all-targets --no-deps
cargo fmt
```

Project layout:

```
src/
├── api/        REST client, DTOs (camelCase + PascalCase), error types
├── auth/       Keychain wrapper, OAuth2 token cache (proactive + reactive refresh)
├── config.rs   Config + Profile + polling validator
├── domain.rs   TUI-facing domain types (Endpoint, DeliveryLog, EventTypeMeta)
├── forward.rs  Listener forwarding (used by both TUI and `listen` CLI)
├── poller.rs   Background tokio task with adaptive cadence + exponential backoff
├── cli/        clap subcommands, output formatters, webhooks dispatcher
├── lib.rs      Entry point: tracing, runtime, dispatch
└── tui/        Ratatui App state, key handling, render, modals
```

Implementation plans: see `docs/superpowers/plans/`.

## Releases

Tag pushes matching `v*` trigger `.github/workflows/release.yml`, generated by [`cargo-dist`](https://opensource.axo.dev/cargo-dist/). For each tag the workflow builds release binaries for three targets, generates the matching installers, and uploads everything to a GitHub Release with a generated changelog:

| Target                  | Runner          | Triple                       | Archive             |
|-------------------------|-----------------|------------------------------|---------------------|
| macOS Apple Silicon     | `macos-latest`  | `aarch64-apple-darwin`       | `.tar.xz`           |
| Linux x86_64            | `ubuntu-latest` | `x86_64-unknown-linux-gnu`   | `.tar.xz`           |
| Windows x86_64          | `windows-latest`| `x86_64-pc-windows-msvc`     | `.zip`              |

Plus three installer artifacts per release:

- `flute-webhooks-installer.sh` (`curl … | sh`)
- `flute-webhooks-installer.ps1` (`irm … | iex`)
- `flute-webhooks.rb` (Homebrew formula in the same repo)

`dist plan` (run locally) prints the exact artifact list a tag would produce — useful before pushing a release tag.

To cut a release:

```bash
git tag v0.2.0
git push origin v0.2.0
```

The workflow only fires on tags matching `v*` (plus manual `workflow_dispatch` from the Actions tab). It does not run on regular pushes or pull requests. A snapshot of the previous (taiki-e-based) workflow lives at `docs/legacy/build.yaml` if you ever need to roll back.

## Troubleshooting

**`no credentials for [uat]`** — run `flute-webhook auth login`.

**Terminal looks broken after a crash** — the panic hook should restore it automatically; if it didn't, run `reset` or `stty sane`.

**Errors flash by too fast** — they don't. Errors pop a red modal that stays until you press `Enter` or `Esc`. While it's up the modal absorbs every other key (so `q` doesn't quit, `c` doesn't open the create form, etc.).

**`Busy — try again in a moment` toast** — the action queue is briefly saturated by an in-flight API call. The next press will go through.

**macOS Keychain prompts every time I run `cargo run`** — every `cargo build` produces a new unsigned binary, and macOS Keychain ACLs are tied to the binary's code signature, so "Always Allow" doesn't survive a rebuild. The app stores credentials as a *single* keychain entry per profile (one prompt instead of two on the legacy layout). For development, install once into `~/.cargo/bin` (`cargo install --path .`) and click "Always Allow" on that stable binary — re-running it won't re-prompt until you `cargo install` again.

**The polling cadence seems slow after an error** — that's the exponential backoff. On consecutive 401/403/404/5xx (or transport) failures the poll interval doubles each time, capped at 30 seconds (or your configured base interval if it's larger — backoff never polls faster than your normal cadence). The counter resets to zero on the first successful poll. The error modal stays up so you can see what's happening.

**Token refresh** — bearer tokens are cached in memory and proactively refreshed 60 seconds before their advertised expiry. If the server returns a 401 anyway (clock skew, server restart, revocation), the client invalidates the cache, fetches a fresh token, and retries the original request once. Only requests that fail twice in a row are surfaced as errors.

**`POST requests require a Content-length`** — fixed: bodyless POST/PUT/PATCH requests now always send `Content-Length: 0` (the Flute gateway requires it on every write-method request).

## License

MIT.
