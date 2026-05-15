# CLAUDE.md — guidance for Claude working in this repo

Rust 2024 edition. A CLI **and** ratatui TUI for the Flute webhook REST API. The TUI lives in `src/tui/`; the scriptable surface lives in `src/cli/`; both share `src/api/` (typed `ApiClient`), `src/auth/` (OAuth + keychain), `src/config.rs`, `src/poller.rs`, and `src/update*.rs`.

## Loop

Build/test/lint commands you will use most:

```bash
cargo build --quiet          # debug build at target/debug/flute-webhook
cargo test                   # 93 tests; lib + integration; should all pass
cargo clippy --all-targets --no-deps   # zero warnings expected
cargo fmt                    # before any commit
dist plan --output-format=json         # inspect the cargo-dist release matrix
```

CI only runs on `v*` tag pushes (cargo-dist's `release.yml`). PRs get a cheap plan-only check; the full build matrix runs only on tag.

## Conventions (already in force — don't undo them)

- **No `.unwrap()` / `.expect()` / `panic!` in production code.** `#[cfg(test)] mod tests` blocks may use them freely. For typed API failures, propagate via `crate::api::error::ApiError`; for everything else, `anyhow::Result` + `?`. The 401-retry helper in `src/api/client.rs` is the canonical pattern for wrapping ApiError around HTTP.
- **`--output json` must yield clean stdout.** Update notices, progress, and tracing go to stderr. The structured failure envelope (`cli::output::ErrorJson`) goes to stdout *only* when `--output json` is set and a non-TUI command fails; in that case `lib::run()` `std::process::exit(1)`s after printing to bypass anyhow's stderr formatter.
- **TUI owns stdout** while running. Anything that would print to stdout in a TUI session must instead go to `~/.flute/flute-webhook.log` via `tracing`. Look at `init_tracing` in `src/lib.rs` for the routing rules.
- **Comments are rare.** Only when WHY is non-obvious — a hidden constraint, a workaround for a specific bug, a subtle invariant. Don't document WHAT well-named code already shows. Don't reference the current task or commit ("added for X" rots).
- **No new docs unless asked.** `readme.md` (human), `AGENTS.md` (runtime-agent contract), and this file (CLAUDE.md) are the only docs. Don't generate planning/decision MDs unless the user asks for them. Update an existing doc rather than creating a sibling.
- **No emojis in source files.**
- **Don't add features beyond the ask.** A bugfix doesn't need surrounding cleanup; one similar block of code is not yet an abstraction to extract; three is. No half-finished scaffolding.

## File map (where things actually live)

```
src/
├── api/
│   ├── client.rs         ApiClient — typed methods, 401-retry, Content-Length: 0 quirk
│   ├── error.rs          ApiError enum + AspNetError DTO (camelCase + PascalCase aliases)
│   └── models.rs         All wire DTOs; field aliases for both casings
├── auth/
│   ├── keychain.rs       Single keychain entry per profile, env-var fallback
│   └── token.rs          OAuth2 token cache, proactive + reactive refresh
├── cli/
│   ├── mod.rs            clap subcommand tree (source of truth for flags)
│   ├── output.rs         JSON pretty-print + table fallbacks + ErrorJson envelope
│   └── webhooks.rs       dispatcher for `webhooks …` subcommands
├── config.rs             Config (TOML), Profile (uat/production), poll validator
├── domain.rs             TUI-facing types (Endpoint, DeliveryLog, EventTypeMeta)
├── forward.rs            Listener forwarding — mirror headers + body to local URL
├── lib.rs                Entry point: arg parsing, tracing, runtime, dispatch
├── poller.rs             Background tokio task: adaptive cadence + exponential backoff (30s cap)
├── tui/
│   ├── app.rs            App state, key handling, AppAction / ActionOutcome channel
│   ├── modals.rs         Every modal (create/edit/delete/details/error/update/listener)
│   ├── mod.rs            event loop, action executor, panic-restore hook
│   └── ui.rs             render() — top bar, body, help bar, modal overlay, toast
├── update.rs             axoupdater wrapper — `update` subcommand
└── update_check.rs       24h-cached version check with opt-out gates
```

Tests:

```
tests/
├── api_client.rs         wiremock integration: bodyless POST, 401 retry, etc.
├── cli_webhooks.rs       wiremock integration: every webhook subcommand
└── tui_render.rs         TestBackend: endpoint count column + update modal
```

## Release process

1. Bump `version` in `Cargo.toml`; `cargo build` to refresh `Cargo.lock`; bump readme badge.
2. PR → squash-merge to `master`.
3. `git tag -a vX.Y.Z -m vX.Y.Z && git push origin vX.Y.Z`. cargo-dist's `release.yml` fires on `v*` tags only.
4. The matrix builds `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`. The Linux runner needs `libdbus-1-dev` + `pkg-config`; these are declared in `dist-workspace.toml` under `[dist.dependencies.apt]` — don't remove them.
5. Artifacts: three platform archives + three installers (`flute-webhooks-installer.sh`, `.ps1`, `flute-webhooks.rb`).

If a tag build fails on Linux with libdbus / pkg-config, the apt block in `dist-workspace.toml` is the place to look first.

## Things that have bitten us — don't re-break

- `flute-webhook` (no s) is the binary; `flute-webhooks` (with s) is the crate / app name cargo-dist uses for installers and receipts. Both forms appear in code — preserve the existing usage when editing.
- The Flute API is at the **root** of the host, not under `/api` or `/isv-api`. Don't add a path prefix to `Profile::api_base_url`.
- Bodyless `POST`/`PUT`/`PATCH` must send `Content-Length: 0` explicitly. See `build_request` in `src/api/client.rs`.
- macOS keychain ACLs are tied to binary signature, so every `cargo build` re-prompts. For development, prefer `cargo install --path .` then re-use that binary.
- The OS keychain backend on Linux links against libdbus at compile time. Tests use the `apple-native`/`windows-native`/`linux-native-sync-persistent` features — don't switch to a mock backend, it'll mask real bugs.

## When you change the CLI surface

Update **`AGENTS.md`** (the runtime contract) at the same commit. The CLI parity table in `readme.md` should also stay in sync, but it's less load-bearing than AGENTS.md.

## When you change the release pipeline

Snapshot the current `release.yml` to `docs/legacy/` before swapping it out, so a revert is a copy-back rather than a workflow rewrite. `docs/legacy/build.yaml` is the prior taiki-e workflow kept for this reason.

## What not to do

- Don't run `git push --force` against `master`. Tag force-updates are case-by-case and require explicit user sign-off.
- Don't pass `--no-verify` to `git commit` or `git push`. Fix the hook output instead.
- Don't run destructive operations (`git reset --hard`, `git clean -f`, force push, branch delete) without user confirmation.
- Don't introduce new dependencies casually — every dep ends up in `release.yml`'s build matrix on three OSes.
- Don't write planning docs, ADRs, or summary markdown unless the user asks for them. PR descriptions carry the "why".
