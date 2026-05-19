//! Clap argument tree for the `flute-webhooks-cli` binary.
//!
//! The web of subcommands matches the `flute-webhooks-cli webhooks …` shape from
//! the FEATURE-FLUTE-CLI spec § 6 — endpoints CRUD + ping, event-types,
//! delivery logs CRUD + retry. Output formatting and dispatch live in the
//! sibling modules: `output` and `webhooks`.

use clap::{Parser, Subcommand, ValueEnum};

pub mod output;
pub mod webhooks;

#[derive(Parser, Debug)]
#[command(
    name = "flute-webhooks-cli",
    version,
    about = "Flute Webhooks TUI and helpers"
)]
pub struct Cli {
    #[arg(long, env = "FLUTE_PROFILE", default_value = "sandbox", global = true)]
    pub profile: String,

    /// Print every HTTP request and response (status, URL, body) at debug
    /// level. Output goes to stdout for non-TUI commands and to
    /// ~/.flute/flute-webhooks-cli.log for the TUI (which owns stdout).
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

    /// Print the current bearer token (debugging aid).
    Token,
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

    /// Update an existing endpoint. Each flag is optional — omitted fields
    /// keep their current value (we GET the current state first, merge the
    /// supplied flags, and PUT the merged version).
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

    /// Delete an endpoint. Refuses without `--yes` to prevent accidents.
    Delete {
        id: String,

        /// Skip the interactive confirmation prompt.
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
