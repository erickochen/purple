use std::sync::mpsc;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, FormField, HostForm, Screen};
use crate::clipboard;
use crate::event::AppEvent;
use crate::ping;
use crate::quick_add;

/// Handle a key event based on the current screen.
pub fn handle_key_event(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    // Global Ctrl+C handler — works on every screen
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.running = false;
        return Ok(());
    }

    match &app.screen {
        Screen::HostList => {
            if app.search_query.is_some() {
                handle_host_list_search(app, key, events_tx);
            } else {
                handle_host_list(app, key, events_tx);
            }
        }
        Screen::AddHost | Screen::EditHost { .. } => handle_form(app, key),
        Screen::ConfirmDelete { .. } => handle_confirm_delete(app, key),
        Screen::Help => handle_help(app, key),
        Screen::KeyList => handle_key_list(app, key),
        Screen::KeyDetail { .. } => handle_key_detail(app, key),
    }
    Ok(())
}

fn handle_host_list(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.running = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev();
        }
        KeyCode::Enter => {
            if let Some(host) = app.selected_host() {
                let alias = host.alias.clone();
                app.pending_connect = Some(alias);
            }
        }
        KeyCode::Char('a') => {
            app.form = HostForm::new();
            app.screen = Screen::AddHost;
        }
        KeyCode::Char('e') => {
            if let (Some(index), Some(host)) = (app.selected_host_index(), app.selected_host()) {
                if let Some(ref source) = host.source_file {
                    let alias = host.alias.clone();
                    let path = source.display();
                    app.set_status(
                        format!("{} lives in {}. Edit it there.", alias, path),
                        true,
                    );
                    return;
                }
                app.form = HostForm::from_entry(host);
                app.screen = Screen::EditHost { index };
            }
        }
        KeyCode::Char('d') => {
            if let (Some(index), Some(host)) = (app.selected_host_index(), app.selected_host()) {
                if let Some(ref source) = host.source_file {
                    let alias = host.alias.clone();
                    let path = source.display();
                    app.set_status(
                        format!("{} lives in {}. Edit it there.", alias, path),
                        true,
                    );
                    return;
                }
                app.screen = Screen::ConfirmDelete { index };
            }
        }
        KeyCode::Char('y') => {
            if let Some(host) = app.selected_host() {
                let cmd = host.ssh_command();
                let alias = host.alias.clone();
                match clipboard::copy_to_clipboard(&cmd) {
                    Ok(()) => {
                        app.set_status(format!("Copied SSH command for {}.", alias), false);
                    }
                    Err(e) => {
                        app.set_status(e, true);
                    }
                }
            }
        }
        KeyCode::Char('p') => {
            if let Some(host) = app.selected_host() {
                let alias = host.alias.clone();
                if !host.proxy_jump.is_empty() {
                    app.ping_status
                        .insert(alias.clone(), crate::app::PingStatus::Skipped);
                    app.set_status(
                        format!("{} uses ProxyJump. Can't ping directly.", alias),
                        true,
                    );
                } else {
                    let hostname = host.hostname.clone();
                    let port = host.port;
                    app.ping_status
                        .insert(alias.clone(), crate::app::PingStatus::Checking);
                    app.set_status(format!("Pinging {}...", alias), false);
                    ping::ping_host(alias, hostname, port, events_tx.clone());
                }
            }
        }
        KeyCode::Char('P') => {
            let hosts_to_ping: Vec<(String, String, u16)> = app
                .hosts
                .iter()
                .filter(|h| !h.hostname.is_empty() && h.proxy_jump.is_empty())
                .map(|h| (h.alias.clone(), h.hostname.clone(), h.port))
                .collect();
            // Mark ProxyJump hosts as skipped (can't ping directly)
            for h in &app.hosts {
                if !h.proxy_jump.is_empty() {
                    app.ping_status
                        .insert(h.alias.clone(), crate::app::PingStatus::Skipped);
                }
            }
            if !hosts_to_ping.is_empty() {
                for (alias, _, _) in &hosts_to_ping {
                    app.ping_status
                        .insert(alias.clone(), crate::app::PingStatus::Checking);
                }
                app.set_status("Pinging all the things...", false);
                ping::ping_all(&hosts_to_ping, events_tx.clone());
            }
        }
        KeyCode::Char('/') => {
            app.start_search();
        }
        KeyCode::Char('K') => {
            app.scan_keys();
            app.screen = Screen::KeyList;
        }
        KeyCode::Char('?') => {
            app.screen = Screen::Help;
        }
        _ => {}
    }
}

