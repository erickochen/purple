//! clap derive structs for the `purple` CLI.
//!
//! Everything related to argument parsing lives here so `main.rs` stays
//! focused on orchestration. `Cli` is the top-level `Parser`; each
//! `Commands` variant holds the subcommand-specific args.

use clap::{Parser, Subcommand};
use clap_complete::Shell;

#[derive(Parser)]
#[command(
    name = "purple",
    about = "Your SSH config is a mess. Purple fixes that.",
    long_about = "Purple is a terminal SSH client for managing your hosts.\n\
                  Add, edit, delete and connect without opening a text editor.\n\n\
                  Life's too short for nano ~/.ssh/config.",
    version
)]
pub struct Cli {
    /// Connect to a host by alias, or filter the TUI
    #[arg(value_name = "ALIAS")]
    pub alias: Option<String>,

    /// Connect directly to a host by alias (skip the TUI)
    #[arg(short, long)]
    pub connect: Option<String>,

    /// List all configured hosts
    #[arg(short, long)]
    pub list: bool,

    /// Path to SSH config file [default: PURPLE_SSH_CONFIG, then ssh_config in preferences, then ~/.ssh/config]
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<String>,

    /// Launch with demo data (no real config needed)
    #[arg(long)]
    pub demo: bool,

    /// Generate shell completions
    #[arg(long, value_name = "SHELL")]
    pub completions: Option<Shell>,

    /// Override theme for this session
    #[arg(long)]
    pub theme: Option<String>,

