//! Key handler for the global Tunnels-tab overview (top_page = Tunnels).
//!
//! Supports navigation, start/stop, and add/edit/delete. Adding from this
//! screen routes through the host picker first because the user has not yet
//! chosen which host the new tunnel belongs to.
//!
//! Takes `&mut App`, not a per-domain slice: it is a tab-router that switches
//! top-pages, opens the jump overlay and delegates tunnel mutations to the
//! shared tunnel-action core (`handler::tunnel`). The row query it shares with
//! the renderer (`ui::tunnels_overview::visible_pairs`) takes slice references,
//! not `App`.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Screen};
use crate::tunnel::TunnelRule;

/// Resolve the tunnel row currently under the cursor. Walks the SAME
/// filtered + sorted sequence the UI renders so the cursor always points
/// at the row visually under it.
fn selected_row(app: &App) -> Option<(String, TunnelRule)> {
    let sel = app.ui.tunnels_overview_state().selected()?;
    crate::ui::tunnels_overview::visible_pairs(
        &app.search,
        &app.hosts_state,
        &app.tunnels,
        &app.history,
    )
    .into_iter()
    .nth(sel)
}

/// Total visible row count; used for cursor clamping.
fn row_count(app: &App) -> usize {
    crate::ui::tunnels_overview::visible_pairs(
        &app.search,
        &app.hosts_state,
        &app.tunnels,
        &app.history,
    )
    .len()
}

fn select_next(app: &mut App) {
    let total = row_count(app);
    if total == 0 {
        app.ui.tunnels_overview_state_mut().select(None);
        return;
    }
    let cur = app.ui.tunnels_overview_state().selected().unwrap_or(0);
    let next = if cur + 1 >= total { 0 } else { cur + 1 };
    app.ui.tunnels_overview_state_mut().select(Some(next));
}

fn select_prev(app: &mut App) {
    let total = row_count(app);
    if total == 0 {
        app.ui.tunnels_overview_state_mut().select(None);
        return;
    }
    let cur = app.ui.tunnels_overview_state().selected().unwrap_or(0);
    let prev = if cur == 0 { total - 1 } else { cur - 1 };
    app.ui.tunnels_overview_state_mut().select(Some(prev));
}

fn toggle_tunnel(app: &mut App) {
    let Some((alias, rule)) = selected_row(app) else {
        return;
    };
    // Start/stop runs through the shared tunnel-action core (see
    // `handler::tunnel`). This screen keeps its own demo + empty guards and
    // its own cursor re-anchoring; only the state mutation is shared.
    if app.tunnels.active_contains(&alias) {
        let (stopped, effects) = {
            let mut ctx = super::tunnel::ctx_from_app(app);
            let stopped = ctx.stop_active_tunnel(&alias);
            (stopped, ctx.effects)
        };
        effects.apply(app);
        if stopped {
            reposition_cursor_on(app, &alias, &rule);
        }
        return;
    }
    if app.demo_mode {
        app.notify_warning(crate::i18n::text(crate::messages::DEMO_TUNNELS_DISABLED));
        return;
    }
    if app
        .hosts_state
        .ssh_config()
        .find_tunnel_directives(&alias)
        .is_empty()
    {
        return;
    }
    let (started, effects) = {
        let mut ctx = super::tunnel::ctx_from_app(app);
        let started = ctx.spawn_active_tunnel(&alias);
        (started, ctx.effects)
    };
    effects.apply(app);
    if started {
        reposition_cursor_on(app, &alias, &rule);
    }
}

/// After an action that may reorder the visible row list (toggle tunnel
/// changes the MostRecent sort key), park the cursor on the same logical
/// row the user acted on. Falls back to clamping into range if the row
/// vanished (e.g. directive removed concurrently).
pub(super) fn reposition_cursor_on(app: &mut App, alias: &str, rule: &TunnelRule) {
    let pairs = crate::ui::tunnels_overview::visible_pairs(
        &app.search,
        &app.hosts_state,
        &app.tunnels,
        &app.history,
    );
    if pairs.is_empty() {
        app.ui.tunnels_overview_state_mut().select(None);
        return;
    }
    let new_idx = pairs
        .iter()
        .position(|(a, r)| a == alias && r == rule)
        .unwrap_or(0)
        .min(pairs.len() - 1);
    app.ui.tunnels_overview_state_mut().select(Some(new_idx));
}

/// True when the host under the cursor lives in an included config file
/// and is therefore read-only from purple's perspective.
fn selected_row_is_included(app: &App) -> bool {
    let Some((alias, _)) = selected_row(app) else {
        return false;
    };
    app.hosts_state
        .list()
        .iter()
        .find(|h| h.alias == alias)
        .map(|h| h.source_file.is_some())
        .unwrap_or(false)
}

