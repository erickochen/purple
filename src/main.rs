mod app;
mod askpass;
mod clipboard;
mod connection;
mod event;
mod handler;
mod fs_util;
mod history;
mod import;
mod ping;
mod preferences;
mod providers;
mod quick_add;
mod ssh_config;
mod ssh_keys;
mod tui;
mod tunnel;
mod ui;
mod update;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};

use app::App;
use event::{AppEvent, EventHandler};
use ssh_config::model::{HostEntry, SshConfigFile};

#[derive(Parser)]
#[command(
    name = "purple",
    about = "Your SSH config is a mess. Purple fixes that.",
    long_about = "Purple is a fast, friendly TUI for managing your SSH hosts.\n\
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

    /// Generate shell completions
    #[arg(long, value_name = "SHELL")]
    completions: Option<Shell>,

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
    /// Sync hosts from cloud providers (DigitalOcean, Vultr, Linode, Hetzner, UpCloud, Proxmox VE)
    Sync {
        /// Sync a specific provider (default: all configured)
        provider: Option<String>,

        /// Preview changes without modifying config
        #[arg(long)]
        dry_run: bool,

        /// Remove hosts that no longer exist on the provider
        #[arg(long)]
        remove: bool,

        /// Replace local tags with provider tags instead of merging
        #[arg(long)]
        reset_tags: bool,
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
    /// Update purple to the latest version
    Update,
}

#[derive(Subcommand)]
enum ProviderCommands {
    /// Add or update a provider configuration
    Add {
        /// Provider name (digitalocean, vultr, linode, hetzner, upcloud, proxmox)
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

fn resolve_config_path(path: &str) -> Result<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(rest))
    } else {
        Ok(PathBuf::from(path))
    }
}

