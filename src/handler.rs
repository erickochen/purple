use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, FormField, HostForm, Screen};

/// Handle a key event based on the current screen.
pub fn handle_key_event(app: &mut App, key: KeyEvent) -> Result<()> {
    // Global Ctrl+C handler — works on every screen
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.running = false;
        return Ok(());
    }

    match &app.screen {
        Screen::HostList => handle_host_list(app, key),
        Screen::AddHost | Screen::EditHost { .. } => handle_form(app, key),
        Screen::ConfirmDelete { .. } => handle_confirm_delete(app, key),
        Screen::Help => handle_help(app, key),
        Screen::KeyList => handle_key_list(app, key),
        Screen::KeyDetail { .. } => handle_key_detail(app, key),
    }
    Ok(())
}

fn handle_host_list(app: &mut App, key: KeyEvent) {
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
            if let Some(index) = app.selected_index() {
                if let Some(host) = app.hosts.get(index) {
                    app.form = HostForm::from_entry(host);
                    app.screen = Screen::EditHost { index };
                }
            }
        }
        KeyCode::Char('d') => {
            if let Some(index) = app.selected_index() {
                if index < app.hosts.len() {
                    app.screen = Screen::ConfirmDelete { index };
                }
            }
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
            app.form.focused_field = app.form.focused_field.next();
        }
        KeyCode::BackTab => {
            app.form.focused_field = app.form.focused_field.prev();
        }
        KeyCode::Up => {
            app.form.focused_field = app.form.focused_field.prev();
        }
        KeyCode::Enter => {
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
                    format!("'{}' already exists. Aliases are like fingerprints — unique.", alias),
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
            // Auto-select the newly added host (appended at end)
            let new_index = app.hosts.len().saturating_sub(1);
            app.list_state.select(Some(new_index));
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
                    format!("'{}' already exists. Aliases are like fingerprints — unique.", alias),
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
                    app.set_status(
                        format!("Locked and loaded with {}.", key.name),
                        false,
                    );
                }
            }
            app.show_key_picker = false;
        }
        _ => {}
    }
}
