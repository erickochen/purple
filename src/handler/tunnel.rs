use crossterm::event::{KeyCode, KeyEvent};
use log::{debug, info};

use super::ctx::{Effectful, Effects, Nav, Notify};
use crate::app::{
    App, ConflictState, FormState, HostState, Screen, StatusCenter, TopPage, TunnelState,
    UiSelection,
};
use crate::history::ConnectionHistory;

/// A narrow, explicit borrow of the App state the tunnel handlers touch. The
/// handlers operate on this slice instead of `&mut App`, so the compiler
/// rejects any reach into unrelated state (vault, containers, providers, ...).
/// Whole-App operations that cannot run on a slice (reload, sort, the
/// tunnel-form helpers shared with four other handlers) are deferred as
/// `Followup`s and applied to the full `App` after the handler returns.
///
/// Shared with the Tunnels-tab overview (`tunnels_overview`) so the start,
/// stop and delete actions have a single implementation instead of being
/// duplicated across the per-host overlay and the tab.
pub(super) struct TunnelCtx<'a> {
    tunnels: &'a mut TunnelState,
    hosts: &'a mut HostState,
    ui: &'a mut UiSelection,
    status: &'a mut StatusCenter,
    forms: &'a mut FormState,
    conflict: &'a mut ConflictState,
    history: &'a mut ConnectionHistory,
    screen: &'a mut Screen,
    demo_mode: bool,
    top_page: TopPage,
    bw_session: Option<&'a str>,
    /// Passwords typed in the TUI this session, read so a tunnel start
    /// reuses the one the user already gave for that host.
    session_passwords: &'a crate::app::SessionPasswords,
    /// Security key PINs typed this session. Only read when
    /// `security_key_allowed` says the installed OpenSSH may receive them.
    security_key_pins: &'a crate::app::SecurityKeyPins,
    security_key_allowed: bool,
    paths: Option<&'a crate::runtime::env::Paths>,
    config_path: &'a std::path::Path,
    pub(super) effects: Effects,
}

impl Nav for TunnelCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

impl Notify for TunnelCtx<'_> {
    fn status_mut(&mut self) -> &mut StatusCenter {
        self.status
    }
}

impl Effectful for TunnelCtx<'_> {
    fn effects_mut(&mut self) -> &mut Effects {
        &mut self.effects
    }
}

