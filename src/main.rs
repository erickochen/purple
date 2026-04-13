mod animation;
mod app;
mod askpass;
mod askpass_env;
mod cli;
mod clipboard;
mod connection;
mod containers;
mod demo;
mod demo_flag;
mod event;
mod file_browser;
mod fs_util;
mod handler;
mod history;
mod import;
mod logging;
mod mcp;
mod ping;
mod preferences;
mod providers;
mod quick_add;
mod snippet;
mod ssh_config;
mod ssh_keys;
mod tui;
mod tunnel;
mod ui;
mod update;
mod vault_ssh;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use log::warn;

use app::App;
use event::{AppEvent, EventHandler};
use ssh_config::model::SshConfigFile;

#[derive(Parser)]
#[command(
    name = "purple",
    about = "Your SSH config is a mess. Purple fixes that.",
    long_about = "Purple is a terminal SSH client for managing your hosts.\n\
                  Add, edit, delete and connect without opening a text editor.\n\n\
                  Life's too short for nano ~/.ssh/config.",
    version
)]
struct Cli {
    /// Connect to a host by alias, or filter the TUI
    #[arg(value_name = "ALIAS")]
    alias: Option<String>,

    /// Connect directly to a host by alias (skip the TUI)
    #[arg(short, long)]
    connect: Option<String>,

    /// List all configured hosts
    #[arg(short, long)]
    list: bool,

    /// Path to SSH config file
    #[arg(long, default_value = "~/.ssh/config")]
    config: String,

    /// Launch with demo data (no real config needed)
    #[arg(long)]
    demo: bool,

    /// Generate shell completions
    #[arg(long, value_name = "SHELL")]
    completions: Option<Shell>,

    /// Override theme for this session
    #[arg(long)]
    theme: Option<String>,

    /// Enable verbose logging (debug level)
    #[arg(long)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
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
    /// Sync hosts from cloud providers (DigitalOcean, Vultr, Linode, Hetzner, UpCloud, Proxmox VE, AWS EC2, Scaleway, GCP, Azure, Tailscale, Oracle Cloud, OVHcloud, Leaseweb, i3D.net, TransIP)
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
    Mcp,
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
}

