use crossterm::event::{KeyCode, KeyEvent};
use log::{debug, info};

use crate::app::{App, Screen};

pub(super) fn handle_tunnel_list(app: &mut App, key: KeyEvent) {
    let alias = match &app.screen {
        Screen::TunnelList { alias } => alias.clone(),
        _ => return,
    };

    // Handle pending tunnel delete confirmation first
    if app.pending_tunnel_delete.is_some() && key.code != KeyCode::Char('?') {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let Some(sel) = app.pending_tunnel_delete.take() else {
                    return;
                };
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
                        app.update_last_modified();
                        app.refresh_tunnel_list(&alias);
                        app.reload_hosts();
                        if app.tunnel_list.is_empty() {
                            app.ui.tunnel_list_state.select(None);
                        } else if sel >= app.tunnel_list.len() {
                            app.ui
                                .tunnel_list_state
                                .select(Some(app.tunnel_list.len() - 1));
                        }
                        app.set_status("Tunnel removed.", false);
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.pending_tunnel_delete = None;
            }
            _ => {}
        }
        return;
    }

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
        KeyCode::PageDown => {
            crate::app::page_down(&mut app.ui.tunnel_list_state, app.tunnel_list.len(), 10);
        }
        KeyCode::PageUp => {
            crate::app::page_up(&mut app.ui.tunnel_list_state, app.tunnel_list.len(), 10);
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
            app.capture_tunnel_form_baseline();
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
                    app.capture_tunnel_form_baseline();
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
                if sel < app.tunnel_list.len() {
                    app.pending_tunnel_delete = Some(sel);
                }
            }
        }
        KeyCode::Enter => {
            // Start/stop tunnel
            if app.active_tunnels.contains_key(&alias) {
                // Stop
                if let Some(mut tunnel) = app.active_tunnels.remove(&alias) {
                    if let Err(e) = tunnel.child.kill() {
                        debug!("[external] Failed to kill tunnel process for {alias}: {e}");
                    }
                    let _ = tunnel.child.wait();
                    app.set_status(format!("Tunnel for {} stopped.", alias), false);
                }
            } else if !app.tunnel_list.is_empty() {
                // Start
                if app.demo_mode {
                    app.set_status("Demo mode. Tunnels disabled.".to_string(), false);
                    return;
                }
                let askpass = app
                    .hosts
                    .iter()
                    .find(|h| h.alias == alias)
                    .and_then(|h| h.askpass.clone());
                match crate::tunnel::start_tunnel(
                    &alias,
                    &app.reload.config_path,
                    askpass.as_deref(),
                    app.bw_session.as_deref(),
                ) {
                    Ok(child) => {
                        for rule in &app.tunnel_list {
                            info!(
                                "Tunnel started: type={} local={} remote={}:{} alias={alias}",
                                rule.tunnel_type.label(),
                                rule.bind_port,
                                rule.remote_host,
                                rule.remote_port
                            );
                        }
                        app.active_tunnels
                            .insert(alias.clone(), crate::tunnel::ActiveTunnel { child });
                        app.set_status(format!("Tunnel for {} started.", alias), false);
                    }
                    Err(e) => {
                        app.set_status(format!("Failed to start tunnel: {}", e), true);
                    }
                }
            }
        }
        KeyCode::Char('?') => {
            let old = std::mem::replace(&mut app.screen, Screen::HostList);
            app.screen = Screen::Help {
                return_screen: Box::new(old),
            };
        }
        _ => {}
    }
}

pub(super) fn handle_tunnel_form(app: &mut App, key: KeyEvent) {
    let (alias, editing) = match &app.screen {
        Screen::TunnelForm { alias, editing } => (alias.clone(), *editing),
        _ => return,
    };

    // Handle discard confirmation dialog
    if app.pending_discard_confirm {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.pending_discard_confirm = false;
                app.clear_form_mtime();
                app.tunnel_form_baseline = None;
                app.screen = Screen::TunnelList { alias };
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.pending_discard_confirm = false;
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            if app.tunnel_form_is_dirty() {
                app.pending_discard_confirm = true;
            } else {
                app.clear_form_mtime();
                app.tunnel_form_baseline = None;
                app.screen = Screen::TunnelList { alias };
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            app.tunnel_form.focused_field = app
                .tunnel_form
                .focused_field
                .next(app.tunnel_form.tunnel_type);
            app.tunnel_form.sync_cursor_to_end();
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.tunnel_form.focused_field = app
                .tunnel_form
                .focused_field
                .prev(app.tunnel_form.tunnel_type);
            app.tunnel_form.sync_cursor_to_end();
        }
        KeyCode::Left => {
            if app.tunnel_form.cursor_pos > 0 {
                app.tunnel_form.cursor_pos -= 1;
            }
        }
        KeyCode::Right => {
            let len = app
                .tunnel_form
                .focused_value()
                .map(|v| v.chars().count())
                .unwrap_or(0);
            if app.tunnel_form.cursor_pos < len {
                app.tunnel_form.cursor_pos += 1;
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
        KeyCode::Char(' ')
            if app.tunnel_form.focused_field == crate::app::TunnelFormField::Type =>
        {
            app.tunnel_form.tunnel_type = app.tunnel_form.tunnel_type.next();
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
            app.set_status(
                "Tunnel list changed externally. Press Esc and re-open.",
                true,
            );
            return;
        }
    }

    // Duplicate detection (runs after old directive removal for edits)
    if app
        .config
        .has_forward(alias, directive_key, &directive_value)
    {
        app.config = config_backup;
        app.set_status("Duplicate tunnel already configured.", true);
        return;
    }

    app.config
        .add_forward(alias, directive_key, &directive_value);
    if let Err(e) = app.config.write() {
        app.config = config_backup;
        app.set_status(format!("Failed to save: {}", e), true);
        return;
    }

    app.undo_stack.clear(); // Clear undo buffer — positions may have shifted
    app.update_last_modified();
    app.refresh_tunnel_list(alias);
    app.reload_hosts();
    // Fix selection after list change
    if app.tunnel_list.is_empty() {
        app.ui.tunnel_list_state.select(None);
    } else if let Some(sel) = app.ui.tunnel_list_state.selected() {
        if sel >= app.tunnel_list.len() {
            app.ui
                .tunnel_list_state
                .select(Some(app.tunnel_list.len() - 1));
        }
    } else {
        // First tunnel added to empty list — initialize selection
        app.ui.tunnel_list_state.select(Some(0));
    }
    app.clear_form_mtime();
    app.tunnel_form_baseline = None;
    app.set_status("Tunnel saved.", false);
    app.screen = Screen::TunnelList {
        alias: alias.to_string(),
    };
}