impl TunnelCtx<'_> {
    /// Reload this host's tunnel directives into the list. Mirrors
    /// `App::refresh_tunnel_list` on the slice.
    fn refresh_tunnel_list(&mut self, alias: &str) {
        self.tunnels.load_directives(self.hosts.ssh_config(), alias);
    }

    /// Tear down tunnel form state and return to the caller's screen. Mirrors
    /// `App::close_tunnel_form` on the slice.
    fn close_tunnel_form(&mut self, return_to: Screen) {
        log::debug!(
            "[purple] close_tunnel_form return_to={:?}",
            std::mem::discriminant(&return_to)
        );
        self.conflict.clear_form_mtimes();
        self.tunnels.set_form_baseline(None);
        self.set_screen(return_to);
    }

    /// True when an external edit changed the config since the form opened.
    fn config_changed_since_form_open(&self) -> bool {
        crate::app::config_changed(self.conflict, self.config_path)
    }

    /// Clamp the tunnel-list selection after the list length changed.
    fn fix_selection_after_list_change(&mut self) {
        if self.tunnels.list().is_empty() {
            self.ui.tunnel_list_state_mut().select(None);
        } else if let Some(sel) = self.ui.tunnel_list_state().selected() {
            if sel >= self.tunnels.list().len() {
                self.ui
                    .tunnel_list_state_mut()
                    .select(Some(self.tunnels.list().len() - 1));
            }
        } else {
            self.ui.tunnel_list_state_mut().select(Some(0));
        }
    }

    // --- Shared tunnel actions (used by both the per-host overlay and the
    // Tunnels tab). Each keeps its own guards (demo, empty list) and cursor
    // handling at the call site; only the state-mutating core lives here.

    /// Stop the active tunnel for `alias`. Returns true if one was stopped.
    /// Defers the bind-port refresh; the caller fixes its own cursor after.
    pub(super) fn stop_active_tunnel(&mut self, alias: &str) -> bool {
        let Some(mut tunnel) = self.tunnels.active_remove(alias) else {
            return false;
        };
        if let Err(e) = tunnel.child.kill() {
            debug!("[external] Failed to kill tunnel process for {alias}: {e}");
        }
        let _ = tunnel.child.wait();
        drop(tunnel);
        debug!("[purple] tunnel stopped: alias={alias}");
        let stopped = alias.to_string();
        self.defer(move |app| {
            app.abandon_security_key_trial(crate::security_key::PinFlow::Tunnel, Some(&stopped));
        });
        self.defer(|app| app.refresh_tunnel_bind_ports());
        self.notify(crate::messages::tunnel_stopped(alias));
        true
    }

    /// Spawn the SSH process for `alias` and register it as an active tunnel.
    /// No demo or empty-list guard; the caller decides those (their order
    /// differs between the overlay and the tab). Returns true on success.
    pub(super) fn spawn_active_tunnel(&mut self, alias: &str) -> bool {
        let host = self.hosts.list().iter().find(|h| h.alias == alias);
        let askpass = host.and_then(|h| h.askpass.clone());
        // Behind a jump host the bastion authenticates first, so only the
        // line naming the target counts. ssh prints the host lowercased.
        let target_host = host
            .filter(|h| !h.proxy_jump.trim().is_empty() || h.has_proxy_command)
            .map(|h| {
                let name = match h.hostname.trim() {
                    "" => alias,
                    hostname => hostname,
                };
                name.to_ascii_lowercase()
            });
        let rules = self.hosts.ssh_config().find_tunnel_directives(alias);
        let session_password = self.session_passwords.get(alias).map(String::as_str);
        let security_key = if self.security_key_allowed {
            self.security_key_pins
                .run_for(self.paths, alias, crate::security_key::PinFlow::Tunnel)
        } else {
            crate::security_key::SecurityKeyRun::default()
        };
        match crate::tunnel::start_tunnel(
            alias,
            self.config_path,
            askpass.as_deref(),
            session_password,
            self.bw_session,
            &security_key,
        ) {
            Ok(child) => {
                for rule in &rules {
                    info!(
                        "[external] Tunnel started: type={} local={} remote={}:{} alias={alias}",
                        rule.tunnel_type.label(),
                        rule.bind_port,
                        rule.remote_host,
                        rule.remote_port
                    );
                }
                self.tunnels.ensure_lsof_poller();
                let parser_tx = self.tunnels.parser_tx();
                let watch = crate::tunnel_live::AuthWatch {
                    target_host,
                    security_key,
                    paths: self.paths.cloned(),
                };
                let active = crate::tunnel::ActiveTunnel::spawn(child, alias, parser_tx, watch);
                self.tunnels.active_insert(alias.to_string(), active);
                self.defer(|app| app.refresh_tunnel_bind_ports());
                // Tunnel start spawns a real ssh session, same as a shell
                // connect, so record it in connection history.
                self.history.record(alias);
                self.record_key_use(alias);
                self.apply_sort();
                self.notify(crate::messages::tunnel_started(alias));
                true
            }
            Err(e) => {
                log::error!("[external] tunnel start failed: alias={alias}: {e}");
                self.notify_error(crate::messages::tunnel_start_failed(&e));
                false
            }
        }
    }

    /// Remove a forward directive from `alias` and persist, rolling back on a
    /// write error. Notifies on failure. On success defers the mtime refresh
    /// and host reload; the caller refreshes its own list/cursor and notifies
    /// the removal. Returns true on success.
    pub(super) fn remove_forward_tx(&mut self, alias: &str, key: &str, value: &str) -> bool {
        let config_backup = self.hosts.ssh_config().clone();
        if !self
            .hosts
            .ssh_config_mut()
            .remove_forward(alias, key, value)
        {
            debug!(
                "[purple] tunnel forward removal skipped: alias={alias} {key} {value} not found"
            );
            self.notify_warning(crate::i18n::text(crate::messages::TUNNEL_NOT_FOUND));
            return false;
        }
        if let Err(e) = self.hosts.ssh_config().write() {
            self.hosts.set_ssh_config(config_backup);
            log::warn!(
                "[config] failed to save tunnel forward removal: alias={alias} {key} {value}: {e}"
            );
            self.notify_error(crate::messages::failed_to_save(&e));
            return false;
        }
        debug!("[purple] tunnel forward removed: alias={alias} {key} {value}");
        self.update_last_modified();
        self.reload_hosts();
        true
    }
}

