mod app;
mod clipboard;
mod connection;
mod event;
mod handler;
mod history;
mod import;
mod ping;
mod preferences;
mod providers;
mod quick_add;
mod ssh_config;
mod ssh_keys;
mod tui;
mod ui;

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
    /// Sync hosts from cloud providers (DigitalOcean, Vultr, Linode, Hetzner, UpCloud)
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
}

#[derive(Subcommand)]
enum ProviderCommands {
    /// Add or update a provider configuration
    Add {
        /// Provider name (digitalocean, vultr, linode, hetzner, upcloud)
        provider: String,

        /// API token
        #[arg(long)]
        token: String,

        /// Alias prefix (default: provider short label)
        #[arg(long)]
        prefix: Option<String>,

        /// Default SSH user (default: root)
        #[arg(long)]
        user: Option<String>,

        /// Default identity file
        #[arg(long)]
        key: Option<String>,
    },
    /// List configured providers
    List,
    /// Remove a provider configuration
    Remove {
        /// Provider name to remove
        provider: String,
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

fn main() -> Result<()> {
    ui::theme::init();
    let cli = Cli::parse();

    // Shell completions (no config file needed)
    if let Some(shell) = cli.completions {
        let mut cmd = Cli::command();
        generate(shell, &mut cmd, "purple", &mut std::io::stdout());
        return Ok(());
    }

    // Provider subcommand doesn't need SSH config
    if let Some(Commands::Provider { command }) = cli.command {
        return handle_provider_command(command);
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
        }) => {
            return handle_sync(config, provider.as_deref(), dry_run, remove);
        }
        Some(Commands::Provider { .. }) => unreachable!(),
        None => {}
    }

    // Direct connect mode (--connect)
    if let Some(alias) = cli.connect {
        let status = connection::connect(&alias)?;
        let code = status.code().unwrap_or(1);
        if code != 255 {
            history::ConnectionHistory::load().record(&alias);
        }
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
            println!("Beaming you up to {}...\n", alias);
            let status = connection::connect(&alias)?;
            let code = status.code().unwrap_or(1);
            if code != 255 {
                history::ConnectionHistory::load().record(&alias);
            }
            std::process::exit(code);
        }
        // No exact match — open TUI with search pre-filled
        let mut app = App::new(config);
        apply_saved_sort(&mut app);
        app.start_search_with(alias);
        if app.filtered_indices.is_empty() {
            app.set_status(
                format!("No exact match for '{}'. Here's what we found.", alias),
                false,
            );
        }
        return run_tui(app, &cli.config);
    }

    // Interactive TUI mode
    let mut app = App::new(config);
    apply_saved_sort(&mut app);
    run_tui(app, &cli.config)
}

fn apply_saved_sort(app: &mut App) {
    let saved = preferences::load_sort_mode();
    if saved != app::SortMode::Original {
        app.sort_mode = saved;
        app.apply_sort();
    }
}

