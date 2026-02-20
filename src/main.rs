mod app;
mod connection;
mod event;
mod handler;
mod ssh_config;
mod ssh_keys;
mod tui;
mod ui;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use app::App;
use event::{AppEvent, EventHandler};
use ssh_config::model::SshConfigFile;

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
    /// Connect directly to a host by alias (skip the TUI)
    #[arg(short, long)]
    connect: Option<String>,

    /// List all configured hosts
    #[arg(short, long)]
    list: bool,

    /// Path to SSH config file
    #[arg(long, default_value = "~/.ssh/config")]
    config: String,
}

fn resolve_config_path(path: &str) -> Result<PathBuf> {
    if path.starts_with("~/") {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(&path[2..]))
    } else {
        Ok(PathBuf::from(path))
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = resolve_config_path(&cli.config)?;
    let config = SshConfigFile::parse(&config_path)?;

    // Direct connect mode
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

    // Interactive TUI mode
    let mut app = App::new(config);
    let mut terminal = tui::Tui::new()?;
    terminal.enter()?;
    let events = EventHandler::new(250);

    while app.running {
        terminal.draw(&mut app)?;

        match events.next()? {
            AppEvent::Key(key) => handler::handle_key_event(&mut app, key)?,
            AppEvent::Tick => app.tick_status(),
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
            let config_path = resolve_config_path(&cli.config)?;
            app.config = SshConfigFile::parse(&config_path)?;
            app.reload_hosts();
        }
    }

    terminal.exit()?;
    Ok(())
}
