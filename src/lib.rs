pub mod api;
pub mod auth;
pub mod cli;
pub mod config;
pub mod domain;
pub mod poller;
pub mod tui;

use clap::Parser;
use std::sync::Mutex;

pub fn run() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    let profile = cli.profile.clone();
    let debug = cli.debug;
    let cmd = cli.command.unwrap_or(cli::Command::Tui);
    let is_tui = matches!(cmd, cli::Command::Tui);

    init_tracing(is_tui, debug);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        match cmd {
            cli::Command::Tui => tui::run(&profile).await,
            cli::Command::Auth(cli::AuthCommand::Login) => auth_login(&profile).await,
            cli::Command::Auth(cli::AuthCommand::Token) => auth_print_token(&profile).await,
        }
    })
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
        match std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
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
    let (id, secret) = auth::keychain::load_with_env_fallback(profile)?
        .ok_or_else(|| anyhow::anyhow!("no credentials for [{profile}]; run `flute-webhook auth login`"))?;
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