/// Where the tunnel form returns to on submit, cancel, or discard. When the
/// user opened the form from the Tunnels-tab overview we hop back to that
/// overview (Screen::HostList — the overview shares the screen variant with
/// the host list and is selected by `top_page`); otherwise we return to the
/// per-host TunnelList overlay.
fn tunnel_form_return_screen(top_page: TopPage, alias: &str) -> Screen {
    if matches!(top_page, TopPage::Tunnels) {
        Screen::HostList
    } else {
        Screen::TunnelList {
            alias: alias.to_string(),
        }
    }
}

pub(super) fn handle_tunnel_list_key(app: &mut App, key: KeyEvent) {
    let effects = {
        let mut ctx = ctx_from_app(app);
        tunnel_list_key(&mut ctx, key);
        ctx.effects
    };
    effects.apply(app);
}

pub(super) fn handle_tunnel_form_key(app: &mut App, key: KeyEvent) {
    let effects = {
        let mut ctx = ctx_from_app(app);
        tunnel_form_key(&mut ctx, key);
        ctx.effects
    };
    effects.apply(app);
}

/// Borrow the disjoint App fields the tunnel handlers need into one slice.
pub(super) fn ctx_from_app(app: &mut App) -> TunnelCtx<'_> {
    let security_key_allowed = app.security_key_pins_allowed();
    TunnelCtx {
        tunnels: &mut app.tunnels,
        hosts: &mut app.hosts_state,
        ui: &mut app.ui,
        status: &mut app.status_center,
        forms: &mut app.forms,
        conflict: &mut app.conflict,
        history: &mut app.history,
        screen: &mut app.screen,
        demo_mode: app.demo_mode,
        top_page: app.top_page,
        bw_session: app.bw_session.as_deref(),
        session_passwords: &app.session_passwords,
        security_key_pins: &app.security_key_pins,
        security_key_allowed,
        paths: app.env.paths(),
        config_path: app.reload.config_path(),
        effects: Effects::default(),
    }
}

