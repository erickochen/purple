use std::sync::mpsc;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Screen};
use crate::event::AppEvent;

pub(super) fn handle_containers(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    // Block all keys except y/Y and Esc/q when a confirmation is pending
    if let Some(ref state) = app.container_state {
        if state.confirm_action.is_some() {
            match key.code {
                KeyCode::Char('y')
                | KeyCode::Char('Y')
                | KeyCode::Esc
                | KeyCode::Char('q')
                | KeyCode::Char('?') => {}
                _ => return Ok(()),
            }
        }
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            if let Some(ref mut state) = app.container_state {
                if state.confirm_action.is_some() {
                    // Cancel pending confirmation, stay in overlay
                    state.confirm_action = None;
                    return Ok(());
                }
            }
            app.container_state = None;
            app.screen = Screen::HostList;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(ref mut state) = app.container_state {
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
            if let Some(ref mut state) = app.container_state {
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
            if let Some(ref mut state) = app.container_state {
                let len = state.containers.len();
                if len > 0 {
                    let i = state.list_state.selected().unwrap_or(0);
                    state.list_state.select(Some((i + 10).min(len - 1)));
                }
            }
        }
        KeyCode::PageUp => {
            if let Some(ref mut state) = app.container_state {
                let len = state.containers.len();
                if len > 0 {
                    let i = state.list_state.selected().unwrap_or(0);
                    state.list_state.select(Some(i.saturating_sub(10)));
                }
            }
        }
        KeyCode::Char('s') => {
            container_action(app, events_tx, crate::containers::ContainerAction::Start);
        }
        KeyCode::Char('x') => {
            // Stop requires confirmation
            if let Some(ref mut state) = app.container_state {
                if state.action_in_progress.is_some() || state.confirm_action.is_some() {
                    return Ok(());
                }
                if let Some(idx) = state.list_state.selected() {
                    if let Some(container) = state.containers.get(idx) {
                        state.confirm_action = Some((
                            crate::containers::ContainerAction::Stop,
                            container.names.clone(),
                            container.id.clone(),
                        ));
                    }
                }
            }
        }
        KeyCode::Char('r') => {
            // Restart requires confirmation
            if let Some(ref mut state) = app.container_state {
                if state.action_in_progress.is_some() || state.confirm_action.is_some() {
                    return Ok(());
                }
                if let Some(idx) = state.list_state.selected() {
                    if let Some(container) = state.containers.get(idx) {
                        state.confirm_action = Some((
                            crate::containers::ContainerAction::Restart,
                            container.names.clone(),
                            container.id.clone(),
                        ));
                    }
                }
            }
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            // Confirm pending action
            if let Some(ref mut state) = app.container_state {
                if let Some((action, _name, _id)) = state.confirm_action.take() {
                    container_action(app, events_tx, action);
                }
            }
        }
        KeyCode::Char('R') => {
            // Refresh container list
            if app.demo_mode {
                app.set_status("Demo mode. Container refresh disabled.".to_string(), false);
                return Ok(());
            }
            if let Some(ref mut state) = app.container_state {
                if state.action_in_progress.is_some() {
                    return Ok(());
                }
                state.loading = true;
                state.error = None;
                let alias = state.alias.clone();
                let askpass = state.askpass.clone();
                let has_tunnel = app.active_tunnels.contains_key(&alias);
                let cached_runtime = state.runtime;
                let config_path = app.reload.config_path.clone();
                let bw = app.bw_session.clone();
                let tx = events_tx.clone();
                crate::containers::spawn_container_listing(
                    alias,
                    config_path,
                    askpass,
                    bw,
                    has_tunnel,
                    cached_runtime,
                    move |a, result| {
                        let _ = tx.send(AppEvent::ContainerListing { alias: a, result });
                    },
                );
            }
        }
        KeyCode::Char('?') => {
            let old = std::mem::replace(&mut app.screen, Screen::HostList);
            app.screen = Screen::Help {
                return_screen: Box::new(old),
            };
        }
        _ => {}
    }
    Ok(())
}

fn container_action(
    app: &mut App,
    events_tx: &mpsc::Sender<AppEvent>,
    action: crate::containers::ContainerAction,
) {
    let Some(ref mut state) = app.container_state else {
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
    if app.demo_mode {
        app.set_status("Demo mode. Container actions disabled.".to_string(), false);
        return;
    }
    let Some(runtime) = state.runtime else {
        return;
    };
    let container_id = container.id.clone();
    let container_name = container.names.clone();
    state.action_in_progress = Some(format!("{} {}...", action.as_str(), container_name));
    let alias = state.alias.clone();
    let askpass = state.askpass.clone();
    let has_tunnel = app.active_tunnels.contains_key(&alias);
    let config_path = app.reload.config_path.clone();
    let bw = app.bw_session.clone();
    let tx = events_tx.clone();
    crate::containers::spawn_container_action(
        alias,
        config_path,
        runtime,
        action,
        container_id,
        askpass,
        bw,
        has_tunnel,
        move |a, act, result| {
            let _ = tx.send(AppEvent::ContainerActionComplete {
                alias: a,
                action: act,
                result,
            });
        },
    );
}
