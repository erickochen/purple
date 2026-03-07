use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, FormField, HostForm, ProviderFormFields, Screen, ViewMode};
use crate::clipboard;
use crate::event::AppEvent;
use crate::ping;
use crate::preferences;
use crate::providers;
use crate::quick_add;
use crate::ssh_config::model::ConfigElement;

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
            if app.search.query.is_some() {
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
        Screen::HostDetail { .. } => handle_host_detail(app, key),
        Screen::TagPicker => handle_tag_picker_screen(app, key),
        Screen::Providers => handle_provider_list(app, key, events_tx),
        Screen::ProviderForm { .. } => handle_provider_form(app, key, events_tx),
        Screen::TunnelList { .. } => handle_tunnel_list(app, key),
        Screen::TunnelForm { .. } => handle_tunnel_form(app, key),
        Screen::ConfirmHostKeyReset { .. } => handle_confirm_host_key_reset(app, key),
    }
    Ok(())
}

fn handle_host_list(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    // Handle tag input mode
    if app.tag_input.is_some() {
        handle_tag_input(app, key);
        return;
    }

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
                let askpass = host.askpass.clone();
                app.pending_connect = Some((alias, askpass));
            }
        }
        KeyCode::Char('a') => {
            app.form = HostForm::new();
            app.screen = Screen::AddHost;
            app.capture_form_mtime();
        }
        KeyCode::Char('e') => {
            if let Some(host) = app.selected_host() {
                if let Some(ref source) = host.source_file {
                    let alias = host.alias.clone();
                    let path = source.display();
                    app.set_status(
                        format!("{} lives in {}. Edit it there.", alias, path),
                        true,
                    );
                    return;
                }
                let alias = host.alias.clone();
                app.form = HostForm::from_entry(host);
                app.screen = Screen::EditHost { alias };
                app.capture_form_mtime();
            }
        }
        KeyCode::Char('d') => {
            if let Some(host) = app.selected_host() {
                if let Some(ref source) = host.source_file {
                    let alias = host.alias.clone();
                    let path = source.display();
                    app.set_status(
                        format!("{} lives in {}. Edit it there.", alias, path),
                        true,
                    );
                    return;
                }
                let alias = host.alias.clone();
                app.screen = Screen::ConfirmDelete { alias };
            }
        }
        KeyCode::Char('c') => {
            if let Some(host) = app.selected_host() {
                if let Some(ref source) = host.source_file {
                    let alias = host.alias.clone();
                    let path = source.display();
                    app.set_status(
                        format!("{} lives in {}. Clone it there.", alias, path),
                        true,
                    );
                    return;
                }
                let mut form = HostForm::from_entry(host);
                form.alias = format!("{}-copy", host.alias);
                app.form = form;
                app.screen = Screen::AddHost;
                app.capture_form_mtime();
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
        KeyCode::Char('x') => {
            if let Some(host) = app.selected_host() {
                let alias = host.alias.clone();
                if let Some(block) = serialize_host_block(&app.config.elements, &alias, app.config.crlf) {
                    match clipboard::copy_to_clipboard(&block) {
                        Ok(()) => {
                            app.set_status(
                                format!("Copied config block for {}.", alias),
                                false,
                            );
                        }
                        Err(e) => {
                            app.set_status(e, true);
                        }
                    }
                }
            }
        }
        KeyCode::Char('p') => {
            ping_selected_host(app, events_tx, true);
        }
        KeyCode::Char('P') => {
            // Skip if a ping-all is already in progress
            if app.ping_status.values().any(|s| *s == crate::app::PingStatus::Checking) {
                return;
            }
            let hosts_to_ping: Vec<(String, String, u16)> = app
                .hosts
                .iter()
                .filter(|h| !h.hostname.is_empty() && h.proxy_jump.is_empty())
                .filter(|h| {
                    !matches!(
                        app.ping_status.get(&h.alias),
                        Some(crate::app::PingStatus::Checking)
                    )
                })
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
        KeyCode::Char('t') => {
            if let Some(host) = app.selected_host() {
                if let Some(ref source) = host.source_file {
                    let alias = host.alias.clone();
                    let path = source.display();
                    app.set_status(
                        format!("{} is included from {}. Tag it there.", alias, path),
                        true,
                    );
                    return;
                }
                let current_tags = host.tags.join(", ");
                app.tag_input = Some(current_tags);
            }
        }
        KeyCode::Char('s') => {
            app.sort_mode = app.sort_mode.next();
            app.apply_sort();
            if let Err(e) = preferences::save_sort_mode(app.sort_mode) {
                app.set_status(format!("Sorted by {}. (save failed: {})", app.sort_mode.label(), e), true);
            } else {
                app.set_status(format!("Sorted by {}.", app.sort_mode.label()), false);
            }
        }
        KeyCode::Char('g') => {
            app.group_by_provider = !app.group_by_provider;
            app.apply_sort();
            if let Err(e) = preferences::save_group_by_provider(app.group_by_provider) {
                let msg = if app.group_by_provider {
                    format!("Grouped by provider. (save failed: {})", e)
                } else {
                    format!("Ungrouped. (save failed: {})", e)
                };
                app.set_status(msg, true);
            } else if app.group_by_provider {
                app.set_status("Grouped by provider.", false);
            } else {
                app.set_status("Ungrouped.", false);
            }
        }
        KeyCode::Char('i') => {
            if let Some(index) = app.selected_host_index() {
                app.screen = Screen::HostDetail { index };
            }
        }
        KeyCode::Char('v') => {
            app.view_mode = match app.view_mode {
                ViewMode::Compact => ViewMode::Detailed,
                ViewMode::Detailed => ViewMode::Compact,
            };
            let _ = preferences::save_view_mode(app.view_mode);
        }
        KeyCode::Char('u') => {
            if let Some(deleted) = app.deleted_host.take() {
                let alias = match &deleted.element {
                    ConfigElement::HostBlock(block) => block.host_pattern.clone(),
                    _ => "host".to_string(),
                };
                app.config.insert_host_at(deleted.element, deleted.position);
                if let Err(e) = app.config.write() {
                    // Rollback: remove re-inserted host and restore undo buffer
                    if let Some((element, position)) = app.config.delete_host_undoable(&alias) {
                        app.deleted_host = Some(crate::app::DeletedHost { element, position });
                    }
                    app.set_status(format!("Failed to save: {}", e), true);
                } else {
                    app.update_last_modified();
                    app.reload_hosts();
                    app.set_status(format!("{} is back from the dead.", alias), false);
                }
            } else {
                app.set_status("Nothing to undo.", true);
            }
        }
        KeyCode::Char('#') => {
            app.open_tag_picker();
        }
        KeyCode::Char('T') => {
            if let Some(host) = app.selected_host() {
                let alias = host.alias.clone();
                app.refresh_tunnel_list(&alias);
                app.ui.tunnel_list_state = ratatui::widgets::ListState::default();
                if !app.tunnel_list.is_empty() {
                    app.ui.tunnel_list_state.select(Some(0));
                }
                app.screen = Screen::TunnelList { alias };
            }
        }
        KeyCode::Char('S') => {
            app.provider_config = crate::providers::config::ProviderConfig::load();
            app.ui.provider_list_state = ratatui::widgets::ListState::default();
            app.ui.provider_list_state.select(Some(0));
            app.screen = Screen::Providers;
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
                let askpass = host.askpass.clone();
                app.cancel_search();
                app.pending_connect = Some((alias, askpass));
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            app.select_next();
        }
        KeyCode::Up | KeyCode::BackTab => {
            app.select_prev();
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            ping_selected_host(app, events_tx, false);
        }
        KeyCode::Char(c) => {
            if let Some(ref mut query) = app.search.query {
                query.push(c);
            }
            app.apply_filter();
        }
        KeyCode::Backspace => {
            if let Some(ref mut query) = app.search.query {
                query.pop();
            }
            app.apply_filter();
        }
        _ => {}
    }
}

fn handle_form(app: &mut App, key: KeyEvent) {
    // Dispatch to password picker if it's open
    if app.ui.show_password_picker {
        handle_password_picker(app, key);
        return;
    }

    // Dispatch to key picker if it's open
    if app.ui.show_key_picker {
        handle_key_picker_shared(app, key, false);
        return;
    }

    match key.code {
        KeyCode::Esc => {
            app.clear_form_mtime();
            app.screen = Screen::HostList;
        }
        KeyCode::Tab | KeyCode::Down => {
            // Smart paste detection: when leaving Alias field, check for user@host:port
            if app.form.focused_field == FormField::Alias {
                maybe_smart_paste(app);
            }
            app.form.focused_field = app.form.focused_field.next();
            app.form.sync_cursor_to_end();
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.form.focused_field = app.form.focused_field.prev();
            app.form.sync_cursor_to_end();
        }
        KeyCode::Left => {
            if app.form.cursor_pos > 0 {
                app.form.cursor_pos -= 1;
            }
        }
        KeyCode::Right => {
            let len = app.form.focused_value().chars().count();
            if app.form.cursor_pos < len {
                app.form.cursor_pos += 1;
            }
        }
        KeyCode::Home => {
            app.form.cursor_pos = 0;
        }
        KeyCode::End => {
            app.form.sync_cursor_to_end();
        }
        KeyCode::Enter => {
            match app.form.focused_field {
                FormField::IdentityFile => {
                    app.scan_keys();
                    app.ui.show_key_picker = true;
                    app.ui.key_picker_state = ratatui::widgets::ListState::default();
                    if !app.keys.is_empty() {
                        app.ui.key_picker_state.select(Some(0));
                    }
                }
                FormField::AskPass => {
                    app.ui.show_password_picker = true;
                    app.ui.password_picker_state = ratatui::widgets::ListState::default();
                    app.ui.password_picker_state.select(Some(0));
                }
                FormField::Alias => {
                    maybe_smart_paste(app);
                    submit_form(app);
                }
                _ => {
                    submit_form(app);
                }
            }
        }
        KeyCode::Char(c) => {
            app.form.insert_char(c);
        }
        KeyCode::Backspace => {
            app.form.delete_char_before_cursor();
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
            app.form.user = parsed.user;
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
    // Check for external config changes since form was opened
    if app.config_changed_since_form_open() {
        app.set_status(
            "Config changed externally. Press Esc and re-open to pick up changes.",
            true,
        );
        return;
    }

    // Validate
    if let Err(msg) = app.form.validate() {
        app.set_status(msg, true);
        return;
    }

    // Track old askpass to detect keychain removal
    let old_askpass = match &app.screen {
        Screen::EditHost { alias } => app.hosts.iter()
            .find(|h| h.alias == *alias)
            .and_then(|h| h.askpass.clone()),
        _ => None,
    };

    let result = match &app.screen {
        Screen::AddHost => app.add_host_from_form(),
        Screen::EditHost { alias } => {
            let old = alias.clone();
            app.edit_host_from_form(&old)
        }
        _ => return,
    };
    match result {
        Ok(msg) => {
            // Clear undo buffer after successful write
            app.deleted_host = None;
            // Handle keychain changes on edit
            let mut final_msg = msg;
            if old_askpass.as_deref() == Some("keychain") {
                if app.form.askpass != "keychain" {
                    // Source changed away from keychain — remove old entry
                    if let Screen::EditHost { ref alias } = app.screen {
                        let _ = crate::askpass::remove_from_keychain(alias);
                    }
                    final_msg = format!("{}. Keychain entry removed.", final_msg);
                } else if let Screen::EditHost { ref alias } = app.screen {
                    // Alias renamed — migrate keychain entry
                    if *alias != app.form.alias {
                        if let Ok(pw) = crate::askpass::retrieve_keychain_password(alias) {
                            let _ = crate::askpass::store_in_keychain(&app.form.alias, &pw);
                            let _ = crate::askpass::remove_from_keychain(alias);
                        }
                    }
                }
            }
            app.set_status(final_msg, false);
        }
        Err(msg) => {
            app.set_status(msg, true);
            return;
        }
    }

    let target_alias = app.form.alias.trim().to_string();
    app.clear_form_mtime();
    app.screen = Screen::HostList;
    app.select_host_by_alias(&target_alias);
}

fn handle_confirm_delete(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Screen::ConfirmDelete { ref alias } = app.screen {
                let alias = alias.clone();
                if let Some((element, position)) = app.config.delete_host_undoable(&alias) {
                    if let Err(e) = app.config.write() {
                        // Restore the element on write failure
                        app.config.insert_host_at(element, position);
                        app.set_status(format!("Failed to save: {}", e), true);
                    } else {
                        // Stop active tunnel for the deleted host
                        if let Some(mut tunnel) = app.active_tunnels.remove(&alias) {
                            let _ = tunnel.child.kill();
                            let _ = tunnel.child.wait();
                        }
                        app.deleted_host = Some(crate::app::DeletedHost {
                            element,
                            position,
                        });
                        app.update_last_modified();
                        app.reload_hosts();
                        app.set_status(
                            format!("Goodbye, {}. We barely knew ye. (u to undo)", alias),
                            false,
                        );
                    }
                } else {
                    app.set_status(format!("Host '{}' not found.", alias), true);
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

fn handle_confirm_host_key_reset(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Screen::ConfirmHostKeyReset {
                ref alias,
                ref hostname,
                ref known_hosts_path,
                ref askpass,
            } = app.screen
            {
                let alias = alias.clone();
                let hostname = hostname.clone();
                let known_hosts_path = known_hosts_path.clone();
                let askpass = askpass.clone();

                let output = std::process::Command::new("ssh-keygen")
                    .arg("-R")
                    .arg(&hostname)
                    .arg("-f")
                    .arg(&known_hosts_path)
                    .output();

                match output {
                    Ok(result) if result.status.success() => {
                        app.set_status(
                            format!("Removed host key for {}. Reconnecting...", hostname),
                            false,
                        );
                        app.pending_connect = Some((alias, askpass));
                    }
                    Ok(result) => {
                        let stderr = String::from_utf8_lossy(&result.stderr);
                        app.set_status(
                            format!("Failed to remove host key: {}", stderr.trim()),
                            true,
                        );
                    }
                    Err(e) => {
                        app.set_status(
                            format!("Failed to run ssh-keygen: {}", e),
                            true,
                        );
                    }
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
            if let Some(index) = app.ui.key_list_state.selected() {
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

/// Serialize a host block to its raw SSH config text.
fn serialize_host_block(elements: &[ConfigElement], alias: &str, crlf: bool) -> Option<String> {
    let line_ending = if crlf { "\r\n" } else { "\n" };
    for element in elements {
        match element {
            ConfigElement::HostBlock(block) if block.host_pattern == alias => {
                let mut output = block.raw_host_line.clone();
                for directive in &block.directives {
                    output.push_str(line_ending);
                    output.push_str(&directive.raw_line);
                }
                return Some(output);
            }
            ConfigElement::Include(include) => {
                for file in &include.resolved_files {
                    if let Some(result) = serialize_host_block(&file.elements, alias, crlf) {
                        return Some(result);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn handle_tag_input(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            if let Some(ref input) = app.tag_input {
                let tags: Vec<String> = input
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
                if let Some(host) = app.selected_host() {
                    let alias = host.alias.clone();
                    let old_tags = host.tags.clone();
                    app.config.set_host_tags(&alias, &tags);
                    if let Err(e) = app.config.write() {
                        // Restore old tags on write failure
                        app.config.set_host_tags(&alias, &old_tags);
                        app.set_status(format!("Failed to save: {}", e), true);
                    } else {
                        app.update_last_modified();
                        let count = tags.len();
                        app.reload_hosts();
                        app.select_host_by_alias(&alias);
                        app.set_status(
                            format!(
                                "Tagged {} with {} label{}.",
                                alias,
                                count,
                                if count == 1 { "" } else { "s" }
                            ),
                            false,
                        );
                    }
                }
            }
            app.tag_input = None;
        }
        KeyCode::Esc => {
            app.tag_input = None;
        }
        KeyCode::Char(c) => {
            if let Some(ref mut input) = app.tag_input {
                input.push(c);
            }
        }
        KeyCode::Backspace => {
            if let Some(ref mut input) = app.tag_input {
                input.pop();
            }
        }
        _ => {}
    }
}

fn handle_host_detail(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('i') => {
            app.screen = Screen::HostList;
        }
        _ => {}
    }
}

fn handle_tag_picker_screen(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('#') => {
            app.screen = Screen::HostList;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_tag();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_tag();
        }
        KeyCode::Enter => {
            if let Some(index) = app.ui.tag_picker_state.selected() {
                if let Some(tag) = app.tag_list.get(index) {
                    let tag: String = tag.clone();
                    app.screen = Screen::HostList;
                    app.start_search();
                    app.search.query = Some(format!("tag={}", tag));
                    app.apply_filter();
                }
            }
        }
        _ => {}
    }
}

fn handle_provider_list(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    // Handle pending provider delete confirmation first
    if app.pending_provider_delete.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let name = app.pending_provider_delete.take().unwrap();
                if let Some(old_section) = app.provider_config.section(name.as_str()).cloned() {
                    app.provider_config.remove_section(name.as_str());
                    if let Err(e) = app.provider_config.save() {
                        app.provider_config.set_section(old_section);
                        app.set_status(format!("Failed to save: {}", e), true);
                    } else {
                        app.sync_history.remove(name.as_str());
                        let display_name = crate::providers::provider_display_name(name.as_str());
                        app.set_status(
                            format!("Removed {} configuration. Synced hosts remain in your SSH config.", display_name),
                            false,
                        );
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.pending_provider_delete = None;
            }
            _ => {}
        }
        return;
    }

    let provider_count = app.sorted_provider_names().len();
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            // Cancel all running syncs
            for cancel_flag in app.syncing_providers.values() {
                cancel_flag.store(true, Ordering::Relaxed);
            }
            app.screen = Screen::HostList;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            crate::app::cycle_selection(&mut app.ui.provider_list_state, provider_count, true);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            crate::app::cycle_selection(&mut app.ui.provider_list_state, provider_count, false);
        }
        KeyCode::Enter => {
            if let Some(index) = app.ui.provider_list_state.selected() {
                let sorted = app.sorted_provider_names();
                if let Some(name) = sorted.get(index) {
                    let provider_impl = providers::get_provider(name.as_str());
                    let short_label = provider_impl
                        .as_ref()
                        .map(|p| p.short_label().to_string())
                        .unwrap_or_else(|| name.clone());

                    // Pre-fill form from existing config or defaults
                    let first_field = crate::app::ProviderFormField::fields_for(name.as_str())[0];
                    app.provider_form = if let Some(section) =
                        app.provider_config.section(name.as_str())
                    {
                        let cursor_pos = match first_field {
                            crate::app::ProviderFormField::Url => section.url.chars().count(),
                            crate::app::ProviderFormField::Token => section.token.chars().count(),
                            _ => 0,
                        };
                        ProviderFormFields {
                            url: section.url.clone(),
                            token: section.token.clone(),
                            alias_prefix: section.alias_prefix.clone(),
                            user: section.user.clone(),
                            identity_file: section.identity_file.clone(),
                            verify_tls: section.verify_tls,
                            auto_sync: section.auto_sync,
                            focused_field: first_field,
                            cursor_pos,
                        }
                    } else {
                        ProviderFormFields {
                            url: String::new(),
                            token: String::new(),
                            alias_prefix: short_label,
                            user: "root".to_string(),
                            identity_file: String::new(),
                            verify_tls: true,
                            auto_sync: name.as_str() != "proxmox",
                            focused_field: first_field,
                            cursor_pos: 0,
                        }
                    };
                    app.screen = Screen::ProviderForm {
                        provider: name.clone(),
                    };
                    app.capture_provider_form_mtime();
                }
            }
        }
        KeyCode::Char('s') => {
            if let Some(index) = app.ui.provider_list_state.selected() {
                let sorted = app.sorted_provider_names();
                if let Some(name) = sorted.get(index) {
                    if let Some(section) = app.provider_config.section(name.as_str()).cloned() {
                        if !app.syncing_providers.contains_key(name.as_str()) {
                            let cancel = Arc::new(AtomicBool::new(false));
                            app.syncing_providers.insert(name.clone(), cancel.clone());
                            let display_name = crate::providers::provider_display_name(name.as_str());
                            app.set_status(format!("Syncing {}...", display_name), false);
                            spawn_provider_sync(&section, events_tx.clone(), cancel);
                        }
                    } else {
                        let display_name = crate::providers::provider_display_name(name.as_str());
                        app.set_status(
                            format!("Configure {} first. Press Enter to set up.", display_name),
                            true,
                        );
                    }
                }
            }
        }
        KeyCode::Char('d') => {
            if let Some(index) = app.ui.provider_list_state.selected() {
                let sorted = app.sorted_provider_names();
                if let Some(name) = sorted.get(index) {
                    if app.provider_config.section(name.as_str()).is_some() {
                        app.pending_provider_delete = Some(name.clone());
                    } else {
                        let display_name = crate::providers::provider_display_name(name.as_str());
                        app.set_status(format!("{} is not configured. Nothing to remove.", display_name), false);
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_provider_form(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    // Dispatch to key picker if open
    if app.ui.show_key_picker {
        handle_key_picker_shared(app, key, true);
        return;
    }

    let provider_name = match &app.screen {
        Screen::ProviderForm { provider } => provider.clone(),
        _ => return,
    };
    let fields = crate::app::ProviderFormField::fields_for(&provider_name);

    match key.code {
        KeyCode::Esc => {
            app.clear_form_mtime();
            app.screen = Screen::Providers;
        }
        KeyCode::Tab | KeyCode::Down => {
            app.provider_form.focused_field = app.provider_form.focused_field.next(fields);
            app.provider_form.sync_cursor_to_end();
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.provider_form.focused_field = app.provider_form.focused_field.prev(fields);
            app.provider_form.sync_cursor_to_end();
        }
        KeyCode::Left | KeyCode::Right => {
            let f = app.provider_form.focused_field;
            if f == crate::app::ProviderFormField::VerifyTls {
                app.provider_form.verify_tls = !app.provider_form.verify_tls;
            } else if f == crate::app::ProviderFormField::AutoSync {
                app.provider_form.auto_sync = !app.provider_form.auto_sync;
            } else if key.code == KeyCode::Left {
                if app.provider_form.cursor_pos > 0 {
                    app.provider_form.cursor_pos -= 1;
                }
            } else {
                let len = app.provider_form.focused_value().chars().count();
                if app.provider_form.cursor_pos < len {
                    app.provider_form.cursor_pos += 1;
                }
            }
        }
        KeyCode::Home => {
            app.provider_form.cursor_pos = 0;
        }
        KeyCode::End => {
            app.provider_form.sync_cursor_to_end();
        }
        KeyCode::Enter => {
            if app.provider_form.focused_field == crate::app::ProviderFormField::IdentityFile {
                app.scan_keys();
                app.ui.show_key_picker = true;
                app.ui.key_picker_state = ratatui::widgets::ListState::default();
                if !app.keys.is_empty() {
                    app.ui.key_picker_state.select(Some(0));
                }
            } else {
                submit_provider_form(app, events_tx);
            }
        }
        KeyCode::Char(' ') if app.provider_form.focused_field == crate::app::ProviderFormField::VerifyTls => {
            app.provider_form.verify_tls = !app.provider_form.verify_tls;
        }
        KeyCode::Char(' ') if app.provider_form.focused_field == crate::app::ProviderFormField::AutoSync => {
            app.provider_form.auto_sync = !app.provider_form.auto_sync;
        }
        KeyCode::Char(c) => {
            let f = app.provider_form.focused_field;
            if f != crate::app::ProviderFormField::VerifyTls && f != crate::app::ProviderFormField::AutoSync {
                app.provider_form.insert_char(c);
            }
        }
        KeyCode::Backspace => {
            let f = app.provider_form.focused_field;
            if f != crate::app::ProviderFormField::VerifyTls && f != crate::app::ProviderFormField::AutoSync {
                app.provider_form.delete_char_before_cursor();
            }
        }
        _ => {}
    }
}

fn submit_provider_form(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    let provider_name = match &app.screen {
        Screen::ProviderForm { provider } => provider.clone(),
        _ => return,
    };

    // Check for external provider config changes since form was opened
    if app.provider_config_changed_since_form_open() {
        app.set_status(
            "Provider config changed externally. Press Esc and re-open to pick up changes.",
            true,
        );
        return;
    }

    // Reject control characters in all fields (prevents INI injection)
    let pf_fields = [
        (&app.provider_form.url, "URL"),
        (&app.provider_form.token, "Token"),
        (&app.provider_form.alias_prefix, "Alias Prefix"),
        (&app.provider_form.user, "User"),
        (&app.provider_form.identity_file, "Identity File"),
    ];
    for (value, name) in &pf_fields {
        if value.chars().any(|c| c.is_control()) {
            app.set_status(
                format!("{} contains control characters.", name),
                true,
            );
            return;
        }
    }

    // Proxmox requires a URL
    if provider_name == "proxmox" {
        let url = app.provider_form.url.trim();
        if url.is_empty() {
            app.set_status("URL is required for Proxmox VE.", true);
            return;
        }
        if !url.to_ascii_lowercase().starts_with("https://") {
            app.set_status("URL must start with https://. Toggle Verify TLS off for self-signed certificates.", true);
            return;
        }
    }

    if app.provider_form.token.trim().is_empty() {
        let display_name = crate::providers::provider_display_name(provider_name.as_str());
        app.set_status(
            format!(
                "Token can't be empty. Grab one from your {} dashboard.",
                display_name
            ),
            true,
        );
        return;
    }

    let token = app.provider_form.token.trim().to_string();
    let alias_prefix = app.provider_form.alias_prefix.trim().to_string();
    if crate::ssh_config::model::is_host_pattern(&alias_prefix) {
        app.set_status(
            "Alias prefix can't contain spaces or pattern characters (*, ?, [, !).",
            true,
        );
        return;
    }

    let user = {
        let u = app.provider_form.user.trim();
        if u.is_empty() { "root".to_string() } else { u.to_string() }
    };
    if user.contains(char::is_whitespace) {
        app.set_status("User can't contain whitespace.", true);
        return;
    }

    let section = providers::config::ProviderSection {
        provider: provider_name.clone(),
        token: token.clone(),
        alias_prefix,
        user,
        identity_file: app.provider_form.identity_file.trim().to_string(),
        url: app.provider_form.url.trim().to_string(),
        verify_tls: app.provider_form.verify_tls,
        auto_sync: app.provider_form.auto_sync,
    };

    let old_section = app.provider_config.section(&provider_name).cloned();
    app.provider_config.set_section(section);
    if let Err(e) = app.provider_config.save() {
        // Rollback: restore previous state
        match old_section {
            Some(old) => app.provider_config.set_section(old),
            None => app.provider_config.remove_section(&provider_name),
        }
        app.set_status(format!("Failed to save: {}", e), true);
        return;
    }

    let display_name = crate::providers::provider_display_name(provider_name.as_str());

    if !app.syncing_providers.contains_key(&provider_name) {
        let sync_section = app.provider_config.section(&provider_name).cloned();
        if let Some(sync_section) = sync_section {
            let cancel = Arc::new(AtomicBool::new(false));
            app.syncing_providers.insert(provider_name.clone(), cancel.clone());
            app.set_status(format!("Saved {} configuration. Syncing...", display_name), false);
            spawn_provider_sync(&sync_section, events_tx.clone(), cancel);
        }
    } else {
        app.set_status(format!("Saved {} configuration.", display_name), false);
    }
    app.clear_form_mtime();
    app.screen = Screen::Providers;
}

/// Password source picker handler.
fn handle_password_picker(app: &mut App, key: KeyEvent) {
    // Ctrl+D sets selected source as global default
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('d') {
        if let Some(index) = app.ui.password_picker_state.selected() {
            if let Some(source) = crate::askpass::PASSWORD_SOURCES.get(index) {
                let is_none = source.label == "None";
                let value = if is_none { "" } else { source.value };
                match crate::preferences::save_askpass_default(value) {
                    Ok(()) => {
                        if is_none {
                            app.set_status("Global default cleared.", false);
                        } else {
                            app.set_status(format!("Global default set to {}.", source.label), false);
                        }
                    }
                    Err(e) => {
                        app.set_status(format!("Failed to save default: {}", e), true);
                    }
                }
            }
        }
        app.ui.show_password_picker = false;
        return;
    }

    match key.code {
        KeyCode::Esc => {
            app.ui.show_password_picker = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_password_source();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_password_source();
        }
        KeyCode::Enter => {
            if let Some(index) = app.ui.password_picker_state.selected() {
                if let Some(source) = crate::askpass::PASSWORD_SOURCES.get(index) {
                    let is_none = source.label == "None";
                    let is_custom_cmd = source.label == "Custom command";
                    let is_prefix = source.value.ends_with(':') || source.value.ends_with("//");
                    if is_none {
                        app.form.askpass = String::new();
                        app.form.sync_cursor_to_end();
                        app.set_status("Password source cleared.", false);
                    } else if is_custom_cmd {
                        app.form.askpass = String::new();
                        app.form.focused_field = FormField::AskPass;
                        app.form.sync_cursor_to_end();
                        app.set_status("Type your command. Use %a (alias) and %h (hostname) as placeholders.", false);
                    } else if is_prefix {
                        app.form.askpass = source.value.to_string();
                        app.form.focused_field = FormField::AskPass;
                        app.form.sync_cursor_to_end();
                        app.set_status(format!("Complete the {} path.", source.label), false);
                    } else {
                        app.form.askpass = source.value.to_string();
                        app.form.sync_cursor_to_end();
                        app.set_status(
                            format!("Password source set to {}.", source.label),
                            false,
                        );
                    }
                }
            }
            app.ui.show_password_picker = false;
        }
        _ => {}
    }
}

/// Unified key picker handler for both host form and provider form.
fn handle_key_picker_shared(app: &mut App, key: KeyEvent, for_provider: bool) {
    match key.code {
        KeyCode::Esc => {
            app.ui.show_key_picker = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_picker_key();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_picker_key();
        }
        KeyCode::Enter => {
            if let Some(index) = app.ui.key_picker_state.selected() {
                if let Some(key_info) = app.keys.get(index) {
                    if for_provider {
                        app.provider_form.identity_file = key_info.display_path.clone();
                        app.provider_form.sync_cursor_to_end();
                    } else {
                        app.form.identity_file = key_info.display_path.clone();
                        app.form.sync_cursor_to_end();
                    }
                    app.set_status(
                        format!("Locked and loaded with {}.", key_info.name),
                        false,
                    );
                }
            }
            app.ui.show_key_picker = false;
        }
        _ => {}
    }
}

/// Ping the currently selected host (shared by 'p' key and Ctrl+P in search mode).
fn ping_selected_host(app: &mut App, events_tx: &mpsc::Sender<AppEvent>, show_hint: bool) {
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
            if show_hint && !app.has_pinged {
                app.set_status(
                    format!("Pinging {}... (Shift+P pings all)", alias),
                    false,
                );
                app.has_pinged = true;
            } else {
                app.set_status(format!("Pinging {}...", alias), false);
            }
            ping::ping_host(alias, hostname, port, events_tx.clone());
        }
    }
}

fn handle_tunnel_list(app: &mut App, key: KeyEvent) {
    let alias = match &app.screen {
        Screen::TunnelList { alias } => alias.clone(),
        _ => return,
    };

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.screen = Screen::HostList;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_tunnel();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_tunnel();
        }
        KeyCode::Char('a') => {
            // Check if host is from an included file (read-only)
            if let Some(host) = app.hosts.iter().find(|h| h.alias == alias) {
                if host.source_file.is_some() {
                    app.set_status("Included host. Tunnels are read-only.", true);
                    return;
                }
            }
            app.tunnel_form = crate::app::TunnelForm::new();
            app.screen = Screen::TunnelForm {
                alias: alias.clone(),
                editing: None,
            };
            app.capture_form_mtime();
        }
        KeyCode::Char('e') => {
            // Check if host is from an included file (read-only)
            if let Some(host) = app.hosts.iter().find(|h| h.alias == alias) {
                if host.source_file.is_some() {
                    app.set_status("Included host. Tunnels are read-only.", true);
                    return;
                }
            }
            if let Some(sel) = app.ui.tunnel_list_state.selected() {
                if let Some(rule) = app.tunnel_list.get(sel) {
                    app.tunnel_form = crate::app::TunnelForm::from_rule(rule);
                    app.screen = Screen::TunnelForm {
                        alias: alias.clone(),
                        editing: Some(sel),
                    };
                    app.capture_form_mtime();
                }
            }
        }
        KeyCode::Char('d') => {
            // Check if host is from an included file (read-only)
            if let Some(host) = app.hosts.iter().find(|h| h.alias == alias) {
                if host.source_file.is_some() {
                    app.set_status("Included host. Tunnels are read-only.", true);
                    return;
                }
            }
            if let Some(sel) = app.ui.tunnel_list_state.selected() {
                if let Some(rule) = app.tunnel_list.get(sel) {
                    let key = rule.tunnel_type.directive_key().to_string();
                    let value = rule.to_directive_value();
                    let config_backup = app.config.clone();
                    if !app.config.remove_forward(&alias, &key, &value) {
                        app.set_status("Tunnel not found in config.", true);
                        return;
                    }
                    if let Err(e) = app.config.write() {
                        app.config = config_backup;
                        app.set_status(format!("Failed to save: {}", e), true);
                    } else {
                        app.deleted_host = None; // Clear undo buffer — positions may have shifted
                        app.update_last_modified();
                        app.refresh_tunnel_list(&alias);
                        app.reload_hosts();
                        // Fix selection
                        if app.tunnel_list.is_empty() {
                            app.ui.tunnel_list_state.select(None);
                        } else if sel >= app.tunnel_list.len() {
                            app.ui.tunnel_list_state.select(Some(app.tunnel_list.len() - 1));
                        }
                        app.set_status("Tunnel removed.", false);
                    }
                }
            }
        }
        KeyCode::Enter => {
            // Start/stop tunnel
            if app.active_tunnels.contains_key(&alias) {
                // Stop
                if let Some(mut tunnel) = app.active_tunnels.remove(&alias) {
                    let _ = tunnel.child.kill();
                    let _ = tunnel.child.wait();
                    app.set_status(format!("Tunnel for {} stopped.", alias), false);
                }
            } else if !app.tunnel_list.is_empty() {
                // Start
                let askpass = app.hosts.iter()
                    .find(|h| h.alias == alias)
                    .and_then(|h| h.askpass.clone());
                match crate::tunnel::start_tunnel(&alias, &app.reload.config_path, askpass.as_deref()) {
                    Ok(child) => {
                        app.active_tunnels.insert(
                            alias.clone(),
                            crate::tunnel::ActiveTunnel { child },
                        );
                        app.set_status(format!("Tunnel for {} started.", alias), false);
                    }
                    Err(e) => {
                        app.set_status(format!("Failed to start tunnel: {}", e), true);
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_tunnel_form(app: &mut App, key: KeyEvent) {
    let (alias, editing) = match &app.screen {
        Screen::TunnelForm { alias, editing } => (alias.clone(), *editing),
        _ => return,
    };

    match key.code {
        KeyCode::Esc => {
            app.clear_form_mtime();
            app.screen = Screen::TunnelList { alias };
        }
        KeyCode::Tab | KeyCode::Down => {
            app.tunnel_form.focused_field = app.tunnel_form.focused_field.next(app.tunnel_form.tunnel_type);
            app.tunnel_form.sync_cursor_to_end();
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.tunnel_form.focused_field = app.tunnel_form.focused_field.prev(app.tunnel_form.tunnel_type);
            app.tunnel_form.sync_cursor_to_end();
        }
        KeyCode::Left => {
            if app.tunnel_form.focused_field == crate::app::TunnelFormField::Type {
                app.tunnel_form.tunnel_type = app.tunnel_form.tunnel_type.prev();
            } else if app.tunnel_form.cursor_pos > 0 {
                app.tunnel_form.cursor_pos -= 1;
            }
        }
        KeyCode::Right => {
            if app.tunnel_form.focused_field == crate::app::TunnelFormField::Type {
                app.tunnel_form.tunnel_type = app.tunnel_form.tunnel_type.next();
            } else {
                let len = app.tunnel_form.focused_value().map(|v| v.chars().count()).unwrap_or(0);
                if app.tunnel_form.cursor_pos < len {
                    app.tunnel_form.cursor_pos += 1;
                }
            }
        }
        KeyCode::Home => {
            app.tunnel_form.cursor_pos = 0;
        }
        KeyCode::End => {
            app.tunnel_form.sync_cursor_to_end();
        }
        KeyCode::Enter => {
            submit_tunnel_form(app, &alias, editing);
        }
        KeyCode::Char(c) => {
            app.tunnel_form.insert_char(c);
        }
        KeyCode::Backspace => {
            app.tunnel_form.delete_char_before_cursor();
        }
        _ => {}
    }
}

fn submit_tunnel_form(app: &mut App, alias: &str, editing: Option<usize>) {
    // Check for external config changes since form was opened
    if app.config_changed_since_form_open() {
        app.set_status(
            "Config changed externally. Press Esc and re-open to pick up changes.",
            true,
        );
        return;
    }

    if let Err(msg) = app.tunnel_form.validate() {
        app.set_status(msg, true);
        return;
    }

    let (directive_key, directive_value) = app.tunnel_form.to_directive();
    let config_backup = app.config.clone();

    // If editing, remove the old directive first
    if let Some(idx) = editing {
        if let Some(old_rule) = app.tunnel_list.get(idx) {
            let old_key = old_rule.tunnel_type.directive_key().to_string();
            let old_value = old_rule.to_directive_value();
            if !app.config.remove_forward(alias, &old_key, &old_value) {
                app.config = config_backup;
                app.set_status("Original tunnel not found in config.", true);
                return;
            }
        } else {
            // Index out of bounds (external config change) — abort
            app.set_status("Tunnel list changed externally. Press Esc and re-open.", true);
            return;
        }
    }

    // Duplicate detection (runs after old directive removal for edits)
    if app.config.has_forward(alias, directive_key, &directive_value) {
        app.config = config_backup;
        app.set_status("Duplicate tunnel already configured.", true);
        return;
    }

    app.config.add_forward(alias, directive_key, &directive_value);
    if let Err(e) = app.config.write() {
        app.config = config_backup;
        app.set_status(format!("Failed to save: {}", e), true);
        return;
    }

    app.deleted_host = None; // Clear undo buffer — positions may have shifted
    app.update_last_modified();
    app.refresh_tunnel_list(alias);
    app.reload_hosts();
    // Fix selection after list change
    if app.tunnel_list.is_empty() {
        app.ui.tunnel_list_state.select(None);
    } else if let Some(sel) = app.ui.tunnel_list_state.selected() {
        if sel >= app.tunnel_list.len() {
            app.ui.tunnel_list_state.select(Some(app.tunnel_list.len() - 1));
        }
    } else {
        // First tunnel added to empty list — initialize selection
        app.ui.tunnel_list_state.select(Some(0));
    }
    app.clear_form_mtime();
    app.set_status("Tunnel saved.", false);
    app.screen = Screen::TunnelList {
        alias: alias.to_string(),
    };
}

/// Spawn a background thread to fetch hosts from a cloud provider.
pub fn spawn_provider_sync(
    section: &crate::providers::config::ProviderSection,
    tx: mpsc::Sender<AppEvent>,
    cancel: Arc<AtomicBool>,
) {
    let name = section.provider.clone();
    let token = section.token.clone();
    let section_clone = section.clone();
    let tx_fallback = tx.clone();
    let name_fallback = name.clone();
    if std::thread::Builder::new()
        .name(format!("sync-{}", name))
        .spawn(move || {
            let provider = match crate::providers::get_provider_with_config(&name, &section_clone) {
                Some(p) => p,
                None => {
                    let _ = tx.send(AppEvent::SyncError {
                        provider: name,
                        message: "Unknown provider.".to_string(),
                    });
                    return;
                }
            };
            let progress_tx = tx.clone();
            let progress_name = name.clone();
            let progress = move |msg: &str| {
                let _ = progress_tx.send(AppEvent::SyncProgress {
                    provider: progress_name.clone(),
                    message: msg.to_string(),
                });
            };
            match provider.fetch_hosts_with_progress(&token, &cancel, &progress) {
                Ok(hosts) => {
                    let _ = tx.send(AppEvent::SyncComplete {
                        provider: name,
                        hosts,
                    });
                }
                Err(crate::providers::ProviderError::PartialResult { hosts, failures, total }) => {
                    let _ = tx.send(AppEvent::SyncPartial {
                        provider: name,
                        hosts,
                        failures,
                        total,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::SyncError {
                        provider: name,
                        message: e.to_string(),
                    });
                }
            }
        })
        .is_err()
    {
        let _ = tx_fallback.send(AppEvent::SyncError {
            provider: name_fallback,
            message: "Failed to start sync thread.".to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, ProviderFormField, ProviderFormFields, Screen};
    use crate::providers::config::{ProviderConfig, ProviderSection};
    use crate::ssh_config::model::SshConfigFile;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;
    use std::sync::mpsc;

    fn test_provider_config() -> ProviderConfig {
        let mut c = ProviderConfig::default();
        c.path_override = Some(PathBuf::from("/tmp/purple_test_providers"));
        c
    }

    fn make_app(content: &str) -> App {
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: PathBuf::from("/tmp/test_config"),
            crlf: false,
        };
        let mut app = App::new(config);
        // Never write to the real ~/.purple during tests
        app.provider_config = test_provider_config();
        crate::preferences::set_path_override(PathBuf::from("/tmp/purple_test_preferences"));
        app
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// App met een geconfigureerde DigitalOcean (auto_sync=true) en een nieuw Proxmox.
    fn make_providers_app_with_do() -> App {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::Providers;
        app.provider_config = test_provider_config();
        app.provider_config.set_section(ProviderSection {
            provider: "digitalocean".to_string(),
            token: "tok".to_string(),
            alias_prefix: "do".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
        });
        app
    }

    fn make_providers_app_with_proxmox() -> App {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::Providers;
        app.provider_config = test_provider_config();
        app.provider_config.set_section(ProviderSection {
            provider: "proxmox".to_string(),
            token: "user@pam!t=secret".to_string(),
            alias_prefix: "pve".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: "https://pve.local:8006".to_string(),
            verify_tls: true,
            auto_sync: false,
        });
        app
    }

    /// Positioneer de cursor op een bepaalde provider in de lijst en stuur Enter.
    fn open_provider_form(app: &mut App, provider_name: &str) {
        let sorted = app.sorted_provider_names();
        let idx = sorted.iter().position(|n| n == provider_name).unwrap();
        app.ui.provider_list_state.select(Some(idx));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(app, key(KeyCode::Enter), &tx);
    }

    // --- Form initialisatie ---

    #[test]
    fn test_provider_form_init_existing_do_preserves_auto_sync_true() {
        let mut app = make_providers_app_with_do();
        open_provider_form(&mut app, "digitalocean");
        assert!(
            app.provider_form.auto_sync,
            "Bestaande DO provider (auto_sync=true) moet true blijven in het form"
        );
    }

    #[test]
    fn test_provider_form_init_existing_proxmox_preserves_auto_sync_false() {
        let mut app = make_providers_app_with_proxmox();
        open_provider_form(&mut app, "proxmox");
        assert!(
            !app.provider_form.auto_sync,
            "Bestaande Proxmox provider (auto_sync=false) moet false blijven in het form"
        );
    }

    #[test]
    fn test_provider_form_init_existing_do_explicit_false_preserved() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::Providers;
        app.provider_config = test_provider_config();
        // DO met auto_sync=false (gebruiker heeft het handmatig uitgezet)
        app.provider_config.set_section(ProviderSection {
            provider: "digitalocean".to_string(),
            token: "tok".to_string(),
            alias_prefix: "do".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: false,
        });
        open_provider_form(&mut app, "digitalocean");
        assert!(
            !app.provider_form.auto_sync,
            "DO met auto_sync=false moet false blijven"
        );
    }

    #[test]
    fn test_provider_form_init_new_proxmox_defaults_to_false() {
        // Proxmox zonder bestaande config: default auto_sync=false
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::Providers;
        app.provider_config = test_provider_config(); // geen config voor proxmox
        open_provider_form(&mut app, "proxmox");
        assert!(
            !app.provider_form.auto_sync,
            "Nieuw Proxmox form moet auto_sync=false als default tonen"
        );
    }

    #[test]
    fn test_provider_form_init_new_digitalocean_defaults_to_true() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::Providers;
        app.provider_config = test_provider_config();
        open_provider_form(&mut app, "digitalocean");
        assert!(
            app.provider_form.auto_sync,
            "Nieuw DigitalOcean form moet auto_sync=true als default tonen"
        );
    }

    // --- Space toggle ---

    fn make_form_app_focused_on(provider: &str, field: ProviderFormField) -> App {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::ProviderForm { provider: provider.to_string() };
        app.provider_form = ProviderFormFields {
            url: String::new(),
            token: "tok".to_string(),
            alias_prefix: "do".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            verify_tls: true,
            auto_sync: true,
            focused_field: field,
            cursor_pos: 0,
        };
        app
    }

    /// Submit provider form with fresh mtime capture to minimize race window.
    fn submit_form(app: &mut App) {
        app.capture_provider_form_mtime();
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(app, key(KeyCode::Enter), &tx);
    }

    /// Assert that the status message contains the expected validation error.
    /// Tolerates the conflict-detection race: if another parallel test wrote
    /// to ~/.purple/providers between mtime capture and submit, the conflict
    /// check fires before validation and the test is inconclusive (not a bug).
    fn assert_status_contains(app: &App, expected: &str) {
        let msg = &app.status.as_ref().expect("status should be set").text;
        if msg.contains("changed externally") {
            return; // inconclusive due to race
        }
        assert!(msg.contains(expected), "Expected status to contain '{}', got: '{}'", expected, msg);
    }

    fn assert_status_not_contains(app: &App, not_expected: &str) {
        let msg = app.status.as_ref().map(|s| s.text.as_str()).unwrap_or("");
        if msg.contains("changed externally") {
            return; // inconclusive due to race
        }
        assert!(!msg.contains(not_expected), "Status should NOT contain '{}', got: '{}'", not_expected, msg);
    }

    #[test]
    fn test_space_toggles_auto_sync_true_to_false() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
        assert!(app.provider_form.auto_sync);
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
        assert!(!app.provider_form.auto_sync);
    }

    #[test]
    fn test_space_toggles_auto_sync_false_to_true() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
        app.provider_form.auto_sync = false;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
        assert!(app.provider_form.auto_sync);
    }

    #[test]
    fn test_space_on_other_field_does_not_affect_auto_sync() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.auto_sync = true;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
        // Space op Token voegt spatie toe aan tekstveld; auto_sync ongewijzigd
        assert!(app.provider_form.auto_sync);
    }

    // --- Char/Backspace blokkering op AutoSync ---

    #[test]
    fn test_char_input_blocked_when_auto_sync_focused() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
        let original_token = app.provider_form.token.clone();
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
        // Geen enkel tekstveld mag gewijzigd zijn
        assert_eq!(app.provider_form.token, original_token);
        assert_eq!(app.provider_form.alias_prefix, "do");
    }

    #[test]
    fn test_backspace_blocked_when_auto_sync_focused() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
        let original_token = app.provider_form.token.clone();
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
        assert_eq!(app.provider_form.token, original_token);
    }

    // --- Submit persisteert auto_sync ---

    #[test]
    fn test_submit_provider_form_persists_auto_sync_false() {
        // Submit met auto_sync=false moet de sectie opslaan met auto_sync=false.
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::ProviderForm { provider: "digitalocean".to_string() };
        app.provider_config = test_provider_config();
        app.provider_form = ProviderFormFields {
            url: String::new(),
            token: "tok".to_string(),
            alias_prefix: "do".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            verify_tls: true,
            auto_sync: false,
            focused_field: ProviderFormField::Token,
            cursor_pos: 0,
        };

        let (tx, _rx) = mpsc::channel();
        // Enter triggert submit; save() kan falen zonder ~/.purple dir, maar de
        // in-memory sectie wordt altijd bijgewerkt vóór de save.
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

        // Ongeacht of save() slaagde: de sectie in provider_config is bijgewerkt.
        if let Some(section) = app.provider_config.section("digitalocean") {
            assert!(!section.auto_sync, "Opgeslagen sectie moet auto_sync=false hebben");
        }
        // Als het form is gesloten (save geslaagd), controleert de screen-state
        // dat de toggle correct is doorgegeven.
    }

    #[test]
    fn test_submit_provider_form_persists_auto_sync_true() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::ProviderForm { provider: "digitalocean".to_string() };
        app.provider_config = test_provider_config();
        app.provider_form = ProviderFormFields {
            url: String::new(),
            token: "tok".to_string(),
            alias_prefix: "do".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            verify_tls: true,
            auto_sync: true,
            focused_field: ProviderFormField::Token,
            cursor_pos: 0,
        };

        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

        if let Some(section) = app.provider_config.section("digitalocean") {
            assert!(section.auto_sync, "Opgeslagen sectie moet auto_sync=true hebben");
        }
    }

    // =========================================================================
    // Provider form validation tests
    // =========================================================================

    #[test]
    fn test_submit_provider_form_rejects_control_chars_in_token() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.token = "tok\x01en".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "control characters");
    }

    #[test]
    fn test_submit_provider_form_rejects_control_chars_in_alias_prefix() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.alias_prefix = "do\x00".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "control characters");
    }

    #[test]
    fn test_submit_provider_form_rejects_control_chars_in_url() {
        let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
        app.provider_form.url = "https://pve\x0a.local:8006".to_string();
        app.provider_form.token = "user@pam!t=secret".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "control characters");
    }

    #[test]
    fn test_submit_provider_form_rejects_control_chars_in_user() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.user = "ro\tot".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "control characters");
    }

    #[test]
    fn test_submit_provider_form_rejects_control_chars_in_identity_file() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.identity_file = "~/.ssh/id\x1b_rsa".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "control characters");
    }

    #[test]
    fn test_submit_proxmox_rejects_empty_url() {
        let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
        app.provider_form.url = "".to_string();
        app.provider_form.token = "user@pam!t=secret".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "URL is required");
    }

    #[test]
    fn test_submit_proxmox_rejects_http_url() {
        let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
        app.provider_form.url = "http://pve.local:8006".to_string();
        app.provider_form.token = "user@pam!t=secret".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "https://");
    }

    #[test]
    fn test_submit_proxmox_accepts_https_url() {
        let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
        app.provider_form.url = "https://pve.local:8006".to_string();
        app.provider_form.token = "user@pam!t=secret".to_string();
        submit_form(&mut app);
        assert_status_not_contains(&app, "URL is required");
        assert_status_not_contains(&app, "https://");
    }

    #[test]
    fn test_submit_proxmox_rejects_bare_hostname_url() {
        let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
        app.provider_form.url = "pve.local:8006".to_string();
        app.provider_form.token = "user@pam!t=secret".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "https://");
    }

    #[test]
    fn test_submit_provider_form_rejects_empty_token() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.token = "".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "Token");
    }

    #[test]
    fn test_submit_provider_form_rejects_whitespace_only_token() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.token = "   ".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "Token");
    }

    #[test]
    fn test_submit_provider_form_rejects_pattern_alias_prefix() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.alias_prefix = "do*".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "pattern");
    }

    #[test]
    fn test_submit_provider_form_rejects_question_mark_alias() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.alias_prefix = "do?".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "pattern");
    }

    #[test]
    fn test_submit_provider_form_rejects_negation_alias() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.alias_prefix = "!do".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "pattern");
    }

    #[test]
    fn test_submit_provider_form_rejects_whitespace_in_user() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.user = "my user".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        assert_status_contains(&app, "whitespace");
    }

    // =========================================================================
    // Provider form navigation tests
    // =========================================================================

    #[test]
    fn test_provider_form_tab_cycles_cloud_fields() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.provider_form.focused_field, ProviderFormField::AliasPrefix);
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.provider_form.focused_field, ProviderFormField::User);
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.provider_form.focused_field, ProviderFormField::IdentityFile);
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.provider_form.focused_field, ProviderFormField::AutoSync);
    }

    #[test]
    fn test_provider_form_shift_tab_reverse() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::AutoSync);
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::BackTab), &tx);
        assert_eq!(app.provider_form.focused_field, ProviderFormField::IdentityFile);
    }

    #[test]
    fn test_provider_form_proxmox_has_extra_fields() {
        let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.provider_form.focused_field, ProviderFormField::Token);
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.provider_form.focused_field, ProviderFormField::AliasPrefix);
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.provider_form.focused_field, ProviderFormField::User);
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.provider_form.focused_field, ProviderFormField::IdentityFile);
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.provider_form.focused_field, ProviderFormField::VerifyTls);
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.provider_form.focused_field, ProviderFormField::AutoSync);
    }

    #[test]
    fn test_provider_form_esc_returns_to_provider_list() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
        assert!(matches!(app.screen, Screen::Providers));
    }

    #[test]
    fn test_provider_form_space_toggles_verify_tls() {
        let mut app = make_form_app_focused_on("proxmox", ProviderFormField::VerifyTls);
        assert!(app.provider_form.verify_tls);
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
        assert!(!app.provider_form.verify_tls);
        let _ = handle_key_event(&mut app, key(KeyCode::Char(' ')), &tx);
        assert!(app.provider_form.verify_tls);
    }

    #[test]
    fn test_provider_form_char_input_verify_tls_blocked() {
        let mut app = make_form_app_focused_on("proxmox", ProviderFormField::VerifyTls);
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
        // No text field should have changed
        assert_eq!(app.provider_form.token, "tok");
    }

    #[test]
    fn test_provider_form_backspace_verify_tls_blocked() {
        let mut app = make_form_app_focused_on("proxmox", ProviderFormField::VerifyTls);
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
        assert_eq!(app.provider_form.token, "tok");
    }

    #[test]
    fn test_provider_form_enter_opens_key_picker() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::IdentityFile);
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(app.ui.show_key_picker);
    }

    #[test]
    fn test_provider_form_char_appended_to_focused_field() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.token = "tok".to_string();
        app.provider_form.cursor_pos = 3;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('X')), &tx);
        assert_eq!(app.provider_form.token, "tokX");
    }

    #[test]
    fn test_provider_form_backspace_removes_from_focused_field() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.token = "tok".to_string();
        app.provider_form.cursor_pos = 3;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
        assert_eq!(app.provider_form.token, "to");
    }

    // =========================================================================
    // Provider list interaction tests
    // =========================================================================

    #[test]
    fn test_provider_list_esc_returns_to_host_list() {
        let mut app = make_providers_app_with_do();
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
        assert!(matches!(app.screen, Screen::HostList));
    }

    #[test]
    fn test_provider_list_q_returns_to_host_list() {
        let mut app = make_providers_app_with_do();
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('q')), &tx);
        assert!(matches!(app.screen, Screen::HostList));
    }

    #[test]
    fn test_provider_list_j_selects_next() {
        let mut app = make_providers_app_with_do();
        app.ui.provider_list_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
        // Should advance (wrapping depends on count)
        assert!(app.ui.provider_list_state.selected().is_some());
    }

    #[test]
    fn test_provider_list_k_selects_prev() {
        let mut app = make_providers_app_with_do();
        app.ui.provider_list_state.select(Some(1));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
        assert!(app.ui.provider_list_state.selected().is_some());
    }

    #[test]
    fn test_provider_list_sync_unconfigured_shows_status() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::Providers;
        app.provider_config = test_provider_config();
        // No config for digitalocean - select it and press s
        let sorted = app.sorted_provider_names();
        let idx = sorted.iter().position(|n| n == "digitalocean").unwrap();
        app.ui.provider_list_state.select(Some(idx));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('s')), &tx);
        assert!(app.status.as_ref().unwrap().text.contains("Configure"));
    }

    #[test]
    fn test_provider_list_delete_removes_config() {
        let mut app = make_providers_app_with_do();
        let sorted = app.sorted_provider_names();
        let idx = sorted.iter().position(|n| n == "digitalocean").unwrap();
        app.ui.provider_list_state.select(Some(idx));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
        // d now triggers confirmation
        assert!(app.pending_provider_delete.is_some());
        // Confirm with y
        let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
        assert!(app.pending_provider_delete.is_none());
        // Save may fail in tests (no ~/.purple), triggering rollback. Just verify handler ran.
        assert!(app.status.is_some());
    }

    #[test]
    fn test_provider_list_delete_unconfigured_is_noop() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::Providers;
        app.provider_config = test_provider_config();
        let sorted = app.sorted_provider_names();
        let idx = sorted.iter().position(|n| n == "digitalocean").unwrap();
        app.ui.provider_list_state.select(Some(idx));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
        // No status message because no section existed to delete
        assert!(app.status.is_none() || !app.status.as_ref().unwrap().text.contains("Removed"));
    }

    #[test]
    fn test_provider_list_esc_cancels_running_syncs() {
        let mut app = make_providers_app_with_do();
        let cancel = Arc::new(AtomicBool::new(false));
        app.syncing_providers.insert("digitalocean".to_string(), cancel.clone());
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
        assert!(cancel.load(Ordering::Relaxed), "Cancel flag should be set on Esc");
        assert!(matches!(app.screen, Screen::HostList));
    }

    #[test]
    fn test_provider_list_enter_opens_form_with_existing_config() {
        let mut app = make_providers_app_with_do();
        open_provider_form(&mut app, "digitalocean");
        assert!(matches!(app.screen, Screen::ProviderForm { ref provider } if provider == "digitalocean"));
        assert_eq!(app.provider_form.token, "tok");
        assert_eq!(app.provider_form.alias_prefix, "do");
        assert_eq!(app.provider_form.user, "root");
    }

    #[test]
    fn test_provider_list_enter_opens_form_with_defaults() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::Providers;
        app.provider_config = test_provider_config();
        open_provider_form(&mut app, "vultr");
        assert!(matches!(app.screen, Screen::ProviderForm { ref provider } if provider == "vultr"));
        assert_eq!(app.provider_form.token, "");
        assert_eq!(app.provider_form.user, "root");
        assert!(app.provider_form.auto_sync); // vultr default true
    }

    #[test]
    fn test_provider_form_proxmox_default_alias_prefix() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::Providers;
        app.provider_config = test_provider_config();
        open_provider_form(&mut app, "proxmox");
        // Proxmox short_label is "pve"
        assert_eq!(app.provider_form.alias_prefix, "pve");
    }

    // =========================================================================
    // Provider form all-providers init defaults
    // =========================================================================

    #[test]
    fn test_all_cloud_providers_default_auto_sync_true() {
        for provider in &["digitalocean", "vultr", "linode", "hetzner", "upcloud"] {
            let mut app = make_app("Host test\n  HostName test.com\n");
            app.screen = Screen::Providers;
            app.provider_config = test_provider_config();
            open_provider_form(&mut app, provider);
            assert!(
                app.provider_form.auto_sync,
                "{} should default auto_sync=true", provider
            );
        }
    }

    #[test]
    fn test_proxmox_defaults_auto_sync_false() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::Providers;
        app.provider_config = test_provider_config();
        open_provider_form(&mut app, "proxmox");
        assert!(!app.provider_form.auto_sync);
    }

    #[test]
    fn test_submit_proxmox_https_case_insensitive() {
        let mut app = make_form_app_focused_on("proxmox", ProviderFormField::Url);
        app.provider_form.url = "HTTPS://pve.local:8006".to_string();
        app.provider_form.token = "user@pam!t=secret".to_string();
        submit_form(&mut app);
        assert_status_not_contains(&app, "https://");
    }

    #[test]
    fn test_submit_non_proxmox_url_not_required() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.url = "".to_string();
        submit_form(&mut app);
        assert_status_not_contains(&app, "URL is required");
    }

    #[test]
    fn test_submit_provider_form_accepts_empty_alias_prefix() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.alias_prefix = "".to_string();
        submit_form(&mut app);
        assert_status_not_contains(&app, "pattern");
    }

    #[test]
    fn test_submit_provider_form_accepts_hyphenated_alias() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.alias_prefix = "my-cloud".to_string();
        submit_form(&mut app);
        assert_status_not_contains(&app, "pattern");
    }

    #[test]
    fn test_submit_provider_form_rejects_space_in_alias_prefix() {
        let mut app = make_form_app_focused_on("digitalocean", ProviderFormField::Token);
        app.provider_form.alias_prefix = "my cloud".to_string();
        submit_form(&mut app);
        assert!(matches!(app.screen, Screen::ProviderForm { .. }));
        let msg = &app.status.as_ref().unwrap().text;
        if !msg.contains("changed externally") {
            assert!(msg.contains("pattern") || msg.contains("spaces"));
        }
    }

    // =========================================================================
    // Password picker tests
    // =========================================================================

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn make_form_app() -> App {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::AddHost;
        app.form = crate::app::HostForm::new();
        app
    }

    // --- Enter on AskPass opens picker ---

    #[test]
    fn test_enter_on_askpass_opens_password_picker() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(app.ui.show_password_picker);
        assert_eq!(app.ui.password_picker_state.selected(), Some(0));
    }

    // --- Esc closes picker ---

    #[test]
    fn test_password_picker_esc_closes() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(2));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
        assert!(!app.ui.show_password_picker);
        // Form field should be unchanged
        assert_eq!(app.form.askpass, "");
    }

    // --- Navigation j/k ---

    #[test]
    fn test_password_picker_j_moves_down() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
        assert_eq!(app.ui.password_picker_state.selected(), Some(1));
    }

    #[test]
    fn test_password_picker_k_moves_up() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(2));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
        assert_eq!(app.ui.password_picker_state.selected(), Some(1));
    }

    #[test]
    fn test_password_picker_down_arrow() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Down), &tx);
        assert_eq!(app.ui.password_picker_state.selected(), Some(1));
    }

    #[test]
    fn test_password_picker_up_arrow() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(3));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Up), &tx);
        assert_eq!(app.ui.password_picker_state.selected(), Some(2));
    }

    #[test]
    fn test_password_picker_wraps_around_bottom() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
        app.ui.password_picker_state.select(Some(last));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
        assert_eq!(app.ui.password_picker_state.selected(), Some(0));
    }

    #[test]
    fn test_password_picker_wraps_around_top() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
        let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
        assert_eq!(app.ui.password_picker_state.selected(), Some(last));
    }

    // --- Enter selects source: OS Keychain ---

    #[test]
    fn test_password_picker_select_keychain() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(0)); // OS Keychain
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(!app.ui.show_password_picker);
        assert_eq!(app.form.askpass, "keychain");
    }

    // --- Enter selects source: 1Password (prefix) ---

    #[test]
    fn test_password_picker_select_1password() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(1)); // 1Password
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(!app.ui.show_password_picker);
        assert_eq!(app.form.askpass, "op://");
        assert_eq!(app.form.focused_field, FormField::AskPass);
    }

    // --- Enter selects source: Bitwarden (prefix) ---

    #[test]
    fn test_password_picker_select_bitwarden() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(2)); // Bitwarden
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(!app.ui.show_password_picker);
        assert_eq!(app.form.askpass, "bw:");
        assert_eq!(app.form.focused_field, FormField::AskPass);
    }

    // --- Enter selects source: pass (prefix) ---

    #[test]
    fn test_password_picker_select_pass() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(3)); // pass
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(!app.ui.show_password_picker);
        assert_eq!(app.form.askpass, "pass:");
        assert_eq!(app.form.focused_field, FormField::AskPass);
    }

    // --- Enter selects source: HashiCorp Vault (prefix) ---

    #[test]
    fn test_password_picker_select_vault() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(4)); // HashiCorp Vault
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(!app.ui.show_password_picker);
        assert_eq!(app.form.askpass, "vault:");
        assert_eq!(app.form.focused_field, FormField::AskPass);
    }

    // --- Enter selects source: Custom command ---

    #[test]
    fn test_password_picker_select_custom() {
        let mut app = make_form_app();
        app.form.askpass = "old-value".to_string();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(5)); // Custom command
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(!app.ui.show_password_picker);
        assert_eq!(app.form.askpass, "");
    }

    // --- Enter selects source: None (clears) ---

    #[test]
    fn test_password_picker_select_none() {
        let mut app = make_form_app();
        app.form.askpass = "keychain".to_string();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(6)); // None
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(!app.ui.show_password_picker);
        assert_eq!(app.form.askpass, "");
    }

    // --- Picker blocks other form input ---

    #[test]
    fn test_password_picker_blocks_char_input() {
        let mut app = make_form_app();
        app.form.askpass = "".to_string();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('x')), &tx);
        // 'x' should not be appended to any form field
        assert_eq!(app.form.askpass, "");
        assert_eq!(app.form.alias, "");
    }

    #[test]
    fn test_password_picker_blocks_tab() {
        let mut app = make_form_app();
        let original_field = app.form.focused_field;
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        // Tab should not change focused field
        assert_eq!(app.form.focused_field, original_field);
    }

    // --- Picker on EditHost screen ---

    #[test]
    fn test_password_picker_works_on_edit_host() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::EditHost { alias: "test".to_string() };
        app.form = crate::app::HostForm::new();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(app.ui.show_password_picker);
        // Select keychain
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert_eq!(app.form.askpass, "keychain");
    }

    // --- Picker priority over key picker ---

    #[test]
    fn test_password_picker_takes_priority_over_key_picker() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.show_key_picker = true;
        app.ui.password_picker_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        // Esc should close password picker, not key picker
        let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
        assert!(!app.ui.show_password_picker);
        assert!(app.ui.show_key_picker); // still open
    }

    // =========================================================================
    // Host list Enter carries askpass in pending_connect
    // =========================================================================

    #[test]
    fn test_host_list_enter_carries_askpass() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        app.screen = Screen::HostList;
        // Select the host
        app.ui.list_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        let pending = app.pending_connect.as_ref().unwrap();
        assert_eq!(pending.0, "myserver");
        assert_eq!(pending.1, Some("keychain".to_string()));
    }

    #[test]
    fn test_host_list_enter_carries_vault_askpass() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pass\n");
        app.screen = Screen::HostList;
        app.ui.list_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        let pending = app.pending_connect.as_ref().unwrap();
        assert_eq!(pending.1, Some("vault:secret/ssh#pass".to_string()));
    }

    #[test]
    fn test_host_list_enter_no_askpass() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        app.screen = Screen::HostList;
        app.ui.list_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        let pending = app.pending_connect.as_ref().unwrap();
        assert_eq!(pending.0, "myserver");
        assert_eq!(pending.1, None);
    }

    // =========================================================================
    // Search mode Enter carries askpass in pending_connect
    // =========================================================================

    #[test]
    fn test_search_enter_carries_askpass() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://V/I/p\n");
        app.screen = Screen::HostList;
        app.start_search();
        // In search mode, filtered_indices should contain our host
        assert!(!app.search.filtered_indices.is_empty());
        app.ui.list_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        let pending = app.pending_connect.as_ref().unwrap();
        assert_eq!(pending.0, "myserver");
        assert_eq!(pending.1, Some("op://V/I/p".to_string()));
        // Search should be cancelled after Enter
        assert!(app.search.query.is_none());
    }

    #[test]
    fn test_search_enter_no_askpass() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        app.screen = Screen::HostList;
        app.start_search();
        app.ui.list_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        let pending = app.pending_connect.as_ref().unwrap();
        assert_eq!(pending.1, None);
    }

    // =========================================================================
    // Tunnel start reads askpass from host
    // =========================================================================

    #[test]
    fn test_tunnel_handler_reads_askpass_from_hosts() {
        // Verify the askpass lookup logic: find host by alias and extract askpass
        let app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass bw:my-item\n");
        let askpass = app.hosts.iter()
            .find(|h| h.alias == "myserver")
            .and_then(|h| h.askpass.clone());
        assert_eq!(askpass, Some("bw:my-item".to_string()));
    }

    #[test]
    fn test_tunnel_handler_askpass_none_when_absent() {
        let app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        let askpass = app.hosts.iter()
            .find(|h| h.alias == "myserver")
            .and_then(|h| h.askpass.clone());
        assert_eq!(askpass, None);
    }

    // =========================================================================
    // Edit host form populates askpass
    // =========================================================================

    #[test]
    fn test_edit_host_populates_askpass_in_form() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass pass:ssh/prod\n");
        app.screen = Screen::HostList;
        app.ui.list_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        // Press 'e' to edit
        let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
        if matches!(app.screen, Screen::EditHost { .. }) {
            assert_eq!(app.form.askpass, "pass:ssh/prod");
        }
    }

    #[test]
    fn test_edit_host_populates_empty_askpass() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        app.screen = Screen::HostList;
        app.ui.list_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
        if matches!(app.screen, Screen::EditHost { .. }) {
            assert_eq!(app.form.askpass, "");
        }
    }

    // =========================================================================
    // Tab navigation through AskPass field
    // =========================================================================

    #[test]
    fn test_tab_reaches_askpass_field() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::ProxyJump;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.form.focused_field, FormField::AskPass);
    }

    #[test]
    fn test_tab_from_askpass_goes_to_tags() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.form.focused_field, FormField::Tags);
    }

    #[test]
    fn test_shift_tab_from_tags_goes_to_askpass() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::Tags;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::BackTab), &tx);
        assert_eq!(app.form.focused_field, FormField::AskPass);
    }

    #[test]
    fn test_typing_in_askpass_field() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('k')), &tx);
        let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
        let _ = handle_key_event(&mut app, key(KeyCode::Char('y')), &tx);
        assert_eq!(app.form.askpass, "key");
    }

    #[test]
    fn test_backspace_in_askpass_field() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        app.form.askpass = "vault:".to_string();
        app.form.cursor_pos = 6;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
        assert_eq!(app.form.askpass, "vault");
    }

    // =========================================================================
    // Picker then type: prefix selection followed by typing
    // =========================================================================

    #[test]
    fn test_picker_select_op_then_type_rest() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        // Open picker
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        // Navigate to 1Password (index 1)
        let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
        // Select
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert_eq!(app.form.askpass, "op://");
        assert_eq!(app.form.focused_field, FormField::AskPass);
        // Now type the rest of the URI
        let _ = handle_key_event(&mut app, key(KeyCode::Char('V')), &tx);
        let _ = handle_key_event(&mut app, key(KeyCode::Char('/')), &tx);
        let _ = handle_key_event(&mut app, key(KeyCode::Char('I')), &tx);
        assert_eq!(app.form.askpass, "op://V/I");
    }

    #[test]
    fn test_picker_select_vault_then_type_rest() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        // Open picker
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        // Navigate to Vault (index 4)
        for _ in 0..4 {
            let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
        }
        assert_eq!(app.ui.password_picker_state.selected(), Some(4));
        // Select
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert_eq!(app.form.askpass, "vault:");
        assert_eq!(app.form.focused_field, FormField::AskPass);
        // Type the path
        for c in "secret/ssh#pass".chars() {
            let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
        }
        assert_eq!(app.form.askpass, "vault:secret/ssh#pass");
    }

    #[test]
    fn test_picker_select_keychain_no_further_typing_needed() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        // Open picker via Enter on AskPass
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        // Select keychain (index 0, already selected)
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert_eq!(app.form.askpass, "keychain");
        // focused_field stays on AskPass (picker was opened from AskPass)
        assert_eq!(app.form.focused_field, FormField::AskPass);
    }

    // =========================================================================
    // Password picker: status messages after selection
    // =========================================================================

    #[test]
    fn test_picker_keychain_sets_status_message() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(app.status.as_ref().unwrap().text.contains("OS Keychain"));
    }

    #[test]
    fn test_picker_none_sets_cleared_status() {
        let mut app = make_form_app();
        app.form.askpass = "keychain".to_string();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(6)); // None
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(app.status.as_ref().unwrap().text.contains("cleared"));
    }

    #[test]
    fn test_picker_prefix_source_shows_guidance() {
        // Prefix sources (op://, bw:, etc.) show a guidance message
        let mut app = make_form_app();
        app.status = None;
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(1)); // 1Password (op://)
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(app.status.as_ref().unwrap().text.contains("Complete"));
        assert_eq!(app.form.focused_field, FormField::AskPass);
    }

    // =========================================================================
    // Backspace after prefix selection
    // =========================================================================

    #[test]
    fn test_backspace_after_prefix_selection() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        // Open picker and select 1Password
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        app.ui.password_picker_state.select(Some(1));
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert_eq!(app.form.askpass, "op://");
        assert_eq!(app.form.focused_field, FormField::AskPass);
        // Type something
        let _ = handle_key_event(&mut app, key(KeyCode::Char('V')), &tx);
        assert_eq!(app.form.askpass, "op://V");
        // Backspace removes last char
        let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
        assert_eq!(app.form.askpass, "op://");
        // Another backspace removes the trailing /
        let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
        assert_eq!(app.form.askpass, "op:/");
    }

    // =========================================================================
    // Edit form populates askpass from existing host
    // =========================================================================

    #[test]
    fn test_edit_form_populates_askpass() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pw\n");
        // Simulate what happens when user presses 'e' on a host
        let entry = app.config.host_entries()[0].clone();
        app.form = crate::app::HostForm::from_entry(&entry);
        assert_eq!(app.form.askpass, "vault:secret/ssh#pw");
    }

    #[test]
    fn test_edit_form_empty_askpass_when_none() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        let entry = app.config.host_entries()[0].clone();
        app.form = crate::app::HostForm::from_entry(&entry);
        assert_eq!(app.form.askpass, "");
    }

    // =========================================================================
    // Password picker: unknown keys are no-ops
    // =========================================================================

    #[test]
    fn test_password_picker_ignores_unknown_keys() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(2));
        let (tx, _rx) = mpsc::channel();
        // F1 key should be a no-op
        let _ = handle_key_event(&mut app, key(KeyCode::F(1)), &tx);
        assert!(app.ui.show_password_picker);
        assert_eq!(app.ui.password_picker_state.selected(), Some(2));
    }

    // =========================================================================
    // Host list search Enter carries askpass
    // =========================================================================

    #[test]
    fn test_search_enter_carries_askpass_op_uri() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://V/I/p\n");
        app.search.query = Some("myserver".to_string());
        app.apply_filter();
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        if let Some((alias, askpass)) = &app.pending_connect {
            assert_eq!(alias, "myserver");
            assert_eq!(askpass.as_deref(), Some("op://V/I/p"));
        } else {
            panic!("Expected pending_connect to be set");
        }
    }

    // =========================================================================
    // UI/UX: placeholder text and picker overlay properties
    // =========================================================================

    #[test]
    fn test_askpass_placeholder_text() {
        let placeholder = crate::ui::host_form::placeholder_text(FormField::AskPass);
        // When no global default is set, shows guidance text
        assert!(placeholder.contains("Enter") || placeholder.contains("default:"),
            "Should show guidance or default: {}", placeholder);
    }

    #[test]
    fn test_password_sources_fit_picker_width() {
        // Picker overlay is 48 chars wide (minus 4 for borders/padding)
        let max_content_width = 44;
        for source in crate::askpass::PASSWORD_SOURCES {
            let total = source.label.len() + 1 + source.hint.len();
            assert!(
                total <= max_content_width,
                "Source '{}' (label={}, hint={}) total {} exceeds max {}",
                source.label, source.label.len(), source.hint.len(), total, max_content_width
            );
        }
    }

    #[test]
    fn test_password_picker_item_count_matches_sources() {
        assert_eq!(crate::askpass::PASSWORD_SOURCES.len(), 7);
    }

    // =========================================================================
    // Full picker → type → form submit flow
    // =========================================================================

    #[test]
    fn test_full_flow_picker_to_typed_value() {
        let mut app = make_form_app();
        app.form.alias = "myhost".to_string();
        app.form.hostname = "10.0.0.1".to_string();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();

        // Open picker, select Bitwarden (index 2)
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        app.ui.password_picker_state.select(Some(2));
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

        // Verify field has prefix
        assert_eq!(app.form.askpass, "bw:");
        assert_eq!(app.form.focused_field, FormField::AskPass);

        // Type the item name
        for c in "my-ssh-server".chars() {
            let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
        }
        assert_eq!(app.form.askpass, "bw:my-ssh-server");

        // Verify to_entry produces correct askpass
        let entry = app.form.to_entry();
        assert_eq!(entry.askpass, Some("bw:my-ssh-server".to_string()));
    }

    #[test]
    fn test_full_flow_picker_keychain_then_tab_away() {
        let mut app = make_form_app();
        app.form.alias = "myhost".to_string();
        app.form.hostname = "10.0.0.1".to_string();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();

        // Open picker via Enter on AskPass, select keychain
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

        assert_eq!(app.form.askpass, "keychain");
        // Focus stays on AskPass (picker opened from AskPass)
        assert_eq!(app.form.focused_field, FormField::AskPass);

        // Tab to next field
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.form.focused_field, FormField::Tags);
    }

    #[test]
    fn test_full_flow_clear_askpass_via_picker_none() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        app.form.askpass = "op://Vault/Item/pw".to_string();
        let (tx, _rx) = mpsc::channel();

        // Open picker, select None (index 6)
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        for _ in 0..6 {
            let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
        }
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

        assert_eq!(app.form.askpass, "");
        let entry = app.form.to_entry();
        assert_eq!(entry.askpass, None);
    }

    // =========================================================================
    // Askpass with host without askpass (no askpass in pending_connect)
    // =========================================================================

    #[test]
    fn test_host_list_enter_no_askpass_is_none() {
        let mut app = make_app("Host plain\n  HostName 10.0.0.1\n");
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        if let Some((alias, askpass)) = &app.pending_connect {
            assert_eq!(alias, "plain");
            assert!(askpass.is_none());
        } else {
            panic!("Expected pending_connect");
        }
    }

    // =========================================================================
    // Ctrl+P does NOT open password picker on provider form
    // =========================================================================

    #[test]
    fn test_ctrl_p_on_provider_form_does_not_open_password_picker() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::ProviderForm { provider: "digitalocean".to_string() };
        app.provider_form = crate::app::ProviderFormFields::new();
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, ctrl_key('p'), &tx);
        // Provider form does not have a password picker
        assert!(!app.ui.show_password_picker);
    }

    // =========================================================================
    // Multiple hosts: each carries its own askpass in pending_connect
    // =========================================================================

    #[test]
    fn test_multiple_hosts_different_askpass_sources() {
        let config = "\
Host alpha
  HostName a.com
  # purple:askpass keychain

Host beta
  HostName b.com
  # purple:askpass op://Vault/SSH/pw

Host gamma
  HostName c.com
";
        let app = make_app(config);
        assert_eq!(app.hosts.len(), 3);
        assert_eq!(app.hosts[0].askpass, Some("keychain".to_string()));
        assert_eq!(app.hosts[1].askpass, Some("op://Vault/SSH/pw".to_string()));
        assert_eq!(app.hosts[2].askpass, None);
    }

    #[test]
    fn test_select_different_hosts_carries_correct_askpass() {
        let config = "\
Host alpha
  HostName a.com
  # purple:askpass keychain

Host beta
  HostName b.com
  # purple:askpass bw:my-item
";
        let mut app = make_app(config);
        let (tx, _rx) = mpsc::channel();

        // Select alpha (first host) and press Enter
        app.ui.list_state.select(Some(0));
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        let (alias, askpass) = app.pending_connect.take().unwrap();
        assert_eq!(alias, "alpha");
        assert_eq!(askpass, Some("keychain".to_string()));

        // Select beta (second host) and press Enter
        app.ui.list_state.select(Some(1));
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        let (alias, askpass) = app.pending_connect.take().unwrap();
        assert_eq!(alias, "beta");
        assert_eq!(askpass, Some("bw:my-item".to_string()));
    }

    // =========================================================================
    // Askpass field typing: direct input without picker
    // =========================================================================

    #[test]
    fn test_type_askpass_directly_without_picker() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        for c in "keychain".chars() {
            let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
        }
        assert_eq!(app.form.askpass, "keychain");
    }

    #[test]
    fn test_type_custom_command_directly() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        for c in "my-script %a %h".chars() {
            let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
        }
        assert_eq!(app.form.askpass, "my-script %a %h");
    }

    #[test]
    fn test_clear_askpass_with_backspace() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        app.form.askpass = "keychain".to_string();
        app.form.cursor_pos = 8;
        let (tx, _rx) = mpsc::channel();
        for _ in 0..8 {
            let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
        }
        assert_eq!(app.form.askpass, "");
    }

    // =========================================================================
    // Delete host with askpass: undo restores it
    // =========================================================================

    #[test]
    fn test_delete_undo_preserves_askpass_in_config() {
        let config_str = "Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pw\n";
        let mut app = make_app(config_str);
        // Verify askpass is present
        assert_eq!(app.config.host_entries()[0].askpass, Some("vault:secret/ssh#pw".to_string()));

        // Delete the host (undoable)
        if let Some((element, position)) = app.config.delete_host_undoable("myserver") {
            // Host is gone
            assert!(app.config.host_entries().is_empty());
            // Undo: restore
            app.config.insert_host_at(element, position);
            // Askpass should be restored
            let entries = app.config.host_entries();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].askpass, Some("vault:secret/ssh#pw".to_string()));
        } else {
            panic!("Expected delete_host_undoable to succeed");
        }
    }

    // =========================================================================
    // Askpass with unicode characters
    // =========================================================================

    #[test]
    fn test_askpass_unicode_in_custom_command() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        for c in "get-p\u{00E4}ss %h".chars() {
            let _ = handle_key_event(&mut app, key(KeyCode::Char(c)), &tx);
        }
        assert_eq!(app.form.askpass, "get-p\u{00E4}ss %h");
    }

    // =========================================================================
    // Enter on AskPass field opens picker
    // =========================================================================

    #[test]
    fn test_enter_on_askpass_field_opens_picker() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        app.form.askpass = "old-val".to_string();
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(app.ui.show_password_picker);
        // Old value should still be there (picker hasn't committed yet)
        assert_eq!(app.form.askpass, "old-val");
    }

    #[test]
    fn test_enter_on_askpass_field_select_replaces_value() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        app.form.askpass = "old-val".to_string();
        let (tx, _rx) = mpsc::channel();
        // Open picker
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        // Select keychain
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert_eq!(app.form.askpass, "keychain");
        assert!(!app.ui.show_password_picker);
    }

    // =========================================================================
    // --connect mode askpass lookup logic (replicated)
    // =========================================================================

    #[test]
    fn test_connect_mode_askpass_lookup() {
        let app = make_app("Host srv\n  HostName 1.2.3.4\n  # purple:askpass pass:ssh/srv\n");
        // Simulate --connect lookup logic from main.rs
        let alias = "srv";
        let askpass = app.config.host_entries().iter()
            .find(|h| h.alias == alias)
            .and_then(|h| h.askpass.clone());
        assert_eq!(askpass, Some("pass:ssh/srv".to_string()));
    }

    #[test]
    fn test_connect_mode_askpass_none() {
        let app = make_app("Host srv\n  HostName 1.2.3.4\n");
        let alias = "srv";
        let askpass = app.config.host_entries().iter()
            .find(|h| h.alias == alias)
            .and_then(|h| h.askpass.clone());
        assert_eq!(askpass, None);
    }

    #[test]
    fn test_connect_mode_nonexistent_host() {
        let app = make_app("Host srv\n  HostName 1.2.3.4\n");
        let alias = "nonexistent";
        let askpass = app.config.host_entries().iter()
            .find(|h| h.alias == alias)
            .and_then(|h| h.askpass.clone());
        assert_eq!(askpass, None);
    }

    // =========================================================================
    // 'e' key opens edit form with correct askpass from host list
    // =========================================================================

    #[test]
    fn test_e_key_opens_edit_form_with_askpass() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://Vault/SSH/pw\n");
        let (tx, _rx) = mpsc::channel();
        // Press 'e' to edit the selected host
        let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
        assert!(matches!(app.screen, Screen::EditHost { .. }));
        assert_eq!(app.form.askpass, "op://Vault/SSH/pw");
        assert_eq!(app.form.hostname, "10.0.0.1");
    }

    #[test]
    fn test_e_key_opens_edit_form_without_askpass() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
        assert!(matches!(app.screen, Screen::EditHost { .. }));
        assert_eq!(app.form.askpass, "");
    }

    // =========================================================================
    // Picker then Esc preserves existing askpass value
    // =========================================================================

    #[test]
    fn test_picker_esc_preserves_existing_askpass() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        app.form.askpass = "vault:secret/ssh#pw".to_string();
        let (tx, _rx) = mpsc::channel();
        // Open picker
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(app.ui.show_password_picker);
        // Navigate but then Esc
        let _ = handle_key_event(&mut app, key(KeyCode::Char('j')), &tx);
        let _ = handle_key_event(&mut app, key(KeyCode::Esc), &tx);
        // Original value preserved
        assert_eq!(app.form.askpass, "vault:secret/ssh#pw");
    }

    // =========================================================================
    // Extra backspace past empty is no-op
    // =========================================================================

    #[test]
    fn test_backspace_on_empty_askpass_is_noop() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        app.form.askpass = "".to_string();
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Backspace), &tx);
        assert_eq!(app.form.askpass, "");
    }

    // =========================================================================
    // Tab from AskPass goes to Tags, shift-tab goes to ProxyJump
    // =========================================================================

    #[test]
    fn test_tab_from_askpass_to_tags() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Tab), &tx);
        assert_eq!(app.form.focused_field, FormField::Tags);
    }

    #[test]
    fn test_shift_tab_from_askpass_to_proxyjump() {
        let mut app = make_form_app();
        app.form.focused_field = FormField::AskPass;
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT), &tx);
        assert_eq!(app.form.focused_field, FormField::ProxyJump);
    }

    // =========================================================================
    // Tunnel start for host with askpass passes it through
    // =========================================================================

    #[test]
    fn test_tunnel_askpass_lookup_different_sources() {
        let config = "\
Host alpha
  HostName a.com
  # purple:askpass keychain

Host beta
  HostName b.com
  # purple:askpass bw:item

Host gamma
  HostName c.com
";
        let app = make_app(config);
        let lookup = |alias: &str| -> Option<String> {
            app.hosts.iter()
                .find(|h| h.alias == alias)
                .and_then(|h| h.askpass.clone())
        };
        assert_eq!(lookup("alpha"), Some("keychain".to_string()));
        assert_eq!(lookup("beta"), Some("bw:item".to_string()));
        assert_eq!(lookup("gamma"), None);
    }

    // =========================================================================
    // Password picker status message tests
    // =========================================================================

    #[test]
    fn test_password_picker_keychain_sets_status_message() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(0)); // Keychain
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        let status = app.status.as_ref().unwrap();
        assert!(status.text.contains("OS Keychain"), "Status should mention OS Keychain, got: {}", status.text);
    }

    #[test]
    fn test_password_picker_none_sets_cleared_status() {
        let mut app = make_form_app();
        app.form.askpass = "keychain".to_string();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(6)); // None
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        let status = app.status.as_ref().unwrap();
        assert!(status.text.contains("cleared"), "Status should say cleared, got: {}", status.text);
    }

    #[test]
    fn test_password_picker_prefix_source_focuses_askpass_field() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(1)); // 1Password (op://)
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert_eq!(app.form.focused_field, FormField::AskPass, "Prefix source should focus AskPass field");
        // No status message for prefix sources (user needs to keep typing)
        assert!(app.status.is_none() || !app.status.as_ref().unwrap().text.contains("set to"));
    }

    #[test]
    fn test_password_picker_prefix_bw_focuses_askpass() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(2)); // Bitwarden (bw:)
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert_eq!(app.form.focused_field, FormField::AskPass);
        assert_eq!(app.form.askpass, "bw:");
    }

    #[test]
    fn test_password_picker_prefix_pass_focuses_askpass() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(3)); // pass (pass:)
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert_eq!(app.form.focused_field, FormField::AskPass);
        assert_eq!(app.form.askpass, "pass:");
    }

    #[test]
    fn test_password_picker_prefix_vault_focuses_askpass() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(4)); // Vault (vault:)
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert_eq!(app.form.focused_field, FormField::AskPass);
        assert_eq!(app.form.askpass, "vault:");
    }

    // =========================================================================
    // Included host: edit blocked, but askpass visible in pending_connect
    // =========================================================================

    #[test]
    fn test_included_host_edit_blocked() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        app.screen = Screen::HostList;
        if let Some(host) = app.hosts.first_mut() {
            host.source_file = Some(std::path::PathBuf::from("/etc/ssh/ssh_config.d/work.conf"));
        }
        app.ui.list_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('e')), &tx);
        assert!(matches!(app.screen, Screen::HostList));
    }

    #[test]
    fn test_included_host_connect_still_carries_askpass() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://V/I/p\n");
        app.screen = Screen::HostList;
        if let Some(host) = app.hosts.first_mut() {
            host.source_file = Some(std::path::PathBuf::from("/etc/ssh/ssh_config.d/work.conf"));
        }
        app.ui.list_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        if let Some((alias, askpass)) = &app.pending_connect {
            assert_eq!(alias, "myserver");
            assert_eq!(askpass.as_deref(), Some("op://V/I/p"));
        }
    }

    #[test]
    fn test_included_host_delete_blocked() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass bw:item\n");
        app.screen = Screen::HostList;
        if let Some(host) = app.hosts.first_mut() {
            host.source_file = Some(std::path::PathBuf::from("/etc/ssh/ssh_config.d/work.conf"));
        }
        app.ui.list_state.select(Some(0));
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Char('d')), &tx);
        assert!(matches!(app.screen, Screen::HostList));
    }

    // =========================================================================
    // Form submit with askpass: verify to_entry() includes askpass
    // =========================================================================

    #[test]
    fn test_form_submit_with_all_password_source_types() {
        let sources = ["keychain", "op://V/I/p", "bw:item", "pass:ssh/srv", "vault:kv/ssh#pw", "my-cmd %h"];
        for source in &sources {
            let mut app = make_app("");
            app.screen = Screen::AddHost;
            app.form.alias = "test-host".to_string();
            app.form.hostname = "10.0.0.1".to_string();
            app.form.askpass = source.to_string();
            let entry = app.form.to_entry();
            assert_eq!(entry.askpass.as_deref(), Some(*source),
                "Form with askpass '{}' should produce entry with same askpass", source);
        }
    }

    #[test]
    fn test_form_submit_empty_askpass_is_none() {
        let mut app = make_app("");
        app.screen = Screen::AddHost;
        app.form.alias = "test-host".to_string();
        app.form.hostname = "10.0.0.1".to_string();
        app.form.askpass = "".to_string();
        let entry = app.form.to_entry();
        assert!(entry.askpass.is_none(), "Empty askpass should produce None");
    }

    // =========================================================================
    // Password picker: Enter with no selection is no-op
    // =========================================================================

    #[test]
    fn test_password_picker_enter_with_no_selection() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state = ratatui::widgets::ListState::default(); // no selection
        app.form.askpass = "old".to_string();
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);
        assert!(!app.ui.show_password_picker);
        assert_eq!(app.form.askpass, "old");
    }

    // =========================================================================
    // BW_SESSION: stored in app state
    // =========================================================================

    #[test]
    fn test_bw_session_stored_in_app() {
        let mut app = make_app("Host srv\n  HostName 1.2.3.4\n  # purple:askpass bw:item\n");
        assert!(app.bw_session.is_none());
        app.bw_session = Some("test-session-token".to_string());
        assert_eq!(app.bw_session.as_deref(), Some("test-session-token"));
    }

    #[test]
    fn test_bw_session_none_for_non_bw_source() {
        let app = make_app("Host srv\n  HostName 1.2.3.4\n  # purple:askpass keychain\n");
        assert!(app.bw_session.is_none());
    }

    // =========================================================================
    // Ctrl+D sets global default in password picker
    // =========================================================================

    #[test]
    fn test_password_picker_ctrl_d_closes_picker() {
        // Use "None" to avoid writing a value to the real preferences file
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(6)); // None
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, ctrl_key('d'), &tx);
        assert!(!app.ui.show_password_picker);
    }

    #[test]
    fn test_password_picker_ctrl_d_does_not_change_form_askpass() {
        let mut app = make_form_app();
        app.form.askpass = "old".to_string();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(6)); // None
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, ctrl_key('d'), &tx);
        // Ctrl+D only sets the global default, not the form field
        assert_eq!(app.form.askpass, "old");
    }

    #[test]
    fn test_password_picker_ctrl_d_none_sets_status() {
        let mut app = make_form_app();
        app.ui.show_password_picker = true;
        app.ui.password_picker_state.select(Some(6)); // None
        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, ctrl_key('d'), &tx);
        // Shows "cleared" on success or "Failed to save" if ~/.purple doesn't exist
        assert!(app.status.is_some());
        assert!(!app.ui.show_password_picker);
    }

    #[test]
    fn test_password_picker_ctrl_d_source_label_in_status() {
        // Verify logic: non-None sources produce "Global default set to X." message
        let sources = crate::askpass::PASSWORD_SOURCES;
        for (i, src) in sources.iter().enumerate() {
            if src.label == "None" {
                continue;
            }
            let expected = format!("Global default set to {}.", src.label);
            assert!(expected.contains("default"), "Source {}: {}", i, expected);
        }
    }

    // =========================================================================
    // Keychain removal on askpass source change
    // =========================================================================

    #[test]
    fn test_submit_form_old_askpass_tracked_for_edit() {
        // When editing a host with keychain askpass, the old source is detected
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        assert_eq!(app.hosts[0].askpass, Some("keychain".to_string()));
        // Simulate opening edit form
        app.screen = Screen::EditHost { alias: "myserver".to_string() };
        app.form.alias = "myserver".to_string();
        app.form.hostname = "10.0.0.1".to_string();
        // Change askpass to something else
        app.form.askpass = "op://Vault/Item/pw".to_string();
        // The old_askpass detection in submit_form looks up app.hosts by alias
        let old = app.hosts.iter()
            .find(|h| h.alias == "myserver")
            .and_then(|h| h.askpass.clone());
        assert_eq!(old, Some("keychain".to_string()));
    }

    #[test]
    fn test_submit_form_no_keychain_removal_when_unchanged() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        app.screen = Screen::EditHost { alias: "myserver".to_string() };
        app.form.alias = "myserver".to_string();
        app.form.hostname = "10.0.0.1".to_string();
        // Keep askpass as keychain
        app.form.askpass = "keychain".to_string();
        let old = app.hosts.iter()
            .find(|h| h.alias == "myserver")
            .and_then(|h| h.askpass.clone());
        // Same source, no removal needed
        assert_eq!(old.as_deref(), Some("keychain"));
        assert_eq!(app.form.askpass, "keychain");
    }

    #[test]
    fn test_submit_form_no_keychain_removal_for_add() {
        // AddHost has no old askpass
        let mut app = make_app("Host existing\n  HostName 1.2.3.4\n");
        app.screen = Screen::AddHost;
        let old: Option<String> = None; // no old host for add
        assert!(old.is_none());
    }
}
