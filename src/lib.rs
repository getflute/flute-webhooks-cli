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
    init_tracing();

    let cli = cli::Cli::parse();
    let profile = cli.profile.clone();
    let cmd = cli.command.unwrap_or(cli::Command::Tui);

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

/// Route tracing to ~/.flute/flute-webhook.log so log lines don't corrupt the
/// alternate-screen TUI render. If the file can't be opened (e.g. read-only
/// home directory), we fall back to a no-op subscriber rather than stderr —
/// stderr-noise interleaving with ratatui paints is the bug we're fixing.
fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "warn,flute_webhook=info".into());

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
        }
        Err(_) => {
            // No log file available — silence tracing entirely so we never
            // write to stderr while the TUI owns the terminal.
            let _ = tracing_subscriber::fmt()
                .with_env_filter("off")
                .with_writer(std::io::sink)
                .try_init();
        }
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
