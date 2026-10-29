//! Key handler for the host list, the app's primary screen.
//!
//! Takes `&mut App`, not a per-domain slice: it is a router that dispatches
//! across every domain (search, tags, ping, tunnels, containers, vault,
//! providers) and switches top-pages. The single-domain handlers use the slice
//! pattern in `ctx.rs`; routers keep the full borrow by design.

use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Screen, ViewMode};
use crate::clipboard;
use crate::event::AppEvent;
use crate::preferences;
use crate::ssh_config::model::ConfigElement;

pub(crate) mod actions;

/// Build a provider hint clause for stale host messages, e.g. "Gone from DigitalOcean".
pub(super) fn stale_provider_hint(host: &crate::ssh_config::model::HostEntry) -> String {
    host.provider
        .as_ref()
        .map(|p| format!("Gone from {}", crate::providers::provider_display_name(p)))
        .unwrap_or_default()
}

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

pub(super) fn handle_key(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    match app.top_page {
        crate::app::TopPage::Tunnels => super::tunnels_overview::handle_key(app, key),
        crate::app::TopPage::Containers => {
            super::containers_overview::handle_key(app, key, events_tx);
        }
        crate::app::TopPage::Snippets => super::snippets_overview::handle_key(app, key),
        crate::app::TopPage::Keys => super::keys_overview::handle_key(app, key),
        crate::app::TopPage::Hosts => {
            if app.search.query().is_some() {
                handle_search_key(app, key, events_tx);
            } else {
                handle_main_key(app, key, events_tx);
            }
        }
    }
    // Containers tab data load: lazy refresh on every key event in the
    // Containers tab. 30s TTL per resource; respects demo mode internally.
    if matches!(app.top_page, crate::app::TopPage::Containers)
        && matches!(app.screen, Screen::HostList)
    {
        super::containers_overview::ensure_inspect_for_selected(app, events_tx);
        super::containers_overview::ensure_logs_for_selected(app, events_tx);
        super::containers_overview::ensure_list_for_selected_host(app, events_tx);
        super::containers_overview::ensure_inspect_for_host_header(app, events_tx);
    }
}