/// Confirm-and-remove the tunnel currently under the cursor. Mirrors the
/// per-host TunnelList delete path so behaviour is identical regardless of
/// which screen launched the deletion.
fn confirm_delete_selected(app: &mut App) {
    let Some(sel) = app.tunnels.take_pending_delete() else {
        return;
    };
    // Resolve the host + rule pair anew: the data is rebuilt every frame so
    // the index stored in `pending_delete` was captured against the same
    // ordering used by `selected_row()`.
    let Some((alias, rule)) = nth_row(app, sel) else {
        app.notify_warning(crate::i18n::text(crate::messages::TUNNEL_NOT_FOUND));
        return;
    };
    let directive_key = rule.tunnel_type.directive_key().to_string();
    let directive_value = rule.to_directive_value();
    // Removal + persist + reload runs through the shared tunnel-action core;
    // this screen keeps its own row_count-based cursor clamp.
    let (removed, effects) = {
        let mut ctx = super::tunnel::ctx_from_app(app);
        let removed = ctx.remove_forward_tx(&alias, &directive_key, &directive_value);
        (removed, ctx.effects)
    };
    effects.apply(app);
    if !removed {
        return;
    }
    // Clamp cursor: row count shrinks by 1, so the previous selection may
    // sit past the new end.
    let total = row_count(app);
    if total == 0 {
        app.ui.tunnels_overview_state_mut().select(None);
    } else if sel >= total {
        app.ui.tunnels_overview_state_mut().select(Some(total - 1));
    }
    app.notify(crate::i18n::text(crate::messages::TUNNEL_REMOVED));
}

/// Resolve the (alias, rule) pair at `target` using the same filtered +
/// sorted sequence the UI renders. Cursor indices captured in
/// `pending_delete` are walked against this list on confirmation.
fn nth_row(app: &App, target: usize) -> Option<(String, TunnelRule)> {
    crate::ui::tunnels_overview::visible_pairs(
        &app.search,
        &app.hosts_state,
        &app.tunnels,
        &app.history,
    )
    .into_iter()
    .nth(target)
}

