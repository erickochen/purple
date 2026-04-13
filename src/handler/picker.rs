use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, FormField};

pub(super) fn handle_password_picker(app: &mut App, key: KeyEvent) {
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
                            app.set_status(
                                format!("Global default set to {}.", source.label),
                                false,
                            );
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
            let mut needs_more_input = false;
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
                        app.set_status(
                            "Type your command. Use %a (alias) and %h (hostname) as placeholders.",
                            false,
                        );
                        needs_more_input = true;
                    } else if is_prefix {
                        app.form.askpass = source.value.to_string();
                        app.form.focused_field = FormField::AskPass;
                        app.form.sync_cursor_to_end();
                        app.set_status(format!("Complete the {} path.", source.label), false);
                        needs_more_input = true;
                    } else {
                        app.form.askpass = source.value.to_string();
                        app.form.sync_cursor_to_end();
                        app.set_status(format!("Password source set to {}.", source.label), false);
                    }
                }
            }
            app.ui.show_password_picker = false;
            if !needs_more_input {
                super::try_auto_submit_after_picker(app);
            }
        }
        _ => {}
    }
}

/// Unified key picker handler for both host form and provider form.
pub(super) fn handle_key_picker_shared(app: &mut App, key: KeyEvent, for_provider: bool) {
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
                    app.set_status(format!("Locked and loaded with {}.", key_info.name), false);
                }
            }
            app.ui.show_key_picker = false;
            if !for_provider {
                super::try_auto_submit_after_picker(app);
            }
        }
        _ => {}
    }
}

/// ProxyJump picker handler for the host form.
pub(super) fn handle_proxyjump_picker(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.ui.show_proxyjump_picker = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_proxyjump();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_proxyjump();
        }
        KeyCode::Enter => {
            let candidates = app.proxyjump_candidates();
            if let Some(index) = app.ui.proxyjump_picker_state.selected() {
                if let Some((alias, _)) = candidates.get(index) {
                    app.form.proxy_jump = alias.clone();
                    app.form.sync_cursor_to_end();
                    app.set_status(format!("Jumping through {}.", alias), false);
                }
            }
            app.ui.show_proxyjump_picker = false;
            super::try_auto_submit_after_picker(app);
        }
        _ => {}
    }
}

pub(super) fn handle_vault_role_picker(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.ui.show_vault_role_picker = false;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_vault_role();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_vault_role();
        }
        KeyCode::Enter => {
            let candidates = app.vault_role_candidates();
            if let Some(index) = app.ui.vault_role_picker_state.selected() {
                if let Some(role) = candidates.get(index) {
                    app.form.vault_ssh = role.clone();
                    app.form.sync_cursor_to_end();
                    app.set_status(format!("Vault SSH role set to {}.", role), false);
                }
            }
            app.ui.show_vault_role_picker = false;
        }
        _ => {}
    }
}
