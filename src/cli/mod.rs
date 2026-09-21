//! Clap argument tree for the `flute-webhooks` binary.
//!
//! The web of subcommands matches the `flute-webhooks webhooks …` shape from
//! the FEATURE-FLUTE-CLI spec § 6 — endpoints CRUD + ping, event-types,
//! delivery logs CRUD + retry. Output formatting and dispatch live in the
//! sibling modules: `output` and `webhooks`.

use clap::{Parser, Subcommand, ValueEnum};

pub mod output;
pub mod webhooks;

#[derive(Parser, Debug)]
#[command(
    name = "flute-webhooks",
    version,
    about = "Flute Webhooks TUI and helpers"
)]
pub struct Cli {
    #[arg(long, env = "FLUTE_PROFILE", default_value = "sandbox", global = true)]
    pub profile: String,

    /// Print every HTTP request and response (status, URL, body) at debug
    /// level. Output goes to stdout for non-TUI commands and to
    /// ~/.flute/flute-webhooks.log for the TUI (which owns stdout).
    #[arg(long, global = true)]
    pub debug: bool,

    /// Output format for non-interactive commands. `table` is human-readable
    /// (default), `json` is `serde_json::to_string_pretty` of the response —
    /// pipe-friendly for `jq`.
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Table)]
    pub output: OutputFormat,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Json,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Launch the interactive TUI.
    Tui,

    /// Auth subcommands.
    #[command(subcommand)]
    Auth(AuthCommand),

    /// Poll the Flute webhook delivery logs and forward every NEW successful
    /// delivery's headers + body to a local URL (e.g. http://127.0.0.1:3000).
    /// Runs in the foreground until Ctrl-C.
    Listen {
        /// Local URL to POST forwarded payloads to.
        #[arg(long)]
        forward_to: String,
    },

    /// Webhook API subcommands: endpoints, deliveries, event-types.
    #[command(subcommand)]
    Webhooks(WebhooksCommand),

    /// Check GitHub Releases for a newer version of flute-webhooks-cli and, if
    /// found and this binary was installed via a cargo-dist installer
    /// (shell, Homebrew, or PowerShell), self-update in place. Users who
    /// built from source get an informational message instead.
    Update,
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    /// Prompt for client_id + client_secret and store them in the OS keychain.
    Login,

    /// Clear stored credentials for the active profile.
    Logout,

    /// Print the current bearer token (debugging aid).
    ///
    /// `token` is accepted as a deprecated hidden alias for backward
    /// compatibility — scripts should migrate to `keys`.
    #[command(alias = "token")]
    Keys,
}

#[derive(Subcommand, Debug)]
pub enum WebhooksCommand {
    /// Manage webhook endpoints.
    #[command(subcommand)]
    Endpoints(EndpointsCommand),

    /// Inspect the available subscribable event types.
    #[command(subcommand, name = "event-types")]
    EventTypes(EventTypesCommand),

    /// Inspect or retry webhook delivery attempts.
    #[command(subcommand)]
    Deliveries(DeliveriesCommand),
}

#[derive(Subcommand, Debug)]
pub enum EndpointsCommand {
    /// List all webhook endpoints for the active profile.
    List,

    /// Get a single endpoint by id.
    Get { id: String },

    /// Create a new endpoint. The signing secret is returned exactly once.
    Create {
        /// HTTPS callback URL.
        #[arg(long)]
        url: String,

        /// Comma-separated list of event types to subscribe to (e.g.
        /// `transaction.card.captured,refund.completed`).
        #[arg(long, value_delimiter = ',')]
        events: Vec<String>,

        /// Friendly display name (defaults to "Untitled Webhook").
        #[arg(long)]
        name: Option<String>,
    },

    /// Update an existing endpoint. Each flag is optional. The CLI sends a
    /// sparse RFC 7396 JSON Merge Patch containing only the fields you
    /// supplied; every other field is left unchanged server-side.
    Update {
        id: String,

        #[arg(long)]
        url: Option<String>,

        #[arg(long, value_delimiter = ',')]
        events: Option<Vec<String>>,

        #[arg(long)]
        name: Option<String>,

        #[arg(long, value_enum)]
        status: Option<EndpointStatusArg>,
    },

    /// Delete an endpoint. Without `--yes`, prompts interactively (table
    /// mode + TTY stdin) or refuses (JSON mode, or when stdin is not a TTY).
    Delete {
        id: String,

        /// Skip the interactive confirmation prompt. Required under
        /// `--output json` and any non-TTY stdin (piped input, CI, MCP).
        #[arg(long)]
        yes: bool,
    },

    /// Send a test ping to an endpoint and print the listener's reply.
    Ping { id: String },
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum EndpointStatusArg {
    Active,
    Inactive,
}

#[derive(Subcommand, Debug)]
pub enum EventTypesCommand {
    /// List every subscribable event type, grouped by category.
    List,
}

#[derive(Subcommand, Debug)]
pub enum DeliveriesCommand {
    /// List recent delivery attempts.
    List {
        /// Restrict to a single endpoint id.
        #[arg(long)]
        endpoint_id: Option<String>,

        /// Restrict to Success or Failure.
        #[arg(long, value_enum)]
        status: Option<DeliveryStatusArg>,

        /// Maximum rows to fetch (default 50).
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },

    /// Print the full request + response detail for a single delivery.
    Get { id: String },

    /// Manually retry a failed delivery. Single-shot — no automatic retry chain.
    Retry { id: String },
}

#[derive(Copy, Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum DeliveryStatusArg {
    Success,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn auth_logout_accepts_global_options_before_and_after_subcommand() {
        for args in [
            vec![
                "flute-webhooks",
                "--profile",
                "production",
                "--output",
                "json",
                "auth",
                "logout",
            ],
            vec![
                "flute-webhooks",
                "auth",
                "logout",
                "--profile",
                "production",
                "--output",
                "json",
            ],
        ] {
            let parsed = Cli::try_parse_from(args).unwrap();
            assert!(matches!(
                parsed.command,
                Some(Command::Auth(AuthCommand::Logout))
            ));
            assert_eq!(parsed.profile, "production");
            assert_eq!(parsed.output, OutputFormat::Json);
        }
    }

    /// `auth keys` is the primary form; `auth token` is a deprecated hidden
    /// alias that must continue to resolve to the same variant so existing
    /// scripts don't break. Guards against accidental removal of the alias.
    #[test]
    fn auth_token_is_accepted_as_deprecated_alias_for_keys() {
        let primary = Cli::try_parse_from(["flute-webhooks", "auth", "keys"])
            .expect("`auth keys` should parse");
        let alias = Cli::try_parse_from(["flute-webhooks", "auth", "token"])
            .expect("`auth token` should still parse as the deprecated alias");

        for parsed in [primary, alias] {
            match parsed.command {
                Some(Command::Auth(AuthCommand::Keys)) => {}
                other => panic!("expected AuthCommand::Keys, got {other:?}"),
            }
        }
    }
}
