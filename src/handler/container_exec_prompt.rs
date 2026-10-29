//! Key handler for the container exec prompt
//! (`Screen::ContainerExecPrompt`). Opens via `e` on the containers
//! tab; the user types a single-line command, Enter submits it
//! through the existing `pending_container_exec` flow with the typed
//! command in place of the default shell.

use crossterm::event::{KeyCode, KeyEvent};

use super::ctx::{Nav, Notify};
use crate::app::{App, ContainerExecRequest, ContainerState, HostState, Screen, StatusCenter};

/// The slice of App the container exec prompt touches: the screen (which holds
/// the typed command buffer), the container cache / submit queue and the host
/// list (for the host's askpass). It navigates and notifies; no whole-App
/// effects are deferred.
struct ExecPromptCtx<'a> {
    screen: &'a mut Screen,
    container_state: &'a mut ContainerState,
    hosts: &'a HostState,
    status: &'a mut StatusCenter,
}

impl Nav for ExecPromptCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

impl Notify for ExecPromptCtx<'_> {
    fn status_mut(&mut self) -> &mut StatusCenter {
        self.status
    }
}

pub(super) fn handle_key(app: &mut App, key: KeyEvent) {
    let mut ctx = ExecPromptCtx {
        screen: &mut app.screen,
        container_state: &mut app.container_state,
        hosts: &app.hosts_state,
        status: &mut app.status_center,
    };
    exec_prompt_key(&mut ctx, key);
}

fn exec_prompt_key(ctx: &mut ExecPromptCtx, key: KeyEvent) {
    let Screen::ContainerExecPrompt {
        alias,
        container_id,
        container_name,
        query,
    } = &mut *ctx.screen
    else {
        return;
    };

    match key.code {
        KeyCode::Esc => {
            log::debug!("[purple] container_exec_prompt: cancelled");
            ctx.set_screen(Screen::HostList);
        }
        KeyCode::Enter => {
            let cmd = query.trim().to_string();
            if cmd.is_empty() {
                // Empty submit is a no-op (no toast, no transition);
                // the user is asking for nothing.
                return;
            }
            // Reject control characters so a paste with embedded
            // newlines or escapes cannot smuggle a multi-line command
            // past the single-line prompt.
            if cmd.chars().any(|c| c.is_control()) {
                ctx.notify_error(
                    crate::i18n::text(crate::messages::CONTAINER_EXEC_INVALID_COMMAND).to_string(),
                );
                return;
            }
            let alias = alias.clone();
            let container_id = container_id.clone();
            let container_name = container_name.clone();
            queue_exec_with_command(ctx, alias, container_id, container_name, cmd);
        }
        KeyCode::Backspace => {
            query.pop();
        }
        // Cap the buffer at 512 chars so a held-down key cannot grow
        // without bound. 512 covers any realistic one-off command and
        // is well below shell ARG_MAX.
        KeyCode::Char(c) if query.chars().count() < 512 => {
            query.push(c);
        }
        _ => {}
    }
}

fn queue_exec_with_command(
    ctx: &mut ExecPromptCtx,
    alias: String,
    container_id: String,
    container_name: String,
    command: String,
) {
    let Some(entry) = ctx.container_state.cache_entry(&alias) else {
        log::debug!(
            "[purple] container_exec_prompt: submit aborted, no cache for alias={}",
            alias
        );
        ctx.set_screen(Screen::HostList);
        return;
    };
    let runtime = entry.runtime;
    let askpass = ctx
        .hosts
        .list()
        .iter()
        .find(|h| h.alias == alias)
        .and_then(|h| h.askpass.clone());

    log::info!(
        "[purple] container_exec_prompt: queue exec alias={} id={} cmd_len={}",
        alias,
        container_id,
        command.len()
    );
    ctx.container_state.queue_exec(ContainerExecRequest {
        alias,
        askpass,
        runtime,
        container_id,
        container_name,
        command: Some(command),
    });
    ctx.set_screen(Screen::HostList);
}
