use std::sync::mpsc;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, FormField, HostForm, Screen};
use crate::clipboard;
use crate::event::AppEvent;
use crate::ping;
use crate::preferences;
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
        Screen::HostDetail { .. } => handle_host_detail(app, key),
        Screen::TagPicker => handle_tag_picker_screen(app, key),
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
                app.pending_connect = Some(alias);
            }
        }
        KeyCode::Char('a') => {
            app.form = HostForm::new();
            app.screen = Screen::AddHost;
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
                if let Some(block) = serialize_host_block(&app.config.elements, &alias) {
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
                    if !app.has_pinged {
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
            let _ = preferences::save_sort_mode(app.sort_mode);
            app.set_status(format!("Sorted by {}.", app.sort_mode.label()), false);
        }
        KeyCode::Char('i') => {
            if let Some(index) = app.selected_host_index() {
                app.screen = Screen::HostDetail { index };
            }
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

    // K opens key picker from any field
    if key.code == KeyCode::Char('K') {
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

    // Clear undo buffer on any write
    app.deleted_host = None;

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
            if !entry.tags.is_empty() {
                app.config.set_host_tags(&alias, &entry.tags);
            }
            if let Err(e) = app.config.write() {
                app.config.delete_host_undoable(&alias);
                app.set_status(format!("Failed to save: {}", e), true);
                return;
            }
            app.update_last_modified();
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
        Screen::EditHost { alias: old_alias } => {
            let old_alias = old_alias.clone();
            if !app.config.has_host(&old_alias) {
                app.set_status("Host no longer exists.", true);
                app.screen = Screen::HostList;
                return;
            }
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
            // Snapshot old entry for rollback
            let old_entry = app.hosts.iter().find(|h| h.alias == old_alias).cloned().unwrap_or_default();
            app.config.update_host(&old_alias, &entry);
            app.config.set_host_tags(&entry.alias, &entry.tags);
            if let Err(e) = app.config.write() {
                // Rollback: restore old entry
                app.config.update_host(&entry.alias, &old_entry);
                app.set_status(format!("Failed to save: {}", e), true);
                return;
            }
            app.update_last_modified();
            app.reload_hosts();
            app.set_status(format!("{} got a makeover.", alias), false);
        }
        _ => {}
    }

    app.screen = Screen::HostList;
}

fn handle_confirm_delete(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            if let Screen::ConfirmDelete { ref alias } = app.screen {
                let alias = alias.clone();
                if let Some((element, position)) = app.config.delete_host_undoable(&alias) {
                    if let Err(e) = app.config.write() {
                        // Restore the element on write failure
                        app.config.insert_host_at(element, position);
                        app.set_status(format!("Failed to save: {}", e), true);
                    } else {
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

/// Serialize a host block to its raw SSH config text.
fn serialize_host_block(elements: &[ConfigElement], alias: &str) -> Option<String> {
    for element in elements {
        match element {
            ConfigElement::HostBlock(block) if block.host_pattern == alias => {
                let mut lines = vec![block.raw_host_line.clone()];
                for directive in &block.directives {
                    lines.push(directive.raw_line.clone());
                }
                return Some(lines.join("\n"));
            }
            ConfigElement::Include(include) => {
                for file in &include.resolved_files {
                    if let Some(result) = serialize_host_block(&file.elements, alias) {
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
            if let Some(index) = app.tag_picker_state.selected() {
                if let Some(tag) = app.tag_list.get(index) {
                    let tag = tag.clone();
                    app.screen = Screen::HostList;
                    app.start_search();
                    app.search_query = Some(format!("tag={}", tag));
                    app.apply_filter();
                }
            }
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
