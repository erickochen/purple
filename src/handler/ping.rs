use std::sync::mpsc;

use crate::app::App;
use crate::event::AppEvent;
use crate::ping;

/// Ping the currently selected host (shared by 'p' key and Ctrl+P in search mode).
pub(super) fn ping_selected_host(
    app: &mut App,
    events_tx: &mpsc::Sender<AppEvent>,
    show_hint: bool,
) {
    if let Some(host) = app.selected_host() {
        let alias = host.alias.clone();
        // For ProxyJump hosts, ping the bastion instead and propagate the
        // result to all dependents (handled in main.rs PingResult handler).
        let (ping_alias, hostname, port) = if !host.proxy_jump.is_empty() {
            let bastion_alias = host.proxy_jump.clone();
            if let Some(bastion) = app.hosts.iter().find(|h| h.alias == bastion_alias) {
                app.ping_status
                    .insert(alias.clone(), crate::app::PingStatus::Checking);
                (
                    bastion.alias.clone(),
                    bastion.hostname.clone(),
                    bastion.port,
                )
            } else {
                app.set_status(
                    format!("Bastion {} not found in config.", bastion_alias),
                    true,
                );
                return;
            }
        } else {
            (alias.clone(), host.hostname.clone(), host.port)
        };
        app.ping_status
            .insert(ping_alias.clone(), crate::app::PingStatus::Checking);
        if show_hint && !app.has_pinged {
            app.set_status(
                format!("Pinging {}... (Shift+P pings all)", ping_alias),
                false,
            );
            app.has_pinged = true;
        } else {
            app.set_status(format!("Pinging {}...", ping_alias), false);
        }
        ping::ping_host(
            ping_alias,
            hostname,
            port,
            events_tx.clone(),
            app.ping_generation,
        );
    }
}