fn handle_host_list_search(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    match key.code {
        KeyCode::Esc => {
            app.cancel_search();
        }
        KeyCode::Enter => {
            if let Some(host) = app.selected_host() {
                let alias = host.alias.clone();
                app.cancel_search();
                app.pending_connect = Some(alias);
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            app.select_next();
        }
        KeyCode::Up | KeyCode::BackTab => {
            app.select_prev();
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Ctrl+P also for ping in search mode
            if let Some(host) = app.selected_host() {
                let alias = host.alias.clone();
                let hostname = host.hostname.clone();
                let port = host.port;
                app.ping_status
                    .insert(alias.clone(), crate::app::PingStatus::Checking);
                ping::ping_host(alias, hostname, port, events_tx.clone());
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut query) = app.search_query {
                query.push(c);
            }
            app.apply_filter();
        }
        KeyCode::Backspace => {
            if let Some(ref mut query) = app.search_query {
                query.pop();
            }
            app.apply_filter();
        }
        _ => {}
    }
}

fn handle_form(app: &mut App, key: KeyEvent) {
    // Dispatch to key picker if it's open
    if app.show_key_picker {
        handle_key_picker(app, key);
        return;
    }

    // Ctrl+K opens key picker when on IdentityFile field
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && key.code == KeyCode::Char('k')
        && app.form.focused_field == FormField::IdentityFile
    {
        app.scan_keys();
        app.show_key_picker = true;
        app.key_picker_state = ratatui::widgets::ListState::default();
        if !app.keys.is_empty() {
            app.key_picker_state.select(Some(0));
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            app.screen = Screen::HostList;
        }
        KeyCode::Tab | KeyCode::Down => {
            // Smart paste detection: when leaving Alias field, check for user@host:port
            if app.form.focused_field == FormField::Alias {
                maybe_smart_paste(app);
            }
            app.form.focused_field = app.form.focused_field.next();
        }
        KeyCode::BackTab => {
            app.form.focused_field = app.form.focused_field.prev();
        }
        KeyCode::Up => {
            app.form.focused_field = app.form.focused_field.prev();
        }
        KeyCode::Enter => {
            // Smart paste on submit too
            if app.form.focused_field == FormField::Alias {
                maybe_smart_paste(app);
            }
            submit_form(app);
        }
        KeyCode::Char(c) => {
            app.form.focused_value_mut().push(c);
        }
        KeyCode::Backspace => {
            app.form.focused_value_mut().pop();
        }
        _ => {}
    }
}

/// If the alias field contains something like user@host:port, auto-parse and fill fields.
fn maybe_smart_paste(app: &mut App) {
    let alias_value = app.form.alias.clone();
    if !quick_add::looks_like_target(&alias_value) {
        return;
    }
    if let Ok(parsed) = quick_add::parse_target(&alias_value) {
        // Only auto-fill if other fields are still at defaults
        if app.form.hostname.is_empty() {
            app.form.hostname = parsed.hostname.clone();
        }
        if app.form.user.is_empty() && !parsed.user.is_empty() {
            app.form.user = parsed.user.clone();
        }
        if app.form.port == "22" && parsed.port != 22 {
            app.form.port = parsed.port.to_string();
        }
        // Generate a clean alias from the hostname
        let clean_alias = parsed
            .hostname
            .split('.')
            .next()
            .unwrap_or(&parsed.hostname)
            .to_string();
        app.form.alias = clean_alias;
        app.set_status("Smart-parsed that for you. Check the fields.", false);
    }
}

fn submit_form(app: &mut App) {
    // Validate
    if let Err(msg) = app.form.validate() {
        app.set_status(msg, true);
        return;
    }

    let entry = app.form.to_entry();
    let alias = entry.alias.clone();

    match &app.screen {
        Screen::AddHost => {
            // Check for duplicate alias
            if app.config.has_host(&alias) {
                app.set_status(
                    format!(
                        "'{}' already exists. Aliases are like fingerprints — unique.",
                        alias
                    ),
                    true,
                );
                return;
            }
            app.config.add_host(&entry);
            if let Err(e) = app.config.write() {
                app.set_status(format!("Failed to save: {}", e), true);
                return;
            }
            app.reload_hosts();
            // Auto-select the newly added host (find it in display list)
            for (i, item) in app.display_list.iter().enumerate() {
                if let crate::app::HostListItem::Host { index } = item {
                    if app.hosts.get(*index).is_some_and(|h| h.alias == alias) {
                        app.list_state.select(Some(i));
                        break;
                    }
                }
            }
            app.set_status(format!("Welcome aboard, {}!", alias), false);
        }
        Screen::EditHost { index } => {
            let Some(old_host) = app.hosts.get(*index) else {
                app.set_status("Host no longer exists.", true);
                app.screen = Screen::HostList;
                return;
            };
            let old_alias = old_host.alias.clone();
            // Check for duplicate if alias changed
            if alias != old_alias && app.config.has_host(&alias) {
                app.set_status(
                    format!(
                        "'{}' already exists. Aliases are like fingerprints — unique.",
                        alias
                    ),
                    true,
                );
                return;
            }
            app.config.update_host(&old_alias, &entry);
            if let Err(e) = app.config.write() {
                app.set_status(format!("Failed to save: {}", e), true);
                return;
            }
            app.reload_hosts();
            app.set_status(format!("{} got a makeover.", alias), false);
        }
        _ => {}
    }

    app.screen = Screen::HostList;
}

fn handle_confirm_delete(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Screen::ConfirmDelete { index } = app.screen {
                if index < app.hosts.len() {
                    let alias = app.hosts[index].alias.clone();
                    app.config.delete_host(&alias);
                    if let Err(e) = app.config.write() {
                        app.set_status(format!("Failed to save: {}", e), true);
                    } else {
                        app.set_status(
                            format!("Goodbye, {}. We barely knew ye.", alias),
                            false,
                        );
                    }
                    app.reload_hosts();
                }
            }
            app.screen = Screen::HostList;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.screen = Screen::HostList;
        }
        _ => {}
    }
}

fn handle_help(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
            app.screen = Screen::HostList;
        }
        _ => {}
    }
}

fn handle_key_list(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('K') => {
            app.screen = Screen::HostList;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_key();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_key();
        }
        KeyCode::Enter => {
            if let Some(index) = app.key_list_state.selected() {
                if index < app.keys.len() {
                    app.screen = Screen::KeyDetail { index };
                }
            }
        }
        _ => {}
    }
}

fn handle_key_detail(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.screen = Screen::KeyList;
        }
        _ => {}
    }
}

fn handle_key_picker(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.show_key_picker = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_picker_key();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_picker_key();
        }
        KeyCode::Enter => {
            if let Some(index) = app.key_picker_state.selected() {
                if let Some(key) = app.keys.get(index) {
                    app.form.identity_file = key.display_path.clone();
                    app.set_status(format!("Locked and loaded with {}.", key.name), false);
                }
            }
            app.show_key_picker = false;
        }
        _ => {}
    }
}