#[derive(Subcommand)]
enum VaultCommands {
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
        Signed certificates are cached under ~/.purple/certs/<alias>-cert.pub and\n\
        `CertificateFile` is wired into the SSH config automatically.\n\n\
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
enum ThemeCommands {
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
enum ProviderCommands {
    /// Add or update a provider configuration
    Add {
        /// Provider name (digitalocean, vultr, linode, hetzner, upcloud, proxmox, aws, scaleway, gcp, azure, tailscale, oracle, ovh, leaseweb, i3d, transip)
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

        /// Base URL for self-hosted providers (required for Proxmox)
        #[arg(long)]
        url: Option<String>,

        /// AWS credential profile from ~/.aws/credentials
        #[arg(long)]
        profile: Option<String>,

        /// Comma-separated regions, zones or subscription IDs (e.g. us-east-1,eu-west-1 for AWS, fr-par-1,nl-ams-1 for Scaleway, us-central1-a for GCP zones or subscription UUIDs for Azure)
        #[arg(long)]
        regions: Option<String>,

        /// GCP project ID
        #[arg(long)]
        project: Option<String>,

        /// OCI compartment OCID (Oracle)
        #[arg(long)]
        compartment: Option<String>,

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
    },
    /// List configured providers
    List,
    /// Remove a provider configuration
    Remove {
        /// Provider name to remove
        provider: String,
    },
}

#[derive(Subcommand)]
enum TunnelCommands {
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
enum PasswordCommands {
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
enum SnippetCommands {
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

pub(crate) fn resolve_config_path(path: &str) -> Result<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(rest))
    } else {
        Ok(PathBuf::from(path))
    }
}

pub(crate) fn resolve_token(explicit: Option<String>, from_stdin: bool) -> Result<String> {
    if let Some(t) = explicit {
        return Ok(t);
    }
    if from_stdin {
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        return Ok(buf.trim().to_string());
    }
    if let Ok(t) = std::env::var("PURPLE_TOKEN") {
        return Ok(t);
    }
    anyhow::bail!("No token provided. Use --token, --token-stdin, or set PURPLE_TOKEN env var.")
}

fn main() -> Result<()> {
    // Askpass mode: when invoked as SSH_ASKPASS, handle the request and exit.
    // Must run before theme init and CLI parse to avoid terminal interference.
    if std::env::var("PURPLE_ASKPASS_MODE").is_ok() {
        return askpass::handle();
    }

    ui::theme::init();
    let cli = Cli::parse();

    // Determine if this is a CLI subcommand (log to stderr too) or TUI (file only)
    let is_cli_subcommand = cli.command.is_some() || cli.list || cli.connect.is_some();
    logging::init(cli.verbose, is_cli_subcommand);

    if let Some(ref name) = cli.theme {
        if let Some(theme) = ui::theme::ThemeDef::find_builtin(name).or_else(|| {
            ui::theme::ThemeDef::load_custom()
                .into_iter()
                .find(|t| t.name.eq_ignore_ascii_case(name))
        }) {
            ui::theme::set_theme(theme);
        } else {
            anyhow::bail!("Unknown theme: {}", name);
        }
    }

    // Shell completions (no config file needed)
    if let Some(shell) = cli.completions {
        let mut cmd = Cli::command();
        generate(shell, &mut cmd, "purple", &mut std::io::stdout());
        return Ok(());
    }

    if cli.demo {
        let app = demo::build_demo_app();
        return run_tui(app);
    }

    // Provider and Update subcommands don't need SSH config
    if let Some(Commands::Provider { command }) = cli.command {
        return cli::handle_provider_command(command);
    }
    if let Some(Commands::Update) = cli.command {
        return update::self_update();
    }
    if let Some(Commands::Password { command }) = cli.command {
        return cli::handle_password_command(command);
    }
    if let Some(Commands::Mcp) = cli.command {
        let config_path = resolve_config_path(&cli.config)?;
        return mcp::run(&config_path);
    }
    if let Some(Commands::Logs { tail, clear }) = cli.command {
        let path = logging::log_path().context("Could not determine log path")?;
        if clear {
            if path.exists() {
                std::fs::remove_file(&path)?;
                println!("Log file deleted: {}", path.display());
            } else {
                println!("No log file found at {}", path.display());
            }
        } else if tail {
            let status = std::process::Command::new("tail")
                .args(["-f", &path.to_string_lossy()])
                .status()
                .context("Failed to run tail")?;
            std::process::exit(status.code().unwrap_or(1));
        } else {
            println!("{}", path.display());
        }
        return Ok(());
    }
    if let Some(Commands::Theme { command }) = cli.command {
        match command {
            ThemeCommands::List => {
                let current = preferences::load_theme().unwrap_or_else(|| "Purple".to_string());
                println!("Built-in themes:");
                for theme in ui::theme::ThemeDef::builtins() {
                    let marker = if theme.name.eq_ignore_ascii_case(&current) {
                        "*"
                    } else {
                        " "
                    };
                    println!("  {} {}", marker, theme.name);
                }
                let custom = ui::theme::ThemeDef::load_custom();
                if !custom.is_empty() {
                    println!("\nCustom themes:");
                    for theme in &custom {
                        let marker = if theme.name.eq_ignore_ascii_case(&current) {
                            "*"
                        } else {
                            " "
                        };
                        println!("  {} {}", marker, theme.name);
                    }
                }
            }
            ThemeCommands::Set { name } => {
                let found = ui::theme::ThemeDef::find_builtin(&name).or_else(|| {
                    ui::theme::ThemeDef::load_custom()
                        .into_iter()
                        .find(|t| t.name.eq_ignore_ascii_case(&name))
                });
                match found {
                    Some(theme) => {
                        preferences::save_theme(&theme.name)?;
                        println!("Theme set to: {}", theme.name);
                    }
                    None => {
                        anyhow::bail!("Unknown theme: {}", name);
                    }
                }
            }
        }
        return Ok(());
    }

    let config_path = resolve_config_path(&cli.config)?;
    let mut config = SshConfigFile::parse(&config_path)?;
    let repaired_groups = config.repair_absorbed_group_comments();
    let orphaned_headers = config.remove_all_orphaned_group_headers();

    // Write startup banner to log file
    {
        let level_str = logging::level_name(cli.verbose);

        let provider_config = providers::config::ProviderConfig::load();

        let provider_names: Vec<String> = provider_config
            .sections
            .iter()
            .map(|s| s.provider.clone())
            .collect();

        let askpass_sources: Vec<String> = config
            .host_entries()
            .iter()
            .filter_map(|h| h.askpass.as_ref())
            .map(|s| s.to_string())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();

        let vault_ssh_info = {
            let has_host_level = config.host_entries().iter().any(|h| h.vault_ssh.is_some());
            let has_provider_level = provider_config
                .sections
                .iter()
                .any(|s| !s.vault_role.is_empty());
            if has_host_level || has_provider_level {
                // Resolve addr from all sources: per-host > per-provider > env var
                let addr = config
                    .host_entries()
                    .iter()
                    .find_map(|h| h.vault_addr.clone())
                    .or_else(|| {
                        provider_config
                            .sections
                            .iter()
                            .find(|s| !s.vault_addr.is_empty())
                            .map(|s| s.vault_addr.clone())
                    })
                    .or_else(|| std::env::var("VAULT_ADDR").ok())
                    .unwrap_or_else(|| "not set".to_string());
                Some(format!("enabled (addr={addr})"))
            } else {
                None
            }
        };

        let ssh_version = logging::detect_ssh_version();
        let term = std::env::var("TERM").unwrap_or_else(|_| "unset".to_string());
        let colorterm = std::env::var("COLORTERM").unwrap_or_else(|_| "unset".to_string());

        logging::write_banner(&logging::BannerInfo {
            version: env!("CARGO_PKG_VERSION"),
            config_path: &config_path.display().to_string(),
            providers: &provider_names,
            askpass_sources: &askpass_sources,
            vault_ssh_info: vault_ssh_info.as_deref(),
            ssh_version: &ssh_version,
            term: &term,
            colorterm: &colorterm,
            level: &level_str,
        });
    }

    // Handle subcommands that need SSH config
    match cli.command {
        Some(Commands::Add { target, alias, key }) => {
            return cli::handle_quick_add(config, &target, alias.as_deref(), key.as_deref());
        }
        Some(Commands::Import {
            file,
            known_hosts,
            group,
        }) => {
            return cli::handle_import(config, file.as_deref(), known_hosts, group.as_deref());
        }
        Some(Commands::Sync {
            provider,
            dry_run,
            remove,
        }) => {
            return cli::handle_sync(config, provider.as_deref(), dry_run, remove);
        }
        Some(Commands::Tunnel { command }) => {
            return cli::handle_tunnel_command(config, command);
        }
        Some(Commands::Snippet { command }) => {
            return cli::handle_snippet_command(config, command, &config_path);
        }
        Some(Commands::Vault {
            command:
                VaultCommands::Sign {
                    alias,
                    all,
                    vault_addr: cli_vault_addr,
                },
        }) => {
            if let Some(ref addr) = cli_vault_addr {
                if !vault_ssh::is_valid_vault_addr(addr) {
                    anyhow::bail!(
                        "Invalid --vault-addr value. Must be non-empty, no whitespace or control chars."
                    );
                }
            }
            let provider_config = providers::config::ProviderConfig::load();
            let entries = config.host_entries();

            if all {
                let mut signed = 0u32;
                let mut failed = 0u32;
                let mut skipped = 0u32;

                for entry in &entries {
                    let role = match vault_ssh::resolve_vault_role(
                        entry.vault_ssh.as_deref(),
                        entry.provider.as_deref(),
                        &provider_config,
                    ) {
                        Some(r) => r,
                        None => {
                            skipped += 1;
                            continue;
                        }
                    };

                    let pubkey = match vault_ssh::resolve_pubkey_path(&entry.identity_file) {
                        Ok(p) => p,
                        Err(e) => {
                            println!("Skipping {}: {}", entry.alias, e);
                            failed += 1;
                            continue;
                        }
                    };
                    let cert_path =
                        vault_ssh::resolve_cert_path(&entry.alias, &entry.certificate_file)?;
                    let status = vault_ssh::check_cert_validity(&cert_path);

                    if !vault_ssh::needs_renewal(&status) {
                        skipped += 1;
                        continue;
                    }

                    // Flag beats per-host beats provider default.
                    let resolved_addr = cli_vault_addr.clone().or_else(|| {
                        vault_ssh::resolve_vault_addr(
                            entry.vault_addr.as_deref(),
                            entry.provider.as_deref(),
                            &provider_config,
                        )
                    });
                    print!("Signing {}... ", entry.alias);
                    match vault_ssh::sign_certificate(
                        &role,
                        &pubkey,
                        &entry.alias,
                        resolved_addr.as_deref(),
                    ) {
                        Ok(result) => {
                            println!("\u{2713}");
                            // Honor the same invariant as the TUI paths: never
                            // overwrite a user-set CertificateFile.
                            if should_write_certificate_file(&entry.certificate_file) {
                                let updated = config.set_host_certificate_file(
                                    &entry.alias,
                                    &result.cert_path.to_string_lossy(),
                                );
                                if !updated {
                                    eprintln!(
                                        "  warning: {} no longer in ssh config; CertificateFile not written (cert saved on disk)",
                                        entry.alias
                                    );
                                }
                            }
                            signed += 1;
                        }
                        Err(e) => {
                            println!("failed: {}", e);
                            failed += 1;
                        }
                    }
                }
                if signed > 0 {
                    if let Err(e) = config.write() {
                        eprintln!("Warning: Failed to update SSH config: {}", e);
                    }
                }
                println!(
                    "\nSigned: {}, failed: {}, skipped (valid): {}",
                    signed, failed, skipped
                );
                if failed > 0 {
                    std::process::exit(1);
                }
            } else if let Some(alias) = alias {
                let entry = entries
                    .iter()
                    .find(|h| h.alias == alias)
                    .with_context(|| format!("Host '{}' not found", alias))?;

                let role = vault_ssh::resolve_vault_role(
                    entry.vault_ssh.as_deref(),
                    entry.provider.as_deref(),
                    &provider_config,
                )
                .with_context(|| {
                    format!(
                        "No Vault SSH role configured for '{}'. Set it in the host form (Vault SSH Role field) or in the provider config (vault_role).",
                        alias
                    )
                })?;

                let pubkey = vault_ssh::resolve_pubkey_path(&entry.identity_file)?;
                let resolved_addr = cli_vault_addr.clone().or_else(|| {
                    vault_ssh::resolve_vault_addr(
                        entry.vault_addr.as_deref(),
                        entry.provider.as_deref(),
                        &provider_config,
                    )
                });
                let result =
                    vault_ssh::sign_certificate(&role, &pubkey, &alias, resolved_addr.as_deref())?;
                // Honor the same invariant as the TUI paths: never overwrite a
                // user-set CertificateFile. Only write the directive (and the
                // SSH config) when the host has none yet.
                if should_write_certificate_file(&entry.certificate_file) {
                    let updated = config
                        .set_host_certificate_file(&alias, &result.cert_path.to_string_lossy());
                    if !updated {
                        // Host disappeared between the `entries` snapshot and
                        // the config mutation. In the single-host CLI path
                        // both reads happen back-to-back in the same process,
                        // so this is effectively unreachable — but surface it
                        // loudly if the invariant ever breaks instead of
                        // silently writing a cert nobody references.
                        anyhow::bail!(
                            "Host '{}' disappeared from ssh config before CertificateFile could be written. Cert saved to {}.",
                            alias,
                            result.cert_path.display()
                        );
                    }
                    config
                        .write()
                        .with_context(|| "Failed to update SSH config with CertificateFile")?;
                }
                println!("Certificate signed: {}", result.cert_path.display());
            } else {
                anyhow::bail!("Provide a host alias or use --all");
            }
            return Ok(());
        }
        Some(Commands::Provider { .. })
        | Some(Commands::Update)
        | Some(Commands::Password { .. })
        | Some(Commands::Mcp)
        | Some(Commands::Theme { .. })
        | Some(Commands::Logs { .. }) => unreachable!(),
        None => {}
    }

    // Direct connect mode (--connect)
    if let Some(alias) = cli.connect {
        let provider_config = providers::config::ProviderConfig::load();
        let entries = config.host_entries();
        let host_entry = entries.iter().find(|h| h.alias == alias).cloned();
        if let Some(ref host) = host_entry {
            if let Some((msg, _is_error)) =
                ensure_vault_ssh_if_needed(&alias, host, &provider_config, &mut config)
            {
                eprintln!("{}", msg);
            }
        }
        let askpass = host_entry
            .as_ref()
            .and_then(|h| h.askpass.clone())
            .or_else(preferences::load_askpass_default);
        let bw_session = ensure_bw_session(None, askpass.as_deref());
        ensure_keychain_password(&alias, askpass.as_deref());
        let result = connection::connect(
            &alias,
            &config_path,
            askpass.as_deref(),
            bw_session.as_deref(),
            false,
        )?;
        let code = result.status.code().unwrap_or(1);
        if code != 255 {
            history::ConnectionHistory::load().record(&alias);
        }
        askpass::cleanup_marker(&alias);
        std::process::exit(code);
    }

    // List mode
    if cli.list {
        let entries = config.host_entries();
        if entries.is_empty() {
            println!("No hosts configured. Run 'purple' to add some!");
        } else {
            for host in &entries {
                let user = if host.user.is_empty() {
                    String::new()
                } else {
                    format!("{}@", host.user)
                };
                let port = if host.port == 22 {
                    String::new()
                } else {
                    format!(":{}", host.port)
                };
                println!("{:<20} {}{}{}", host.alias, user, host.hostname, port);
            }
        }
        return Ok(());
    }

    // Positional argument: exact match → connect, otherwise → TUI with filter
    if let Some(ref alias) = cli.alias {
        let host_opt = config
            .host_entries()
            .iter()
            .find(|h| h.alias == *alias)
            .cloned();
        if let Some(host) = host_opt {
            let provider_config = providers::config::ProviderConfig::load();
            if let Some((msg, _is_error)) =
                ensure_vault_ssh_if_needed(&host.alias, &host, &provider_config, &mut config)
            {
                eprintln!("{}", msg);
            }
            let alias = host.alias.clone();
            let askpass = host
                .askpass
                .clone()
                .or_else(preferences::load_askpass_default);
            let bw_session = ensure_bw_session(None, askpass.as_deref());
            ensure_keychain_password(&alias, askpass.as_deref());
            println!("Beaming you up to {}...\n", alias);
            let result = connection::connect(
                &alias,
                &config_path,
                askpass.as_deref(),
                bw_session.as_deref(),
                false,
            )?;
            let code = result.status.code().unwrap_or(1);
            if code != 255 {
                history::ConnectionHistory::load().record(&alias);
            }
            askpass::cleanup_marker(&alias);
            std::process::exit(code);
        }
        // No exact match — open TUI with search pre-filled
        let mut app = App::new(config);
        apply_saved_sort(&mut app);
        if repaired_groups > 0 || orphaned_headers > 0 {
            app.set_status(
                format!(
                    "Repaired SSH config ({} absorbed, {} orphaned group headers).",
                    repaired_groups, orphaned_headers
                ),
                false,
            );
        }
        app.start_search_with(alias);
        if app.search.filtered_indices.is_empty() {
            app.set_status(
                format!("No exact match for '{}'. Here's what we found.", alias),
                false,
            );
        }
        return run_tui(app);
    }

    // Interactive TUI mode
    let mut app = App::new(config);
    apply_saved_sort(&mut app);
    if repaired_groups > 0 || orphaned_headers > 0 {
        app.set_status(
            format!(
                "Repaired SSH config ({} absorbed, {} orphaned group headers).",
                repaired_groups, orphaned_headers
            ),
            false,
        );
    }
    run_tui(app)
}

fn apply_saved_sort(app: &mut App) {
    let saved = preferences::load_sort_mode();
    let group = preferences::load_group_by();
    app.sort_mode = saved;
    app.group_by = group;
    app.view_mode = preferences::load_view_mode();
    // Clear stale tag preference if the tag no longer exists in any host
    if app.clear_stale_group_tag() {
        if let Err(e) = preferences::save_group_by(&app.group_by) {
            app.set_status(
                format!("Group preference reset. (save failed: {})", e),
                true,
            );
        }
    }
    if saved != app::SortMode::Original || !matches!(app.group_by, app::GroupBy::None) {
        app.apply_sort();
        // After startup sort, select the first host in the sorted order
        // rather than preserving the arbitrary first-in-config selection.
        app.select_first_host();
    }
}

/// Build a rolling sync summary from completed providers.
/// Format a sync diff summary like "(+3 ~1 -2)" from add/update/stale counts.
/// Returns empty string when all counts are zero.
/// Build the status-bar summary shown after a bulk Vault SSH signing run
/// completes. When `failed > 0` and `first_error` is present, the scrubbed
/// error is appended so the user sees the actual reason (missing role,
/// permission denied, connection refused, etc.) instead of a bare
/// "1 failed" count.
/// Replace the spinner frame prefix in a status text. Returns None if the
/// text does not start with a known spinner frame.
fn replace_spinner_frame(text: &str, new_frame: &str) -> Option<String> {
    let starts_with_spinner = crate::animation::SPINNER_FRAMES
        .iter()
        .any(|f| text.starts_with(f));
    if !starts_with_spinner {
        return None;
    }
    text.split_once(' ')
        .map(|(_, rest)| format!("{} {}", new_frame, rest))
}

fn format_vault_sign_summary(
    signed: u32,
    failed: u32,
    skipped: u32,
    first_error: Option<&str>,
) -> String {
    let total = signed + failed + skipped;
    let cert_word = if total == 1 {
        "certificate"
    } else {
        "certificates"
    };
    if failed > 0 {
        if let Some(err) = first_error {
            if total == 1 {
                // Single host: just show the error, no stats prefix
                return err.to_string();
            }
            format!(
                "Signed {} of {} {}. {} failed: {}",
                signed, total, cert_word, failed, err
            )
        } else {
            format!(
                "Signed {} of {} {}. {} failed",
                signed, total, cert_word, failed
            )
        }
    } else if skipped > 0 && signed == 0 {
        format!(
            "All {} {} already valid. Nothing to sign.",
            total, cert_word
        )
    } else if skipped > 0 {
        format!(
            "Signed {} of {} {}. {} already valid.",
            signed, total, cert_word, skipped
        )
    } else {
        format!("Signed {} of {} {}.", signed, total, cert_word)
    }
}

fn format_sync_diff(added: usize, updated: usize, stale: usize) -> String {
    let diff_parts: Vec<String> = [(added, "+"), (updated, "~"), (stale, "-")]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, prefix)| format!("{}{}", prefix, n))
        .collect();
    if diff_parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", diff_parts.join(" "))
    }
}

