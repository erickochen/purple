use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, FormField, Screen};
use crate::quick_add;

pub(super) fn handle_form(app: &mut App, key: KeyEvent) {
    // Dispatch to password picker if it's open
    if app.ui.show_password_picker {
        super::picker::handle_password_picker(app, key);
        return;
    }

    // Dispatch to key picker if it's open
    if app.ui.show_key_picker {
        super::picker::handle_key_picker_shared(app, key, false);
        return;
    }

    // Dispatch to proxyjump picker if it's open
    if app.ui.show_proxyjump_picker {
        super::picker::handle_proxyjump_picker(app, key);
        return;
    }

    // Handle discard confirmation dialog
    if app.pending_discard_confirm {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.pending_discard_confirm = false;
                app.clear_form_mtime();
                app.form_baseline = None;
                app.screen = Screen::HostList;
                app.flush_pending_vault_write();
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
            if app.host_form_is_dirty() {
                app.pending_discard_confirm = true;
            } else {
                app.clear_form_mtime();
                app.form_baseline = None;
                app.screen = Screen::HostList;
                app.flush_pending_vault_write();
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            // Smart paste detection: when leaving Alias field, check for user@host:port
            if app.form.focused_field == FormField::Alias {
                maybe_smart_paste(app);
            }
            if !app.form.expanded {
                // Collapsed mode: Tab/Down from last required field expands
                match app.form.focused_field {
                    FormField::Alias => {
                        app.form.focused_field = FormField::Hostname;
                    }
                    FormField::Hostname => {
                        app.form.expanded = true;
                        app.form.focused_field = FormField::User;
                    }
                    // Defensive: if focus is on an optional field while collapsed, reset
                    _ => {
                        app.form.focused_field = FormField::Alias;
                    }
                }
            } else {
                // Progressive disclosure: advance through the visible field
                // subset so Tab skips over the hidden `VaultAddr` field when
                // no role is set.
                app.form.focus_next_visible();
            }
            app.form.sync_cursor_to_end();
            app.form.update_hint();
        }
        KeyCode::BackTab | KeyCode::Up => {
            if !app.form.expanded {
                // Collapsed: cycle within required fields only
                app.form.focused_field = match app.form.focused_field {
                    FormField::Alias => FormField::Hostname,
                    // Any other field (including Hostname): go to Alias
                    _ => FormField::Alias,
                };
            } else {
                app.form.focus_prev_visible();
            }
            app.form.sync_cursor_to_end();
            app.form.update_hint();
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
        KeyCode::Enter => match app.form.focused_field {
            FormField::IdentityFile => {
                app.scan_keys();
                app.ui.show_key_picker = true;
                app.ui.key_picker_state = ratatui::widgets::ListState::default();
                if !app.keys.is_empty() {
                    app.ui.key_picker_state.select(Some(0));
                }
            }
            FormField::ProxyJump => {
                let candidates = app.proxyjump_candidates();
                app.ui.show_proxyjump_picker = true;
                app.ui.proxyjump_picker_state = ratatui::widgets::ListState::default();
                if !candidates.is_empty() {
                    app.ui.proxyjump_picker_state.select(Some(0));
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
        },
        KeyCode::Char(c) => {
            app.form.insert_char(c);
            app.form.update_hint();
        }
        KeyCode::Backspace => {
            app.form.delete_char_before_cursor();
            app.form.update_hint();
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

pub(super) fn submit_form(app: &mut App) {
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
        Screen::EditHost { alias } => app
            .hosts
            .iter()
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
            app.undo_stack.clear();
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
                            if crate::askpass::store_in_keychain(&app.form.alias, &pw).is_ok() {
                                let _ = crate::askpass::remove_from_keychain(alias);
                            }
                        }
                    }
                }
            }
            // Drain any side-channel cleanup warning produced during the
            // mutation. When set, it overrides the success message because
            // the user needs to see that something on disk failed.
            if let Some(warning) = app.cert_cleanup_warning.take() {
                app.set_status(warning, true);
            } else {
                app.set_status(final_msg, false);
            }
        }
        Err(msg) => {
            app.set_status(msg, true);
            return;
        }
    }

    let target_alias = app.form.alias.trim().to_string();
    // Editing a stale host means the user asserts it is still wanted
    if let Screen::EditHost { ref alias } = app.screen {
        app.config.clear_host_stale(alias);
        // If alias was renamed, also clear on the new alias
        if *alias != target_alias {
            app.config.clear_host_stale(&target_alias);
        }
    }
    app.clear_form_mtime();
    app.form_baseline = None;
    app.screen = Screen::HostList;
    app.select_host_by_alias(&target_alias);
    app.flush_pending_vault_write();
}