/// Handle a key event while the user is on the Tunnels tab. Caller is
/// responsible for confirming `app.top_page == TopPage::Tunnels`.
pub(super) fn handle_key(app: &mut App, key: KeyEvent) {
    // Pending delete confirmation is exclusive: only y/n/Esc are handled,
    // every other key is ignored so a stray press cannot silently cancel.
    if app.tunnels.pending_delete().is_some() {
        match super::route_confirm_key(key) {
            super::ConfirmAction::Yes => confirm_delete_selected(app),
            super::ConfirmAction::No => {
                app.tunnels.cancel_delete();
            }
            super::ConfirmAction::Ignored => {}
        }
        return;
    }

    // Search-mode intercept: while a query is being typed, navigation keys
    // are forwarded into the query buffer rather than moving the cursor.
    if app.search.query().is_some() {
        handle_search_keys(app, key);
        return;
    }

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => select_next(app),
        KeyCode::Char('k') | KeyCode::Up => select_prev(app),
        KeyCode::PageDown => {
            for _ in 0..10 {
                select_next(app);
            }
        }
        KeyCode::PageUp => {
            for _ in 0..10 {
                select_prev(app);
            }
        }
        KeyCode::Char('g') if row_count(app) > 0 => {
            app.ui.tunnels_overview_state_mut().select(Some(0));
        }
        KeyCode::Char('G') => {
            let total = row_count(app);
            if total > 0 {
                app.ui.tunnels_overview_state_mut().select(Some(total - 1));
            }
        }
        KeyCode::Char('/') => {
            // Enter search mode. Stays on the Tunnels tab; filtering
            // happens at row-build time in ui::tunnels_overview.
            app.search.set_query(Some(String::new()));
            app.ui.tunnels_overview_state_mut().select(Some(0));
        }
        KeyCode::Char('s') => {
            // Capture the row under the cursor BEFORE cycling so we can
            // re-anchor on it after sort. Resetting to index 0 makes the
            // detail panel flap open/closed when the new row-0 happens to
            // have a different active state than the previously-selected
            // one.
            let pinned = selected_row(app);
            app.tunnels.set_sort_mode(app.tunnels.sort_mode().next());
            match pinned {
                Some((alias, rule)) => reposition_cursor_on(app, &alias, &rule),
                None => app.ui.tunnels_overview_state_mut().select(Some(0)),
            }
            app.notify(crate::messages::sorted_by(app.tunnels.sort_mode().label()));
        }
        KeyCode::Char(':') => {
            log::debug!("[purple] jump: opened from tunnels overview");
            app.open_jump(crate::app::JumpMode::Tunnels);
        }
        KeyCode::Tab => {
            app.cycle_top_page_next();
            app.search.set_query(None);
        }
        KeyCode::BackTab => {
            app.cycle_top_page_prev();
            app.search.set_query(None);
        }
        KeyCode::Char('a') => {
            // Add: route through host picker — the user has not yet picked
            // which host the tunnel belongs to.
            let editable = super::tunnel_host_picker::editable_aliases(app);
            if editable.is_empty() {
                app.notify_warning(crate::i18n::text(crate::messages::TUNNEL_NO_EDITABLE_HOSTS));
                return;
            }
            app.ui.tunnel_host_picker_state_mut().select(Some(0));
            app.ui.tunnel_host_picker_query_mut().clear();
            app.set_screen(Screen::TunnelHostPicker);
        }
        KeyCode::Char('e') => {
            let Some((alias, rule)) = selected_row(app) else {
                return;
            };
            if selected_row_is_included(app) {
                app.notify_warning(crate::i18n::text(
                    crate::messages::TUNNEL_INCLUDED_READ_ONLY,
                ));
                return;
            }
            // The TunnelForm uses an `editing: Option<usize>` index into
            // `app.tunnels.list()` (the per-host tunnel list). Refresh that
            // list for the chosen host and resolve the matching index so
            // the form's edit/save path operates on the right rule.
            app.refresh_tunnel_list(&alias);
            let editing_idx = app.tunnels.list().iter().position(|r| r == &rule);
            let Some(idx) = editing_idx else {
                app.notify_warning(crate::i18n::text(crate::messages::TUNNEL_NOT_FOUND));
                return;
            };
            app.open_tunnel_edit_form(alias, &rule, idx);
        }
        KeyCode::Char('d') => {
            let Some(sel) = app.ui.tunnels_overview_state().selected() else {
                return;
            };
            if selected_row_is_included(app) {
                app.notify_warning(crate::i18n::text(
                    crate::messages::TUNNEL_INCLUDED_READ_ONLY,
                ));
                return;
            }
            if sel < row_count(app) {
                app.tunnels.request_delete(sel);
            }
        }
        KeyCode::Enter => toggle_tunnel(app),
        KeyCode::Char('n') => {
            // Same as host-list: dismiss the upgrade toast and open the
            // What's New overlay so release notes / changelog are
            // reachable from either main tab.
            super::whats_new::dismiss_whats_new_toast(app);
            app.set_screen(Screen::WhatsNew(crate::app::WhatsNewState::default()));
        }
        KeyCode::Char('?') => {
            // Open the help overlay; pressing ? again or Esc returns here.
            app.set_screen(Screen::Help {
                return_screen: Box::new(Screen::HostList),
            });
        }
        KeyCode::Char('q') => {
            app.running = false;
        }
        // Mirrors host-list policy: idle Esc never quits. The first idle press
        // surfaces a one-shot toast pointing to `q`; the flag is shared with the
        // host list so the hint shows at most once per session across both tabs.
        // The guard also skips the hint when a sticky toast is active so an
        // informational nudge cannot displace a sticky error. Subsequent idle
        // Esc presses (or Esc while a sticky is up) fall through to the no-op
        // arm below.
        KeyCode::Esc
            if !app.ui.esc_quit_hint_shown()
                && !app.status_center.toast().is_some_and(|t| t.sticky) =>
        {
            log::debug!("[purple] esc on idle tunnels overview, showing quit hint toast");
            app.notify(crate::i18n::text(crate::messages::ESC_QUIT_HINT));
            app.ui.set_esc_quit_hint_shown(true);
        }
        _ => {}
    }
}

/// Sub-handler used while the search query is being edited. Mirrors
/// the host-list search rhythm: navigation keys move the cursor
/// through the filtered set without leaving input mode, and `Enter`
/// confirms by acting on the highlighted row (start/stop) instead of
/// just dismissing the input. That keeps the muscle memory consistent
/// across the two main tabs — type, navigate, act, all without
/// leaving the search bar.
fn handle_search_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.search.set_query(None);
            app.ui.tunnels_overview_state_mut().select(Some(0));
        }
        KeyCode::Enter => {
            // Act on the highlighted row and dismiss the input. Re-
            // pressing `/` re-opens with an empty query.
            app.search.set_query(None);
            toggle_tunnel(app);
        }
        KeyCode::Down | KeyCode::Tab => select_next(app),
        KeyCode::Up | KeyCode::BackTab => select_prev(app),
        KeyCode::PageDown => {
            for _ in 0..10 {
                select_next(app);
            }
        }
        KeyCode::PageUp => {
            for _ in 0..10 {
                select_prev(app);
            }
        }
        KeyCode::Backspace => {
            app.search.pop_query_char();
            app.ui.tunnels_overview_state_mut().select(Some(0));
        }
        KeyCode::Char(c) => {
            app.search.push_query_char(c);
            app.ui.tunnels_overview_state_mut().select(Some(0));
        }
        _ => {}
    }
}
