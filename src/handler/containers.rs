use std::path::Path;
use std::sync::mpsc;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

use super::ctx::{Nav, Notify};
use crate::app::{
    App, ContainerSession, ContainerState, ContainersOverviewState, Screen, StatusCenter,
    TunnelState,
};
use crate::event::AppEvent;

/// The slice of App the per-host containers overlay touches: the overlay
/// session (the container list and pending action/confirm), the container
/// cache (read on open), the containers-tab overview (to mark an auto-list
/// in flight), the active-tunnel set (read-only, so a listing reuses an open
/// tunnel), the status center, the screen, and the process bits a background
/// SSH call needs (demo flag, config path, askpass session). It never reaches
/// into providers, vault or any other domain. Every container action spawns a
/// background thread that owns its inputs, so there is no whole-App effect to
/// defer.
struct ContainersCtx<'a> {
    session: &'a mut Option<ContainerSession>,
    container_state: &'a mut ContainerState,
    containers_overview: &'a mut ContainersOverviewState,
    tunnels: &'a TunnelState,
    status: &'a mut StatusCenter,
    screen: &'a mut Screen,
    demo_mode: bool,
    bw_session: Option<&'a str>,
    /// Passwords typed in the TUI this session, read so a container call
    /// reuses the one the user already gave for that host.
    session_passwords: &'a crate::app::SessionPasswords,
    /// Proven security key PINs a container call may hand out.
    security_key: crate::security_key::SecurityKeyRun,
    config_path: &'a Path,
    env: std::sync::Arc<crate::runtime::env::Env>,
}

impl Nav for ContainersCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

impl Notify for ContainersCtx<'_> {
    fn status_mut(&mut self) -> &mut StatusCenter {
        self.status
    }
}

