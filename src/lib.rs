pub mod api;
pub mod auth;
pub mod cli;
pub mod config;
pub mod domain;
pub mod forward;
pub mod poller;
pub mod tui;
pub mod update;
pub mod update_check;

use clap::{CommandFactory, Parser};
use std::io::IsTerminal;
use std::sync::Mutex;

pub fn run() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    let profile = cli.profile.clone();
    let debug = cli.debug;
    let output_fmt = cli.output;

    // No subcommand: print help and exit. Previously defaulted to launching the
    // TUI, which surprised users who ran `flute-webhook --debug` expecting to
    // be told what to do.
    let Some(cmd) = cli.command else {
        cli::Cli::command().print_help()?;
        println!();
        return Ok(());
    };
    let is_tui = matches!(cmd, cli::Command::Tui);

    init_tracing(is_tui, debug);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        // Update check policy: skip entirely for `update` and `auth` (the
        // user is mid-task and would be surprised by a banner) and when
        // stderr isn't a TTY (piped output, CI logs). For the TUI we
        // pre-fetch the result so the modal is ready on the first frame.
        // For other commands we run the check *after* the command finishes
        // and print to stderr so JSON on stdout stays clean.
        let should_check = should_run_update_check(&cmd);

        let pending_tui_notice = if should_check && is_tui {
            update_check::check_for_update().await
        } else {
            None
        };

        let dispatch_result = match cmd {
            cli::Command::Tui => tui::run(&profile, pending_tui_notice).await,
            cli::Command::Auth(cli::AuthCommand::Login) => auth_login(&profile).await,
            cli::Command::Auth(cli::AuthCommand::Token) => auth_print_token(&profile).await,
            cli::Command::Listen { forward_to } => listen(&profile, &forward_to).await,
            cli::Command::Webhooks(c) => run_webhooks(&profile, output_fmt, c).await,
            cli::Command::Update => update::run().await,
        };

        if should_check
            && !is_tui
            && dispatch_result.is_ok()
            && let Some(version) = update_check::check_for_update().await
        {
            eprintln!("\n{}", update_check::notice_for(&version));
        }

        // Structured error envelope for agents: when --output json is set and
        // the command failed, print a JSON object describing the error to
        // stdout (so it lands in the same stream the agent is already parsing
        // for success) and exit non-zero ourselves. This bypasses anyhow's
        // default Debug-formatted stderr dump, which is text and would
        // confuse a json-only consumer.
        if let Err(e) = &dispatch_result
            && output_fmt == cli::OutputFormat::Json
            && !is_tui
        {
            let envelope = cli::output::ErrorJson::from_anyhow(e);
            if let Ok(json) = serde_json::to_string_pretty(&envelope) {
                println!("{json}");
            }
            std::process::exit(1);
        }

        dispatch_result
    })
}

/// Apply all the opt-out gates for the startup update check. The TUI's pre-
/// dispatch fetch and the CLI's post-dispatch fetch share this predicate so
/// the rules don't drift between the two paths.
fn should_run_update_check(cmd: &cli::Command) -> bool {
    // Don't check when the user is explicitly invoking `update` or wrangling
    // credentials — both are tight, single-purpose tasks where a trailing
    // banner adds noise.
    if matches!(cmd, cli::Command::Update | cli::Command::Auth(_)) {
        return false;
    }
    // Don't pollute non-interactive output streams.
    if !std::io::stderr().is_terminal() {
        return false;
    }
    let cfg = config::load_or_default();
    !update_check::opt_out(&cfg)
}

/// Build an authenticated ApiClient and dispatch a `webhooks …` subcommand.
async fn run_webhooks(
    profile: &str,
    output: cli::OutputFormat,
    cmd: cli::WebhooksCommand,
) -> anyhow::Result<()> {
    use std::sync::Arc;
    use std::time::Duration;
    let p = config::Profile::by_name(profile)
        .ok_or_else(|| anyhow::anyhow!("unknown profile: {profile}"))?;
    let (id, secret) = auth::keychain::load_with_env_fallback(profile)?.ok_or_else(|| {
        anyhow::anyhow!("no credentials for [{profile}]; run `flute-webhook auth login`")
    })?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let fetcher = Arc::new(auth::token::OAuth2Fetcher {
        oauth_url: p.oauth_url.clone(),
        client_id: id,
        client_secret: secret,
        http: http.clone(),
    });
    let api = api::ApiClient {
        base_url: p.api_base_url.clone(),
        http,
        tokens: auth::token::TokenStore::new(fetcher),
    };
    cli::webhooks::run(&api, output, cmd).await
}