pub(super) fn handle_main_key(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    // Handle tag input mode
    if app.tags.input().is_some() {
        super::host_detail::handle_tag_input(app, key);
        return;
    }

    match key.code {
        KeyCode::Char('q') => {
            if let Some(cancel) = app.vault.signing_cancel() {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            app.running = false;
        }
        KeyCode::Esc => {
            if app.hosts_state.group_filter().is_some() {
                app.clear_group_filter();
            } else if !app.hosts_state.multi_select().is_empty() {
                app.hosts_state.clear_multi_select();
            } else if !app.ui.esc_quit_hint_shown()
                && !app.status_center.toast().is_some_and(|t| t.sticky)
            {
                // Esc never quits the app. The first time a user presses Esc
                // on an idle host list we surface a one-shot toast pointing to
                // `q`, so accidental Esc presses discover the conventional
                // exit key. Skip the hint when a sticky toast is active so an
                // informational nudge cannot displace a sticky error the user
                // still needs to see; the flag stays unset so the hint will
                // surface on a later Esc once the sticky toast is dismissed.
                log::debug!("[purple] esc on idle host list, showing quit hint toast");
                app.notify(crate::i18n::text(crate::messages::ESC_QUIT_HINT));
                app.ui.set_esc_quit_hint_shown(true);
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next();
            super::ping::refresh_selected_if_stale(app, events_tx);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev();
            super::ping::refresh_selected_if_stale(app, events_tx);
        }
        KeyCode::Tab => {
            app.cycle_top_page_next();
            app.search.set_query(None);
            super::ping::refresh_selected_if_stale(app, events_tx);
        }
        KeyCode::BackTab => {
            app.cycle_top_page_prev();
            app.search.set_query(None);
            super::ping::refresh_selected_if_stale(app, events_tx);
        }
        KeyCode::PageDown => {
            app.page_down_host();
            super::ping::refresh_selected_if_stale(app, events_tx);
        }
        KeyCode::PageUp => {
            app.page_up_host();
            super::ping::refresh_selected_if_stale(app, events_tx);
        }
        KeyCode::Enter => {
            if app.is_pattern_selected() {
                return;
            }
            if let Some(host) = app.selected_host() {
                let alias = host.alias.clone();
                let askpass = host.askpass.clone();
                let stale_hint = super::host_form::stale_hint_for(host);
                if let Some(hint) = stale_hint {
                    app.notify_warning(crate::messages::stale_host(&hint));
                }
                if app.demo_mode {
                    app.notify_warning(crate::i18n::text(
                        crate::messages::DEMO_CONNECTION_DISABLED,
                    ));
                    return;
                }
                app.ui.queue_connect(alias, askpass);
            }
        }
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let visible_indices: Vec<usize> = app
                .hosts_state
                .display_list()
                .iter()
                .filter_map(|item| match item {
                    crate::app::HostListItem::Host { index } => Some(*index),
                    _ => None,
                })
                .collect();
            let all_selected = !visible_indices.is_empty()
                && visible_indices
                    .iter()
                    .all(|idx| app.hosts_state.multi_select().contains(idx));
            if all_selected {
                app.hosts_state.clear_multi_select();
            } else {
                for idx in visible_indices {
                    app.hosts_state.multi_select_mut().insert(idx);
                }
            }
        }
        KeyCode::Char('a') => {
            app.open_host_add_form();
        }
        KeyCode::Char('A') => {
            app.open_host_pattern_add_form();
        }
        KeyCode::Char('e') => {
            if let Some(pattern) = app.selected_pattern().cloned() {
                if pattern.source_file.is_some() {
                    app.notify_error(crate::messages::included_file_edit(&pattern.pattern));
                    return;
                }
                app.open_host_pattern_edit_form(&pattern);
            } else if let Some(host) = app.selected_host().cloned() {
                let hint = super::host_form::stale_hint_for(&host);
                app.open_host_edit_form(host, hint);
            }
        }
        KeyCode::Char('d') => {
            if let Some(pattern) = app.selected_pattern() {
                if pattern.source_file.is_some() {
                    app.notify_error(crate::messages::included_file_delete(&pattern.pattern));
                    return;
                }
                let alias = pattern.pattern.clone();
                app.set_screen(Screen::ConfirmDelete { alias });
            } else if let Some(host) = app.selected_host() {
                if let Some(ref source) = host.source_file {
                    let alias = host.alias.clone();
                    let path = source.display();
                    app.notify_warning(crate::messages::included_host_lives_in(&alias, &path));
                    return;
                }
                let stale_hint = super::host_form::stale_hint_for(host);
                let alias = host.alias.clone();
                if let Some(hint) = stale_hint {
                    app.notify_warning(crate::messages::stale_host(&hint));
                }
                app.set_screen(Screen::ConfirmDelete { alias });
            }
        }
        KeyCode::Char('c') => actions::clone_selected(app),
        KeyCode::Char('y') => {
            if app.is_pattern_selected() {
                return;
            }
            if let Some(host) = app.selected_host() {
                let launcher = crate::ssh_launcher::SshLauncher::resolve(&app.env);
                let cmd = host.ssh_command_with(
                    &launcher.shell_display(),
                    app.env.paths(),
                    app.reload.config_path(),
                );
                let alias = host.alias.clone();
                match clipboard::copy_to_clipboard(&cmd) {
                    Ok(()) => {
                        app.notify(crate::messages::copied_ssh_command(&alias));
                    }
                    Err(e) => {
                        app.notify_error(e);
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
                if let Some(block) = serialize_host_block(
                    &app.hosts_state.ssh_config().elements,
                    &alias,
                    app.hosts_state.ssh_config().crlf,
                ) {
                    match clipboard::copy_to_clipboard(&block) {
                        Ok(()) => {
                            app.notify(crate::messages::copied_config_block(&alias));
                        }
                        Err(e) => {
                            app.notify_error(e);
                        }
                    }
                }
            }
        }
        KeyCode::Char('p') => {
            if app.is_pattern_selected() {
                return;
            }
            if !app.ping.status_is_empty() {
                log::debug!(
                    "[purple] p: clearing {} ping result(s) + timestamps",
                    app.ping.status_len()
                );
                app.ping.clear_results();
                app.clear_status();
            } else {
                super::ping::ping_selected_host(app, events_tx, true);
            }
        }
        KeyCode::Char('P') => actions::ping_all_hosts(app, events_tx),
        KeyCode::Char('!') => actions::toggle_down_filter(app),
        KeyCode::Char('/') => {
            app.start_search();
        }
        KeyCode::Char('K') => {
            app.scan_keys();
            app.set_screen(Screen::KeyList);
        }
        KeyCode::Char('t') => {
            // Context-sensitive: with a multi-host selection active, open
            // the bulk tag editor. Otherwise fall back to the single-host
            // tag input bar. `t` consistently means "edit tags" — only the
            // scope changes.
            if !app.hosts_state.multi_select().is_empty() {
                if !app.open_bulk_tag_editor() {
                    app.notify_warning(crate::i18n::text(crate::messages::NO_HOSTS_TO_TAG));
                }
                return;
            }
            if app.is_pattern_selected() {
                return;
            }
            if let Some(host) = app.selected_host() {
                if let Some(ref source) = host.source_file {
                    let alias = host.alias.clone();
                    let path = source.display();
                    app.notify_error(crate::messages::included_host_tag_there(&alias, &path));
                    return;
                }
                let current_tags = host.tags.join(", ");
                app.tags.open_tag_input(current_tags);
            }
        }
        KeyCode::Char('s') => {
            app.hosts_state.advance_sort_mode();
            app.apply_sort();
            if let Err(e) =
                preferences::save_sort_mode(app.env().paths(), app.hosts_state.sort_mode())
            {
                app.notify_error(crate::messages::sorted_by_save_failed(
                    app.hosts_state.sort_mode().label(),
                    &e,
                ));
            } else {
                app.notify(crate::messages::sorted_by(
                    app.hosts_state.sort_mode().label(),
                ));
            }
        }
        KeyCode::Char('L') => match crate::i18n::toggle_for_session(!app.demo_mode) {
            Err(e) => app.notify_error(crate::messages::language::error(&e)),
            Ok(()) => app.notify(crate::messages::language::selected(
                if crate::i18n::is_chinese() {
                    "zh-CN"
                } else {
                    "en"
                },
            )),
        },
        KeyCode::Char('g') => actions::cycle_group_by(app),
        KeyCode::Char('i') => {
            if app.is_pattern_selected() {
                return;
            }
            if let Some(index) = app.selected_host_index() {
                app.set_screen(Screen::HostDetail { index });
            }
        }
        KeyCode::Char('v') => {
            app.hosts_state.toggle_view_mode();
            app.ui.set_detail_toggle_pending(true);
            app.ui.set_detail_scroll(0);
            if let Err(e) =
                preferences::save_view_mode(app.env().paths(), app.hosts_state.view_mode())
            {
                log::warn!("[config] Failed to persist view mode: {e}");
            }
        }
        KeyCode::Char(']') if app.hosts_state.view_mode() == ViewMode::Detailed => {
            app.ui
                .set_detail_scroll(app.ui.detail_scroll().saturating_add(1));
        }
        KeyCode::Char('[') if app.hosts_state.view_mode() == ViewMode::Detailed => {
            app.ui
                .set_detail_scroll(app.ui.detail_scroll().saturating_sub(1));
        }
        KeyCode::Char('u') => actions::undo_last(app),
        KeyCode::Char('#') => {
            app.open_tag_picker();
        }
        KeyCode::Char('m') => actions::open_theme_picker(app),
        KeyCode::Char('T') => {
            if app.is_pattern_selected() {
                return;
            }
            if let Some(host) = app.selected_host() {
                let stale_hint = super::host_form::stale_hint_for(host);
                let alias = host.alias.clone();
                if let Some(hint) = stale_hint {
                    app.notify_warning(crate::messages::stale_host(&hint));
                }
                app.refresh_tunnel_list(&alias);
                *app.ui.tunnel_list_state_mut() = ratatui::widgets::ListState::default();
                if !app.tunnels.list().is_empty() {
                    app.ui.tunnel_list_state_mut().select(Some(0));
                }
                app.set_screen(Screen::TunnelList { alias });
            }
        }
        KeyCode::Char('S') => {
            if !app.demo_mode {
                let cfg = crate::providers::config::ProviderConfig::load(app.env.paths());
                *app.providers.config_mut() = cfg;
            }
            *app.ui.provider_list_state_mut() = ratatui::widgets::ListState::default();
            app.ui.provider_list_state_mut().select(Some(0));
            app.set_screen(Screen::Providers);
        }
        KeyCode::Char('I') => {
            let count = crate::import::count_known_hosts_candidates(app.env().paths());
            if count > 0 {
                app.set_screen(Screen::ConfirmImport { count });
            } else {
                app.notify_warning(crate::i18n::text(crate::messages::NO_IMPORTABLE_HOSTS));
            }
        }
        KeyCode::Char('X') => {
            let stale = app.hosts_state.ssh_config().stale_hosts();
            if stale.is_empty() {
                app.notify_warning(crate::i18n::text(crate::messages::NO_STALE_HOSTS));
            } else {
                let aliases: Vec<String> = stale.into_iter().map(|(a, _)| a).collect();
                app.providers.set_pending_purge(crate::app::PendingPurge {
                    aliases,
                    provider: None,
                });
                app.set_screen(Screen::ConfirmPurgeStale);
            }
        }
        KeyCode::Char('V') => actions::initiate_bulk_vault_sign(app),
        // SPACE GUARD MUST PRECEDE the generic Char(c) arm below. Plain
        // Space and Ctrl+Space both toggle the multi-select set; without
        // this ordering the char arm would capture Space first.
        KeyCode::Char(' ') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app.is_pattern_selected() {
                return;
            }
            if let Some(idx) = app.selected_host_index() {
                app.hosts_state.toggle_multi_select(idx);
            }
        }
        KeyCode::Char(' ') => {
            // Plain Space mirrors Ctrl+Space so users familiar with
            // ranger/k9s/mutt's muscle memory get the same mark toggle
            // without a modifier. Ctrl+Space still works.
            if app.is_pattern_selected() {
                return;
            }
            if let Some(idx) = app.selected_host_index() {
                app.hosts_state.toggle_multi_select(idx);
            }
        }
        KeyCode::Char('r') => actions::run_snippet_on_selection(app),
        KeyCode::Char('R') => {
            if app.is_pattern_selected() {
                return;
            }
            let aliases: Vec<String> = app
                .hosts_state
                .display_list()
                .iter()
                .filter_map(|item| match item {
                    crate::app::HostListItem::Host { index } => {
                        Some(app.hosts_state.list()[*index].alias.clone())
                    }
                    _ => None,
                })
                .collect();
            if aliases.is_empty() {
                app.notify_warning(crate::i18n::text(crate::messages::NO_HOSTS_TO_RUN));
            } else {
                super::snippet::open_snippet_picker(app, aliases);
            }
        }
        KeyCode::Char(':') => {
            log::debug!("[purple] jump: opened from host list");
            app.open_jump(crate::app::JumpMode::Hosts);
        }
        KeyCode::Char('F') => actions::open_file_browser(app, events_tx),
        KeyCode::Char('C') => actions::open_container_overlay(app, events_tx),
        KeyCode::Char('n') if app.search.query().is_none() => {
            log::debug!("[purple] opening whats-new overlay via n");
            super::whats_new::dismiss_whats_new_toast(app);
            app.set_screen(Screen::WhatsNew(crate::app::WhatsNewState::default()));
        }
        KeyCode::Char('?') => {
            app.push_help_overlay();
        }
        _ => {}
    }
}

pub(super) fn handle_search_key(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    match key.code {
        KeyCode::Esc => {
            app.cancel_search();
        }
        KeyCode::Enter => {
            if let Some(host) = app.selected_host() {
                let alias = host.alias.clone();
                let askpass = host.askpass.clone();
                let stale_hint = super::host_form::stale_hint_for(host);
                app.cancel_search();
                if let Some(hint) = stale_hint {
                    app.notify_warning(crate::messages::stale_host(&hint));
                }
                if app.demo_mode {
                    app.notify_warning(crate::i18n::text(
                        crate::messages::DEMO_CONNECTION_DISABLED,
                    ));
                    return;
                }
                app.ui.queue_connect(alias, askpass);
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            app.select_next();
            super::ping::refresh_selected_if_stale(app, events_tx);
        }
        KeyCode::Up | KeyCode::BackTab => {
            app.select_prev();
            super::ping::refresh_selected_if_stale(app, events_tx);
        }
        KeyCode::PageDown => {
            app.page_down_host();
            super::ping::refresh_selected_if_stale(app, events_tx);
        }
        KeyCode::PageUp => {
            app.page_up_host();
            super::ping::refresh_selected_if_stale(app, events_tx);
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if !app.ping.status_is_empty() {
                log::debug!(
                    "[purple] ctrl+p: clearing {} ping result(s) + timestamps",
                    app.ping.status_len()
                );
                let was_filtering = app.ping.filter_down_only();
                app.ping.clear_results();
                if was_filtering {
                    app.cancel_search();
                }
                app.clear_status();
            } else {
                super::ping::ping_selected_host(app, events_tx, false);
            }
        }
        KeyCode::Char(' ') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(idx) = app.selected_host_index() {
                app.hosts_state.toggle_multi_select(idx);
            }
        }
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let visible_indices: Vec<usize> = app.search.filtered_indices().to_vec();
            let all_selected = !visible_indices.is_empty()
                && visible_indices
                    .iter()
                    .all(|idx| app.hosts_state.multi_select().contains(idx));
            if all_selected {
                app.hosts_state.clear_multi_select();
            } else {
                for idx in visible_indices {
                    app.hosts_state.multi_select_mut().insert(idx);
                }
            }
        }
        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(host) = app.selected_host().cloned() {
                let hint = super::host_form::stale_hint_for(&host);
                app.open_host_edit_form(host, hint);
            }
        }
        KeyCode::Char('!') if app.ping.filter_down_only() => {
            app.ping.set_filter_down_only(false);
            if app.search.query().is_some_and(|q| q.is_empty()) {
                app.cancel_search();
            } else {
                app.apply_filter();
            }
            app.clear_status();
        }
        KeyCode::Char(c) => {
            app.search.push_query_char(c);
            app.apply_filter();
        }
        KeyCode::Backspace => {
            app.search.pop_query_char();
            app.apply_filter();
        }
        _ => {}
    }
}