impl ContainersCtx<'_> {
    fn ctx_from_app<'a>(app: &'a mut App) -> ContainersCtx<'a> {
        let security_key = app.security_key_confirmed_run();
        ContainersCtx {
            session: &mut app.container_session,
            container_state: &mut app.container_state,
            containers_overview: &mut app.containers_overview,
            tunnels: &app.tunnels,
            status: &mut app.status_center,
            screen: &mut app.screen,
            demo_mode: app.demo_mode,
            bw_session: app.bw_session.as_deref(),
            session_passwords: &app.session_passwords,
            security_key,
            config_path: app.reload.config_path(),
            env: std::sync::Arc::clone(&app.env),
        }
    }

    /// An owned SSH context for `alias`, reusing an active tunnel when one is
    /// open. Mirrors the inline `OwnedSshContext` builds the handlers used to
    /// assemble from `&mut App`.
    fn ssh_context(
        &self,
        alias: &str,
        askpass: Option<String>,
    ) -> crate::ssh_context::OwnedSshContext {
        crate::ssh_context::OwnedSshContext {
            alias: alias.to_string(),
            config_path: self.config_path.to_path_buf(),
            askpass,
            session_password: self.session_passwords.get(alias).cloned(),
            bw_session: self.bw_session.map(|s| s.to_string()),
            has_tunnel: self.tunnels.active_contains(alias),
            trust_new_host_key: false,
            security_key: self.security_key.clone(),
            env: std::sync::Arc::clone(&self.env),
        }
    }

    /// Build `ContainerSession` from cache, switch to `Screen::Containers`,
    /// and (unless in demo mode) spawn the background SSH listing thread.
    fn open_overlay_for_host(
        &mut self,
        alias: String,
        askpass: Option<String>,
        events_tx: &mpsc::Sender<AppEvent>,
    ) {
        let (cached_runtime, cached_containers) =
            if let Some(entry) = self.container_state.cache_entry(&alias) {
                (Some(entry.runtime), entry.containers.clone())
            } else {
                (None, Vec::new())
            };
        let mut list_state = ratatui::widgets::ListState::default();
        if !cached_containers.is_empty() {
            list_state.select(Some(0));
        }
        *self.session = Some(crate::app::ContainerSession {
            alias: alias.clone(),
            askpass: askpass.clone(),
            runtime: cached_runtime,
            containers: cached_containers,
            list_state,
            loading: !self.demo_mode,
            error: None,
            action_in_progress: None,
            confirm_action: None,
        });
        self.set_screen(Screen::Containers {
            alias: alias.clone(),
        });
        if !self.demo_mode {
            // Mark in-flight so `ensure_list_for_selected_host` will not
            // double-spawn if the user Tabs to the Containers tab before
            // this listing returns.
            self.containers_overview
                .mark_auto_list_pending(alias.clone());
            let ctx = self.ssh_context(&alias, askpass);
            let tx = events_tx.clone();
            crate::containers::spawn_container_listing(ctx, cached_runtime, move |a, result| {
                let _ = tx.send(AppEvent::ContainerListing { alias: a, result });
            });
        }
    }

    fn container_action(
        &mut self,
        events_tx: &mpsc::Sender<AppEvent>,
        action: crate::containers::ContainerAction,
    ) {
        let Some(ref mut state) = *self.session else {
            return;
        };
        if state.action_in_progress.is_some() {
            return;
        }
        let Some(idx) = state.list_state.selected() else {
            return;
        };
        let Some(container) = state.containers.get(idx) else {
            return;
        };
        if crate::containers::validate_container_id(&container.id).is_err() {
            return;
        }
        if self.demo_mode {
            self.notify_warning(crate::i18n::text(
                crate::messages::DEMO_CONTAINER_ACTIONS_DISABLED,
            ));
            return;
        }
        let Some(runtime) = state.runtime else {
            return;
        };
        let container_id = container.id.clone();
        let container_name = container.names.clone();
        state.action_in_progress = Some(format!("{} {}...", action.as_str(), container_name));
        let alias = state.alias.clone();
        // `state` borrows only `self.session`; `config_path`, `bw_session` and
        // `tunnels` are disjoint slice fields, so the OwnedSshContext is built
        // inline rather than via `ssh_context` (which would borrow all of self).
        let ctx = crate::ssh_context::OwnedSshContext {
            alias: alias.clone(),
            config_path: self.config_path.to_path_buf(),
            askpass: state.askpass.clone(),
            session_password: self.session_passwords.get(&alias).cloned(),
            bw_session: self.bw_session.map(|s| s.to_string()),
            has_tunnel: self.tunnels.active_contains(&alias),
            trust_new_host_key: false,
            security_key: self.security_key.clone(),
            env: std::sync::Arc::clone(&self.env),
        };
        let tx = events_tx.clone();
        crate::containers::spawn_container_action(
            ctx,
            runtime,
            action,
            container_id,
            move |a, act, result| {
                let _ = tx.send(AppEvent::ContainerActionComplete {
                    alias: a,
                    action: act,
                    result,
                });
            },
        );
    }
}

/// Build `ContainerSession` from cache, switch to `Screen::Containers`, and
/// (unless in demo mode) spawn the background SSH listing thread. Shared
/// by every entry point that opens the per-host containers overlay
/// (`C` on the host list, `Enter` on the containers overview tab) so the
/// state setup cannot drift between callers. Stale-host warnings stay at
/// the call site since they depend on cursor-resolved metadata the
/// caller already has in scope.
pub(super) fn open_overlay_for_host(
    app: &mut App,
    alias: String,
    askpass: Option<String>,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let mut ctx = ContainersCtx::ctx_from_app(app);
    ctx.open_overlay_for_host(alias, askpass, events_tx);
}

pub(super) fn handle_key(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    let mut ctx = ContainersCtx::ctx_from_app(app);
    containers_key(&mut ctx, key, events_tx)
}

