mod app;
mod clipboard;
mod connection;
mod event;
mod handler;
mod history;
mod import;
mod ping;
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

    let config_path = resolve_config_path(&cli.config)?;
    let config = SshConfigFile::parse(&config_path)?;

    // Handle subcommands
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
        None => {}
    }

    // Direct connect mode (--connect)
    if let Some(alias) = cli.connect {
        history::ConnectionHistory::load().record(&alias);
        let status = connection::connect(&alias)?;
        std::process::exit(status.code().unwrap_or(1));
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
            history::ConnectionHistory::load().record(&alias);
            println!("Beaming you up to {}...\n", alias);
            let status = connection::connect(&alias)?;
            std::process::exit(status.code().unwrap_or(1));
        }
        // No exact match — open TUI with search pre-filled
        let mut app = App::new(config);
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
    let app = App::new(config);
    run_tui(app, &cli.config)
}

fn run_tui(mut app: App, config_str: &str) -> Result<()> {
    // First-launch welcome hint (one-shot: creates .purple/ so it won't show again)
    if app.status.is_none() && !app.hosts.is_empty() {
        if let Some(home) = dirs::home_dir() {
            let purple_dir = home.join(".purple");
            if !purple_dir.exists() {
                let _ = std::fs::create_dir_all(&purple_dir);
                app.set_status("Welcome to Purple. Press ? for the cheat sheet.", false);
            }
        }
    }

    let mut terminal = tui::Tui::new()?;
    terminal.enter()?;
    let events = EventHandler::new(250);
    let events_tx = events.sender();
    let mut last_config_check = std::time::Instant::now();

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
        }

        // Handle pending SSH connection
        if let Some(alias) = app.pending_connect.take() {
            app.history.record(&alias);
            events.pause();
            terminal.exit()?;
            println!("Beaming you up to {}...\n", alias);
            let status = connection::connect(&alias);
            println!();
            match &status {
                Ok(exit) => {
                    if let Some(code) = exit.code() {
                        if code != 0 {
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
        Ok((imported, skipped)) => {
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
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}
