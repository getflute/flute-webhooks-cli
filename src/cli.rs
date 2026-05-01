use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "flute-webhook", version, about = "Flute Webhooks TUI and helpers")]
pub struct Cli {
    #[arg(long, env = "FLUTE_PROFILE", default_value = "uat", global = true)]
    pub profile: String,

    /// Print every HTTP request and response (status, URL, body) at debug
    /// level. Output goes to stdout for non-TUI commands and to
    /// ~/.flute/flute-webhook.log for the TUI (which owns stdout).
    #[arg(long, global = true)]
    pub debug: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Launch the interactive TUI (default if no subcommand is given).
    Tui,

    /// Auth subcommands.
    #[command(subcommand)]
    Auth(AuthCommand),
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    /// Prompt for client_id + client_secret and store them in the OS keychain.
    Login,

    /// Print the current bearer token (debugging aid).
    Token,
}
