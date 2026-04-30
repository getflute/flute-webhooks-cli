pub mod api;
pub mod auth;
pub mod cli;
pub mod config;
pub mod domain;
pub mod poller;
pub mod tui;

use clap::Parser;

pub fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,flute_webhook=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

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
