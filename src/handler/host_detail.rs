use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Screen};

pub(super) fn handle_tag_input(app: &mut App, key: KeyEvent) {
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
            app.tag_input_cursor = 0;
        }
        KeyCode::Esc => {
            app.tag_input = None;
            app.tag_input_cursor = 0;
        }
        KeyCode::Left => {
            if app.tag_input_cursor > 0 {
                app.tag_input_cursor -= 1;
            }
        }
        KeyCode::Right => {
            if let Some(ref input) = app.tag_input {
                if app.tag_input_cursor < input.chars().count() {
                    app.tag_input_cursor += 1;
                }
            }
        }
        KeyCode::Home => {
            app.tag_input_cursor = 0;
        }
        KeyCode::End => {
            if let Some(ref input) = app.tag_input {
                app.tag_input_cursor = input.chars().count();
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut input) = app.tag_input {
                let byte_pos = crate::app::char_to_byte_pos(input, app.tag_input_cursor);
                input.insert(byte_pos, c);
                app.tag_input_cursor += 1;
            }
        }
        KeyCode::Backspace => {
            if app.tag_input_cursor > 0 {
                if let Some(ref mut input) = app.tag_input {
                    let byte_pos = crate::app::char_to_byte_pos(input, app.tag_input_cursor);
                    let prev = crate::app::char_to_byte_pos(input, app.tag_input_cursor - 1);
                    input.drain(prev..byte_pos);
                    app.tag_input_cursor -= 1;
                }
            }
        }
        _ => {}
    }
}

pub(super) fn handle_host_detail(app: &mut App, key: KeyEvent) {
    let index = match app.screen {
        Screen::HostDetail { index } => index,
        _ => return,
    };
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('i') => {
            app.screen = Screen::HostList;
        }
        KeyCode::Char('?') => {
            let old = std::mem::replace(&mut app.screen, Screen::HostList);
            app.screen = Screen::Help {
                return_screen: Box::new(old),
            };
        }
        KeyCode::Char('e') => {
            if let Some(host) = app.hosts.get(index).cloned() {
                super::open_edit_form(app, host);
            }
        }
        KeyCode::Char('T') => {
            if let Some(host) = app.hosts.get(index) {
                let stale_hint = if host.stale.is_some() {
                    Some(super::stale_provider_hint(host))
                } else {
                    None
                };
                let alias = host.alias.clone();
                if let Some(hint) = stale_hint {
                    app.set_status(format!("Stale host.{}", hint), true);
                }
                app.refresh_tunnel_list(&alias);
                app.ui.tunnel_list_state = ratatui::widgets::ListState::default();
                if !app.tunnel_list.is_empty() {
                    app.ui.tunnel_list_state.select(Some(0));
                }
                app.screen = Screen::TunnelList { alias };
            }
        }
        KeyCode::Char('r') => {
            if let Some(host) = app.hosts.get(index) {
                let stale_hint = if host.stale.is_some() {
                    Some(super::stale_provider_hint(host))
                } else {
                    None
                };
                let alias = host.alias.clone();
                if let Some(hint) = stale_hint {
                    app.set_status(format!("Stale host.{}", hint), true);
                }
                app.screen = Screen::SnippetPicker {
                    target_aliases: vec![alias],
                };
                app.ui.snippet_picker_state = ratatui::widgets::ListState::default();
                let indices = app.filtered_snippet_indices();
                if !indices.is_empty() {
                    app.ui.snippet_picker_state.select(Some(0));
                }
            }
        }
        _ => {}
    }
}