fn run_tui(mut app: App, config_str: &str) -> Result<()> {
    // First-launch welcome hint (one-shot: creates .purple/ so it won't show again)
    if app.status.is_none() && !app.hosts.is_empty() {
        if let Some(home) = dirs::home_dir() {
            let purple_dir = home.join(".purple");
            if !purple_dir.exists() {
                let _ = std::fs::create_dir_all(&purple_dir);
                app.set_status("Welcome to purple. Press ? for the cheat sheet.", false);
            }
        }
    }

    let mut terminal = tui::Tui::new()?;
    terminal.enter()?;
    let events = EventHandler::new(250);
    let events_tx = events.sender();
    let mut last_config_check = std::time::Instant::now();

    // Auto-sync all configured providers on startup
    for section in app.provider_config.configured_providers().to_vec() {
        if !app.syncing_providers.contains(&section.provider) {
            app.syncing_providers.insert(section.provider.clone());
            handler::spawn_provider_sync(&section.provider, &section.token, events_tx.clone());
        }
    }

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
            }
            AppEvent::PingResult { alias, reachable } => {
                let status = if reachable {
                    app::PingStatus::Reachable
                } else {
                    app::PingStatus::Unreachable
                };
                app.ping_status.insert(alias, status);
            }
            AppEvent::SyncComplete { provider, hosts } => {
                let section = app.provider_config.section(&provider).cloned();
                if let Some(section) = section {
                    if let Some(provider_impl) = providers::get_provider(&provider) {
                        let result = providers::sync::sync_provider(
                            &mut app.config,
                            &*provider_impl,
                            &hosts,
                            &section,
                            false,
                            false,
                        );
                        if result.added > 0 || result.updated > 0 {
                            if let Err(e) = app.config.write() {
                                app.set_status(format!("Sync failed to save: {}", e), true);
                                app.syncing_providers.remove(&provider);
                                continue;
                            }
                            app.update_last_modified();
                            app.reload_hosts();
                        }
                        let display_name = match provider.as_str() {
                            "digitalocean" => "DigitalOcean",
                            "vultr" => "Vultr",
                            "linode" => "Linode",
                            "hetzner" => "Hetzner",
                            "upcloud" => "UpCloud",
                            name => name,
                        };
                        app.set_status(
                            format!(
                                "Synced {}: added {}, updated {}, unchanged {}.",
                                display_name, result.added, result.updated, result.unchanged
                            ),
                            false,
                        );
                    }
                }
                app.syncing_providers.remove(&provider);
            }
            AppEvent::SyncError { provider, message } => {
                let display_name = match provider.as_str() {
                    "digitalocean" => "DigitalOcean",
                    "vultr" => "Vultr",
                    "linode" => "Linode",
                    "hetzner" => "Hetzner",
                    "upcloud" => "UpCloud",
                    name => name,
                };
                app.set_status(
                    format!("! {} sync failed: {}", display_name, message),
                    true,
                );
                app.syncing_providers.remove(&provider);
            }
            AppEvent::PollError => {
                app.running = false;
            }
        }

        // Handle pending SSH connection
        if let Some(alias) = app.pending_connect.take() {
            events.pause();
            terminal.exit()?;
            println!("Beaming you up to {}...\n", alias);
            let status = connection::connect(&alias);
            println!();
            match &status {
                Ok(exit) => {
                    let code = exit.code().unwrap_or(1);
                    if code != 255 {
                        app.history.record(&alias);
                    }
                    if code != 0 {
                        app.set_status(
                            format!("SSH to {} exited with code {}.", alias, code),
                            true,
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Connection failed: {}", e);
                    app.set_status(format!("Connection to {} failed.", alias), true);
                }
            }
            terminal.enter()?;
            events.resume();
            last_config_check = std::time::Instant::now();
            // Reload in case config changed externally
            let config_path = resolve_config_path(config_str)?;
            app.config = SshConfigFile::parse(&config_path)?;
            app.reload_hosts();
        }
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
    if alias_str.contains('*') || alias_str.contains('?') {
        eprintln!("Alias can't contain wildcards. Use --alias to pick a different name.");
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
        identity_file: key.unwrap_or("").to_string(),
        proxy_jump: String::new(),
        source_file: None,
        tags: Vec::new(),
        provider: None,
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
) -> Result<()> {
    let provider_config = providers::config::ProviderConfig::load();
    let sections: Vec<&providers::config::ProviderSection> = if let Some(name) = provider_name {
        if providers::get_provider(name).is_none() {
            eprintln!(
                "Never heard of '{}'. Try: digitalocean, vultr, linode, hetzner, upcloud.",
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

    for section in &sections {
        let provider = match providers::get_provider(&section.provider) {
            Some(p) => p,
            None => {
                eprintln!(
                    "Skipping unknown provider '{}'. Try: digitalocean, vultr, linode, hetzner, upcloud.",
                    section.provider
                );
                any_failures = true;
                continue;
            }
        };
        let display_name = match section.provider.as_str() {
            "digitalocean" => "DigitalOcean",
            "vultr" => "Vultr",
            "linode" => "Linode",
            "hetzner" => "Hetzner",
            "upcloud" => "UpCloud",
            name => name,
        };
        print!("Syncing {}... ", display_name);

        match provider.fetch_hosts(&section.token) {
            Ok(hosts) => {
                println!("{} servers found.", hosts.len());
                let result = providers::sync::sync_provider(
                    &mut config, &*provider, &hosts, section, remove, dry_run,
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
            Err(e) => {
                println!("failed.");
                eprintln!("! {}: {}", display_name, e);
                any_failures = true;
            }
        }
    }

    if any_changes && !dry_run {
        config.write()?;
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
            prefix,
            user,
            key,
        } => {
            let p = match providers::get_provider(&provider) {
                Some(p) => p,
                None => {
                    eprintln!(
                        "Never heard of '{}'. Try: digitalocean, vultr, linode, hetzner, upcloud.",
                        provider
                    );
                    std::process::exit(1);
                }
            };

            let section = providers::config::ProviderSection {
                provider: provider.clone(),
                token,
                alias_prefix: prefix.unwrap_or_else(|| p.short_label().to_string()),
                user: user.unwrap_or_else(|| "root".to_string()),
                identity_file: key.unwrap_or_default(),
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
                    let display_name = match s.provider.as_str() {
                        "digitalocean" => "DigitalOcean",
                        "vultr" => "Vultr",
                        "linode" => "Linode",
                        "hetzner" => "Hetzner",
                        "upcloud" => "UpCloud",
                        name => name,
                    };
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
