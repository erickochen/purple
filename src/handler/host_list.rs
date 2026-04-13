use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, HostForm, Screen, ViewMode};
use crate::clipboard;
use crate::event::AppEvent;
use crate::preferences;
use crate::ssh_config::model::ConfigElement;

mod actions;

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

pub(super) fn handle_host_list(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    // Handle tag input mode
    if app.tags.input.is_some() {
        super::host_detail::handle_tag_input(app, key);
        return;
    }

    match key.code {
        KeyCode::Char('q') => {
            if let Some(ref cancel) = app.vault.signing_cancel {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            app.running = false;
        }
        KeyCode::Esc => {
            if app.group_filter.is_some() {
                app.clear_group_filter();
            } else {
                if let Some(ref cancel) = app.vault.signing_cancel {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                app.running = false;
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next_skipping_headers();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev_skipping_headers();
        }
        KeyCode::Tab => {
            app.next_group_tab();
        }
        KeyCode::BackTab => {
            app.prev_group_tab();
        }
        KeyCode::PageDown => {
            app.page_down_host();
        }
        KeyCode::PageUp => {
            app.page_up_host();
        }
        KeyCode::Enter => {
            if app.is_pattern_selected() {
                return;
            }
            if let Some(host) = app.selected_host() {
                let alias = host.alias.clone();
                let askpass = host.askpass.clone();
                let stale_hint = if host.stale.is_some() {
                    Some(super::stale_provider_hint(host))
                } else {
                    None
                };
                if let Some(hint) = stale_hint {
                    app.set_status(format!("Stale host.{}", hint), true);
                }
                if app.demo_mode {
                    app.set_status("Demo mode. Connection disabled.".to_string(), false);
                    return;
                }
                app.pending_connect = Some((alias, askpass));
            }
        }
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let visible_indices: Vec<usize> = app
                .display_list
                .iter()
                .filter_map(|item| match item {
                    crate::app::HostListItem::Host { index } => Some(*index),
                    _ => None,
                })
                .collect();
            let all_selected = !visible_indices.is_empty()
                && visible_indices
                    .iter()
                    .all(|idx| app.multi_select.contains(idx));
            if all_selected {
                app.multi_select.clear();
            } else {
                for idx in visible_indices {
                    app.multi_select.insert(idx);
                }
            }
        }
        KeyCode::Char('a') => {
            app.form = HostForm::new();
            app.screen = Screen::AddHost;
            app.capture_form_mtime();
            app.capture_form_baseline();
        }
        KeyCode::Char('A') => {
            app.form = HostForm::new_pattern();
            app.screen = Screen::AddHost;
            app.capture_form_mtime();
            app.capture_form_baseline();
        }
        KeyCode::Char('e') => {
            if let Some(pattern) = app.selected_pattern().cloned() {
                if pattern.source_file.is_some() {
                    app.set_status(
                        format!("{} is in an included file. Edit it there.", pattern.pattern),
                        true,
                    );
                    return;
                }
                app.form = HostForm::from_pattern_entry(&pattern);
                app.screen = Screen::EditHost {
                    alias: pattern.pattern,
                };
                app.capture_form_mtime();
                app.capture_form_baseline();
            } else if let Some(host) = app.selected_host().cloned() {
                super::open_edit_form(app, host);
            }
        }
        KeyCode::Char('d') => {
            if let Some(pattern) = app.selected_pattern() {
                if pattern.source_file.is_some() {
                    app.set_status(
                        format!(
                            "{} is in an included file. Delete it there.",
                            pattern.pattern
                        ),
                        true,
                    );
                    return;
                }
                let alias = pattern.pattern.clone();
                app.screen = Screen::ConfirmDelete { alias };
            } else if let Some(host) = app.selected_host() {
                if let Some(ref source) = host.source_file {
                    let alias = host.alias.clone();
                    let path = source.display();
                    app.set_status(format!("{} lives in {}. Edit it there.", alias, path), true);
                    return;
                }
                let stale_hint = if host.stale.is_some() {
                    Some(super::stale_provider_hint(host))
                } else {
                    None
                };
                let alias = host.alias.clone();
                if let Some(hint) = stale_hint {
                    app.set_status(format!("Stale host.{}", hint), true);
                }
                app.screen = Screen::ConfirmDelete { alias };
            }
        }
        KeyCode::Char('c') => actions::clone_selected(app),
        KeyCode::Char('y') => {
            if app.is_pattern_selected() {
                return;
            }
            if let Some(host) = app.selected_host() {
                let cmd = host.ssh_command(&app.reload.config_path);
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
            if app.is_pattern_selected() {
                return;
            }
            if let Some(host) = app.selected_host() {
                let alias = host.alias.clone();
                if let Some(block) =
                    serialize_host_block(&app.config.elements, &alias, app.config.crlf)
                {
                    match clipboard::copy_to_clipboard(&block) {
                        Ok(()) => {
                            app.set_status(format!("Copied config block for {}.", alias), false);
                        }
                        Err(e) => {
                            app.set_status(e, true);
                        }
                    }
                }
            }
        }
        KeyCode::Char('p') => {
            if app.is_pattern_selected() {
                return;
            }
            if !app.ping.status.is_empty() {
                app.ping.status.clear();
                app.ping.filter_down_only = false;
                app.ping.checked_at = None;
                app.ping.generation += 1;
                app.status = None;
            } else {
                super::ping::ping_selected_host(app, events_tx, true);
            }
        }
        KeyCode::Char('P') => {
            if !app.ping.status.is_empty() {
                app.ping.status.clear();
                app.ping.filter_down_only = false;
                app.ping.checked_at = None;
                app.ping.generation += 1;
                app.status = None;
            } else {
                let hosts_to_ping: Vec<(String, String, u16)> = app
                    .hosts
                    .iter()
                    .filter(|h| !h.hostname.is_empty() && h.proxy_jump.is_empty())
                    .map(|h| (h.alias.clone(), h.hostname.clone(), h.port))
                    .collect();
                // Mark ProxyJump hosts as Checking (their status will be
                // inherited from the bastion once it responds).
                for h in &app.hosts {
                    if !h.proxy_jump.is_empty() {
                        app.ping
                            .status
                            .insert(h.alias.clone(), crate::app::PingStatus::Checking);
                    }
                }
                if !hosts_to_ping.is_empty() {
                    for (alias, _, _) in &hosts_to_ping {
                        app.ping
                            .status
                            .insert(alias.clone(), crate::app::PingStatus::Checking);
                    }
                    app.set_info_status("Pinging all the things...");
                    crate::ping::ping_all(&hosts_to_ping, events_tx.clone(), app.ping.generation);
                }
            }
        }
        KeyCode::Char('!') => {
            if app.ping.status.is_empty() {
                app.set_status("Ping first (p/P), then filter with !.", true);
            } else {
                app.ping.filter_down_only = !app.ping.filter_down_only;
                if app.ping.filter_down_only {
                    // Activate search mode to trigger filtering
                    if app.search.query.is_none() {
                        app.search.query = Some(String::new());
                    }
                    app.apply_filter();
                    let count = app.search.filtered_indices.len();
                    app.set_status(
                        format!(
                            "Showing {} unreachable host{}.",
                            count,
                            if count == 1 { "" } else { "s" }
                        ),
                        false,
                    );
                } else {
                    // If search was only active for down-only, clear it
                    if app.search.query.as_ref().is_some_and(|q| q.is_empty()) {
                        app.search.query = None;
                        app.search.filtered_indices.clear();
                        app.search.filtered_pattern_indices.clear();
                    } else {
                        app.apply_filter();
                    }
                    app.status = None;
                }
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
            if app.is_pattern_selected() {
                return;
            }
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
                app.tags.cursor = current_tags.chars().count();
                app.tags.input = Some(current_tags);
            }
        }
        KeyCode::Char('s') => {
            app.sort_mode = app.sort_mode.next();
            app.apply_sort();
            if let Err(e) = preferences::save_sort_mode(app.sort_mode) {
                app.set_status(
                    format!("Sorted by {}. (save failed: {})", app.sort_mode.label(), e),
                    true,
                );
            } else {
                app.set_status(format!("Sorted by {}.", app.sort_mode.label()), false);
            }
        }
        KeyCode::Char('g') => {
            use crate::app::GroupBy;
            match &app.group_by {
                GroupBy::None => {
                    app.group_by = GroupBy::Provider;
                    app.group_filter = None;
                    app.apply_sort();
                    if let Err(e) = preferences::save_group_by(&app.group_by) {
                        app.set_status(
                            format!("Grouped by {}. (save failed: {})", app.group_by.label(), e),
                            true,
                        );
                    } else {
                        app.set_status(format!("Grouped by {}.", app.group_by.label()), false);
                    }
                }
                GroupBy::Provider => {
                    let user_tags: Vec<String> = {
                        let mut seen = std::collections::HashSet::new();
                        let mut tags = Vec::new();
                        for host in &app.hosts {
                            for tag in &host.tags {
                                if seen.insert(tag.clone()) {
                                    tags.push(tag.clone());
                                }
                            }
                        }
                        tags.sort_by_cached_key(|a| a.to_lowercase());
                        tags
                    };
                    if user_tags.is_empty() {
                        app.group_by = GroupBy::None;
                        app.group_filter = None;
                        app.apply_sort();
                        if let Err(e) = preferences::save_group_by(&app.group_by) {
                            app.set_status(format!("Ungrouped. (save failed: {})", e), true);
                        } else {
                            app.set_status("Ungrouped.", false);
                        }
                    } else {
                        // Switch to tag mode directly. The nav bar shows all
                        // tags as tabs, no picker needed.
                        app.group_by = GroupBy::Tag(String::new());
                        app.group_filter = None;
                        app.apply_sort();
                        if let Err(e) = preferences::save_group_by(&app.group_by) {
                            app.set_status(format!("Grouped by tag. (save failed: {})", e), true);
                        } else {
                            app.set_status("Grouped by tag.", false);
                        }
                    }
                }
                GroupBy::Tag(_) => {
                    app.group_by = GroupBy::None;
                    app.group_filter = None;
                    app.apply_sort();
                    if let Err(e) = preferences::save_group_by(&app.group_by) {
                        app.set_status(format!("Ungrouped. (save failed: {})", e), true);
                    } else {
                        app.set_status("Ungrouped.", false);
                    }
                }
            }
        }
        KeyCode::Char('i') => {
            if app.is_pattern_selected() {
                return;
            }
            if let Some(index) = app.selected_host_index() {
                app.screen = Screen::HostDetail { index };
            }
        }
        KeyCode::Char('v') => {
            app.view_mode = if app.view_mode == ViewMode::Compact {
                ViewMode::Detailed
            } else {
                ViewMode::Compact
            };
            app.detail_toggle_pending = true;
            app.ui.detail_scroll = 0;
            let _ = preferences::save_view_mode(app.view_mode);
        }
        KeyCode::Char(']') if app.view_mode == ViewMode::Detailed => {
            app.ui.detail_scroll = app.ui.detail_scroll.saturating_add(1);
        }
        KeyCode::Char('[') if app.view_mode == ViewMode::Detailed => {
            app.ui.detail_scroll = app.ui.detail_scroll.saturating_sub(1);
        }
        KeyCode::Char('u') => {
            if let Some(deleted) = app.undo_stack.pop() {
                let alias = match &deleted.element {
                    ConfigElement::HostBlock(block) => block.host_pattern.clone(),
                    _ => "host".to_string(),
                };
                app.config.insert_host_at(deleted.element, deleted.position);
                if let Err(e) = app.config.write() {
                    // Rollback: remove re-inserted host and restore undo buffer
                    if let Some((element, position)) = app.config.delete_host_undoable(&alias) {
                        app.undo_stack
                            .push(crate::app::DeletedHost { element, position });
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
        KeyCode::Char('m') => {
            let current = crate::ui::theme::current_theme().name;
            let builtins = crate::ui::theme::ThemeDef::builtins();
            let custom = crate::ui::theme::ThemeDef::load_custom();
            let idx = builtins
                .iter()
                .position(|t| t.name.eq_ignore_ascii_case(&current))
                .or_else(|| {
                    if custom.is_empty() {
                        None
                    } else {
                        custom
                            .iter()
                            .position(|t| t.name.eq_ignore_ascii_case(&current))
                            .map(|i| builtins.len() + 1 + i) // +1 for divider
                    }
                })
                .unwrap_or(0);
            app.ui.theme_picker_state.select(Some(idx));
            app.ui.theme_picker_builtins = builtins;
            app.ui.theme_picker_custom = custom;
            app.ui.theme_picker_saved_name =
                crate::preferences::load_theme().unwrap_or_else(|| "Purple".to_string());
            app.ui.theme_picker_original = Some(crate::ui::theme::current_theme());
            app.screen = Screen::ThemePicker;
        }
        KeyCode::Char('T') => {
            if app.is_pattern_selected() {
                return;
            }
            if let Some(host) = app.selected_host() {
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
        KeyCode::Char('S') => {
            if !app.demo_mode {
                app.provider_config = crate::providers::config::ProviderConfig::load();
            }
            app.ui.provider_list_state = ratatui::widgets::ListState::default();
            app.ui.provider_list_state.select(Some(0));
            app.screen = Screen::Providers;
        }
        KeyCode::Char('I') => {
            let count = crate::import::count_known_hosts_candidates();
            if count > 0 {
                app.screen = Screen::ConfirmImport { count };
            } else {
                app.set_status("No importable hosts in known_hosts.", true);
            }
        }
        KeyCode::Char('X') => {
            let stale = app.config.stale_hosts();
            if stale.is_empty() {
                app.set_status("No stale hosts.", true);
            } else {
                let aliases: Vec<String> = stale.into_iter().map(|(a, _)| a).collect();
                app.screen = Screen::ConfirmPurgeStale {
                    aliases,
                    provider: None,
                };
            }
        }
        KeyCode::Char('V') => actions::initiate_bulk_vault_sign(app),
        KeyCode::Char(' ') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.is_pattern_selected() {
                return;
            }
            if let Some(idx) = app.selected_host_index() {
                if app.multi_select.contains(&idx) {
                    app.multi_select.remove(&idx);
                } else {
                    app.multi_select.insert(idx);
                }
            }
        }
        KeyCode::Char('r') => {
            if app.is_pattern_selected() {
                return;
            }
            let (aliases, stale_hint): (Vec<String>, Option<String>) =
                if app.multi_select.is_empty() {
                    if let Some(host) = app.selected_host() {
                        let hint = if host.stale.is_some() {
                            Some(super::stale_provider_hint(host))
                        } else {
                            None
                        };
                        (vec![host.alias.clone()], hint)
                    } else {
                        (Vec::new(), None)
                    }
                } else {
                    let has_stale = app
                        .multi_select
                        .iter()
                        .any(|&idx| app.hosts.get(idx).is_some_and(|h| h.stale.is_some()));
                    (
                        app.multi_select
                            .iter()
                            .filter_map(|&idx| app.hosts.get(idx).map(|h| h.alias.clone()))
                            .collect(),
                        if has_stale {
                            Some(" Selection includes stale hosts.".to_string())
                        } else {
                            None
                        },
                    )
                };
            if let Some(hint) = stale_hint {
                app.set_status(format!("Stale host.{}", hint), true);
            }
            if aliases.is_empty() {
                app.set_status("No host selected.", true);
            } else {
                super::snippet::open_snippet_picker(app, aliases);
            }
        }
        KeyCode::Char('R') => {
            if app.is_pattern_selected() {
                return;
            }
            let aliases: Vec<String> = app
                .display_list
                .iter()
                .filter_map(|item| match item {
                    crate::app::HostListItem::Host { index } => {
                        Some(app.hosts[*index].alias.clone())
                    }
                    _ => None,
                })
                .collect();
            if aliases.is_empty() {
                app.set_status("No hosts to run on.", true);
            } else {
                super::snippet::open_snippet_picker(app, aliases);
            }
        }
        KeyCode::Char(':') => {
            log::debug!("palette: opened from host list");
            app.palette = Some(crate::app::CommandPaletteState::new());
        }
        KeyCode::Char('F') => actions::open_file_browser(app, events_tx),
        KeyCode::Char('C') => actions::open_container_overlay(app, events_tx),
        KeyCode::Char('?') => {
            let old = std::mem::replace(&mut app.screen, Screen::HostList);
            app.screen = Screen::Help {
                return_screen: Box::new(old),
            };
        }
        _ => {}
    }
}

pub(super) fn handle_host_list_search(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    match key.code {
        KeyCode::Esc => {
            app.cancel_search();
        }
        KeyCode::Enter => {
            if let Some(host) = app.selected_host() {
                let alias = host.alias.clone();
                let askpass = host.askpass.clone();
                let stale_hint = if host.stale.is_some() {
                    Some(super::stale_provider_hint(host))
                } else {
                    None
                };
                app.cancel_search();
                if let Some(hint) = stale_hint {
                    app.set_status(format!("Stale host.{}", hint), true);
                }
                if app.demo_mode {
                    app.set_status("Demo mode. Connection disabled.".to_string(), false);
                    return;
                }
                app.pending_connect = Some((alias, askpass));
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            app.select_next();
        }
        KeyCode::Up | KeyCode::BackTab => {
            app.select_prev();
        }
        KeyCode::PageDown => {
            app.page_down_host();
        }
        KeyCode::PageUp => {
            app.page_up_host();
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if !app.ping.status.is_empty() {
                app.ping.status.clear();
                app.ping.checked_at = None;
                app.ping.generation += 1;
                if app.ping.filter_down_only {
                    app.cancel_search();
                } else {
                    app.ping.filter_down_only = false;
                }
                app.status = None;
            } else {
                super::ping::ping_selected_host(app, events_tx, false);
            }
        }
        KeyCode::Char(' ') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(idx) = app.selected_host_index() {
                if app.multi_select.contains(&idx) {
                    app.multi_select.remove(&idx);
                } else {
                    app.multi_select.insert(idx);
                }
            }
        }
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let visible_indices: Vec<usize> = app.search.filtered_indices.clone();
            let all_selected = !visible_indices.is_empty()
                && visible_indices
                    .iter()
                    .all(|idx| app.multi_select.contains(idx));
            if all_selected {
                app.multi_select.clear();
            } else {
                for idx in visible_indices {
                    app.multi_select.insert(idx);
                }
            }
        }
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(host) = app.selected_host().cloned() {
                super::open_edit_form(app, host);
            }
        }
        KeyCode::Char('!') if app.ping.filter_down_only => {
            app.ping.filter_down_only = false;
            if app.search.query.as_ref().is_some_and(|q| q.is_empty()) {
                app.cancel_search();
            } else {
                app.apply_filter();
            }
            app.status = None;
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