fn tunnel_list_key(ctx: &mut TunnelCtx, key: KeyEvent) {
    let alias = match &*ctx.screen {
        Screen::TunnelList { alias } => alias.clone(),
        _ => return,
    };

    // Handle pending tunnel delete confirmation first via the central
    // confirm-key router so the y/n/Esc contract is uniform across all
    // confirm dialogs. Stray keys (including `_ => {}`) must not silently
    // cancel destructive operations; route_confirm_key narrows to y/Y/n/N/Esc.
    if ctx.tunnels.pending_delete().is_some() && key.code != KeyCode::Char('?') {
        match super::route_confirm_key(key) {
            super::ConfirmAction::Yes => {
                let Some(sel) = ctx.tunnels.take_pending_delete() else {
                    return;
                };
                let Some((directive_key, value)) = ctx.tunnels.list().get(sel).map(|rule| {
                    (
                        rule.tunnel_type.directive_key().to_string(),
                        rule.to_directive_value(),
                    )
                }) else {
                    return;
                };
                if ctx.remove_forward_tx(&alias, &directive_key, &value) {
                    ctx.refresh_tunnel_list(&alias);
                    if ctx.tunnels.list().is_empty() {
                        ctx.ui.tunnel_list_state_mut().select(None);
                    } else if sel >= ctx.tunnels.list().len() {
                        ctx.ui
                            .tunnel_list_state_mut()
                            .select(Some(ctx.tunnels.list().len() - 1));
                    }
                    ctx.notify(crate::i18n::text(crate::messages::TUNNEL_REMOVED));
                }
            }
            super::ConfirmAction::No => {
                ctx.tunnels.cancel_delete();
            }
            super::ConfirmAction::Ignored => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            ctx.set_screen(Screen::HostList);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            crate::app::cycle_selection(
                ctx.ui.tunnel_list_state_mut(),
                ctx.tunnels.list().len(),
                true,
            );
        }
        KeyCode::Char('k') | KeyCode::Up => {
            crate::app::cycle_selection(
                ctx.ui.tunnel_list_state_mut(),
                ctx.tunnels.list().len(),
                false,
            );
        }
        KeyCode::PageDown => {
            crate::app::page_down(ctx.ui.tunnel_list_state_mut(), ctx.tunnels.list().len(), 10);
        }
        KeyCode::PageUp => {
            crate::app::page_up(ctx.ui.tunnel_list_state_mut(), ctx.tunnels.list().len(), 10);
        }
        KeyCode::Char('a') => {
            // Check if host is from an included file (read-only)
            if host_is_read_only(ctx, &alias) {
                ctx.notify_warning(crate::i18n::text(
                    crate::messages::TUNNEL_INCLUDED_READ_ONLY,
                ));
                return;
            }
            let alias = alias.clone();
            ctx.effects
                .defer(move |app| app.open_tunnel_add_form(alias));
        }
        KeyCode::Char('e') => {
            if host_is_read_only(ctx, &alias) {
                ctx.notify_warning(crate::i18n::text(
                    crate::messages::TUNNEL_INCLUDED_READ_ONLY,
                ));
                return;
            }
            if let Some(sel) = ctx.ui.tunnel_list_state().selected()
                && let Some(rule) = ctx.tunnels.list().get(sel).cloned()
            {
                let alias = alias.clone();
                ctx.effects
                    .defer(move |app| app.open_tunnel_edit_form(alias, &rule, sel));
            }
        }
        KeyCode::Char('d') => {
            if host_is_read_only(ctx, &alias) {
                ctx.notify_warning(crate::i18n::text(
                    crate::messages::TUNNEL_INCLUDED_READ_ONLY,
                ));
                return;
            }
            if let Some(sel) = ctx.ui.tunnel_list_state().selected()
                && sel < ctx.tunnels.list().len()
            {
                ctx.tunnels.request_delete(sel);
            }
        }
        KeyCode::Enter => {
            // Start/stop tunnel. The per-host overlay guards empty-list first
            // (an empty list is a silent no-op), then demo mode.
            if ctx.tunnels.active_contains(&alias) {
                ctx.stop_active_tunnel(&alias);
            } else if !ctx.tunnels.list().is_empty() {
                if ctx.demo_mode {
                    ctx.notify_warning(crate::i18n::text(crate::messages::DEMO_TUNNELS_DISABLED));
                    return;
                }
                ctx.spawn_active_tunnel(&alias);
            }
        }
        KeyCode::Char('?') => {
            ctx.push_help_overlay();
        }
        _ => {}
    }
}

/// True if `alias` resolves to a host defined in an Include'd (read-only) file.
fn host_is_read_only(ctx: &TunnelCtx, alias: &str) -> bool {
    ctx.hosts
        .list()
        .iter()
        .find(|h| h.alias == alias)
        .is_some_and(|h| h.source_file.is_some())
}