/// Headless polling mode: every new successful delivery seen on the API gets
/// POST'd to `forward_to` with the original headers and JSON body. Runs in the
/// foreground until Ctrl-C. Uses the same OAuth2 + ApiClient stack as the TUI
/// — the only difference is the absence of the ratatui front-end.
async fn listen(profile: &str, forward_to: &str) -> anyhow::Result<()> {
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::Duration;

    let p = config::Profile::by_name(profile)
        .ok_or_else(|| anyhow::anyhow!("unknown profile: {profile}"))?;
    let cfg = config::load_or_default();
    let secs = config::validate_poll_interval(cfg.poll_interval_seconds).seconds;

    let (id, secret) = auth::keychain::load_with_env_fallback(profile)?.ok_or_else(|| {
        anyhow::anyhow!("no credentials for [{profile}]; run `flute-webhook auth login`")
    })?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let fetcher = Arc::new(auth::token::OAuth2Fetcher {
        oauth_url: p.oauth_url.clone(),
        client_id: id,
        client_secret: secret,
        http: http.clone(),
    });
    let api = api::ApiClient {
        base_url: p.api_base_url.clone(),
        http: http.clone(),
        tokens: auth::token::TokenStore::new(fetcher),
    };

    println!("flute-webhook listen — profile=[{profile}] forwarding to {forward_to}");
    println!("Ctrl-C to stop.");

    // Prime seen_log_ids with the first page of currently-known logs so we
    // don't replay history on startup. From here on we only forward genuinely
    // new successful arrivals.
    let mut seen: HashSet<String> = match api.list_delivery_logs(500).await {
        Ok(r) => r
            .items
            .unwrap_or_default()
            .into_iter()
            .map(|l| l.id)
            .collect(),
        Err(e) => {
            eprintln!("warm-up fetch failed: {e}");
            HashSet::new()
        }
    };

    loop {
        tokio::time::sleep(Duration::from_secs(secs)).await;

        match api.list_delivery_logs(500).await {
            Ok(resp) => {
                let logs = resp.items.unwrap_or_default();
                for l in logs.iter() {
                    let success =
                        matches!(l.status, api::models::WebhookDeliveryLogStatus::Success);
                    if success && !seen.contains(&l.id) {
                        let prefix = l.id.get(..8.min(l.id.len())).unwrap_or("");
                        match forward::forward_log(&http, &api, &l.id, forward_to).await {
                            Ok(_) => println!(
                                "forwarded {prefix}… ({})",
                                l.event_type.clone().unwrap_or_default()
                            ),
                            Err(e) => eprintln!("forward {prefix}… failed: {e}"),
                        }
                    }
                }
                for l in logs {
                    seen.insert(l.id);
                }
            }
            Err(e) => {
                eprintln!("poll failed: {e}");
            }
        }
    }
}

/// Route tracing output based on (is_tui, debug):
///   tui + debug      -> ~/.flute/flute-webhook.log at DEBUG (stdout owned by ratatui)
///   tui + no debug   -> ~/.flute/flute-webhook.log at INFO/WARN
///   non-tui + debug  -> stdout at DEBUG (per --debug spec: "outputs every HTTP call to stdout")
///   non-tui + normal -> stderr at INFO/WARN
///
/// RUST_LOG always overrides the default filter when set.
fn init_tracing(is_tui: bool, debug: bool) {
    let env_filter = if debug {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "debug,flute_webhook=debug,reqwest=debug,hyper=info".into())
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "warn,flute_webhook=info".into())
    };

    if is_tui {
        let _ = std::fs::create_dir_all(config::config_dir());
        let log_path = config::config_dir().join("flute-webhook.log");
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            Ok(file) => {
                let _ = tracing_subscriber::fmt()
                    .with_env_filter(env_filter)
                    .with_writer(Mutex::new(file))
                    .with_ansi(false)
                    .try_init();
                if debug {
                    eprintln!("debug mode: HTTP traces -> {}", log_path.display());
                }
            }
            Err(_) => {
                let _ = tracing_subscriber::fmt()
                    .with_env_filter("off")
                    .with_writer(std::io::sink)
                    .try_init();
            }
        }
    } else if debug {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(std::io::stdout)
            .with_ansi(false)
            .try_init();
    } else {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(std::io::stderr)
            .try_init();
    }
}

async fn auth_login(profile: &str) -> anyhow::Result<()> {
    use std::io::{self, BufRead, Write};
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    print!("client_id for [{profile}]: ");
    stdout.flush()?;
    let mut id = String::new();
    stdin.lock().read_line(&mut id)?;
    let id = id.trim().to_string();

    let secret = rpassword::prompt_password(format!("client_secret for [{profile}]: "))?;
    let secret = secret.trim().to_string();

    if id.is_empty() || secret.is_empty() {
        anyhow::bail!("client_id and client_secret are both required");
    }

    auth::keychain::store_client_credentials(profile, &id, &secret)?;
    println!("Stored credentials for profile [{profile}] in OS keychain.");
    Ok(())
}

async fn auth_print_token(profile: &str) -> anyhow::Result<()> {
    let p = config::Profile::by_name(profile)
        .ok_or_else(|| anyhow::anyhow!("unknown profile: {profile}"))?;
    let (id, secret) = auth::keychain::load_with_env_fallback(profile)?.ok_or_else(|| {
        anyhow::anyhow!("no credentials for [{profile}]; run `flute-webhook auth login`")
    })?;
    let fetcher = std::sync::Arc::new(auth::token::OAuth2Fetcher {
        oauth_url: p.oauth_url,
        client_id: id,
        client_secret: secret,
        http: reqwest::Client::new(),
    });
    let store = auth::token::TokenStore::new(fetcher);
    let bearer = store.bearer().await?;
    println!("{bearer}");
    Ok(())
}