fn containers_key(
    ctx: &mut ContainersCtx,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    // Handle pending container-action confirmation via the shared confirm
    // router. `?` (help) is the only key allowed to bypass the confirm gate;
    // every other key routes through route_confirm_key so a misplaced
    // keypress can never silently cancel or execute a destructive action.
    // `q` is intentionally not whitelisted here: in confirm-context it must
    // be Ignored, not treated as cancel.
    let confirm_pending = ctx
        .session
        .as_ref()
        .is_some_and(|s| s.confirm_action.is_some());
    if confirm_pending && key.code != KeyCode::Char('?') {
        match super::route_confirm_key(key) {
            super::ConfirmAction::Yes => {
                let taken = ctx.session.as_mut().and_then(|s| s.confirm_action.take());
                if let Some((action, _name, _id)) = taken {
                    ctx.container_action(events_tx, action);
                }
            }
            super::ConfirmAction::No => {
                if let Some(ref mut state) = *ctx.session {
                    state.confirm_action = None;
                }
            }
            super::ConfirmAction::Ignored => {}
        }
        return Ok(());
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            // No confirm pending (the early-return above handles that case):
            // close the overlay.
            *ctx.session = None;
            ctx.set_screen(Screen::HostList);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(ref mut state) = *ctx.session {
                let len = state.containers.len();
                if len > 0 {
                    let i = state.list_state.selected().unwrap_or(0);
                    state
                        .list_state
                        .select(Some(if i == 0 { len - 1 } else { i - 1 }));
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(ref mut state) = *ctx.session {
                let len = state.containers.len();
                if len > 0 {
                    let i = state.list_state.selected().unwrap_or(0);
                    state
                        .list_state
                        .select(Some(if i + 1 >= len { 0 } else { i + 1 }));
                }
            }
        }
        KeyCode::PageDown => {
            if let Some(ref mut state) = *ctx.session {
                let len = state.containers.len();
                if len > 0 {
                    let i = state.list_state.selected().unwrap_or(0);
                    state.list_state.select(Some((i + 10).min(len - 1)));
                }
            }
        }
        KeyCode::PageUp => {
            if let Some(ref mut state) = *ctx.session {
                let len = state.containers.len();
                if len > 0 {
                    let i = state.list_state.selected().unwrap_or(0);
                    state.list_state.select(Some(i.saturating_sub(10)));
                }
            }
        }
        KeyCode::Char('s') => {
            ctx.container_action(events_tx, crate::containers::ContainerAction::Start);
        }
        KeyCode::Char('x') => {
            // Stop requires confirmation
            if let Some(ref mut state) = *ctx.session {
                if state.action_in_progress.is_some() || state.confirm_action.is_some() {
                    return Ok(());
                }
                if let Some(idx) = state.list_state.selected()
                    && let Some(container) = state.containers.get(idx)
                {
                    state.confirm_action = Some((
                        crate::containers::ContainerAction::Stop,
                        container.names.clone(),
                        container.id.clone(),
                    ));
                }
            }
        }
        KeyCode::Char('r') => {
            // Restart requires confirmation
            if let Some(ref mut state) = *ctx.session {
                if state.action_in_progress.is_some() || state.confirm_action.is_some() {
                    return Ok(());
                }
                if let Some(idx) = state.list_state.selected()
                    && let Some(container) = state.containers.get(idx)
                {
                    state.confirm_action = Some((
                        crate::containers::ContainerAction::Restart,
                        container.names.clone(),
                        container.id.clone(),
                    ));
                }
            }
        }
        KeyCode::Char('R') => {
            // Refresh container list
            if ctx.demo_mode {
                ctx.notify_warning(crate::i18n::text(
                    crate::messages::DEMO_CONTAINER_REFRESH_DISABLED,
                ));
                return Ok(());
            }
            // Resolve the SSH context before borrowing the session mutably for
            // the in-flight flags, so `ssh_context` (which reads `tunnels`) and
            // the `&mut session` borrow do not overlap.
            let spawn = if let Some(ref mut state) = *ctx.session {
                if state.action_in_progress.is_some() {
                    return Ok(());
                }
                state.loading = true;
                state.error = None;
                Some((state.alias.clone(), state.runtime, state.askpass.clone()))
            } else {
                None
            };
            if let Some((alias, cached_runtime, askpass)) = spawn {
                let ctx_ssh = ctx.ssh_context(&alias, askpass);
                let tx = events_tx.clone();
                crate::containers::spawn_container_listing(
                    ctx_ssh,
                    cached_runtime,
                    move |a, result| {
                        let _ = tx.send(AppEvent::ContainerListing { alias: a, result });
                    },
                );
            }
        }
        KeyCode::Char('?') => {
            ctx.push_help_overlay();
        }
        _ => {}
    }
    Ok(())
}
