use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent};
use log::{debug, info};

use super::ctx::{Effectful, Effects, Nav};
use crate::app::{App, Screen, TunnelState};
use crate::event::AppEvent;

/// The slice of App the file-browser handler touches: the active browser
/// session (`session`), the active-tunnel set (read-only, to decide whether
/// scp reuses an existing tunnel) and the screen. The scp transfer and
/// remote-listing threads own their inputs (built from the config path,
/// askpass and bw session), so the only whole-App operation is closing the
/// browser. Closing saves the session's paths into `file_browser_state` and
/// flips the screen; that touches a field outside the slice, so it is deferred
/// as an effect after the slice borrow ends. The slice never reaches into
/// hosts, providers or any other domain.
struct FileBrowserCtx<'a> {
    session: &'a mut Option<crate::file_browser::FileBrowserSession>,
    tunnels: &'a TunnelState,
    screen: &'a mut Screen,
    bw_session: Option<&'a str>,
    /// Passwords typed in the TUI this session, read so every listing and
    /// transfer after the prompt reuses the one the user already gave.
    session_passwords: &'a crate::app::SessionPasswords,
    /// Security key PINs a listing or transfer on this host may hand out.
    security_key: crate::security_key::SecurityKeyRun,
    config_path: &'a std::path::Path,
    env: std::sync::Arc<crate::runtime::env::Env>,
    effects: Effects,
}

impl Nav for FileBrowserCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

impl Effectful for FileBrowserCtx<'_> {
    fn effects_mut(&mut self) -> &mut Effects {
        &mut self.effects
    }
}

/// The SSH context a file-browser listing or transfer runs with. Takes the
/// slice fields separately rather than the whole `FileBrowserCtx` so it can
/// be called while the session borrow is live, the way `fb_enter` does.
#[allow(clippy::too_many_arguments)]
fn fb_ssh_context(
    alias: &str,
    askpass: Option<String>,
    tunnels: &TunnelState,
    session_passwords: &crate::app::SessionPasswords,
    bw_session: Option<&str>,
    security_key: &crate::security_key::SecurityKeyRun,
    config_path: &std::path::Path,
    env: &std::sync::Arc<crate::runtime::env::Env>,
) -> crate::ssh_context::OwnedSshContext {
    crate::ssh_context::OwnedSshContext {
        alias: alias.to_string(),
        config_path: config_path.to_path_buf(),
        askpass,
        session_password: session_passwords.get(alias).cloned(),
        bw_session: bw_session.map(str::to_string),
        has_tunnel: tunnels.active_contains(alias),
        trust_new_host_key: false,
        security_key: security_key.clone(),
        env: std::sync::Arc::clone(env),
    }
}

pub(super) fn handle_key(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    let security_key = app
        .file_browser_session
        .as_ref()
        .map(|fb| app.security_key_run_for(&fb.alias, crate::security_key::PinFlow::FileBrowser))
        .unwrap_or_default();
    let effects = {
        let mut ctx = FileBrowserCtx {
            session: &mut app.file_browser_session,
            tunnels: &app.tunnels,
            screen: &mut app.screen,
            bw_session: app.bw_session.as_deref(),
            session_passwords: &app.session_passwords,
            security_key,
            config_path: app.reload.config_path(),
            env: std::sync::Arc::clone(&app.env),
            effects: Effects::default(),
        };
        file_browser_key(&mut ctx, key, events_tx);
        ctx.effects
    };
    effects.apply(app);
}