fn tunnel_form_key(ctx: &mut TunnelCtx, key: KeyEvent) {
    let (alias, editing) = match &*ctx.screen {
        Screen::TunnelForm { alias, editing } => (alias.clone(), *editing),
        _ => return,
    };

    // Handle discard confirmation dialog via the shared confirm router.
    if ctx.forms.is_discard_pending() {
        match super::route_confirm_key(key) {
            super::ConfirmAction::Yes => {
                ctx.forms.dismiss_discard_confirm();
                let return_to = tunnel_form_return_screen(ctx.top_page, &alias);
                ctx.close_tunnel_form(return_to);
            }
            super::ConfirmAction::No => {
                ctx.forms.dismiss_discard_confirm();
            }
            super::ConfirmAction::Ignored => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc => {
            if ctx.tunnels.form_is_dirty() {
                ctx.forms.request_discard_confirm();
            } else {
                let return_to = tunnel_form_return_screen(ctx.top_page, &alias);
                ctx.close_tunnel_form(return_to);
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            ctx.tunnels.form_mut().focus_next();
        }
        KeyCode::BackTab | KeyCode::Up => {
            ctx.tunnels.form_mut().focus_prev();
        }
        KeyCode::Left if ctx.tunnels.form_mut().cursor_pos > 0 => {
            ctx.tunnels.form_mut().cursor_pos -= 1;
        }
        KeyCode::Right => {
            let len = ctx
                .tunnels
                .form()
                .focused_value()
                .map(|v| v.chars().count())
                .unwrap_or(0);
            if ctx.tunnels.form_mut().cursor_pos < len {
                ctx.tunnels.form_mut().cursor_pos += 1;
            }
        }
        KeyCode::Home => {
            ctx.tunnels.form_mut().cursor_pos = 0;
        }
        KeyCode::End => {
            ctx.tunnels.form_mut().sync_cursor_to_end();
        }
        KeyCode::Enter => {
            submit_tunnel_form(ctx, &alias, editing);
        }
        // SPACE GUARD MUST PRECEDE the generic Char(c) arm below so that
        // Space on a Type field cycles tunnel kind instead of being
        // captured as a literal space character.
        KeyCode::Char(' ')
            if ctx.tunnels.form_mut().focused_field == crate::app::TunnelFormField::Type =>
        {
            ctx.tunnels.form_mut().tunnel_type = ctx.tunnels.form_mut().tunnel_type.next();
        }
        KeyCode::Char(c) => {
            ctx.tunnels.form_mut().insert_char(c);
        }
        KeyCode::Backspace => {
            ctx.tunnels.form_mut().delete_char_before_cursor();
        }
        _ => {}
    }
}

fn submit_tunnel_form(ctx: &mut TunnelCtx, alias: &str, editing: Option<usize>) {
    // Check for external config changes since form was opened
    if ctx.config_changed_since_form_open() {
        ctx.notify_warning(crate::i18n::text(
            crate::messages::CONFIG_CHANGED_EXTERNALLY,
        ));
        return;
    }

    if let Err(msg) = ctx.tunnels.form_mut().validate() {
        ctx.notify_error(msg);
        return;
    }

    let (directive_key, directive_value) = ctx.tunnels.form_mut().to_directive();
    let config_backup = ctx.hosts.ssh_config().clone();

    // If editing, remove the old directive first
    if let Some(idx) = editing {
        if let Some(old_rule) = ctx.tunnels.list().get(idx) {
            let old_key = old_rule.tunnel_type.directive_key().to_string();
            let old_value = old_rule.to_directive_value();
            if !ctx
                .hosts
                .ssh_config_mut()
                .remove_forward(alias, &old_key, &old_value)
            {
                ctx.hosts.set_ssh_config(config_backup);
                ctx.notify_warning(crate::i18n::text(
                    crate::messages::TUNNEL_ORIGINAL_NOT_FOUND,
                ));
                return;
            }
        } else {
            // Index out of bounds (external config change) — abort
            ctx.notify_warning(crate::i18n::text(crate::messages::TUNNEL_LIST_CHANGED));
            return;
        }
    }

    // Duplicate detection (runs after old directive removal for edits)
    if ctx
        .hosts
        .ssh_config()
        .has_forward(alias, directive_key, &directive_value)
    {
        ctx.hosts.set_ssh_config(config_backup);
        ctx.notify_warning(crate::i18n::text(crate::messages::TUNNEL_DUPLICATE));
        return;
    }

    ctx.hosts
        .ssh_config_mut()
        .add_forward(alias, directive_key, &directive_value);
    if let Err(e) = ctx.hosts.ssh_config().write() {
        ctx.hosts.set_ssh_config(config_backup);
        log::warn!(
            "[config] failed to save tunnel forward: alias={alias} {directive_key} {directive_value}: {e}"
        );
        ctx.notify_error(crate::messages::failed_to_save(&e));
        return;
    }
    if editing.is_some() {
        debug!("[purple] tunnel forward edited: alias={alias} {directive_key} {directive_value}");
    } else {
        debug!("[purple] tunnel forward added: alias={alias} {directive_key} {directive_value}");
    }

    ctx.hosts.clear_undo(); // Clear undo buffer. Positions may have shifted.
    ctx.update_last_modified();
    ctx.refresh_tunnel_list(alias);
    ctx.reload_hosts();
    ctx.fix_selection_after_list_change();
    ctx.notify(crate::i18n::text(crate::messages::TUNNEL_SAVED));
    let return_to = tunnel_form_return_screen(ctx.top_page, alias);
    ctx.close_tunnel_form(return_to);
}
