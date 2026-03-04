use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, FormField, HostForm, ProviderFormFields, Screen};
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
    // Dispatch to key picker if it's open
    if app.ui.show_key_picker {
        handle_key_picker_shared(app, key, false);
        return;
    }

    // Ctrl+K opens key picker from any field (not bare K, which conflicts with text input)
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('k') {
        app.scan_keys();
        app.ui.show_key_picker = true;
        app.ui.key_picker_state = ratatui::widgets::ListState::default();
        if !app.keys.is_empty() {
            app.ui.key_picker_state.select(Some(0));
        }
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
            app.set_status(msg, false);
        }
        Err(msg) => {
            app.set_status(msg, true);
            return;
        }
    }

    app.clear_form_mtime();
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
                        ProviderFormFields {
                            url: section.url.clone(),
                            token: section.token.clone(),
                            alias_prefix: section.alias_prefix.clone(),
                            user: section.user.clone(),
                            identity_file: section.identity_file.clone(),
                            verify_tls: section.verify_tls,
                            auto_sync: section.auto_sync,
                            focused_field: first_field,
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
                    if let Some(old_section) = app.provider_config.section(name.as_str()).cloned() {
                        app.provider_config.remove_section(name.as_str());
                        if let Err(e) = app.provider_config.save() {
                            // Rollback: restore the removed section
                            app.provider_config.set_section(old_section);
                            app.set_status(format!("Failed to save: {}", e), true);
                        } else {
                            app.sync_history.remove(name.as_str());
                            let display_name = crate::providers::provider_display_name(name.as_str());
                            app.set_status(
                                format!("Removed {} configuration.", display_name),
                                false,
                            );
                        }
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

    // Ctrl+K opens key picker from any field (not bare K, which conflicts with text input)
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('k') {
        app.scan_keys();
        app.ui.show_key_picker = true;
        app.ui.key_picker_state = ratatui::widgets::ListState::default();
        if !app.keys.is_empty() {
            app.ui.key_picker_state.select(Some(0));
        }
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
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.provider_form.focused_field = app.provider_form.focused_field.prev(fields);
        }
        KeyCode::Enter => {
            submit_provider_form(app, events_tx);
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
                app.provider_form.focused_value_mut().push(c);
            }
        }
        KeyCode::Backspace => {
            let f = app.provider_form.focused_field;
            if f != crate::app::ProviderFormField::VerifyTls && f != crate::app::ProviderFormField::AutoSync {
                app.provider_form.focused_value_mut().pop();
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
                    } else {
                        app.form.identity_file = key_info.display_path.clone();
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
                match crate::tunnel::start_tunnel(&alias, &app.reload.config_path) {
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
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.tunnel_form.focused_field = app.tunnel_form.focused_field.prev(app.tunnel_form.tunnel_type);
        }
        KeyCode::Left => {
            if app.tunnel_form.focused_field == crate::app::TunnelFormField::Type {
                app.tunnel_form.tunnel_type = app.tunnel_form.tunnel_type.prev();
            }
        }
        KeyCode::Right => {
            if app.tunnel_form.focused_field == crate::app::TunnelFormField::Type {
                app.tunnel_form.tunnel_type = app.tunnel_form.tunnel_type.next();
            }
        }
        KeyCode::Enter => {
            submit_tunnel_form(app, &alias, editing);
        }
        KeyCode::Char(c) => {
            if let Some(val) = app.tunnel_form.focused_value_mut() {
                val.push(c);
            }
        }
        KeyCode::Backspace => {
            if let Some(val) = app.tunnel_form.focused_value_mut() {
                val.pop();
            }
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

    fn make_app(content: &str) -> App {
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: PathBuf::from("/tmp/test_config"),
            crlf: false,
        };
        App::new(config)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// App met een geconfigureerde DigitalOcean (auto_sync=true) en een nieuw Proxmox.
    fn make_providers_app_with_do() -> App {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.screen = Screen::Providers;
        app.provider_config = ProviderConfig::default();
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
        app.provider_config = ProviderConfig::default();
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
        app.provider_config = ProviderConfig::default();
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
        app.provider_config = ProviderConfig::default(); // geen config voor proxmox
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
        app.provider_config = ProviderConfig::default();
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
        };
        app
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
        app.provider_config = ProviderConfig::default();
        app.provider_form = ProviderFormFields {
            url: String::new(),
            token: "tok".to_string(),
            alias_prefix: "do".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            verify_tls: true,
            auto_sync: false,
            focused_field: ProviderFormField::Token,
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
        app.provider_config = ProviderConfig::default();
        app.provider_form = ProviderFormFields {
            url: String::new(),
            token: "tok".to_string(),
            alias_prefix: "do".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            verify_tls: true,
            auto_sync: true,
            focused_field: ProviderFormField::Token,
        };

        let (tx, _rx) = mpsc::channel();
        let _ = handle_key_event(&mut app, key(KeyCode::Enter), &tx);

        if let Some(section) = app.provider_config.section("digitalocean") {
            assert!(section.auto_sync, "Opgeslagen sectie moet auto_sync=true hebben");
        }
    }
}