fn file_browser_key(ctx: &mut FileBrowserCtx, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    use crate::file_browser::BrowserPane;

    let fb = match ctx.session.as_mut() {
        Some(fb) => fb,
        None => return,
    };

    // Block input while transfer is running
    if fb.transferring.is_some() {
        return;
    }

    // Dismiss transfer error dialog
    if fb.transfer_error.is_some() && key.code != KeyCode::Char('?') {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                fb.transfer_error = None;
            }
            _ => {}
        }
        return;
    }

    // SCP copy confirm dispatch via the shared confirm router. `?` (help) is
    // the only key allowed to bypass; everything else routes through
    // route_confirm_key so a misplaced keypress can never silently kick off
    // a transfer or dismiss the dialog.
    if fb.confirm_copy.is_some() && key.code != KeyCode::Char('?') {
        match super::route_confirm_key(key) {
            super::ConfirmAction::Yes => {
                let Some(req) = fb.confirm_copy.take() else {
                    return;
                };
                let alias = fb.alias.clone();
                let askpass = fb.askpass.clone();
                let local_path = fb.local_path.clone();
                let remote_path = if fb.remote_path.ends_with('/') {
                    fb.remote_path.clone()
                } else {
                    format!("{}/", fb.remote_path)
                };
                let scp_args = crate::file_browser::build_scp_args(
                    &alias,
                    req.source_pane,
                    &local_path,
                    &remote_path,
                    &req.sources,
                    req.has_dirs,
                );

                // Show transfer status in the file browser
                let label = if req.sources.len() == 1 {
                    crate::messages::scp_copying_one(&req.sources[0])
                } else {
                    crate::messages::scp_copying_many(req.sources.len())
                };
                fb.transferring = Some(label);

                // Run scp in background thread
                let direction = match req.source_pane {
                    crate::file_browser::BrowserPane::Local => "upload",
                    crate::file_browser::BrowserPane::Remote => "download",
                };
                info!(
                    "[external] SCP transfer started: {direction} {} <-> {alias}:{}",
                    local_path.display(),
                    remote_path
                );
                let ssh_ctx = fb_ssh_context(
                    &alias,
                    askpass,
                    ctx.tunnels,
                    ctx.session_passwords,
                    ctx.bw_session,
                    &ctx.security_key,
                    ctx.config_path,
                    &ctx.env,
                );
                let tx = events_tx.clone();
                let direction_str = direction.to_string();
                std::thread::spawn(move || {
                    debug!(
                        "[external] SCP command: scp -F {} ...",
                        ssh_ctx.config_path.display()
                    );
                    let result = crate::file_browser::run_scp(&ssh_ctx.borrow(), &scp_args);
                    let (success, message) = match result {
                        Ok(r) if r.status.success() => {
                            info!("[external] SCP transfer completed: {direction_str} {alias}");
                            (true, String::new())
                        }
                        Ok(r) => {
                            // A non-zero scp exit is a routine remote failure
                            // (file missing, no permission), already logged at
                            // warn by run_scp; do not re-log it as an error.
                            let code = r.status.code().unwrap_or(1);
                            let err = crate::file_browser::filter_ssh_warnings(&r.stderr_output);
                            if !err.is_empty() {
                                debug!("[external] SCP stderr: {}", err.trim());
                            }
                            if err.is_empty() {
                                (false, crate::messages::scp_failed_exit_code(code))
                            } else {
                                (false, err)
                            }
                        }
                        Err(e) => (false, crate::messages::scp_spawn_failed(&e)),
                    };
                    let _ = tx.send(crate::event::AppEvent::ScpComplete {
                        alias: ssh_ctx.alias,
                        success,
                        message,
                    });
                });
            }
            super::ConfirmAction::No => {
                fb.confirm_copy = None;
            }
            super::ConfirmAction::Ignored => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            ctx.defer(App::close_file_browser);
        }
        KeyCode::Tab => {
            fb.active_pane = match fb.active_pane {
                BrowserPane::Local => BrowserPane::Remote,
                BrowserPane::Remote => BrowserPane::Local,
            };
        }
        KeyCode::Char('j') | KeyCode::Down => {
            match fb.active_pane {
                BrowserPane::Local => {
                    let len = fb.local_entries.len() + 1; // +1 for ..
                    crate::app::cycle_selection(&mut fb.local_list_state, len, true);
                }
                BrowserPane::Remote => {
                    if !fb.remote_loading && fb.remote_error.is_none() {
                        let len = fb.remote_entries.len() + 1;
                        crate::app::cycle_selection(&mut fb.remote_list_state, len, true);
                    }
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => match fb.active_pane {
            BrowserPane::Local => {
                let len = fb.local_entries.len() + 1;
                crate::app::cycle_selection(&mut fb.local_list_state, len, false);
            }
            BrowserPane::Remote => {
                if !fb.remote_loading && fb.remote_error.is_none() {
                    let len = fb.remote_entries.len() + 1;
                    crate::app::cycle_selection(&mut fb.remote_list_state, len, false);
                }
            }
        },
        KeyCode::PageDown => match fb.active_pane {
            BrowserPane::Local => {
                let len = fb.local_entries.len() + 1;
                crate::app::page_down(&mut fb.local_list_state, len, 10);
            }
            BrowserPane::Remote => {
                let len = fb.remote_entries.len() + 1;
                crate::app::page_down(&mut fb.remote_list_state, len, 10);
            }
        },
        KeyCode::PageUp => match fb.active_pane {
            BrowserPane::Local => {
                let len = fb.local_entries.len() + 1;
                crate::app::page_up(&mut fb.local_list_state, len, 10);
            }
            BrowserPane::Remote => {
                let len = fb.remote_entries.len() + 1;
                crate::app::page_up(&mut fb.remote_list_state, len, 10);
            }
        },
        KeyCode::Enter => {
            let config_path = ctx.config_path.to_path_buf();
            let env = std::sync::Arc::clone(&ctx.env);
            let bw_session = ctx.bw_session.map(str::to_string);
            let has_tunnel = ctx.tunnels.active_contains(&fb.alias);
            let session_password = ctx.session_passwords.get(&fb.alias).cloned();
            fb_enter(
                fb,
                &config_path,
                env,
                bw_session.as_deref(),
                session_password.as_deref(),
                &ctx.security_key,
                has_tunnel,
                events_tx,
            );
        }
        KeyCode::Backspace => {
            // Go up in the active pane
            match fb.active_pane {
                BrowserPane::Local => {
                    if let Some(parent) = fb.local_path.parent() {
                        fb.local_path = parent.to_path_buf();
                        match crate::file_browser::list_local(
                            &fb.local_path,
                            fb.show_hidden,
                            fb.sort,
                        ) {
                            Ok(entries) => {
                                fb.local_entries = entries;
                                fb.local_error = None;
                            }
                            Err(e) => {
                                fb.local_entries = Vec::new();
                                fb.local_error = Some(e.to_string());
                            }
                        }
                        fb.local_list_state.select(Some(0));
                        fb.local_selected.clear();
                    }
                }
                BrowserPane::Remote => {
                    // One remote ssh at a time: a PIN on trial must reach the
                    // key from one run only.
                    if fb.remote_loading {
                        return;
                    }
                    let path = fb.remote_path.clone();
                    let parent = if path == "/" {
                        "/".to_string()
                    } else {
                        let trimmed = path.trim_end_matches('/');
                        match trimmed.rfind('/') {
                            Some(0) => "/".to_string(),
                            Some(pos) => trimmed[..pos].to_string(),
                            None => "/".to_string(),
                        }
                    };
                    if parent != fb.remote_path {
                        fb.remote_path = parent.clone();
                        fb.remote_loading = true;
                        fb.remote_entries.clear();
                        fb.remote_selected.clear();
                        fb.remote_error = None;
                        fb.remote_list_state = ratatui::widgets::ListState::default();
                        let ssh_ctx = fb_ssh_context(
                            &fb.alias,
                            fb.askpass.clone(),
                            ctx.tunnels,
                            ctx.session_passwords,
                            ctx.bw_session,
                            &ctx.security_key,
                            ctx.config_path,
                            &ctx.env,
                        );
                        let show_hidden = fb.show_hidden;
                        let sort = fb.sort;
                        crate::file_browser::spawn_remote_listing(
                            ssh_ctx,
                            parent,
                            show_hidden,
                            sort,
                            fb_send(events_tx.clone()),
                        );
                    }
                }
            }
        }
        KeyCode::Char(' ') => {
            // Toggle multi-select. Plain Space and Ctrl+Space both trigger
            // the same toggle: macOS reserves Ctrl+Space for the input
            // source switcher and tmux often binds Ctrl+Space, so the
            // bare key has to work too. Ctrl+Space stays for muscle memory.
            match fb.active_pane {
                BrowserPane::Local => {
                    let idx = fb.local_list_state.selected().unwrap_or(0);
                    if idx > 0
                        && let Some(entry) = fb.local_entries.get(idx - 1)
                    {
                        let name = entry.name.clone();
                        if fb.local_selected.contains(&name) {
                            fb.local_selected.remove(&name);
                        } else {
                            fb.local_selected.insert(name);
                        }
                    }
                }
                BrowserPane::Remote => {
                    let idx = fb.remote_list_state.selected().unwrap_or(0);
                    if idx > 0
                        && let Some(entry) = fb.remote_entries.get(idx - 1)
                    {
                        let name = entry.name.clone();
                        if fb.remote_selected.contains(&name) {
                            fb.remote_selected.remove(&name);
                        } else {
                            fb.remote_selected.insert(name);
                        }
                    }
                }
            }
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            // Select all / deselect all (toggle). Plain `a`/`A` and
            // Ctrl+A all trigger the same toggle: tmux binds Ctrl+A as
            // the prefix by default, so the bare key has to work too.
            // Ctrl+A keeps working for users who never hit that conflict.
            match fb.active_pane {
                BrowserPane::Local => {
                    if fb.local_selected.len() == fb.local_entries.len()
                        && !fb.local_entries.is_empty()
                    {
                        fb.local_selected.clear();
                    } else {
                        fb.local_selected =
                            fb.local_entries.iter().map(|e| e.name.clone()).collect();
                    }
                }
                BrowserPane::Remote => {
                    if fb.remote_selected.len() == fb.remote_entries.len()
                        && !fb.remote_entries.is_empty()
                    {
                        fb.remote_selected.clear();
                    } else {
                        fb.remote_selected =
                            fb.remote_entries.iter().map(|e| e.name.clone()).collect();
                    }
                }
            }
        }
        // One remote ssh at a time: a PIN on trial must reach the key from
        // one run only.
        KeyCode::Char('.') | KeyCode::Char('R') if fb.remote_loading => {}
        KeyCode::Char('.') => {
            fb.show_hidden = !fb.show_hidden;
            // Refresh local
            match crate::file_browser::list_local(&fb.local_path, fb.show_hidden, fb.sort) {
                Ok(entries) => {
                    fb.local_entries = entries;
                    fb.local_error = None;
                }
                Err(e) => {
                    fb.local_entries = Vec::new();
                    fb.local_error = Some(e.to_string());
                }
            }
            fb.local_list_state.select(Some(0));
            fb.local_selected.clear();
            // Refresh remote
            if !fb.remote_path.is_empty() {
                fb.remote_loading = true;
                fb.remote_entries.clear();
                fb.remote_selected.clear();
                fb.remote_error = None;
                fb.remote_list_state = ratatui::widgets::ListState::default();
                let ssh_ctx = fb_ssh_context(
                    &fb.alias,
                    fb.askpass.clone(),
                    ctx.tunnels,
                    ctx.session_passwords,
                    ctx.bw_session,
                    &ctx.security_key,
                    ctx.config_path,
                    &ctx.env,
                );
                let path = fb.remote_path.clone();
                let show_hidden = fb.show_hidden;
                let sort = fb.sort;
                crate::file_browser::spawn_remote_listing(
                    ssh_ctx,
                    path,
                    show_hidden,
                    sort,
                    fb_send(events_tx.clone()),
                );
            }
        }
        KeyCode::Char('R') => {
            // Refresh both panes
            match crate::file_browser::list_local(&fb.local_path, fb.show_hidden, fb.sort) {
                Ok(entries) => {
                    fb.local_entries = entries;
                    fb.local_error = None;
                }
                Err(e) => {
                    fb.local_entries = Vec::new();
                    fb.local_error = Some(e.to_string());
                }
            }
            fb.local_list_state.select(Some(0));
            fb.local_selected.clear();
            if !fb.remote_path.is_empty() {
                fb.remote_loading = true;
                fb.remote_entries.clear();
                fb.remote_selected.clear();
                fb.remote_error = None;
                fb.remote_list_state = ratatui::widgets::ListState::default();
                let ssh_ctx = fb_ssh_context(
                    &fb.alias,
                    fb.askpass.clone(),
                    ctx.tunnels,
                    ctx.session_passwords,
                    ctx.bw_session,
                    &ctx.security_key,
                    ctx.config_path,
                    &ctx.env,
                );
                let path = fb.remote_path.clone();
                let show_hidden = fb.show_hidden;
                let sort = fb.sort;
                crate::file_browser::spawn_remote_listing(
                    ssh_ctx,
                    path,
                    show_hidden,
                    sort,
                    fb_send(events_tx.clone()),
                );
            }
        }
        KeyCode::Char('s') => {
            // Toggle sort mode
            fb.sort = match fb.sort {
                crate::file_browser::BrowserSort::Name => crate::file_browser::BrowserSort::Date,
                crate::file_browser::BrowserSort::Date => crate::file_browser::BrowserSort::DateAsc,
                crate::file_browser::BrowserSort::DateAsc => crate::file_browser::BrowserSort::Name,
            };
            // Re-sort entries in place
            crate::file_browser::sort_entries(&mut fb.local_entries, fb.sort);
            crate::file_browser::sort_entries(&mut fb.remote_entries, fb.sort);
            fb.local_list_state.select(Some(0));
            fb.remote_list_state.select(Some(0));
        }
        KeyCode::Char('?') => {
            ctx.push_help_overlay();
        }
        _ => {}
    }
}

pub(super) fn fb_send(
    tx: mpsc::Sender<AppEvent>,
) -> impl FnOnce(String, String, Result<Vec<crate::file_browser::FileEntry>, String>) + Send + 'static
{
    move |alias, path, entries| {
        let _ = tx.send(AppEvent::FileBrowserListing {
            alias,
            path,
            entries,
        });
    }
}

/// `Enter` in the file browser: navigate into a directory, ascend via the
/// `..` row, or stage an scp copy of the selection. `config_path`,
/// `bw_session`, `session_password` and `has_tunnel` are resolved by the
/// caller because `fb` borrows the same `App`.
#[allow(clippy::too_many_arguments)]
fn fb_enter(
    fb: &mut crate::file_browser::FileBrowserSession,
    config_path: &std::path::Path,
    env: std::sync::Arc<crate::runtime::env::Env>,
    bw_session: Option<&str>,
    session_password: Option<&str>,
    security_key: &crate::security_key::SecurityKeyRun,
    has_tunnel: bool,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    use crate::file_browser::{BrowserPane, CopyRequest};
    match fb.active_pane {
        BrowserPane::Local => {
            let idx = fb.local_list_state.selected().unwrap_or(0);
            if idx == 0 {
                // ".." - go up
                if let Some(parent) = fb.local_path.parent() {
                    fb.local_path = parent.to_path_buf();
                    match crate::file_browser::list_local(&fb.local_path, fb.show_hidden, fb.sort) {
                        Ok(entries) => {
                            fb.local_entries = entries;
                            fb.local_error = None;
                        }
                        Err(e) => {
                            fb.local_entries = Vec::new();
                            fb.local_error = Some(e.to_string());
                        }
                    }
                    fb.local_list_state.select(Some(0));
                    fb.local_selected.clear();
                }
            } else if let Some(entry) = fb.local_entries.get(idx - 1).cloned() {
                if !fb.local_selected.is_empty() {
                    // Multi-select active: copy all selected items. Not while
                    // a listing runs: one remote ssh at a time.
                    if fb.remote_path.is_empty() || fb.remote_loading {
                        return;
                    }
                    let sources: Vec<String> = fb.local_selected.iter().cloned().collect();
                    let has_dirs = sources
                        .iter()
                        .any(|n| fb.local_entries.iter().any(|e| e.name == *n && e.is_dir));
                    fb.confirm_copy = Some(CopyRequest {
                        sources,
                        source_pane: BrowserPane::Local,
                        has_dirs,
                    });
                } else if entry.is_dir {
                    // No selection: navigate into directory
                    fb.local_path = fb.local_path.join(&entry.name);
                    match crate::file_browser::list_local(&fb.local_path, fb.show_hidden, fb.sort) {
                        Ok(entries) => {
                            fb.local_entries = entries;
                            fb.local_error = None;
                        }
                        Err(e) => {
                            fb.local_entries = Vec::new();
                            fb.local_error = Some(e.to_string());
                        }
                    }
                    fb.local_list_state.select(Some(0));
                    fb.local_selected.clear();
                } else {
                    // No selection, cursor on file: copy single file. Not
                    // while a listing runs: one remote ssh at a time.
                    if fb.remote_path.is_empty() || fb.remote_loading {
                        return;
                    }
                    fb.confirm_copy = Some(CopyRequest {
                        sources: vec![entry.name.clone()],
                        source_pane: BrowserPane::Local,
                        has_dirs: false,
                    });
                }
            }
        }
        BrowserPane::Remote => {
            if fb.remote_loading || fb.remote_error.is_some() {
                return;
            }
            let idx = fb.remote_list_state.selected().unwrap_or(0);
            if idx == 0 {
                // ".." - go up
                let path = fb.remote_path.clone();
                let parent = if path == "/" {
                    "/".to_string()
                } else {
                    let trimmed = path.trim_end_matches('/');
                    match trimmed.rfind('/') {
                        Some(0) => "/".to_string(),
                        Some(pos) => trimmed[..pos].to_string(),
                        None => "/".to_string(),
                    }
                };
                if parent != fb.remote_path {
                    fb.remote_path = parent.clone();
                    fb.remote_loading = true;
                    fb.remote_entries.clear();
                    fb.remote_selected.clear();
                    fb.remote_error = None;
                    fb.remote_list_state = ratatui::widgets::ListState::default();
                    let ctx = crate::ssh_context::OwnedSshContext {
                        alias: fb.alias.clone(),
                        config_path: config_path.to_path_buf(),
                        askpass: fb.askpass.clone(),
                        session_password: session_password.map(str::to_string),
                        bw_session: bw_session.map(str::to_string),
                        has_tunnel,
                        trust_new_host_key: false,
                        security_key: security_key.clone(),
                        env: std::sync::Arc::clone(&env),
                    };
                    let show_hidden = fb.show_hidden;
                    let sort = fb.sort;
                    crate::file_browser::spawn_remote_listing(
                        ctx,
                        parent,
                        show_hidden,
                        sort,
                        fb_send(events_tx.clone()),
                    );
                }
            } else if let Some(entry) = fb.remote_entries.get(idx - 1).cloned() {
                if !fb.remote_selected.is_empty() {
                    // Multi-select active: copy all selected items
                    let sources: Vec<String> = fb.remote_selected.iter().cloned().collect();
                    let has_dirs = sources
                        .iter()
                        .any(|n| fb.remote_entries.iter().any(|e| e.name == *n && e.is_dir));
                    fb.confirm_copy = Some(CopyRequest {
                        sources,
                        source_pane: BrowserPane::Remote,
                        has_dirs,
                    });
                } else if entry.is_dir {
                    // No selection: navigate into directory
                    let new_path = if fb.remote_path.ends_with('/') {
                        format!("{}{}", fb.remote_path, entry.name)
                    } else {
                        format!("{}/{}", fb.remote_path, entry.name)
                    };
                    fb.remote_path = new_path.clone();
                    fb.remote_loading = true;
                    fb.remote_entries.clear();
                    fb.remote_selected.clear();
                    fb.remote_error = None;
                    fb.remote_list_state = ratatui::widgets::ListState::default();
                    let ctx = crate::ssh_context::OwnedSshContext {
                        alias: fb.alias.clone(),
                        config_path: config_path.to_path_buf(),
                        askpass: fb.askpass.clone(),
                        session_password: session_password.map(str::to_string),
                        bw_session: bw_session.map(str::to_string),
                        has_tunnel,
                        trust_new_host_key: false,
                        security_key: security_key.clone(),
                        env: std::sync::Arc::clone(&env),
                    };
                    let show_hidden = fb.show_hidden;
                    let sort = fb.sort;
                    crate::file_browser::spawn_remote_listing(
                        ctx,
                        new_path,
                        show_hidden,
                        sort,
                        fb_send(events_tx.clone()),
                    );
                } else {
                    // No selection, cursor on file: copy single file
                    fb.confirm_copy = Some(CopyRequest {
                        sources: vec![entry.name.clone()],
                        source_pane: BrowserPane::Remote,
                        has_dirs: false,
                    });
                }
            }
        }
    }
}
