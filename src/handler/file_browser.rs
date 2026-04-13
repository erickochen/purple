use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use log::{debug, error, info};

use crate::app::{App, Screen};
use crate::event::AppEvent;

pub(super) fn handle_file_browser(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    use crate::file_browser::{BrowserPane, CopyRequest};

    let fb = match app.file_browser.as_mut() {
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

    // If confirm dialog is showing, handle that first
    if fb.confirm_copy.is_some() && key.code != KeyCode::Char('?') {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let Some(req) = fb.confirm_copy.take() else {
                    return;
                };
                let alias = fb.alias.clone();
                let askpass = fb.askpass.clone();
                let has_active_tunnel = app.active_tunnels.contains_key(&alias);
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
                    format!("Copying {}...", req.sources[0])
                } else {
                    format!("Copying {} files...", req.sources.len())
                };
                fb.transferring = Some(label);

                // Run scp in background thread
                let direction = match req.source_pane {
                    crate::file_browser::BrowserPane::Local => "upload",
                    crate::file_browser::BrowserPane::Remote => "download",
                };
                info!(
                    "SCP transfer started: {direction} {} <-> {alias}:{}",
                    local_path.display(),
                    remote_path
                );
                let config_path = app.reload.config_path.clone();
                let bw = app.bw_session.clone();
                let tx = events_tx.clone();
                let direction_str = direction.to_string();
                std::thread::spawn(move || {
                    debug!("SCP command: scp -F {} ...", config_path.display());
                    let result = crate::file_browser::run_scp(
                        &alias,
                        &config_path,
                        askpass.as_deref(),
                        bw.as_deref(),
                        has_active_tunnel,
                        &scp_args,
                    );
                    let (success, message) = match result {
                        Ok(r) if r.status.success() => {
                            info!("SCP transfer completed: {direction_str} {alias}");
                            (true, String::new())
                        }
                        Ok(r) => {
                            let code = r.status.code().unwrap_or(1);
                            error!("[external] SCP transfer failed: {alias} exit={code}");
                            let err = crate::file_browser::filter_ssh_warnings(&r.stderr_output);
                            if !err.is_empty() {
                                debug!("[external] SCP stderr: {}", err.trim());
                            }
                            if err.is_empty() {
                                (false, format!("Copy failed (exit code {}).", code))
                            } else {
                                (false, err)
                            }
                        }
                        Err(e) => (false, format!("scp failed: {}", e)),
                    };
                    let _ = tx.send(crate::event::AppEvent::ScpComplete {
                        alias,
                        success,
                        message,
                    });
                });
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                fb.confirm_copy = None;
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            // Save paths for next time
            if let Some(ref fb) = app.file_browser {
                let alias = fb.alias.clone();
                let local = fb.local_path.clone();
                let remote = fb.remote_path.clone();
                app.file_browser_paths.insert(alias, (local, remote));
            }
            app.file_browser = None;
            app.screen = Screen::HostList;
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
            match fb.active_pane {
                BrowserPane::Local => {
                    let idx = fb.local_list_state.selected().unwrap_or(0);
                    if idx == 0 {
                        // ".." - go up
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
                    } else if let Some(entry) = fb.local_entries.get(idx - 1).cloned() {
                        if !fb.local_selected.is_empty() {
                            // Multi-select active: copy all selected items
                            if fb.remote_path.is_empty() {
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
                        } else {
                            // No selection, cursor on file: copy single file
                            if fb.remote_path.is_empty() {
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
                            let alias = fb.alias.clone();
                            let ctx = crate::ssh_context::OwnedSshContext {
                                alias,
                                config_path: app.reload.config_path.clone(),
                                askpass: fb.askpass.clone(),
                                bw_session: app.bw_session.clone(),
                                has_tunnel: app.active_tunnels.contains_key(&fb.alias),
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
                            let has_dirs = sources.iter().any(|n| {
                                fb.remote_entries.iter().any(|e| e.name == *n && e.is_dir)
                            });
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
                            let alias = fb.alias.clone();
                            let ctx = crate::ssh_context::OwnedSshContext {
                                alias,
                                config_path: app.reload.config_path.clone(),
                                askpass: fb.askpass.clone(),
                                bw_session: app.bw_session.clone(),
                                has_tunnel: app.active_tunnels.contains_key(&fb.alias),
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
                        let alias = fb.alias.clone();
                        let ctx = crate::ssh_context::OwnedSshContext {
                            alias,
                            config_path: app.reload.config_path.clone(),
                            askpass: fb.askpass.clone(),
                            bw_session: app.bw_session.clone(),
                            has_tunnel: app.active_tunnels.contains_key(&fb.alias),
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
                }
            }
        }
        KeyCode::Char(' ') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Toggle multi-select
            match fb.active_pane {
                BrowserPane::Local => {
                    let idx = fb.local_list_state.selected().unwrap_or(0);
                    if idx > 0 {
                        if let Some(entry) = fb.local_entries.get(idx - 1) {
                            let name = entry.name.clone();
                            if fb.local_selected.contains(&name) {
                                fb.local_selected.remove(&name);
                            } else {
                                fb.local_selected.insert(name);
                            }
                        }
                    }
                }
                BrowserPane::Remote => {
                    let idx = fb.remote_list_state.selected().unwrap_or(0);
                    if idx > 0 {
                        if let Some(entry) = fb.remote_entries.get(idx - 1) {
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
        }
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Select all / deselect all (toggle)
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
                let alias = fb.alias.clone();
                let ctx = crate::ssh_context::OwnedSshContext {
                    alias,
                    config_path: app.reload.config_path.clone(),
                    askpass: fb.askpass.clone(),
                    bw_session: app.bw_session.clone(),
                    has_tunnel: app.active_tunnels.contains_key(&fb.alias),
                };
                let path = fb.remote_path.clone();
                let show_hidden = fb.show_hidden;
                let sort = fb.sort;
                crate::file_browser::spawn_remote_listing(
                    ctx,
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
                let alias = fb.alias.clone();
                let ctx = crate::ssh_context::OwnedSshContext {
                    alias,
                    config_path: app.reload.config_path.clone(),
                    askpass: fb.askpass.clone(),
                    bw_session: app.bw_session.clone(),
                    has_tunnel: app.active_tunnels.contains_key(&fb.alias),
                };
                let path = fb.remote_path.clone();
                let show_hidden = fb.show_hidden;
                let sort = fb.sort;
                crate::file_browser::spawn_remote_listing(
                    ctx,
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
            let old = std::mem::replace(&mut app.screen, Screen::HostList);
            app.screen = Screen::Help {
                return_screen: Box::new(old),
            };
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