fn resolve_token(explicit: Option<String>, from_stdin: bool) -> Result<String> {
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

    // Shell completions (no config file needed)
    if let Some(shell) = cli.completions {
        let mut cmd = Cli::command();
        generate(shell, &mut cmd, "purple", &mut std::io::stdout());
        return Ok(());
    }

    // Provider and Update subcommands don't need SSH config
    if let Some(Commands::Provider { command }) = cli.command {
        return handle_provider_command(command);
    }
    if let Some(Commands::Update) = cli.command {
        return update::self_update();
    }
    if let Some(Commands::Password { command }) = cli.command {
        return handle_password_command(command);
    }

    let config_path = resolve_config_path(&cli.config)?;
    let config = SshConfigFile::parse(&config_path)?;

    // Handle subcommands that need SSH config
    match cli.command {
        Some(Commands::Add { target, alias, key }) => {
            return handle_quick_add(config, &target, alias.as_deref(), key.as_deref());
        }
        Some(Commands::Import {
            file,
            known_hosts,
            group,
        }) => {
            return handle_import(config, file.as_deref(), known_hosts, group.as_deref());
        }
        Some(Commands::Sync {
            provider,
            dry_run,
            remove,
            reset_tags,
        }) => {
            return handle_sync(config, provider.as_deref(), dry_run, remove, reset_tags);
        }
        Some(Commands::Tunnel { command }) => {
            return handle_tunnel_command(config, command);
        }
        Some(Commands::Provider { .. }) | Some(Commands::Update) | Some(Commands::Password { .. }) => unreachable!(),
        None => {}
    }

    // Direct connect mode (--connect)
    if let Some(alias) = cli.connect {
        let askpass = config.host_entries().iter()
            .find(|h| h.alias == alias)
            .and_then(|h| h.askpass.clone())
            .or_else(preferences::load_askpass_default);
        let bw_session = ensure_bw_session(None, askpass.as_deref());
        ensure_keychain_password(&alias, askpass.as_deref());
        let result = connection::connect(&alias, &config_path, askpass.as_deref(), bw_session.as_deref())?;
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
        let entries = config.host_entries();
        if let Some(host) = entries.iter().find(|h| h.alias == *alias) {
            let alias = host.alias.clone();
            let askpass = host.askpass.clone()
                .or_else(preferences::load_askpass_default);
            let bw_session = ensure_bw_session(None, askpass.as_deref());
            ensure_keychain_password(&alias, askpass.as_deref());
            println!("Beaming you up to {}...\n", alias);
            let result = connection::connect(&alias, &config_path, askpass.as_deref(), bw_session.as_deref())?;
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
    run_tui(app)
}

fn apply_saved_sort(app: &mut App) {
    let saved = preferences::load_sort_mode();
    let group = preferences::load_group_by_provider();
    app.sort_mode = saved;
    app.group_by_provider = group;
    app.view_mode = preferences::load_view_mode();
    if saved != app::SortMode::Original || group {
        app.apply_sort();
    }
}

fn run_tui(mut app: App) -> Result<()> {
    // First-launch welcome hint (one-shot: creates .purple/ so it won't show again)
    if app.status.is_none() && !app.hosts.is_empty() {
        if let Some(home) = dirs::home_dir() {
            let purple_dir = home.join(".purple");
            if !purple_dir.exists() {
                let _ = std::fs::create_dir_all(&purple_dir);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &purple_dir,
                        std::fs::Permissions::from_mode(0o700),
                    );
                }
                app.set_status("Welcome to purple. Press ? for the cheat sheet.", false);
            }
        }
    }

    let mut terminal = tui::Tui::new()?;
    terminal.enter()?;
    let events = EventHandler::new(250);
    let events_tx = events.sender();
    let mut last_config_check = std::time::Instant::now();

    // Auto-sync configured providers on startup (skipped when auto_sync=false)
    for section in app.provider_config.configured_providers().to_vec() {
        if !section.auto_sync {
            continue;
        }
        if !app.syncing_providers.contains_key(&section.provider) {
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            app.syncing_providers.insert(section.provider.clone(), cancel.clone());
            handler::spawn_provider_sync(&section, events_tx.clone(), cancel);
        }
    }

    // Background version check
    update::spawn_version_check(events_tx.clone());

    while app.running {
        terminal.draw(&mut app)?;

        match events.next()? {
            AppEvent::Key(key) => handler::handle_key_event(&mut app, key, &events_tx)?,
            AppEvent::Tick => {
                app.tick_status();
                // Throttle config file stat() to every 4 seconds
                if last_config_check.elapsed() >= std::time::Duration::from_secs(4) {
                    app.check_config_changed();
                    last_config_check = std::time::Instant::now();
                }
                // Poll active tunnels for exit
                let exited = app.poll_tunnels();
                for (_alias, msg, is_error) in exited {
                    app.set_status(msg, is_error);
                }
            }
            AppEvent::PingResult { alias, reachable } => {
                let status = if reachable {
                    app::PingStatus::Reachable
                } else {
                    app::PingStatus::Unreachable
                };
                app.ping_status.insert(alias, status);
            }
            AppEvent::SyncProgress { provider, message } => {
                let name = providers::provider_display_name(&provider);
                app.set_status(format!("{}: {}", name, message), false);
            }
            AppEvent::SyncComplete { provider, hosts } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let (msg, is_err, total) = app.apply_sync_result(&provider, hosts);
                if is_err {
                    app.sync_history.insert(provider.clone(), app::SyncRecord {
                        timestamp: now,
                        message: msg.clone(),
                        is_error: true,
                    });
                } else {
                    let label = if total == 1 { "server" } else { "servers" };
                    app.sync_history.insert(provider.clone(), app::SyncRecord {
                        timestamp: now,
                        message: format!("{} {}", total, label),
                        is_error: false,
                    });
                }
                app.set_status(msg, is_err);
                app.syncing_providers.remove(&provider);
            }
            AppEvent::SyncPartial { provider, hosts, failures, total } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let display_name = providers::provider_display_name(provider.as_str());
                let (msg, is_err, synced) = app.apply_sync_result(&provider, hosts);
                if is_err {
                    app.sync_history.insert(provider.clone(), app::SyncRecord {
                        timestamp: now,
                        message: msg.clone(),
                        is_error: true,
                    });
                    app.set_status(msg, true);
                } else {
                    let label = if synced == 1 { "server" } else { "servers" };
                    app.sync_history.insert(provider.clone(), app::SyncRecord {
                        timestamp: now,
                        message: format!("{} {} ({} of {} failed)", synced, label, failures, total),
                        is_error: true,
                    });
                    app.set_status(
                        format!("{}: {} synced, {} of {} failed to fetch.", display_name, synced, failures, total),
                        true,
                    );
                }
                app.syncing_providers.remove(&provider);
            }
            AppEvent::SyncError { provider, message } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let display_name = providers::provider_display_name(provider.as_str());
                app.sync_history.insert(provider.clone(), app::SyncRecord {
                    timestamp: now,
                    message: message.clone(),
                    is_error: true,
                });
                app.set_status(
                    format!("{} sync failed: {}", display_name, message),
                    true,
                );
                app.syncing_providers.remove(&provider);
            }
            AppEvent::UpdateAvailable { version } => {
                app.update_available = Some(version);
            }
            AppEvent::PollError => {
                app.running = false;
            }
        }

        // Handle pending SSH connection
        if let Some((alias, host_askpass)) = app.pending_connect.take() {
            let askpass = host_askpass.or_else(preferences::load_askpass_default);
            events.pause();
            terminal.exit()?;
            if let Some(token) = ensure_bw_session(app.bw_session.as_deref(), askpass.as_deref()) {
                app.bw_session = Some(token);
            }
            ensure_keychain_password(&alias, askpass.as_deref());
            println!("Beaming you up to {}...\n", alias);
            let result = connection::connect(&alias, &app.reload.config_path, askpass.as_deref(), app.bw_session.as_deref());
            println!();
            match &result {
                Ok(cr) => {
                    let code = cr.status.code().unwrap_or(1);
                    if code != 255 {
                        app.history.record(&alias);
                    }
                    if code != 0 {
                        if let Some((hostname, known_hosts_path)) = connection::parse_host_key_error(&cr.stderr_output) {
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

    // Kill all active tunnels on exit
    for (_, mut tunnel) in app.active_tunnels.drain() {
        let _ = tunnel.child.kill();
        let _ = tunnel.child.wait();
    }

    terminal.exit()?;
    Ok(())
}

fn handle_quick_add(
    mut config: SshConfigFile,
    target: &str,
    alias: Option<&str>,
    key: Option<&str>,
) -> Result<()> {
    let parsed = quick_add::parse_target(target).map_err(|e| anyhow::anyhow!(e))?;

    let alias_str = alias
        .map(|a| a.to_string())
        .unwrap_or_else(|| {
            parsed
                .hostname
                .split('.')
                .next()
                .unwrap_or(&parsed.hostname)
                .to_string()
        });

    if alias_str.trim().is_empty() {
        eprintln!("Alias can't be empty. Use --alias to specify one.");
        std::process::exit(1);
    }
    if alias_str.contains(char::is_whitespace) {
        eprintln!("Alias can't contain whitespace. Use --alias to pick a simpler name.");
        std::process::exit(1);
    }
    if ssh_config::model::is_host_pattern(&alias_str) {
        eprintln!("Alias can't contain pattern characters. Use --alias to pick a different name.");
        std::process::exit(1);
    }

    // Reject control characters in alias, hostname, user and key
    let key_val = key.unwrap_or("").to_string();
    for (value, name) in [
        (&alias_str, "Alias"),
        (&parsed.hostname, "Hostname"),
        (&parsed.user, "User"),
        (&key_val, "Identity file"),
    ] {
        if value.chars().any(|c| c.is_control()) {
            eprintln!("{} contains control characters.", name);
            std::process::exit(1);
        }
    }

    // Reject whitespace in hostname and user (matches TUI validation)
    if parsed.hostname.contains(char::is_whitespace) {
        eprintln!("Hostname can't contain whitespace.");
        std::process::exit(1);
    }
    if parsed.user.contains(char::is_whitespace) {
        eprintln!("User can't contain whitespace.");
        std::process::exit(1);
    }

    if config.has_host(&alias_str) {
        eprintln!("'{}' already exists. Use --alias to pick a different name.", alias_str);
        std::process::exit(1);
    }

    let entry = HostEntry {
        alias: alias_str.clone(),
        hostname: parsed.hostname,
        user: parsed.user,
        port: parsed.port,
        identity_file: key_val,
        ..Default::default()
    };

    config.add_host(&entry);
    config.write()?;
    println!("Welcome aboard, {}!", alias_str);
    Ok(())
}

fn handle_import(
    mut config: SshConfigFile,
    file: Option<&str>,
    known_hosts: bool,
    group: Option<&str>,
) -> Result<()> {
    let result = if known_hosts {
        import::import_from_known_hosts(&mut config, group)
    } else if let Some(path) = file {
        let resolved = resolve_config_path(path)?;
        import::import_from_file(&mut config, &resolved, group)
    } else {
        eprintln!("Provide a file or use --known-hosts. Run 'purple import --help' for details.");
        std::process::exit(1);
    };

    match result {
        Ok((imported, skipped, parse_failures, read_errors)) => {
            if imported > 0 {
                config.write()?;
            }
            println!(
                "Imported {} host{}, skipped {} duplicate{}.",
                imported,
                if imported == 1 { "" } else { "s" },
                skipped,
                if skipped == 1 { "" } else { "s" },
            );
            if parse_failures > 0 {
                eprintln!(
                    "! {} line{} could not be parsed (invalid format).",
                    parse_failures,
                    if parse_failures == 1 { "" } else { "s" },
                );
            }
            if read_errors > 0 {
                eprintln!(
                    "! {} line{} could not be read (encoding error).",
                    read_errors,
                    if read_errors == 1 { "" } else { "s" },
                );
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

fn handle_sync(
    mut config: SshConfigFile,
    provider_name: Option<&str>,
    dry_run: bool,
    remove: bool,
    reset_tags: bool,
) -> Result<()> {
    let provider_config = providers::config::ProviderConfig::load();
    let sections: Vec<&providers::config::ProviderSection> = if let Some(name) = provider_name {
        if providers::get_provider(name).is_none() {
            eprintln!(
                "Never heard of '{}'. Try: digitalocean, vultr, linode, hetzner, upcloud, proxmox.",
                name
            );
            std::process::exit(1);
        }
        match provider_config.section(name) {
            Some(s) => vec![s],
            None => {
                eprintln!(
                    "No configuration for {}. Run 'purple provider add {}' first.",
                    name, name
                );
                std::process::exit(1);
            }
        }
    } else {
        let configured = provider_config.configured_providers();
        if configured.is_empty() {
            eprintln!("No providers configured. Run 'purple provider add' to set one up.");
            std::process::exit(1);
        }
        configured.iter().collect()
    };

    let mut any_changes = false;
    let mut any_failures = false;
    let mut any_hard_failures = false;

    for section in &sections {
        let provider = match providers::get_provider_with_config(&section.provider, section) {
            Some(p) => p,
            None => {
                eprintln!(
                    "Skipping unknown provider '{}'. Try: digitalocean, vultr, linode, hetzner, upcloud, proxmox.",
                    section.provider
                );
                any_failures = true;
                // Not a hard failure: unknown provider contributes no changes,
                // so other providers' successful results should still be written.
                continue;
            }
        };
        let display_name = providers::provider_display_name(section.provider.as_str());
        let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdout());
        print!("Syncing {}... ", display_name);
        let _ = std::io::Write::flush(&mut std::io::stdout());

        let last_summary = std::cell::RefCell::new(String::new());
        let progress = |msg: &str| {
            *last_summary.borrow_mut() = msg.to_string();
            if is_tty {
                print!("\x1b[2K\rSyncing {}... {}", display_name, msg);
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
        };
        let fetch_result = provider.fetch_hosts_with_progress(&section.token, &std::sync::atomic::AtomicBool::new(false), &progress);
        let summary = last_summary.into_inner();
        // Complete the Syncing line: TTY overwrites with summary; non-TTY appends.
        if is_tty {
            if summary.is_empty() {
                print!("\x1b[2K\rSyncing {}... ", display_name);
            } else {
                println!("\x1b[2K\rSyncing {}... {}", display_name, summary);
            }
            let _ = std::io::Write::flush(&mut std::io::stdout());
        } else if !summary.is_empty() {
            println!("{}", summary);
        }
        let (hosts, suppress_remove) = match fetch_result {
            Ok(hosts) => (hosts, false),
            Err(providers::ProviderError::PartialResult { hosts, failures, total }) => {
                println!(
                    "{} servers found ({} of {} failed to fetch).",
                    hosts.len(), failures, total
                );
                if remove {
                    eprintln!("! {}: skipping --remove due to partial failures.", display_name);
                }
                any_failures = true;
                (hosts, true)
            }
            Err(e) => {
                println!("failed.");
                eprintln!("! {}: {}", display_name, e);
                any_failures = true;
                any_hard_failures = true;
                continue;
            }
        };
        if !suppress_remove {
            println!("{} servers found.", hosts.len());
        }
        let effective_remove = remove && !suppress_remove;
        let result = providers::sync::sync_provider_with_options(
            &mut config, &*provider, &hosts, section, effective_remove, dry_run, reset_tags,
        );
        let prefix = if dry_run { "  Would have: " } else { "  " };
        println!(
            "{}Added {}, updated {}, unchanged {}.",
            prefix, result.added, result.updated, result.unchanged
        );
        if result.removed > 0 {
            println!("  Removed {}.", result.removed);
        }
        if result.added > 0 || result.updated > 0 || result.removed > 0 {
            any_changes = true;
        }
    }

    if any_changes && !dry_run {
        if any_hard_failures {
            eprintln!("! Skipping config write due to sync failures. Fix the errors and re-run.");
        } else {
            config.write()?;
        }
    }

    if any_failures {
        std::process::exit(1);
    }

    Ok(())
}

fn handle_provider_command(command: ProviderCommands) -> Result<()> {
    match command {
        ProviderCommands::Add {
            provider,
            token,
            token_stdin,
            mut prefix,
            mut user,
            mut key,
            url,
            no_verify_tls,
            verify_tls,
            auto_sync,
            no_auto_sync,
        } => {
            let p = match providers::get_provider(&provider) {
                Some(p) => p,
                None => {
                    eprintln!(
                        "Never heard of '{}'. Try: digitalocean, vultr, linode, hetzner, upcloud, proxmox.",
                        provider
                    );
                    std::process::exit(1);
                }
            };

            // --url, --no-verify-tls and --verify-tls are Proxmox-only; clear them for other providers
            let mut token = token;
            let mut url = url;
            let mut no_verify_tls = no_verify_tls;
            let mut verify_tls = verify_tls;
            if provider != "proxmox" {
                if url.is_some() {
                    eprintln!("Warning: --url is only used by the Proxmox provider. Ignoring.");
                    url = None;
                }
                if no_verify_tls {
                    eprintln!("Warning: --no-verify-tls is only used by the Proxmox provider. Ignoring.");
                    no_verify_tls = false;
                }
                if verify_tls {
                    eprintln!("Warning: --verify-tls is only used by the Proxmox provider. Ignoring.");
                    verify_tls = false;
                }
            }

            // When updating an existing section, fall back to stored values for fields not supplied
            let existing_section = providers::config::ProviderConfig::load()
                .section(&provider)
                .cloned();

            if let Some(ref existing) = existing_section {
                // URL fallback only applies to Proxmox (only provider that uses the url field)
                if provider == "proxmox" && url.is_none() && !existing.url.is_empty() {
                    url = Some(existing.url.clone());
                }
                if token.is_none() && !token_stdin && std::env::var("PURPLE_TOKEN").is_err() && !existing.token.is_empty() {
                    token = Some(existing.token.clone());
                }
                if prefix.is_none() {
                    prefix = Some(existing.alias_prefix.clone());
                }
                if user.is_none() {
                    user = Some(existing.user.clone());
                }
                if key.is_none() && !existing.identity_file.is_empty() {
                    key = Some(existing.identity_file.clone());
                }
                // Preserve verify_tls=false unless the user explicitly overrides it either way
                if !no_verify_tls && !verify_tls && !existing.verify_tls {
                    no_verify_tls = true;
                }
            }

            // Proxmox requires --url
            if provider == "proxmox" {
                if url.is_none() || url.as_deref().unwrap_or("").trim().is_empty() {
                    eprintln!("Proxmox requires --url (e.g. --url https://pve.example.com:8006).");
                    std::process::exit(1);
                }
                let u = url.as_deref().unwrap();
                if !u.to_ascii_lowercase().starts_with("https://") {
                    eprintln!("URL must start with https://. For self-signed certificates use --no-verify-tls.");
                    std::process::exit(1);
                }
            }

            let token = match resolve_token(token, token_stdin) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };

            if token.trim().is_empty() {
                eprintln!(
                    "Token can't be empty. Grab one from your {} dashboard.",
                    providers::provider_display_name(&provider)
                );
                std::process::exit(1);
            }

            let alias_prefix = prefix.unwrap_or_else(|| p.short_label().to_string());
            if ssh_config::model::is_host_pattern(&alias_prefix) {
                eprintln!("Alias prefix can't contain spaces or pattern characters (*, ?, [, !).");
                std::process::exit(1);
            }

            let user = user.unwrap_or_else(|| "root".to_string());
            let identity_file = key.unwrap_or_default();

            // Reject control characters in all fields (prevents INI injection)
            let url_value = url.clone().unwrap_or_default();
            for (value, name) in [
                (&url_value, "URL"),
                (&token, "Token"),
                (&alias_prefix, "Alias prefix"),
                (&user, "User"),
                (&identity_file, "Identity file"),
            ] {
                if value.chars().any(|c| c.is_control()) {
                    eprintln!("{} contains control characters.", name);
                    std::process::exit(1);
                }
            }
            if user.contains(char::is_whitespace) {
                eprintln!("User can't contain whitespace.");
                std::process::exit(1);
            }

            // Resolve auto_sync: explicit flags > existing config > provider default
            let resolved_auto_sync = if auto_sync {
                true
            } else if no_auto_sync {
                false
            } else if let Some(ref existing) = existing_section {
                existing.auto_sync
            } else {
                provider != "proxmox"
            };

            let section = providers::config::ProviderSection {
                provider: provider.clone(),
                token,
                alias_prefix,
                user,
                identity_file,
                url: url.unwrap_or_default(),
                verify_tls: !no_verify_tls,
                auto_sync: resolved_auto_sync,
            };

            let mut config = providers::config::ProviderConfig::load();
            config.set_section(section);
            config
                .save()
                .map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
            println!("Saved {} configuration.", provider);
            Ok(())
        }
        ProviderCommands::List => {
            let config = providers::config::ProviderConfig::load();
            let sections = config.configured_providers();
            if sections.is_empty() {
                println!("No providers configured. Run 'purple provider add' to set one up.");
            } else {
                for s in sections {
                    let display_name = providers::provider_display_name(s.provider.as_str());
                    println!(
                        "  {:<16} {}-*{:>8}",
                        display_name, s.alias_prefix, s.user
                    );
                }
            }
            Ok(())
        }
        ProviderCommands::Remove { provider } => {
            let mut config = providers::config::ProviderConfig::load();
            if config.section(&provider).is_none() {
                eprintln!("No configuration for '{}'. Nothing to remove.", provider);
                std::process::exit(1);
            }
            config.remove_section(&provider);
            config
                .save()
                .map_err(|e| anyhow::anyhow!("Failed to save: {}", e))?;
            println!("Removed {} configuration.", provider);
            Ok(())
        }
    }
}

fn handle_tunnel_command(mut config: SshConfigFile, command: TunnelCommands) -> Result<()> {
    match command {
        TunnelCommands::List { alias } => {
            if let Some(alias) = alias {
                // Show tunnels for a specific host
                if !config.has_host(&alias) {
                    eprintln!("No host '{}' found.", alias);
                    std::process::exit(1);
                }
                let rules = config.find_tunnel_directives(&alias);
                if rules.is_empty() {
                    println!("No tunnels configured for {}.", alias);
                } else {
                    println!("Tunnels for {}:", alias);
                    for rule in &rules {
                        println!("  {}", rule.display());
                    }
                }
            } else {
                // Show all hosts with tunnels
                let entries = config.host_entries();
                let with_tunnels: Vec<_> = entries.iter().filter(|e| e.tunnel_count > 0).collect();
                if with_tunnels.is_empty() {
                    println!("No tunnels configured.");
                } else {
                    for (i, host) in with_tunnels.iter().enumerate() {
                        if i > 0 {
                            println!();
                        }
                        println!("{}:", host.alias);
                        for rule in config.find_tunnel_directives(&host.alias) {
                            println!("  {}", rule.display());
                        }
                    }
                }
            }
            Ok(())
        }
        TunnelCommands::Add { alias, forward } => {
            if !config.has_host(&alias) {
                eprintln!("No host '{}' found.", alias);
                std::process::exit(1);
            }
            if config.is_included_host(&alias) {
                eprintln!("Host '{}' is from an included file and cannot be modified.", alias);
                std::process::exit(1);
            }
            let rule = tunnel::TunnelRule::from_cli_spec(&forward).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let key = rule.tunnel_type.directive_key();
            let value = rule.to_directive_value();
            // Check for duplicate forward
            if config.has_forward(&alias, key, &value) {
                eprintln!("Forward {} already exists on {}.", forward, alias);
                std::process::exit(1);
            }
            config.add_forward(&alias, key, &value);
            if let Err(e) = config.write() {
                eprintln!("Failed to save config: {}", e);
                std::process::exit(1);
            }
            println!("Added {} to {}.", forward, alias);
            Ok(())
        }
        TunnelCommands::Remove { alias, forward } => {
            if !config.has_host(&alias) {
                eprintln!("No host '{}' found.", alias);
                std::process::exit(1);
            }
            if config.is_included_host(&alias) {
                eprintln!("Host '{}' is from an included file and cannot be modified.", alias);
                std::process::exit(1);
            }
            let rule = tunnel::TunnelRule::from_cli_spec(&forward).unwrap_or_else(|e| {
                eprintln!("{}", e);
                std::process::exit(1);
            });
            let key = rule.tunnel_type.directive_key();
            let value = rule.to_directive_value();
            let removed = config.remove_forward(&alias, key, &value);
            if !removed {
                eprintln!("No matching forward {} found on {}.", forward, alias);
                std::process::exit(1);
            }
            if let Err(e) = config.write() {
                eprintln!("Failed to save config: {}", e);
                std::process::exit(1);
            }
            println!("Removed {} from {}.", forward, alias);
            Ok(())
        }
        TunnelCommands::Start { alias } => {
            if !config.has_host(&alias) {
                eprintln!("No host '{}' found.", alias);
                std::process::exit(1);
            }
            let tunnels = config.find_tunnel_directives(&alias);
            if tunnels.is_empty() {
                eprintln!("No forwarding directives configured for '{}'.", alias);
                std::process::exit(1);
            }
            println!("Starting tunnel for {}... (Ctrl+C to stop)", alias);
            // Run ssh -N in foreground with inherited stdio
            let status = std::process::Command::new("ssh")
                .arg("-F")
                .arg(&config.path)
                .arg("-N")
                .arg("--")
                .arg(&alias)
                .status()
                .map_err(|e| anyhow::anyhow!("Failed to start ssh: {}", e))?;
            let code = status.code().unwrap_or(1);
            std::process::exit(code);
        }
    }
}

/// Read a line of input with echo disabled. Returns None if the user presses Esc.
fn prompt_hidden_input(prompt: &str) -> Result<Option<String>> {
    eprint!("{}", prompt);
    crossterm::terminal::enable_raw_mode()?;
    let mut input = String::new();
    loop {
        if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
            match key.code {
                crossterm::event::KeyCode::Enter => break,
                crossterm::event::KeyCode::Char(c) => {
                    input.push(c);
                    eprint!("*");
                }
                crossterm::event::KeyCode::Backspace => {
                    if input.pop().is_some() {
                        eprint!("\x08 \x08");
                    }
                }
                crossterm::event::KeyCode::Esc => {
                    crossterm::terminal::disable_raw_mode()?;
                    eprintln!();
                    return Ok(None);
                }
                _ => {}
            }
        }
    }
    crossterm::terminal::disable_raw_mode()?;
    eprintln!();
    Ok(Some(input))
}

/// Pre-flight check for Bitwarden vault. If the askpass source uses `bw:` and
/// no session token is cached, prompts the user to unlock the vault.
/// Returns Some(token) only when a new token was obtained. None means no action needed.
fn ensure_bw_session(existing: Option<&str>, askpass: Option<&str>) -> Option<String> {
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
                let password = match prompt_hidden_input("Bitwarden master password: ") {
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
fn ensure_keychain_password(alias: &str, askpass: Option<&str>) {
    if askpass != Some("keychain") {
        return;
    }
    // Check if password already exists
    if askpass::keychain_has_password(alias) {
        return;
    }
    // Prompt for password and store it
    let password = match prompt_hidden_input(&format!("Password for {} (stored in keychain): ", alias)) {
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
        Err(e) => eprintln!("Failed to store in keychain: {}. SSH will prompt for password.", e),
    }
}

fn handle_password_command(command: PasswordCommands) -> Result<()> {
    match command {
        PasswordCommands::Set { alias } => {
            let password = match prompt_hidden_input(&format!("Password for {}: ", alias))? {
                Some(p) if !p.is_empty() => p,
                Some(_) => {
                    eprintln!("Password can't be empty.");
                    std::process::exit(1);
                }
                None => {
                    eprintln!("Cancelled.");
                    std::process::exit(1);
                }
            };

            askpass::store_in_keychain(&alias, &password)?;
            println!("Password stored for {}. Set 'keychain' as password source to use it.", alias);
            Ok(())
        }
        PasswordCommands::Remove { alias } => {
            askpass::remove_from_keychain(&alias)?;
            println!("Password removed for {}.", alias);
            Ok(())
        }
    }
}