    /// Enable verbose logging (debug level)
    #[arg(
        long,
        long_help = "Enable verbose logging (debug level).\n\n\
                     Logs are written to purple.log in the state directory,\n\
                     ~/.purple by default (rotates at 5MB). `purple logs` prints\n\
                     the path; tail with `purple logs --tail`.\n\n\
                     Set PURPLE_LOG=trace|debug|info|warn|error to override the\n\
                     level without --verbose. PURPLE_LOG takes precedence."
    )]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Commands {
    /// Quick-add a host: purple add user@host:port --alias myserver
    Add {
        /// Target in user@hostname:port format
        target: String,

        /// Alias for the host (default: derived from hostname)
        #[arg(short, long)]
        alias: Option<String>,

        /// Path to identity file (SSH key)
        #[arg(short, long)]
        key: Option<String>,
    },
    /// Import hosts from a file or known_hosts
    Import {
        /// File with one host per line (user@host:port format)
        file: Option<String>,

        /// Import from ~/.ssh/known_hosts instead
        #[arg(long)]
        known_hosts: bool,

        /// Group label for imported hosts
        #[arg(short, long)]
        group: Option<String>,
    },
    /// Sync hosts from cloud providers (DigitalOcean, Vultr, Linode, Hetzner, UpCloud, Proxmox VE, AWS EC2, Scaleway, GCP, Azure, Tailscale, Oracle Cloud, OVHcloud, Leaseweb, i3D.net, TransIP, Teleport, NetBox)
    Sync {
        /// Sync a specific provider (default: all configured)
        provider: Option<String>,

        /// Preview changes without modifying config
        #[arg(long)]
        dry_run: bool,

        /// Remove hosts that no longer exist on the provider
        #[arg(long)]
        remove: bool,
    },
    /// Manage cloud provider configurations
    Provider {
        #[command(subcommand)]
        command: ProviderCommands,
    },
    /// Manage SSH tunnels
    Tunnel {
        #[command(subcommand)]
        command: TunnelCommands,
    },
    /// Manage passwords in the OS keychain for SSH hosts
    Password {
        #[command(subcommand)]
        command: PasswordCommands,
    },
    /// Manage command snippets for quick execution on hosts
    Snippet {
        #[command(subcommand)]
        command: SnippetCommands,
    },
    /// Update purple to the latest version
    Update,
    /// Start MCP server (Model Context Protocol) for AI agent integration
    Mcp {
        /// Restrict tools to read-only operations. Denies run_command and container_action,
        /// and removes them from tools/list. Recommended when exposing purple to autonomous agents.
        #[arg(long)]
        read_only: bool,

        /// Disable the MCP audit log. By default every tool call is appended to
        /// mcp-audit.log in the state directory (~/.purple by default) as JSON Lines.
        #[arg(long)]
        no_audit: bool,

        /// Custom path for the MCP audit log (default: mcp-audit.log in the state
        /// directory, ~/.purple by default). Ignored when --no-audit is set.
        #[arg(long, value_name = "PATH")]
        audit_log: Option<String>,
    },
    /// Manage color themes
    Theme {
        #[command(subcommand)]
        command: ThemeCommands,
    },
    /// HashiCorp Vault SSH secrets engine operations (signed SSH certificates)
    Vault {
        #[command(subcommand)]
        command: VaultCommands,
    },
    /// View or manage log file
    Logs {
        /// Follow log output in real time
        #[arg(long)]
        tail: bool,

        /// Delete the log file
        #[arg(long)]
        clear: bool,
    },
    /// Print release notes since a prior version
    WhatsNew {
        /// Only include entries newer than this version (e.g. 2.40.0)
        #[arg(long)]
        since: Option<String>,
    },
    /// Render marketing SVG screenshots from the demo TUI. Internal: drives the
    /// release imagery pipeline so README and landing-page assets never go stale.
    #[command(hide = true)]
    GenAssets {
        /// Output directory for the generated SVGs
        #[arg(default_value = "assets/svg")]
        out_dir: String,

        /// Directory holding ui-mono-regular.woff2 and ui-mono-bold.woff2 to
        /// embed. Pass an empty value to render with the system monospace.
        #[arg(long, default_value = "assets/fonts")]
        font_dir: String,

        /// Also write the looping animated hero SVG (all five tabs with
        /// detail panels, search typing itself) to this path.
        #[arg(long)]
        hero_out: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum VaultCommands {
    /// Sign an SSH certificate for a host (or --all) via the Vault SSH secrets engine
    #[command(
        long_about = "Sign one or more SSH certificates via the HashiCorp Vault SSH secrets engine.\n\n\
        Prerequisites:\n\
        - The `vault` CLI is installed and authenticated (run `vault login` or set VAULT_TOKEN)\n\
        - VAULT_ADDR points at your Vault server\n\
        - A role is configured on the host (Vault SSH role field in the host form) or\n  \
          on its provider (provider-level vault_role default)\n\
        - The SSH secrets engine is enabled on Vault and your token has `update` capability\n  \
          on the role path\n\n\
        Signed certificates are stored as certs/<alias>-cert.pub in the data\n\
        directory (~/.purple by default) and `CertificateFile` is wired into the\n\
        SSH config automatically.\n\n\
        Distinct from the Vault KV secrets engine used as a password source (`vault:`\n\
        askpass prefix); see `purple password` for that."
    )]
    Sign {
        /// Host alias to sign (omit for --all)
        alias: Option<String>,
        /// Sign all hosts with a Vault SSH role configured
        #[arg(long)]
        all: bool,
        /// Override VAULT_ADDR for this invocation only.
        /// Highest precedence: flag > per-host comment > provider default > shell env.
        #[arg(long, value_name = "URL")]
        vault_addr: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ThemeCommands {
    /// List available themes
    List,
    /// Set the active theme
    Set {
        /// Theme name
        name: String,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum ProviderCommands {
    /// Add or update a provider configuration
    Add {
        /// Provider name (digitalocean, vultr, linode, hetzner, upcloud, proxmox, aws, scaleway, gcp, azure, tailscale, oracle, ovh, leaseweb, i3d, transip, teleport, netbox)
        provider: String,

        /// API token (or set PURPLE_TOKEN env var, or use --token-stdin)
        #[arg(long)]
        token: Option<String>,

        /// Read token from stdin (e.g. from a password manager)
        #[arg(long)]
        token_stdin: bool,

        /// Alias prefix (default: provider short label)
        #[arg(long)]
        prefix: Option<String>,

        /// Default SSH user (default: root)
        #[arg(long)]
        user: Option<String>,

        /// Default identity file
        #[arg(long)]
        key: Option<String>,

        /// Base URL for self-hosted providers (required for Proxmox and NetBox)
        #[arg(long)]
        url: Option<String>,

        /// AWS credential profile from ~/.aws/config or ~/.aws/credentials.
        /// A profile with role_arn and source_profile is assumed via STS, so
        /// one key pair reaches several accounts.
        #[arg(long)]
        profile: Option<String>,

        /// Reach AWS instances through Systems Manager Session Manager:
        /// `off` (default), `auto` to route only the nodes Session Manager
        /// reports online or `always` to route every instance without asking
        #[arg(long, value_name = "MODE")]
        ssm: Option<String>,

        /// Comma-separated regions, zones or subscription IDs (e.g. us-east-1,eu-west-1 for AWS, fr-par-1,nl-ams-1 for Scaleway, us-central1-a for GCP zones or subscription UUIDs for Azure)
        #[arg(long)]
        regions: Option<String>,

        /// GCP project ID
        #[arg(long)]
        project: Option<String>,

        /// OCI compartment OCID (Oracle)
        #[arg(long)]
        compartment: Option<String>,

        /// NetBox filter query string (e.g. "status=active&tag=ssh")
        #[arg(long)]
        filter: Option<String>,

        /// Skip TLS certificate verification (for self-signed certs)
        #[arg(long, conflicts_with = "verify_tls")]
        no_verify_tls: bool,

        /// Explicitly enable TLS certificate verification (overrides stored setting)
        #[arg(long, conflicts_with = "no_verify_tls")]
        verify_tls: bool,

        /// Enable automatic sync on startup
        #[arg(long, conflicts_with = "no_auto_sync")]
        auto_sync: bool,

        /// Disable automatic sync on startup
        #[arg(long, conflicts_with = "auto_sync")]
        no_auto_sync: bool,

        /// Optional label when adding a second config for the same
        /// provider (e.g. --label work, --label personal). Required once
        /// a provider already has a labeled config.
        #[arg(long)]
        label: Option<String>,
    },
    /// List configured providers
    List,
    /// Remove a provider configuration. Pass `provider` for ALL configs
    /// of that provider, or `provider:label` for one specific config.
    Remove {
        /// Provider name (`digitalocean`) or `provider:label` (`digitalocean:work`)
        provider: String,
    },
}

#[derive(Subcommand)]
pub enum TunnelCommands {
    /// List configured tunnels
    List {
        /// Show tunnels for a specific host
        alias: Option<String>,
    },
    /// Add a tunnel to a host
    Add {
        /// Host alias
        alias: String,

        /// Forward spec: L:port:host:port (local), R:port:host:port (remote) or D:port (SOCKS)
        forward: String,
    },
    /// Remove a tunnel from a host
    Remove {
        /// Host alias
        alias: String,

        /// Forward spec: L:port:host:port (local), R:port:host:port (remote) or D:port (SOCKS)
        forward: String,
    },
    /// Start a tunnel (foreground, Ctrl+C to stop)
    Start {
        /// Host alias
        alias: String,
    },
}

#[derive(Subcommand)]
pub enum PasswordCommands {
    /// Store a password in the OS keychain for a host
    Set {
        /// Host alias
        alias: String,
    },
    /// Remove a password from the OS keychain
    Remove {
        /// Host alias
        alias: String,
    },
}

#[derive(Subcommand)]
pub enum SnippetCommands {
    /// List all saved snippets
    List,
    /// Add a new snippet
    Add {
        /// Snippet name
        name: String,

        /// Command to run on the remote host
        command: String,

        /// Short description
        #[arg(long)]
        description: Option<String>,

        /// Run in an interactive shell so ~/.bashrc or ~/.zshrc is loaded (e.g. for nvm)
        #[arg(long)]
        interactive: bool,
    },
    /// Remove a snippet
    Remove {
        /// Snippet name
        name: String,
    },
    /// Run a snippet on one or more hosts
    Run {
        /// Snippet name
        name: String,

        /// Host alias (run on a single host)
        alias: Option<String>,

        /// Run on all hosts matching this tag
        #[arg(long)]
        tag: Option<String>,

        /// Run on all hosts
        #[arg(long)]
        all: bool,

        /// Run on hosts concurrently
        #[arg(long)]
        parallel: bool,
    },
}
