mod app;
mod clipboard;
mod connection;
mod event;
mod handler;
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
    if let Some(Commands::Add { target, alias, key }) = cli.command {
        return handle_quick_add(config, &target, alias.as_deref(), key.as_deref());
    }

    // Direct connect mode (--connect)
    if let Some(alias) = cli.connect {
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
    let mut terminal = tui::Tui::new()?;
    terminal.enter()?;
    let events = EventHandler::new(250);
    let events_tx = events.sender();

    while app.running {
        terminal.draw(&mut app)?;

        match events.next()? {
            AppEvent::Key(key) => handler::handle_key_event(&mut app, key, &events_tx)?,
            AppEvent::Tick => app.tick_status(),
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
            terminal.exit()?;
            println!("Beaming you up to {}...\n", alias);
            let status = connection::connect(&alias);
            println!();
            if let Err(ref e) = status {
                eprintln!("Connection failed: {}", e);
            }
            terminal.enter()?;
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
        eprintln!("'{}' already exists. Pick a different name.", alias_str);
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
    };

    config.add_host(&entry);
    config.write()?;
    println!("Welcome aboard, {}!", alias_str);
    Ok(())
}
