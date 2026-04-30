use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "flute-webhook", version, about = "Flute Webhooks TUI and helpers")]
pub struct Cli {
    #[arg(long, env = "FLUTE_PROFILE", default_value = "uat")]
    pub profile: String,

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