/// Shows "Synced: AWS, DO, Vultr" that grows as each provider finishes.
/// Clears the batch state once all providers are done so the status can expire normally.
fn set_sync_summary(app: &mut App) {
    let still_syncing = !app.syncing_providers.is_empty();
    let names = app.sync_done.join(", ");
    if still_syncing {
        app.set_background_status(format!("Synced: {}...", names), app.sync_had_errors);
    } else {
        app.set_background_status(format!("Synced: {}", names), app.sync_had_errors);
        app.sync_done.clear();
        app.sync_had_errors = false;
        app::SyncRecord::save_all(&app.sync_history);
    }
}

/// First-launch initialization: create ~/.purple/ and back up the original SSH config.
/// Returns `Some(has_backup)` if this was a first launch, or `None` if already initialized.
fn first_launch_init(purple_dir: &Path, config_path: &Path) -> Option<bool> {
    if purple_dir.exists() {
        return None;
    }
    if let Err(e) = std::fs::create_dir_all(purple_dir) {
        warn!("[config] Failed to create ~/.purple directory: {e}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(purple_dir, std::fs::Permissions::from_mode(0o700))
        {
            warn!("[config] Failed to set ~/.purple directory permissions: {e}");
        }
    }
    // One-time backup of the original SSH config before purple touches it.
    // Stored as config.original and never overwritten or pruned.
    let original_backup = purple_dir.join("config.original");
    if config_path.exists() {
        if let Err(e) = std::fs::copy(config_path, &original_backup) {
            warn!(
                "[config] Failed to backup SSH config to {}: {e}",
                original_backup.display()
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) =
                std::fs::set_permissions(&original_backup, std::fs::Permissions::from_mode(0o600))
            {
                warn!("[config] Failed to set backup permissions: {e}");
            }
        }
    }
    Some(original_backup.exists())
}

fn run_tui(mut app: App) -> Result<()> {
    // First-launch welcome hint (one-shot: creates .purple/ so it won't show again)
    if app.status.is_none() && !app.demo_mode {
        if let Some(home) = dirs::home_dir() {
            let purple_dir = home.join(".purple");
            if let Some(has_backup) = first_launch_init(&purple_dir, &app.reload.config_path) {
                let host_count = app.hosts.len();
                let known_hosts_count = if host_count == 0 {
                    import::count_known_hosts_candidates()
                } else {
                    0
                };
                app.known_hosts_count = known_hosts_count;
                app.screen = app::Screen::Welcome {
                    has_backup,
                    host_count,
                    known_hosts_count,
                };
            }
        }
    }

    let mut terminal = tui::Tui::new()?;
    terminal.enter()?;
    let events = EventHandler::new(250);
    let events_tx = events.sender();
    let mut last_config_check = std::time::Instant::now();

    // Skip background tasks in demo mode (ping status is pre-populated)
    if !app.demo_mode {
        // Auto-sync configured providers on startup (skipped when auto_sync=false)
        for section in app.provider_config.configured_providers().to_vec() {
            if !section.auto_sync {
                continue;
            }
            if !app.syncing_providers.contains_key(&section.provider) {
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                app.syncing_providers
                    .insert(section.provider.clone(), cancel.clone());
                handler::spawn_provider_sync(&section, events_tx.clone(), cancel);
            }
        }

        // Auto-ping all hosts on startup if enabled in preferences
        if app.auto_ping {
            let hosts_to_ping: Vec<(String, String, u16)> = app
                .hosts
                .iter()
                .filter(|h| !h.hostname.is_empty() && h.proxy_jump.is_empty())
                .map(|h| (h.alias.clone(), h.hostname.clone(), h.port))
                .collect();
            for h in &app.hosts {
                if !h.proxy_jump.is_empty() {
                    app.ping_status
                        .insert(h.alias.clone(), app::PingStatus::Skipped);
                }
            }
            if !hosts_to_ping.is_empty() {
                for (alias, _, _) in &hosts_to_ping {
                    app.ping_status
                        .insert(alias.clone(), app::PingStatus::Checking);
                }
                ping::ping_all(&hosts_to_ping, events.sender(), app.ping_generation);
            }
        }

        // Background version check
        update::spawn_version_check(events_tx.clone());
    } // end skip background tasks in demo mode

    let mut anim = animation::AnimationState::new();

    while app.running {
        anim.detect_transitions(&mut app);
        terminal.draw(&mut app, &mut anim)?;

        // During animation, use a short timeout for smooth frames (~60fps).
        // During ping checking, use 80ms timeout for spinner.
        // Otherwise, block until the next event arrives.
        let vault_signing = app.vault_signing_cancel.is_some();
        let event = if anim.is_animating(&app) {
            events.next_timeout(std::time::Duration::from_millis(16))?
        } else if anim.has_checking_hosts(&app) || vault_signing {
            events.next_timeout(std::time::Duration::from_millis(80))?
        } else {
            Some(events.next()?)
        };

        match event {
            Some(AppEvent::Key(key)) => {
                handler::handle_key_event(&mut app, key, &events_tx)?;
            }
            Some(AppEvent::Tick) | None => {
                app.tick_status();
                app.tick_toast();
                if anim.has_checking_hosts(&app) || vault_signing {
                    anim.tick_spinner();
                }
                // Update the spinner character in the signing status text
                // so the spinner animates between VaultSignProgress events.
                if vault_signing {
                    if let Some(ref mut status) = app.status {
                        if status.sticky && !status.is_error() {
                            let frame = crate::animation::SPINNER_FRAMES[anim.spinner_tick
                                as usize
                                % crate::animation::SPINNER_FRAMES.len()];
                            if let Some(updated) = replace_spinner_frame(&status.text, frame) {
                                status.text = updated;
                            }
                        }
                    }
                }
                // Expire ping results after 60s TTL
                if let Some(checked_at) = app.ping_checked_at {
                    if checked_at.elapsed() > std::time::Duration::from_secs(60) {
                        app.ping_status.clear();
                        app.ping_checked_at = None;
                        app.ping_generation += 1;
                        if app.filter_down_only {
                            app.cancel_search();
                        }
                        app.set_background_status("Ping expired. Press P to refresh.", false);
                    }
                }
                // Throttle config file stat() to every 4 seconds
                if last_config_check.elapsed() >= std::time::Duration::from_secs(4) {
                    app.check_config_changed();
                    last_config_check = std::time::Instant::now();
                }
                // Poll active tunnels for exit
                let exited = app.poll_tunnels();
                for (_alias, msg, is_error) in exited {
                    app.set_background_status(msg, is_error);
                }
            }
            Some(AppEvent::PingResult {
                alias,
                rtt_ms,
                generation,
            }) => {
                if generation == app.ping_generation {
                    let status = app::classify_ping(rtt_ms, app.slow_threshold_ms);
                    app.ping_status.insert(alias.clone(), status.clone());
                    // Propagate bastion status to all ProxyJump dependents.
                    // If this host is a bastion for other hosts, they inherit
                    // its reachability (one check per bastion, not per host).
                    app::propagate_ping_to_dependents(
                        &app.hosts,
                        &mut app.ping_status,
                        &alias,
                        &status,
                    );
                    // Update live filter/sort as results arrive
                    if app.filter_down_only {
                        app.apply_filter();
                    }
                    if app.sort_mode == app::SortMode::Status {
                        app.apply_sort();
                    }
                    // Update "last checked" timestamp when all pings are done
                    if !app.ping_status.is_empty()
                        && app
                            .ping_status
                            .values()
                            .all(|s| !matches!(s, app::PingStatus::Checking))
                    {
                        app.ping_checked_at = Some(std::time::Instant::now());
                    }
                }
            }
            Some(AppEvent::SyncProgress { provider, message }) => {
                // Only show per-provider progress while that provider is still syncing.
                // Late progress events (arriving after SyncComplete) are discarded.
                if app.syncing_providers.contains_key(&provider) && app.sync_done.is_empty() {
                    let name = providers::provider_display_name(&provider);
                    app.set_background_status(format!("{}: {}", name, message), false);
                }
            }
            Some(AppEvent::SyncComplete { provider, hosts }) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let display_name = providers::provider_display_name(&provider);
                let (_msg, is_err, total, added, updated, stale) =
                    app.apply_sync_result(&provider, hosts, false);
                if is_err {
                    app.sync_history.insert(
                        provider.clone(),
                        app::SyncRecord {
                            timestamp: now,
                            message: format!("{}: sync failed", display_name),
                            is_error: true,
                        },
                    );
                    app.sync_had_errors = true;
                } else {
                    let label = if total == 1 { "server" } else { "servers" };
                    let message = format!(
                        "{} {}{}",
                        total,
                        label,
                        format_sync_diff(added, updated, stale)
                    );
                    app.sync_history.insert(
                        provider.clone(),
                        app::SyncRecord {
                            timestamp: now,
                            message,
                            is_error: false,
                        },
                    );
                }
                app.syncing_providers.remove(&provider);
                app.sync_done.push(display_name.to_string());
                set_sync_summary(&mut app);
                // Reset config check timer so auto-reload doesn't immediately
                // detect our own write as an "external" change
                last_config_check = std::time::Instant::now();
            }
            Some(AppEvent::SyncPartial {
                provider,
                hosts,
                failures,
                total,
            }) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let display_name = providers::provider_display_name(provider.as_str());
                let (msg, is_err, synced, added, updated, stale) =
                    app.apply_sync_result(&provider, hosts, true);
                if is_err {
                    app.sync_history.insert(
                        provider.clone(),
                        app::SyncRecord {
                            timestamp: now,
                            message: msg,
                            is_error: true,
                        },
                    );
                } else {
                    let label = if synced == 1 { "server" } else { "servers" };
                    app.sync_history.insert(
                        provider.clone(),
                        app::SyncRecord {
                            timestamp: now,
                            message: format!(
                                "{} {}{} ({} of {} failed)",
                                synced,
                                label,
                                format_sync_diff(added, updated, stale),
                                failures,
                                total
                            ),
                            is_error: true,
                        },
                    );
                }
                app.sync_had_errors = true;
                app.syncing_providers.remove(&provider);
                app.sync_done.push(display_name.to_string());
                set_sync_summary(&mut app);
                last_config_check = std::time::Instant::now();
            }
            Some(AppEvent::SyncError { provider, message }) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let display_name = providers::provider_display_name(provider.as_str());
                app.sync_history.insert(
                    provider.clone(),
                    app::SyncRecord {
                        timestamp: now,
                        message: message.clone(),
                        is_error: true,
                    },
                );
                app.sync_had_errors = true;
                app.syncing_providers.remove(&provider);
                app.sync_done.push(display_name.to_string());
                set_sync_summary(&mut app);
                last_config_check = std::time::Instant::now();
            }
            Some(AppEvent::UpdateAvailable { version, headline }) => {
                app.update_available = Some(version);
                app.update_headline = headline;
            }
            Some(AppEvent::FileBrowserListing {
                alias,
                path,
                entries,
            }) => {
                let mut record_connection = false;
                if let Some(ref mut fb) = app.file_browser {
                    if fb.alias == alias {
                        fb.remote_loading = false;
                        match entries {
                            Ok(listing) => {
                                if !fb.connection_recorded {
                                    fb.connection_recorded = true;
                                    record_connection = true;
                                }
                                if fb.remote_path.is_empty() || fb.remote_path != path {
                                    fb.remote_path = path;
                                }
                                fb.remote_entries = listing;
                                fb.remote_error = None;
                                fb.remote_list_state = ratatui::widgets::ListState::default();
                                fb.remote_list_state.select(Some(0));
                            }
                            Err(msg) => {
                                if fb.remote_path.is_empty() {
                                    fb.remote_path = path;
                                }
                                fb.remote_error = Some(msg);
                                fb.remote_entries.clear();
                            }
                        }
                    }
                }
                if record_connection {
                    app.history.record(&alias);
                    app.apply_sort();
                }
                // Force full redraw: ssh may have written to /dev/tty
                terminal.force_redraw();
            }
            Some(AppEvent::ScpComplete {
                alias,
                success,
                message,
            }) => {
                // Track whether we need to spawn a remote refresh (can't do it inside the fb borrow
                // because spawn_remote_listing needs values from app too)
                let mut refresh_remote: Option<(
                    String,
                    Option<String>,
                    String,
                    bool,
                    file_browser::BrowserSort,
                )> = None;
                let matched = if let Some(ref mut fb) = app.file_browser {
                    if fb.alias == alias {
                        fb.transferring = None;
                        if success {
                            app.history.record(&alias);
                            fb.local_selected.clear();
                            fb.remote_selected.clear();
                            match file_browser::list_local(&fb.local_path, fb.show_hidden, fb.sort)
                            {
                                Ok(entries) => {
                                    fb.local_entries = entries;
                                    fb.local_error = None;
                                }
                                Err(e) => {
                                    fb.local_entries = Vec::new();
                                    fb.local_error = Some(e.to_string());
                                }
                            }
                            fb.local_list_state.select(Some(0));
                            if !fb.remote_path.is_empty() {
                                fb.remote_loading = true;
                                fb.remote_entries.clear();
                                fb.remote_error = None;
                                fb.remote_list_state = ratatui::widgets::ListState::default();
                                refresh_remote = Some((
                                    fb.alias.clone(),
                                    fb.askpass.clone(),
                                    fb.remote_path.clone(),
                                    fb.show_hidden,
                                    fb.sort,
                                ));
                            }
                        } else {
                            fb.transfer_error = Some(message.clone());
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if matched && success {
                    app.set_background_status("Transfer complete.", false);
                    // Rebuild display list so frecency sort and LAST column reflect the transfer
                    app.apply_sort();
                }
                if let Some((fb_alias, askpass_fb, path, show_hidden, sort)) = refresh_remote {
                    let config_path = app.reload.config_path.clone();
                    let has_tunnel = app.active_tunnels.contains_key(&fb_alias);
                    let bw = app.bw_session.clone();
                    let tx = events_tx.clone();
                    file_browser::spawn_remote_listing(
                        fb_alias,
                        config_path,
                        path,
                        show_hidden,
                        sort,
                        askpass_fb,
                        bw,
                        has_tunnel,
                        move |a, p, e| {
                            let _ = tx.send(AppEvent::FileBrowserListing {
                                alias: a,
                                path: p,
                                entries: e,
                            });
                        },
                    );
                }
                askpass::cleanup_marker(&alias);
                // Force full redraw: ssh may have written to /dev/tty
                terminal.force_redraw();
            }
            Some(AppEvent::SnippetHostDone {
                run_id,
                alias,
                stdout,
                stderr,
                exit_code,
            }) => {
                if exit_code == Some(0) {
                    app.history.record(&alias);
                    app.apply_sort();
                }
                if let Some(ref mut state) = app.snippet_output {
                    if state.run_id == run_id {
                        state.results.push(app::SnippetHostOutput {
                            alias,
                            stdout,
                            stderr,
                            exit_code,
                        });
                    }
                }
            }
            Some(AppEvent::SnippetProgress {
                run_id,
                completed,
                total,
            }) => {
                if let Some(ref mut state) = app.snippet_output {
                    if state.run_id == run_id {
                        state.completed = completed;
                        state.total = total;
                    }
                }
            }
            Some(AppEvent::SnippetAllDone { run_id }) => {
                if let Some(ref mut state) = app.snippet_output {
                    if state.run_id == run_id {
                        state.all_done = true;
                    }
                }
            }
            Some(AppEvent::ContainerListing { alias, result }) => {
                // Always update cache, even if overlay is closed
                match &result {
                    Ok((runtime, containers)) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        app.container_cache.insert(
                            alias.clone(),
                            crate::containers::ContainerCacheEntry {
                                timestamp: now,
                                runtime: *runtime,
                                containers: containers.clone(),
                            },
                        );
                        crate::containers::save_container_cache(&app.container_cache);
                    }
                    Err(e) => {
                        // Preserve runtime even on error
                        if let Some(rt) = e.runtime {
                            if let Some(entry) = app.container_cache.get_mut(&alias) {
                                entry.runtime = rt;
                            }
                        }
                    }
                }
                // Update overlay state if open
                if let Some(ref mut state) = app.container_state {
                    if state.alias == alias {
                        match result {
                            Ok((runtime, containers)) => {
                                state.runtime = Some(runtime);
                                state.containers = containers;
                                state.loading = false;
                                state.error = None;
                                if let Some(sel) = state.list_state.selected() {
                                    if sel >= state.containers.len() && !state.containers.is_empty()
                                    {
                                        state.list_state.select(Some(0));
                                    }
                                } else if !state.containers.is_empty() {
                                    state.list_state.select(Some(0));
                                }
                            }
                            Err(e) => {
                                if let Some(rt) = e.runtime {
                                    state.runtime = Some(rt);
                                }
                                state.loading = false;
                                state.error = Some(e.message);
                            }
                        }
                    }
                }
                askpass::cleanup_marker(&alias);
            }
            Some(AppEvent::ContainerActionComplete {
                alias,
                action,
                result,
            }) => {
                // Check if overlay matches and extract refresh info before set_status
                let should_refresh = if let Some(ref mut state) = app.container_state {
                    if state.alias == alias {
                        state.action_in_progress = None;
                        match result {
                            Ok(()) => {
                                state.loading = true;
                                Some((state.alias.clone(), state.askpass.clone(), state.runtime))
                            }
                            Err(e) => {
                                state.error = Some(e);
                                None
                            }
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some((refresh_alias, askpass, cached_runtime)) = should_refresh {
                    app.set_background_status(
                        format!("Container {} complete.", action.as_str()),
                        false,
                    );
                    let has_tunnel = app.active_tunnels.contains_key(&refresh_alias);
                    let config_path = app.reload.config_path.clone();
                    let bw = app.bw_session.clone();
                    let tx = events_tx.clone();
                    crate::containers::spawn_container_listing(
                        refresh_alias,
                        config_path,
                        askpass,
                        bw,
                        has_tunnel,
                        cached_runtime,
                        move |a, r| {
                            let _ = tx.send(AppEvent::ContainerListing {
                                alias: a,
                                result: r,
                            });
                        },
                    );
                }
                askpass::cleanup_marker(&alias);
            }
            Some(AppEvent::VaultSignResult {
                alias,
                certificate_file: existing_cert_file,
                success,
                message,
            }) => {
                if success {
                    // The CertificateFile snapshot is carried in the event so
                    // we never re-look up the host (which would be O(n) and
                    // racy under concurrent renames). `sign_certificate`
                    // always writes to purple's default path; if the host
                    // already has its own CertificateFile we leave the SSH
                    // config alone and cache validity against the configured
                    // path instead.
                    let mut host_missing = false;
                    if should_write_certificate_file(&existing_cert_file) {
                        if let Ok(cert_path) = vault_ssh::cert_path_for(&alias) {
                            let updated = app
                                .config
                                .set_host_certificate_file(&alias, &cert_path.to_string_lossy());
                            if !updated {
                                // Alias disappeared between scheduling the
                                // sign and processing the result (renamed or
                                // deleted from another code path). The cert
                                // is safely on disk; warn the user so they
                                // know the ssh config was NOT wired up.
                                host_missing = true;
                            }
                        }
                    }
                    // Single consolidated cache refresh path. `refresh_cert_cache`
                    // resolves the cert file (honoring any custom CertificateFile
                    // we just wrote) and records mtime for external-change
                    // detection. Replaces the two manual `check_cert_validity`
                    // inserts that used to live in both branches above.
                    app.refresh_cert_cache(&alias);
                    if host_missing {
                        app.set_status(
                            format!(
                                "Vault SSH cert saved for {} but host no longer in config (renamed or deleted). CertificateFile NOT written.",
                                alias
                            ),
                            true,
                        );
                    } else {
                        app.set_status(format!("Signed Vault SSH cert for {}", alias), false);
                    }
                } else {
                    app.set_status(
                        format!("Vault SSH: failed to sign {}: {}", alias, message),
                        true,
                    );
                }
            }
            Some(AppEvent::VaultSignProgress { alias, done, total }) => {
                // Truncate long aliases so the status line fits even on
                // narrow terminals; the full alias is recoverable from the
                // host list.
                const ALIAS_BUDGET: usize = 40;
                let display_alias: String = if alias.chars().count() > ALIAS_BUDGET {
                    let cut: String = alias.chars().take(ALIAS_BUDGET - 1).collect();
                    format!("{}\u{2026}", cut)
                } else {
                    alias.clone()
                };
                let spinner = crate::animation::SPINNER_FRAMES
                    [anim.spinner_tick as usize % crate::animation::SPINNER_FRAMES.len()];
                app.set_sticky_status(
                    format!(
                        "{} Signing {}/{}: {} (V to cancel)",
                        spinner, done, total, display_alias
                    ),
                    false,
                );
            }
            Some(AppEvent::VaultSignAllDone {
                signed,
                failed,
                skipped,
                cancelled,
                aborted_message,
                first_error,
            }) => {
                app.vault_signing_cancel = None;
                // Join the background thread now that it has finished.
                if let Some(handle) = app.vault_sign_thread.take() {
                    let _ = handle.join();
                }
                if let Some(msg) = aborted_message {
                    app.set_sticky_status(msg, true);
                    continue;
                }
                if cancelled {
                    let mut msg = format!(
                        "Vault SSH signing cancelled ({} signed, {} failed)",
                        signed, failed
                    );
                    if let Some(err) = first_error.as_ref() {
                        msg.push_str(": ");
                        msg.push_str(err);
                    }
                    if failed > 0 {
                        app.set_sticky_status(msg, true);
                    } else {
                        app.set_status(msg, false);
                    }
                    continue;
                }
                let summary_msg =
                    format_vault_sign_summary(signed, failed, skipped, first_error.as_deref());
                if signed > 0 {
                    if app.is_form_open() {
                        // Defer config write to avoid mtime conflict with open forms
                        app.pending_vault_config_write = true;
                        if failed > 0 {
                            app.set_sticky_status(summary_msg, true);
                        } else {
                            app.set_status(summary_msg, false);
                        }
                    } else if app.external_config_changed() {
                        // The on-disk ssh config (or an include) was modified
                        // by an external editor while the bulk-sign worker was
                        // running. Writing now would overwrite those edits.
                        // The certs are already safely on disk under
                        // ~/.purple/certs/, so we reload the fresh config from
                        // disk and re-apply `CertificateFile` directives only
                        // for hosts that still exist and still have no custom
                        // path. This way external edits are preserved AND new
                        // certs get wired up where safe.
                        let reapply: Vec<(String, String)> = app
                            .config
                            .host_entries()
                            .into_iter()
                            .filter_map(|h| {
                                if h.vault_ssh.is_some()
                                    && should_write_certificate_file(&h.certificate_file)
                                {
                                    vault_ssh::cert_path_for(&h.alias).ok().map(|p| {
                                        (h.alias.clone(), p.to_string_lossy().into_owned())
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect();
                        match ssh_config::model::SshConfigFile::parse(&app.reload.config_path) {
                            Ok(fresh) => {
                                app.config = fresh;
                                let mut reapplied = 0usize;
                                for (alias, cert_path) in &reapply {
                                    let entry = app
                                        .config
                                        .host_entries()
                                        .into_iter()
                                        .find(|h| &h.alias == alias);
                                    if let Some(entry) = entry {
                                        if should_write_certificate_file(&entry.certificate_file)
                                            && app
                                                .config
                                                .set_host_certificate_file(alias, cert_path)
                                        {
                                            reapplied += 1;
                                        }
                                    }
                                }
                                if reapplied > 0 {
                                    if let Err(e) = app.config.write() {
                                        app.set_sticky_status(
                                            format!(
                                                "External edits detected; signed {} certs but failed to re-apply CertificateFile: {}",
                                                signed, e
                                            ),
                                            true,
                                        );
                                    } else {
                                        app.update_last_modified();
                                        app.reload_hosts();
                                        app.set_status(
                                            format!(
                                                "{} External ssh config edits detected, merged {} CertificateFile directives.",
                                                summary_msg, reapplied
                                            ),
                                            failed > 0,
                                        );
                                    }
                                } else {
                                    app.reload_hosts();
                                    app.set_sticky_status(
                                        format!(
                                            "{} External ssh config edits detected; certs on disk, no CertificateFile written.",
                                            summary_msg
                                        ),
                                        true,
                                    );
                                }
                            }
                            Err(e) => {
                                app.set_sticky_status(
                                    format!(
                                        "Signed {} certs but cannot re-parse ssh config after external edit: {}. Certs are on disk under ~/.purple/certs/.",
                                        signed, e
                                    ),
                                    true,
                                );
                            }
                        }
                    } else if let Err(e) = app.config.write() {
                        app.set_sticky_status(
                            format!(
                                "Signed {} certs but failed to update SSH config: {}",
                                signed, e
                            ),
                            true,
                        );
                    } else {
                        app.update_last_modified();
                        app.reload_hosts();
                        if failed > 0 {
                            app.set_sticky_status(summary_msg, true);
                        } else {
                            app.set_status(summary_msg, false);
                        }
                    }
                } else if failed > 0 {
                    app.set_sticky_status(summary_msg, true);
                } else {
                    app.set_status(summary_msg, false);
                }
            }
            Some(AppEvent::CertCheckResult { alias, status }) => {
                app.cert_check_in_flight.remove(&alias);
                let mtime = current_cert_mtime(&alias, &app);
                app.cert_status_cache
                    .insert(alias, (std::time::Instant::now(), status, mtime));
            }
            Some(AppEvent::CertCheckError { alias, message }) => {
                // Cache the error as Invalid so the lazy-check loop doesn't
                // re-spawn a background thread on every poll tick (which at
                // ~100ms would spawn ~10 threads/sec against a broken path).
                // The lazy-check block uses CERT_ERROR_BACKOFF_SECS instead of
                // the normal TTL for Invalid entries, so transient errors
                // still recover within 30 seconds instead of the 5-minute
                // valid-cert TTL.
                app.cert_check_in_flight.remove(&alias);
                app.cert_status_cache.insert(
                    alias.clone(),
                    (
                        std::time::Instant::now(),
                        vault_ssh::CertStatus::Invalid(message.clone()),
                        None,
                    ),
                );
                app.set_background_status(
                    format!("Cert check failed for {}: {}", alias, message),
                    true,
                );
            }
            Some(AppEvent::PollError) => {
                app.running = false;
            }
        }

        // Lazy cert status check: when the selected host has a vault role and
        // no cached status, spawn a background check.
        if let Some(selected) = app.selected_host() {
            if vault_ssh::resolve_vault_role(
                selected.vault_ssh.as_deref(),
                selected.provider.as_deref(),
                &app.provider_config,
            )
            .is_some()
            {
                // Stat the cert file once per iteration to detect external
                // writes (CLI sign, another purple instance) within one frame.
                // Compared against the mtime recorded when the cache entry was
                // populated; any mismatch forces a re-check, no matter the TTL.
                let current_mtime =
                    vault_ssh::resolve_cert_path(&selected.alias, &selected.certificate_file)
                        .ok()
                        .and_then(|p| std::fs::metadata(&p).ok())
                        .and_then(|m| m.modified().ok());
                let cache_stale = cache_entry_is_stale(
                    app.cert_status_cache.get(&selected.alias),
                    current_mtime,
                    |t| t.elapsed().as_secs(),
                );

                let sign_in_flight = app
                    .vault_sign_in_flight
                    .lock()
                    .map(|g| g.contains(&selected.alias))
                    .unwrap_or(false);
                if cache_stale
                    && !app.cert_check_in_flight.contains(&selected.alias)
                    && !sign_in_flight
                {
                    let alias = selected.alias.clone();
                    let cert_file = selected.certificate_file.clone();
                    app.cert_check_in_flight.insert(alias.clone());
                    let tx = events_tx.clone();
                    std::thread::spawn(move || {
                        let check_path = match vault_ssh::resolve_cert_path(&alias, &cert_file) {
                            Ok(p) => p,
                            Err(e) => {
                                let _ = tx.send(event::AppEvent::CertCheckError {
                                    alias,
                                    message: e.to_string(),
                                });
                                return;
                            }
                        };
                        let status = vault_ssh::check_cert_validity(&check_path);
                        let _ = tx.send(event::AppEvent::CertCheckResult { alias, status });
                    });
                }
            }
        }

        // Handle pending SSH connection
        if let Some((alias, host_askpass)) = app.pending_connect.take() {
            let vault_host = app.hosts.iter().find(|h| h.alias == alias).cloned();
            let askpass = host_askpass.or_else(preferences::load_askpass_default);
            let has_active_tunnel = app.active_tunnels.contains_key(&alias);
            let use_tmux = connection::is_in_tmux() && askpass.is_none();

            if use_tmux {
                // Tmux mode: open SSH in a new tmux window. TUI stays alive.
                // Vault SSH cert signing runs first (eprintln warnings are
                // harmless on the alternate screen — ratatui repaints over
                // them on the next draw cycle).
                let vault_msg = if let Some(ref host) = vault_host {
                    let msg = ensure_vault_ssh_if_needed(
                        &alias,
                        host,
                        &app.provider_config,
                        &mut app.config,
                    );
                    if msg.is_some() {
                        app.reload_hosts();
                        app.refresh_cert_cache(&alias);
                    }
                    msg
                } else {
                    None
                };

                match connection::connect_tmux_window(
                    &alias,
                    &app.reload.config_path,
                    has_active_tunnel,
                ) {
                    Ok(()) => {
                        if let Some((ref msg, is_error)) = vault_msg {
                            app.set_status(msg.clone(), is_error);
                        } else {
                            app.set_status(format!("Opened {} in new tmux window.", alias), false);
                        }
                    }
                    Err(e) => {
                        app.set_status(format!("tmux: {e}"), true);
                    }
                }
            } else {
                // Standard mode: suspend TUI, run SSH inline, restore TUI.
                // Order preserved: pause events, exit TUI, THEN run vault
                // signing and password setup (which may eprintln or prompt
                // for input on the real terminal).
                events.pause();
                terminal.exit()?;
                let vault_msg = if let Some(ref host) = vault_host {
                    let msg = ensure_vault_ssh_if_needed(
                        &alias,
                        host,
                        &app.provider_config,
                        &mut app.config,
                    );
                    if msg.is_some() {
                        app.reload_hosts();
                        app.refresh_cert_cache(&alias);
                    }
                    msg
                } else {
                    None
                };
                if let Some(token) =
                    ensure_bw_session(app.bw_session.as_deref(), askpass.as_deref())
                {
                    app.bw_session = Some(token);
                }
                ensure_keychain_password(&alias, askpass.as_deref());
                println!("Beaming you up to {}...\n", alias);
                let result = connection::connect(
                    &alias,
                    &app.reload.config_path,
                    askpass.as_deref(),
                    app.bw_session.as_deref(),
                    has_active_tunnel,
                );
                println!();
                match &result {
                    Ok(cr) => {
                        let code = cr.status.code().unwrap_or(1);
                        if code != 255 {
                            app.history.record(&alias);
                        }
                        if code != 0 {
                            if let Some((hostname, known_hosts_path)) =
                                connection::parse_host_key_error(&cr.stderr_output)
                            {
                                app.screen = app::Screen::ConfirmHostKeyReset {
                                    alias: alias.clone(),
                                    hostname,
                                    known_hosts_path,
                                    askpass,
                                };
                            } else {
                                app.set_status(
                                    format!("SSH to {} exited with code {}.", alias, code),
                                    true,
                                );
                            }
                        } else if let Some((ref msg, is_error)) = vault_msg {
                            app.set_status(msg.clone(), is_error);
                        }
                    }
                    Err(e) => {
                        eprintln!("Connection failed: {}", e);
                        app.set_status(format!("Connection to {} failed.", alias), true);
                    }
                }
                askpass::cleanup_marker(&alias);
                terminal.enter()?;
                events.resume();
                last_config_check = std::time::Instant::now();
                // Reload in case config changed externally
                app.config = SshConfigFile::parse(&app.reload.config_path)?;
                app.reload_hosts();
                app.update_last_modified();
            }
        }

        // Handle pending snippet execution
        if let Some((snip, aliases)) = app.pending_snippet.take() {
            events.pause();
            terminal.exit()?;

            let multi = aliases.len() > 1;
            for alias in &aliases {
                let askpass = app
                    .hosts
                    .iter()
                    .find(|h| h.alias == *alias)
                    .and_then(|h| h.askpass.clone())
                    .or_else(preferences::load_askpass_default);
                if let Some(token) =
                    ensure_bw_session(app.bw_session.as_deref(), askpass.as_deref())
                {
                    app.bw_session = Some(token);
                }
                ensure_keychain_password(alias, askpass.as_deref());

                if multi {
                    println!("── {} ──", alias);
                } else {
                    println!("Running '{}' on {}...\n", snip.name, alias);
                }
                let has_tunnel = app.active_tunnels.contains_key(alias);
                match snippet::run_snippet(
                    alias,
                    &app.reload.config_path,
                    &snip.command,
                    askpass.as_deref(),
                    app.bw_session.as_deref(),
                    false,
                    has_tunnel,
                ) {
                    Ok(r) => {
                        if r.status.success() {
                            app.history.record(alias);
                        } else if multi {
                            eprintln!("Exited with code {}.", r.status.code().unwrap_or(1));
                        } else {
                            println!("\nExited with code {}.", r.status.code().unwrap_or(1));
                        }
                    }
                    Err(e) => eprintln!("[{}] Failed: {}", alias, e),
                }
                if multi {
                    println!();
                }
            }

            if !multi {
                println!("\nDone.");
            } else {
                println!("Done. Ran '{}' on {} hosts.", snip.name, aliases.len());
            }
            println!("\nPress Enter to continue...");
            let _ = std::io::stdin().read_line(&mut String::new());
            terminal.enter()?;
            events.resume();
            last_config_check = std::time::Instant::now();
            // Reload so sort order (e.g. most recent) reflects the new history
            app.config = SshConfigFile::parse(&app.reload.config_path)?;
            app.reload_hosts();
            app.update_last_modified();
        }
    }

    // Final flush of any deferred vault config write before teardown so on-disk
    // state is not left behind.
    app.flush_pending_vault_write();

    // Cancel and join the background vault signing thread, if running.
    if let Some(ref cancel) = app.vault_signing_cancel {
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if let Some(handle) = app.vault_sign_thread.take() {
        let _ = handle.join();
    }

    // Kill all active tunnels on exit
    for (_, mut tunnel) in app.active_tunnels.drain() {
        let _ = tunnel.child.kill();
        let _ = tunnel.child.wait();
    }

    terminal.exit()?;
    Ok(())
}

pub(crate) fn current_cert_mtime(alias: &str, app: &app::App) -> Option<std::time::SystemTime> {
    let host = app.hosts.iter().find(|h| h.alias == alias)?;
    let cert_path = vault_ssh::resolve_cert_path(alias, &host.certificate_file).ok()?;
    std::fs::metadata(&cert_path)
        .ok()
        .and_then(|m| m.modified().ok())
}

/// Decide whether a `cert_status_cache` entry should be re-checked.
///
/// Returns true when:
/// - there is no cached entry at all, or
/// - the cert file's current mtime differs from the cached mtime
///   (an external actor signed or deleted the cert behind our back), or
/// - the entry's age exceeds its TTL. `CertStatus::Invalid` uses a shorter
///   backoff so transient errors recover quickly without hammering the
///   background check thread on every poll tick.
///
/// The `elapsed_secs` closure is taken as a parameter so tests can inject
/// deterministic elapsed times instead of calling the real clock.
pub(crate) fn cache_entry_is_stale<F>(
    entry: Option<&(
        std::time::Instant,
        vault_ssh::CertStatus,
        Option<std::time::SystemTime>,
    )>,
    current_mtime: Option<std::time::SystemTime>,
    elapsed_secs: F,
) -> bool
where
    F: FnOnce(std::time::Instant) -> u64,
{
    let Some((checked_at, status, cached_mtime)) = entry else {
        return true;
    };
    if current_mtime != *cached_mtime {
        return true;
    }
    let ttl = if matches!(status, vault_ssh::CertStatus::Invalid(_)) {
        vault_ssh::CERT_ERROR_BACKOFF_SECS
    } else {
        vault_ssh::CERT_STATUS_CACHE_TTL_SECS
    };
    elapsed_secs(*checked_at) > ttl
}

/// Check and renew Vault SSH certificate if the host has a vault role configured.
/// Writes the cert file to ~/.purple/certs/ AND sets CertificateFile on the host
/// block when it is empty, so `ssh` actually uses the freshly signed cert.
///
/// Returns `Some(message)` when a signing action was attempted (success or failure),
/// `None` when no vault role is configured or the cert is still valid.
pub(crate) fn ensure_vault_ssh_if_needed(
    alias: &str,
    host: &ssh_config::model::HostEntry,
    provider_config: &providers::config::ProviderConfig,
    config: &mut ssh_config::model::SshConfigFile,
) -> Option<(String, bool)> {
    let role = vault_ssh::resolve_vault_role(
        host.vault_ssh.as_deref(),
        host.provider.as_deref(),
        provider_config,
    )?;

    let pubkey = match vault_ssh::resolve_pubkey_path(&host.identity_file) {
        Ok(p) => p,
        Err(e) => return Some((format!("Vault SSH cert failed: {}", e), true)),
    };

    // Check if the cert needs renewal before calling ensure_cert, so we can
    // distinguish "renewed" from "already valid" for status feedback.
    let check_path = vault_ssh::resolve_cert_path(alias, &host.certificate_file).ok()?;
    let status = vault_ssh::check_cert_validity(&check_path);
    if !vault_ssh::needs_renewal(&status) {
        return None; // Cert valid, no action needed
    }

    // Resolve the Vault address at signing time (host override > provider
    // default > None). None lets the `vault` CLI use its own env resolution.
    let vault_addr = vault_ssh::resolve_vault_addr(
        host.vault_addr.as_deref(),
        host.provider.as_deref(),
        provider_config,
    );
    match vault_ssh::ensure_cert(
        &role,
        &pubkey,
        alias,
        &host.certificate_file,
        vault_addr.as_deref(),
    ) {
        Ok(cert_path) => {
            // If the host block did not already set CertificateFile, wire the
            // freshly signed cert into the SSH config so `ssh` actually uses it.
            // Otherwise the cert on disk is silently ignored.
            if should_write_certificate_file(&host.certificate_file) {
                let cert_str = cert_path.to_string_lossy().to_string();
                let updated = config.set_host_certificate_file(alias, &cert_str);
                if !updated {
                    eprintln!(
                        "Warning: Signed cert for {} but host block is no longer in ssh config; CertificateFile not written (cert saved to {})",
                        alias,
                        cert_path.display()
                    );
                } else if let Err(e) = config.write() {
                    eprintln!(
                        "Warning: Signed cert for {} but failed to update SSH config CertificateFile: {}",
                        alias, e
                    );
                }
            }
            Some((format!("Signed SSH certificate for {}.", alias), false))
        }
        Err(e) => {
            eprintln!("Warning: Vault SSH signing failed: {}", e);
            Some((format!("Vault SSH signing failed: {}", e), true))
        }
    }
}

/// Decide whether `ensure_vault_ssh_if_needed` (and the equivalent
/// `VaultSignResult` event handler, the `purple vault sign` CLI paths and
/// every host-form mutator) should write a `CertificateFile` directive after a
/// successful Vault SSH signing.
///
/// The rule is simple but load-bearing: only write when the host has no
/// existing `CertificateFile`. A user-set custom path must never be silently
/// overwritten with purple's default cert path. Whitespace-only values count
/// as empty so that a stray space typed in the form does not lock purple out
/// of writing the directive.
pub(crate) fn should_write_certificate_file(existing: &str) -> bool {
    existing.trim().is_empty()
}

/// Pre-flight check for Bitwarden vault. If the askpass source uses `bw:` and
/// no session token is cached, prompts the user to unlock the vault.
/// Returns Some(token) only when a new token was obtained. None means no action needed.
pub(crate) fn ensure_bw_session(existing: Option<&str>, askpass: Option<&str>) -> Option<String> {
    let askpass = askpass?;
    if !askpass.starts_with("bw:") || existing.is_some() {
        return None;
    }
    // Check vault status
    let status = askpass::bw_vault_status();
    match status {
        askpass::BwStatus::Unlocked => {
            // Vault already unlocked (e.g. BW_SESSION in environment). No action needed.
            None
        }
        askpass::BwStatus::NotInstalled => {
            eprintln!("Bitwarden CLI (bw) not found. SSH will prompt for password.");
            None
        }
        askpass::BwStatus::NotAuthenticated => {
            eprintln!("Bitwarden vault not logged in. Run 'bw login' first.");
            None
        }
        askpass::BwStatus::Locked => {
            // Prompt for master password and unlock
            for attempt in 0..2 {
                let password = match cli::prompt_hidden_input("Bitwarden master password: ") {
                    Ok(Some(p)) if !p.is_empty() => p,
                    Ok(Some(_)) => {
                        eprintln!("Empty password. SSH will prompt for password.");
                        return None;
                    }
                    Ok(None) => {
                        // User pressed Esc
                        return None;
                    }
                    Err(e) => {
                        eprintln!("Failed to read password: {}", e);
                        return None;
                    }
                };
                match askpass::bw_unlock(&password) {
                    Ok(token) => return Some(token),
                    Err(e) => {
                        if attempt == 0 {
                            eprintln!("Unlock failed: {}. Try again.", e);
                        } else {
                            eprintln!("Unlock failed: {}. SSH will prompt for password.", e);
                        }
                    }
                }
            }
            None
        }
    }
}

/// Pre-flight check for keychain password. If the askpass source is `keychain` and
/// no password is stored yet, prompts the user to enter one and stores it.
pub(crate) fn ensure_keychain_password(alias: &str, askpass: Option<&str>) {
    if askpass != Some("keychain") {
        return;
    }
    // Check if password already exists
    if askpass::keychain_has_password(alias) {
        return;
    }
    // Prompt for password and store it
    let password =
        match cli::prompt_hidden_input(&format!("Password for {} (stored in keychain): ", alias)) {
            Ok(Some(p)) if !p.is_empty() => p,
            Ok(Some(_)) => {
                eprintln!("Empty password. SSH will prompt for password.");
                return;
            }
            Ok(None) => return, // Esc
            Err(_) => return,
        };
    match askpass::store_in_keychain(alias, &password) {
        Ok(()) => eprintln!("Password stored in keychain."),
        Err(e) => eprintln!(
            "Failed to store in keychain: {}. SSH will prompt for password.",
            e
        ),
    }
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
