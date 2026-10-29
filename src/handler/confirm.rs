use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent};

use super::ctx::{Effectful, Effects, Nav, Notify};
use crate::app::{App, ContainerState, HostState, KeysState, Screen, StatusCenter, UiSelection};
use crate::event::AppEvent;

/// The slice of App a single-domain confirm dialog touches: the screen (read to
/// extract the confirm payload, written to transition out) and the deferred
/// effect queue. The confirmed Yes-action of these dialogs is a terminal
/// whole-App operation (run the known-hosts import, purge stale hosts, kick off
/// the vault bulk-sign), so it runs as a deferred effect after the slice borrow
/// ends. The slice never reaches into hosts, vault, containers or any other
/// domain directly.
struct ConfirmCtx<'a> {
    screen: &'a mut Screen,
    effects: Effects,
}

impl Nav for ConfirmCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

impl Effectful for ConfirmCtx<'_> {
    fn effects_mut(&mut self) -> &mut Effects {
        &mut self.effects
    }
}

/// The slice of App the key-push confirm touches: the Keys-tab push state
/// (the frozen alias selection) and the screen. Confirming kicks off the
/// background push worker, a terminal whole-App op (reads the key list and
/// config path, spawns a thread), so it runs deferred after the screen
/// transition. Declining clears the frozen selection on the slice and returns
/// to the picker. The slice never reaches into hosts, vault or any other
/// domain directly.
struct KeyPushConfirmCtx<'a> {
    keys: &'a mut KeysState,
    screen: &'a mut Screen,
    effects: Effects,
}

impl Nav for KeyPushConfirmCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

impl Effectful for KeyPushConfirmCtx<'_> {
    fn effects_mut(&mut self) -> &mut Effects {
        &mut self.effects
    }
}

/// The slice of App the host-key-reset confirm touches: the connect queue and
/// known-hosts count on `ui`, the status center, the screen and the demo flag.
/// The `ssh-keygen -R` subprocess runs inline (it owns its arguments and reads
/// no App state) and its result drives only slice-local notifications and the
/// queued connect, so nothing is deferred. The slice never reaches into hosts,
/// vault or any other domain directly.
struct HostKeyResetCtx<'a> {
    ui: &'a mut UiSelection,
    status: &'a mut StatusCenter,
    screen: &'a mut Screen,
    demo_mode: bool,
}

impl Nav for HostKeyResetCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

impl Notify for HostKeyResetCtx<'_> {
    fn status_mut(&mut self) -> &mut StatusCenter {
        self.status
    }
}

/// The slice of App every container-action confirm touches: the container cache
/// and pending-action queue (`container_state`), the host list (read-only, to
/// resolve each target's askpass), and the screen. Confirming queues one
/// `ContainerActionRequest` per target. the drain loop later spawns the SSH
/// thread, so the confirm itself runs entirely on the slice with no deferred
/// whole-App effect. The slice never reaches into vault, providers or any other
/// domain directly.
struct ContainerConfirmCtx<'a> {
    container_state: &'a mut ContainerState,
    hosts: &'a HostState,
    screen: &'a mut Screen,
}

impl Nav for ContainerConfirmCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

/// Result of routing a confirm-dialog key event.
///
/// Confirm dialogs accept exactly three classes of keys:
/// - `Yes`: y / Y
/// - `No`: n / N / Esc
/// - `Ignored`: anything else (must NOT change app state)
///
/// **Critical safety invariant**: a `_ =>` catch-all in a confirm handler
/// that transitions screen state is forbidden. A misplaced keypress must not
/// silently cancel a destructive operation. Use [`route_confirm_key`] in every
/// confirm handler to enforce the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    Yes,
    No,
    Ignored,
}

/// Single source of truth for confirm-dialog key routing.
pub fn route_confirm_key(key: KeyEvent) -> ConfirmAction {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => ConfirmAction::Yes,
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => ConfirmAction::No,
        _ => ConfirmAction::Ignored,
    }
}

/// Run known_hosts import and set status. Used by both ConfirmImport and Welcome handlers.
pub(super) fn execute_known_hosts_import(app: &mut App) {
    let config_backup = app.hosts_state.ssh_config().clone();
    match crate::import::import_from_known_hosts(
        app.env.paths(),
        app.hosts_state.ssh_config_mut(),
        Some("known_hosts"),
    ) {
        Ok((imported, skipped, _, _)) => {
            if imported > 0 {
                if let Err(e) = app.hosts_state.ssh_config().write() {
                    app.hosts_state.set_ssh_config(config_backup);
                    app.notify_error(crate::messages::failed_to_save(&e));
                    return;
                }
                app.reload_hosts();
                app.notify(crate::messages::imported_hosts(imported, skipped));
            } else {
                app.notify(crate::messages::all_hosts_exist(skipped));
            }
            app.ui.set_known_hosts_count(0);
        }
        Err(e) => {
            app.notify_error(e);
        }
    }
}

pub(super) fn handle_import_key(app: &mut App, key: KeyEvent) {
    let effects = {
        let mut ctx = ConfirmCtx {
            screen: &mut app.screen,
            effects: Effects::default(),
        };
        match route_confirm_key(key) {
            ConfirmAction::Yes => {
                ctx.set_screen(Screen::HostList);
                ctx.defer(execute_known_hosts_import);
            }
            ConfirmAction::No => {
                ctx.set_screen(Screen::HostList);
            }
            ConfirmAction::Ignored => {}
        }
        ctx.effects
    };
    effects.apply(app);
}

pub(super) fn handle_purge_stale_key(app: &mut App, key: KeyEvent) {
    if !matches!(app.screen, Screen::ConfirmPurgeStale) {
        return;
    }
    let route = route_confirm_key(key);
    if route == ConfirmAction::Ignored {
        return;
    }
    // Take the payload up front: yes/no both transition away, so the
    // pending slot must clear in either case to avoid stale reads if
    // the user opens a fresh purge dialog later.
    let Some(payload) = app.providers.take_pending_purge() else {
        // State pruned out from under us. Drop to HostList so the user
        // is not stranded on an empty purge dialog.
        app.set_screen(Screen::HostList);
        return;
    };
    let provider = payload.provider;
    let return_screen = if provider.is_some() {
        Screen::Providers
    } else {
        Screen::HostList
    };
    let effects = {
        let mut ctx = ConfirmCtx {
            screen: &mut app.screen,
            effects: Effects::default(),
        };
        match route {
            ConfirmAction::Yes => {
                // Defer the purge (a whole-App op: deletes hosts, kills tunnels,
                // reloads). It is the terminal action. moving it past the screen
                // assignment is safe because it reads no screen state and emits
                // the only toast either way.
                ctx.defer(move |app| execute_purge_stale(app, provider.as_deref()));
                ctx.set_screen(return_screen);
            }
            ConfirmAction::No => {
                ctx.set_screen(return_screen);
            }
            ConfirmAction::Ignored => unreachable!(),
        }
        ctx.effects
    };
    effects.apply(app);
}

fn execute_purge_stale(app: &mut App, provider: Option<&str>) {
    let stale = app.hosts_state.ssh_config().stale_hosts();
    if stale.is_empty() {
        return;
    }
    // Filter by provider if specified.
    let targets: Vec<(String, u64)> = if let Some(prov) = provider {
        stale
            .into_iter()
            .filter(|(alias, _)| {
                app.hosts_state
                    .ssh_config()
                    .host_entries()
                    .iter()
                    .any(|e| e.alias == *alias && e.provider.as_deref() == Some(prov))
            })
            .collect()
    } else {
        stale
    };
    if targets.is_empty() {
        return;
    }
    let config_backup = app.hosts_state.ssh_config().clone();
    let count = targets.len();
    for (alias, _) in &targets {
        app.hosts_state.ssh_config_mut().delete_host(alias);
    }
    if let Err(e) = app.hosts_state.ssh_config().write() {
        app.hosts_state.set_ssh_config(config_backup);
        app.notify_error(crate::messages::failed_to_save(&e));
        return;
    }
    // Kill active tunnels only after successful write (no rollback needed).
    for (alias, _) in &targets {
        if let Some(mut tunnel) = app.tunnels.active_remove(alias) {
            let _ = tunnel.child.kill();
            let _ = tunnel.child.wait();
        }
    }
    app.hosts_state.clear_undo();
    app.update_last_modified();
    app.reload_hosts();
    log::debug!(
        "[purple] purged {} stale host(s){}",
        count,
        provider
            .map(|p| format!(" provider={p}"))
            .unwrap_or_default()
    );
    let msg = if let Some(prov) = provider {
        let display = crate::providers::provider_display_name(prov);
        format!(
            "Removed {} stale {} host{}.",
            count,
            display,
            if count == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "Removed {} stale host{}.",
            count,
            if count == 1 { "" } else { "s" }
        )
    };
    app.notify(msg);
}

// Stays on `&mut App` (not a context slice). The delete path is cross-cutting
// and result-driven: it reads `delete_host_undoable`'s Option to branch, then
// touches hosts_state (undo stack + ssh_config), tunnels, vault cert cleanup,
// ui and status, and interleaves `update_last_modified`/`reload_hosts` with
// the subsequent notify/set_screen. Deferring the reload past the success
// toast could reorder reload's vault-flush error against it, so a slice here
// would add risk with no clean win.
pub(super) fn handle_delete_key(app: &mut App, key: KeyEvent) {
    let Screen::ConfirmDelete { alias } = &app.screen else {
        return;
    };
    let alias = alias.clone();
    // Use the central confirm-key router so the y/n/Esc contract is uniform
    // across all confirm dialogs.
    match route_confirm_key(key) {
        ConfirmAction::Yes => {
            let siblings = app.hosts_state.ssh_config().siblings_of(&alias);

            if !siblings.is_empty() {
                // Multi-alias block: strip only the selected token.
                // `delete_host_undoable` refuses this case (returning
                // None) because re-inserting the whole element via
                // `insert_host_at` cannot reverse a token strip. We
                // therefore skip the undo stack and surface the event
                // via a dedicated toast that names the surviving
                // siblings, so the user knows what did and did not
                // change on disk.
                app.hosts_state.ssh_config_mut().delete_host(&alias);
                if let Err(e) = app.hosts_state.ssh_config().write() {
                    // Disk write failed: reload from disk to discard
                    // the in-memory strip so view and storage match.
                    app.notify_error(crate::messages::failed_to_save(&e));
                    app.reload_hosts();
                } else {
                    if let Some(mut tunnel) = app.tunnels.active_remove(&alias) {
                        let _ = tunnel.child.kill();
                        let _ = tunnel.child.wait();
                    }
                    app.update_last_modified();
                    app.reload_hosts();
                    log::debug!(
                        "[purple] host alias stripped: alias={alias}, {} sibling(s) kept",
                        siblings.len()
                    );
                    app.notify(crate::messages::siblings_stripped(&alias, siblings.len()));
                }
            } else if let Some((element, position)) = app
                .hosts_state
                .ssh_config_mut()
                .delete_host_undoable(&alias)
            {
                if let Err(e) = app.hosts_state.ssh_config().write() {
                    // Restore the element on write failure
                    app.hosts_state
                        .ssh_config_mut()
                        .insert_host_at(element, position);
                    app.notify_error(crate::messages::failed_to_save(&e));
                } else {
                    // Stop active tunnel for the deleted host
                    if let Some(mut tunnel) = app.tunnels.active_remove(&alias) {
                        let _ = tunnel.child.kill();
                        let _ = tunnel.child.wait();
                    }
                    // Clean up cert file if it exists. NotFound is the
                    // expected case for hosts that never had a cert. Other
                    // errors are surfaced via the status bar (never via
                    // eprintln, which would corrupt the ratatui screen).
                    let mut cert_cleanup_warning: Option<String> = None;
                    if !crate::demo_flag::is_demo()
                        && let Ok(cert_path) =
                            crate::vault_ssh::cert_path_for(app.env().paths(), &alias)
                    {
                        match std::fs::remove_file(&cert_path) {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Err(e) => {
                                cert_cleanup_warning = Some(crate::messages::cert_cleanup_warning(
                                    &cert_path.display(),
                                    &e,
                                ));
                            }
                        }
                    }
                    app.hosts_state
                        .undo_stack_mut()
                        .push(crate::app::DeletedHost { element, position });
                    if app.hosts_state.undo_stack().len() > 50 {
                        app.hosts_state.undo_stack_mut().remove(0);
                    }
                    app.update_last_modified();
                    app.reload_hosts();
                    log::debug!("[purple] host deleted: alias={alias} (undoable)");
                    if let Some(warning) = cert_cleanup_warning {
                        app.notify_error(warning);
                    } else {
                        app.notify(crate::messages::goodbye_host(&alias));
                    }
                }
            } else {
                app.notify_warning(crate::messages::host_not_found(&alias));
            }
            app.set_screen(Screen::HostList);
        }
        ConfirmAction::No => {
            app.set_screen(Screen::HostList);
        }
        ConfirmAction::Ignored => {}
    }
}

pub(super) fn handle_vault_sign_key(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    // Vault Sign is a destructive/material action: signing N certificates
    // hits Vault, may take time and is hard to reverse. Stray keys must NOT
    // cancel. use `route_confirm_key` so only y/Y/n/N/Esc are honored.
    // History: an earlier `_ => app.screen = Screen::HostList` catch-all
    // could be triggered by any keypress next to `y` (e.g. fat-fingered
    // `t` or `u`), silently aborting a bulk sign.
    if !matches!(app.screen, Screen::ConfirmVaultSign) {
        return;
    }
    let route = route_confirm_key(key);
    if route == ConfirmAction::Ignored {
        return;
    }
    // Take the signable list up front so both yes and no paths leave
    // the slot clean.
    let Some(signable) = app.vault.take_pending_sign() else {
        // State pruned (target host removed during the dialog). Drop to
        // HostList so the confirm screen is not orphaned.
        app.set_screen(Screen::HostList);
        return;
    };
    let effects = {
        let mut ctx = ConfirmCtx {
            screen: &mut app.screen,
            effects: Effects::default(),
        };
        match route {
            ConfirmAction::Yes => {
                // Transition back to the host list and kick off the background
                // signing loop. The signing loop is the terminal whole-App op
                // (touches vault, env, status), so it runs deferred after the
                // screen transition. Its first action is the progress toast,
                // with no intervening toast.
                ctx.set_screen(Screen::HostList);
                let tx = events_tx.clone();
                ctx.defer(move |app| start_vault_bulk_sign(app, signable, &tx));
            }
            ConfirmAction::No => {
                ctx.set_screen(Screen::HostList);
            }
            ConfirmAction::Ignored => unreachable!(),
        }
        ctx.effects
    };
    effects.apply(app);
}

/// Start the background vault bulk sign loop with fast-fail, progress, TOCTOU
/// coordination and cancellation. Stores the JoinHandle on App for clean exit.
fn start_vault_bulk_sign(
    app: &mut App,
    signable: Vec<crate::vault_ssh::VaultSignTarget>,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let total = signable.len();
    if total == 0 {
        return;
    }
    app.notify_progress(crate::messages::vault_signing_progress(
        crate::animation::SPINNER_FRAMES[0],
        0,
        total,
        "",
    ));

    let cancel = Arc::new(AtomicBool::new(false));
    app.vault.set_signing_cancel(cancel.clone());

    let in_flight = app.vault.sign_in_flight().clone();
    let tx = events_tx.clone();
    // Capture the resolved environment before spawning: a worker thread does
    // not inherit the parent's `Env`, so the PATH/paths must move in.
    let env = std::sync::Arc::clone(&app.env);
    let spawn_result = std::thread::Builder::new()
        .name("vault-bulk-sign".into())
        .spawn(move || {
            let mut signed = 0u32;
            let mut failed = 0u32;
            let mut skipped = 0u32;
            let mut consecutive_failures = 0usize;
            let mut first_error: Option<String> = None;
            let mut aborted_message: Option<String> = None;

            for (idx, target) in signable.iter().enumerate() {
                let crate::vault_ssh::VaultSignTarget {
                    alias,
                    role,
                    certificate_file: cert_file,
                    pubkey,
                    vault_addr,
                } = target;
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let done = idx + 1;

                // TOCTOU: skip host if another thread already has it in-flight.
                // Otherwise mark it in-flight for the duration of this iteration.
                {
                    // If the mutex is poisoned a worker thread panicked while holding
                    // the lock. Recover the inner value without clearing. clearing
                    // the whole set would make every in-flight alias simultaneously
                    // eligible for re-signing, risking duplicate cert writes.
                    let mut set = match in_flight.lock() {
                        Ok(g) => g,
                        Err(p) => p.into_inner(),
                    };
                    if !set.insert(alias.clone()) {
                        skipped += 1;
                        let _ = tx.send(AppEvent::VaultSignProgress {
                            alias: alias.clone(),
                            done,
                            total,
                        });
                        continue;
                    }
                }

                let _ = tx.send(AppEvent::VaultSignProgress {
                    alias: alias.clone(),
                    done,
                    total,
                });

                let cert_path =
                    match crate::vault_ssh::resolve_cert_path(env.paths(), alias, cert_file) {
                        Ok(p) => p,
                        Err(e) => {
                            failed += 1;
                            consecutive_failures += 1;
                            let scrubbed = crate::vault_ssh::scrub_vault_stderr(&e.to_string());
                            if first_error.is_none() {
                                first_error = Some(scrubbed);
                            }
                            remove_in_flight(&in_flight, alias);
                            if consecutive_failures >= 3 {
                                aborted_message = Some(crate::messages::vault_signing_aborted(
                                    failed,
                                    first_error.as_deref(),
                                ));
                                break;
                            }
                            continue;
                        }
                    };
                let status = crate::vault_ssh::check_cert_validity(&env, &cert_path);
                if !crate::vault_ssh::needs_renewal(&status) {
                    skipped += 1;
                    consecutive_failures = 0;
                    remove_in_flight(&in_flight, alias);
                    continue;
                }

                let sign_result = crate::vault_ssh::sign_certificate(
                    &env,
                    role,
                    pubkey,
                    alias,
                    vault_addr.as_deref(),
                );
                // Always clean up in_flight for this alias before handling the
                // result. Using a single cleanup point (rather than per-arm)
                // prevents orphaned aliases when new control flow is added.
                remove_in_flight(&in_flight, alias);
                match sign_result {
                    Ok(_) => {
                        let _ = tx.send(AppEvent::VaultSignResult {
                            alias: alias.clone(),
                            certificate_file: cert_file.clone(),
                            success: true,
                            message: String::new(),
                        });
                        signed += 1;
                        consecutive_failures = 0;
                    }
                    Err(e) => {
                        let raw = e.to_string();
                        let scrubbed = crate::vault_ssh::scrub_vault_stderr(&raw);
                        if first_error.is_none() {
                            first_error = Some(scrubbed.clone());
                        }
                        let _ = tx.send(AppEvent::VaultSignResult {
                            alias: alias.clone(),
                            certificate_file: cert_file.clone(),
                            success: false,
                            message: scrubbed,
                        });
                        failed += 1;
                        consecutive_failures += 1;
                        if consecutive_failures >= 3 {
                            aborted_message = Some(crate::messages::vault_signing_aborted(
                                failed,
                                first_error.as_deref(),
                            ));
                            break;
                        }
                    }
                }
            }

            let cancelled = cancel.load(Ordering::Relaxed);
            let _ = tx.send(AppEvent::VaultSignAllDone {
                signed,
                failed,
                skipped,
                cancelled,
                aborted_message,
                first_error,
            });
        });
    match spawn_result {
        Ok(handle) => {
            log::info!("[purple] vault sign thread: spawned");
            app.vault.set_sign_thread(handle);
        }
        Err(e) => {
            // Spawn failed (e.g. OS thread limit). Clear the cancel flag and
            // surface the error. otherwise the status bar is stuck at
            // "Signing 0/N" with no way for the user to recover.
            log::warn!("[purple] vault sign thread: spawn failed: {}", e);
            let _ = app.vault.finalize_signing_run();
            app.notify_error(crate::messages::vault_spawn_failed(&e));
        }
    }
}

pub(super) fn remove_in_flight(
    set: &std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    alias: &str,
) {
    // On mutex poison, recover the inner value and remove only the target alias.
    // Do NOT clear the entire set. other in-flight aliases are still owned by
    // live worker iterations and clearing them would allow duplicate signs.
    let mut guard = match set.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.remove(alias);
}

pub(super) fn handle_host_key_reset_key(app: &mut App, key: KeyEvent) {
    let mut ctx = HostKeyResetCtx {
        ui: &mut app.ui,
        status: &mut app.status_center,
        screen: &mut app.screen,
        demo_mode: app.demo_mode,
    };
    let Screen::ConfirmHostKeyReset {
        alias,
        hostname,
        known_hosts_path,
        askpass,
    } = &*ctx.screen
    else {
        return;
    };
    let alias = alias.clone();
    let hostname = hostname.clone();
    let known_hosts_path = known_hosts_path.clone();
    let askpass = askpass.clone();
    // Host key reset wipes the host's known_hosts entry. uniform y/n/Esc
    // contract via the central router so stray keys cannot trigger it.
    match route_confirm_key(key) {
        ConfirmAction::Yes => {
            let output = std::process::Command::new("ssh-keygen")
                .arg("-R")
                .arg(&hostname)
                .arg("-f")
                .arg(&known_hosts_path)
                .output();

            match output {
                Ok(result) if result.status.success() => {
                    ctx.notify(crate::messages::removed_host_key(&hostname));
                    if ctx.demo_mode {
                        ctx.notify_warning(crate::i18n::text(
                            crate::messages::DEMO_CONNECTION_DISABLED,
                        ));
                    } else {
                        ctx.ui.queue_connect(alias, askpass);
                    }
                }
                Ok(result) => {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    ctx.notify_error(crate::messages::host_key_remove_failed(stderr.trim()));
                }
                Err(e) => {
                    ctx.notify_error(crate::messages::ssh_keygen_failed(&e));
                }
            }
            ctx.set_screen(Screen::HostList);
        }
        ConfirmAction::No => {
            ctx.set_screen(Screen::HostList);
        }
        ConfirmAction::Ignored => {}
    }
}

/// Trust-on-first-contact dialog. `y` runs the held operation again with
/// `StrictHostKeyChecking=accept-new`, so ssh records the host key and
/// carries on into authentication. `n` and Esc drop it and offer the next
/// host waiting for an answer. Accepting a host key is a security decision,
/// hence the uniform y/n/Esc contract via `route_confirm_key`.
///
/// Stays on `&mut App` rather than a slice: both answers run a whole-App
/// operation (a push worker or a file-browser listing).
pub(super) fn handle_host_key_trust_key(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let Screen::ConfirmHostKeyTrust { alias, retry, .. } = &app.screen else {
        return;
    };
    let alias = alias.clone();
    let retry = retry.clone();
    match route_confirm_key(key) {
        ConfirmAction::Yes => {
            log::debug!("[purple] host key trust: accepted alias={alias}");
            let back = super::password_prompt::return_screen(app);
            app.set_screen(back);
            super::password_prompt::execute_retry(app, retry, true, events_tx);
            super::event_loop::key_push::drain_next_key_push_prompt(app);
        }
        ConfirmAction::No => {
            log::debug!("[purple] host key trust: declined alias={alias}");
            let back = super::password_prompt::return_screen(app);
            app.set_screen(back);
            app.notify_warning(crate::messages::host_key_trust::declined(&alias));
            super::event_loop::key_push::drain_next_key_push_prompt(app);
        }
        ConfirmAction::Ignored => {}
    }
}

/// A confirmed container action plus its target(s). Single-container
/// confirms carry one target; stack and host-wide confirms carry many.
struct ContainerConfirm {
    alias: String,
    targets: Vec<(String, String)>,
    action: crate::containers::ContainerAction,
}

/// Shared y/n/Esc routing for every container action confirm. Yes
/// queues the action for each target then drops to the host list; No
/// and Esc drop without side effects; anything else is ignored.
fn apply_container_confirm(
    ctx: &mut ContainerConfirmCtx,
    key: KeyEvent,
    confirm: ContainerConfirm,
) {
    match route_confirm_key(key) {
        ConfirmAction::Yes => {
            for (container_id, container_name) in confirm.targets {
                queue_container_action(
                    ctx,
                    confirm.alias.clone(),
                    container_id,
                    container_name,
                    confirm.action,
                );
            }
            ctx.set_screen(Screen::HostList);
        }
        ConfirmAction::No => {
            ctx.set_screen(Screen::HostList);
        }
        ConfirmAction::Ignored => {}
    }
}

/// Borrow the disjoint App fields every container-action confirm needs.
fn container_confirm_ctx(app: &mut App) -> ContainerConfirmCtx<'_> {
    ContainerConfirmCtx {
        container_state: &mut app.container_state,
        hosts: &app.hosts_state,
        screen: &mut app.screen,
    }
}

/// Confirm handler for `K` (kick = restart): restart a single container.
pub(super) fn handle_container_restart_key(app: &mut App, key: KeyEvent) {
    let mut ctx = container_confirm_ctx(app);
    let Screen::ConfirmContainerRestart {
        alias,
        container_id,
        container_name,
        ..
    } = &*ctx.screen
    else {
        return;
    };
    let confirm = ContainerConfirm {
        alias: alias.clone(),
        targets: vec![(container_id.clone(), container_name.clone())],
        action: crate::containers::ContainerAction::Restart,
    };
    apply_container_confirm(&mut ctx, key, confirm);
}

/// Confirm handler for `S` (stop): stop a single container.
pub(super) fn handle_container_stop_key(app: &mut App, key: KeyEvent) {
    let mut ctx = container_confirm_ctx(app);
    let Screen::ConfirmContainerStop {
        alias,
        container_id,
        container_name,
        ..
    } = &*ctx.screen
    else {
        return;
    };
    let confirm = ContainerConfirm {
        alias: alias.clone(),
        targets: vec![(container_id.clone(), container_name.clone())],
        action: crate::containers::ContainerAction::Stop,
    };
    apply_container_confirm(&mut ctx, key, confirm);
}

/// Confirm handler for `Ctrl-K` (stack kick): restart every member of a
/// compose stack. The drain queue processes one request per tick, so the
/// restarts run sequentially.
pub(super) fn handle_stack_restart_key(app: &mut App, key: KeyEvent) {
    bulk_confirm_key(
        app,
        key,
        Screen::ConfirmStackRestart,
        crate::containers::ContainerAction::Restart,
    );
}

/// Confirm handler for `K` on a host-divider row: restart every running
/// container on the host, ignoring compose-project boundaries.
pub(super) fn handle_host_restart_all_key(app: &mut App, key: KeyEvent) {
    bulk_confirm_key(
        app,
        key,
        Screen::ConfirmHostRestartAll,
        crate::containers::ContainerAction::Restart,
    );
}

/// Confirm handler for `S` on a host-divider row: stop every running
/// container on the host.
pub(super) fn handle_host_stop_all_key(app: &mut App, key: KeyEvent) {
    bulk_confirm_key(
        app,
        key,
        Screen::ConfirmHostStopAll,
        crate::containers::ContainerAction::Stop,
    );
}

/// Shared dispatcher for the three bulk container confirm dialogs.
/// `expected_screen` guards against firing the wrong action if the
/// screen has drifted from `pending_bulk_confirm`. An ignored key
/// leaves the payload in place so the dialog stays open; yes/no take
/// the payload because `apply_container_confirm` transitions away.
fn bulk_confirm_key(
    app: &mut App,
    key: KeyEvent,
    expected_screen: Screen,
    action: crate::containers::ContainerAction,
) {
    if app.screen != expected_screen {
        return;
    }
    if route_confirm_key(key) == ConfirmAction::Ignored {
        return;
    }
    let Some(payload) = app.containers_overview.take_pending_bulk_confirm() else {
        // State pruned (host removed via reload_hosts while the dialog was
        // open). Drop to HostList so the user is not stuck on an empty
        // confirm screen with a non-functional Esc.
        app.set_screen(Screen::HostList);
        return;
    };
    let targets: Vec<(String, String)> = payload
        .members
        .iter()
        .map(|m| (m.container_id.clone(), m.container_name.clone()))
        .collect();
    let confirm = ContainerConfirm {
        alias: payload.alias,
        targets,
        action,
    };
    let mut ctx = container_confirm_ctx(app);
    apply_container_confirm(&mut ctx, key, confirm);
}

fn queue_container_action(
    ctx: &mut ContainerConfirmCtx,
    alias: String,
    container_id: String,
    container_name: String,
    action: crate::containers::ContainerAction,
) {
    let Some(entry) = ctx.container_state.cache_entry(&alias) else {
        log::debug!(
            "[purple] container_action: queue aborted, no cache for alias={}",
            alias
        );
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
        "[purple] container_action queued: alias={} id={} action={:?}",
        alias,
        container_id,
        action
    );
    ctx.container_state
        .queue_action(crate::app::ContainerActionRequest {
            alias,
            askpass,
            runtime,
            container_id,
            container_name,
            action,
        });
}

/// Confirm for the `p` push action from the Keys tab. Stakes test:
/// pushing modifies remote `authorized_keys`, so the footer uses
/// action verbs (`push` / `keep`) and we only accept y/n/Esc.
pub(super) fn handle_key_push_key(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    let effects = {
        let mut ctx = KeyPushConfirmCtx {
            keys: &mut app.keys,
            screen: &mut app.screen,
            effects: Effects::default(),
        };
        match route_confirm_key(key) {
            ConfirmAction::Yes => {
                let key_index = match &*ctx.screen {
                    Screen::ConfirmKeyPush { key_index } => *key_index,
                    _ => return,
                };
                let aliases = std::mem::take(&mut ctx.keys.push_mut().committed);
                ctx.set_screen(Screen::HostList);
                // The push worker is the terminal whole-App op (reads the key
                // list and config path, spawns a thread). it runs deferred
                // after the screen transition. its first observable action is
                // the guard/progress toast, with no intervening toast.
                let tx = events_tx.clone();
                ctx.defer(move |app| {
                    start_key_push(app, key_index, aliases, false, &tx);
                });
            }
            ConfirmAction::No => {
                // Return to the picker with the selection still intact so the
                // user can refine it.
                let key_index = match &*ctx.screen {
                    Screen::ConfirmKeyPush { key_index } => *key_index,
                    _ => return,
                };
                ctx.keys.push_mut().committed.clear();
                ctx.set_screen(Screen::KeyPushPicker { key_index });
            }
            ConfirmAction::Ignored => {}
        }
        ctx.effects
    };
    effects.apply(app);
}

/// Confirm dialog for `Screen::ConfirmRunSnippet`. On Yes, run the chosen
/// snippet against the committed hosts (this sets the next screen itself via
/// `run_flow_snippet`). On No/Esc, return to the host picker with the selection
/// intact. y/n/Esc are the only effective inputs (`route_confirm_key`).
pub(super) fn handle_run_snippet_confirm_key(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    match route_confirm_key(key) {
        ConfirmAction::Yes => {
            super::snippet::run_flow_snippet(app, events_tx);
        }
        ConfirmAction::No => {
            app.set_screen(Screen::SnippetHostPicker);
        }
        ConfirmAction::Ignored => {}
    }
}

/// The authentication inputs one host's push runs with: the askpass source
/// that resolves for it, any password typed this session and the Bitwarden
/// token. Resolved on the main thread because the worker has no App access.
pub(crate) fn push_auth_for(
    app: &App,
    alias: &str,
    trust_new_host_key: bool,
) -> crate::key_push::PushAuth {
    crate::key_push::PushAuth {
        askpass: app.askpass_source_for(alias),
        session_password: app.session_password_for(alias),
        bw_session: app.bw_session.clone(),
        trust_new_host_key,
        security_key: app.security_key_run_for(alias, crate::security_key::PinFlow::KeyPush),
        paths: app.env().paths().cloned(),
    }
}

/// Spawn the background push worker. Reads the pubkey from disk on the
/// main thread (cheap) so we surface an early error toast before
/// committing to the run. On read failure we abort and stay on
/// HostList. Refuses to start a second push while a first is still in
/// flight (`expected_count > 0`); the user must press Esc to cancel
/// before triggering another run.
///
/// `trust_new_host_key` is set only by the retry that follows the trust
/// dialog, so ssh records the host key on that one run. Returns true when
/// the worker started.
pub(crate) fn start_key_push(
    app: &mut App,
    key_index: usize,
    aliases: Vec<String>,
    trust_new_host_key: bool,
    events_tx: &mpsc::Sender<AppEvent>,
) -> bool {
    // Refuse second push while a previous run still has live state OR a
    // worker handle that has not been observed to finish. Belt-and-braces:
    // expected_count protects the in-flight branch, worker.is_finished()
    // protects the post-cancel branch where the worker is still draining
    // but its results no longer count toward any expected total.
    if app.keys.push().expected_count > 0
        || app
            .keys
            .push()
            .worker
            .as_ref()
            .is_some_and(|h| !h.is_finished())
    {
        log::debug!(
            "[purple] key_push: rejected second push, run already in progress ({} of {})",
            app.keys.push().results.len(),
            app.keys.push().expected_count
        );
        app.notify_warning(crate::i18n::text(
            crate::messages::KEY_PUSH_ALREADY_IN_PROGRESS,
        ));
        return false;
    }
    if aliases.is_empty() {
        log::debug!("[purple] key_push: rejected, no aliases committed");
        app.notify_error(crate::i18n::text(
            crate::messages::KEY_PUSH_NO_HOSTS_SELECTED,
        ));
        return false;
    }
    let Some(key_info) = app.keys.list().get(key_index).cloned() else {
        return false;
    };
    if key_info.is_certificate {
        app.notify_error(crate::i18n::text(
            crate::messages::KEY_PUSH_CERT_NOT_PUSHABLE,
        ));
        return false;
    }
    let pub_path = crate::key_push::pubkey_path_for(app.env.paths(), &key_info.display_path);
    let raw = match crate::key_push::read_pubkey_file(&pub_path) {
        Ok(s) => s,
        Err(crate::key_push::PubkeyValidationError::TooLarge(n)) => {
            log::warn!(
                "[config] key_push: pubkey too large path={} bytes={}",
                pub_path.display(),
                n
            );
            app.notify_error(crate::messages::key_push_pubkey_too_large(
                &key_info.name,
                n,
            ));
            return false;
        }
        Err(crate::key_push::PubkeyValidationError::NotARegularFile) => {
            log::warn!(
                "[config] key_push: pubkey not a regular file path={}",
                pub_path.display()
            );
            app.notify_error(crate::messages::key_push_pubkey_not_regular(&key_info.name));
            return false;
        }
        Err(_) => {
            // Other validation variants are unreachable here (read_pubkey_file
            // only returns TooLarge / NotARegularFile / IO collapsed into
            // NotARegularFile). Defensive fallthrough.
            app.notify_error(crate::messages::key_push_no_pubkey(&key_info.name));
            return false;
        }
    };
    let pubkey = match crate::key_push::validate_pubkey(&raw) {
        Ok(s) => s,
        Err(err) => {
            let detail = match &err {
                crate::key_push::PubkeyValidationError::Empty => "file is empty",
                crate::key_push::PubkeyValidationError::MultiLine => {
                    "must be a single line; multi-line input is rejected"
                }
                crate::key_push::PubkeyValidationError::UnsupportedType(_) => {
                    "key algorithm not allowed for static push"
                }
                crate::key_push::PubkeyValidationError::MalformedBase64 => {
                    "base64 key body did not parse"
                }
                _ => "unexpected format",
            };
            log::warn!(
                "[config] key_push: invalid pubkey path={} err={:?}",
                pub_path.display(),
                err
            );
            app.notify_error(crate::messages::key_push_invalid_pubkey(
                &key_info.name,
                detail,
            ));
            return false;
        }
    };

    // Reset accumulators and start a new run.
    let (run_id, cancel) = app.keys.push_mut().start_run(
        aliases.len(),
        key_info.display_path.clone(),
        trust_new_host_key,
    );
    let inflight = app.keys.push().inflight.clone();

    app.notify_progress(crate::messages::key_push_in_progress(
        &key_info.name,
        aliases.len(),
    ));

    // Resolve each host's authentication inputs here: the worker thread has
    // no App access, so the askpass source, any password typed this session
    // and the Bitwarden token all have to travel with the target.
    let targets: Vec<(String, crate::key_push::PushAuth)> = aliases
        .into_iter()
        .map(|alias| {
            let auth = push_auth_for(app, &alias, trust_new_host_key);
            (alias, auth)
        })
        .collect();

    let config_path = app.hosts_state.ssh_config().path.clone();
    let tx = events_tx.clone();
    let pubkey_payload = pubkey;
    let handle = std::thread::Builder::new()
        .name("key-push".into())
        .spawn(move || {
            for (alias, auth) in targets {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let outcome = crate::key_push::push_to_host(
                    &pubkey_payload,
                    &alias,
                    &config_path,
                    &auth,
                    &cancel,
                    &inflight,
                );
                let _ = tx.send(AppEvent::KeyPushResult {
                    run_id,
                    result: crate::key_push::KeyPushResult { alias, outcome },
                });
            }
        });
    match handle {
        Ok(h) => {
            app.keys.push_mut().worker = Some(h);
            true
        }
        Err(e) => {
            log::error!("[purple] key_push: failed to spawn worker: {}", e);
            // Drop the progress toast through the status-center invariant
            // so the user does not see "Pushing..." stuck under the
            // failure message.
            app.status_center.clear_sticky_status();
            app.notify_error(crate::messages::key_push_thread_spawn_failed());
            app.keys.push_mut().clear_inflight_state();
            false
        }
    }
}

#[cfg(test)]
mod key_push_confirm_tests {
    //! Coverage for the gate functions wrapping the push-worker spawn.
    //! Every test exercises a guard path (already-running, missing pubkey,
    //! certificate key, empty selection, return-to-picker) and asserts the
    //! observable state. The happy-spawn path is intentionally not unit
    //! tested here because it forks an ssh subprocess; that path is
    //! covered by the event-loop tests against the run-completion flow.
    use super::*;
    use crate::ssh_config::model::SshConfigFile;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_app() -> (App, std::path::PathBuf) {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content("Host h1\n  HostName 1.1.1.1\n"),
            path: scratch.join("test_config"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        // Seed a non-cert key whose .pub file lives in the scratch dir so
        // `read_pubkey_file` succeeds via the absolute display path.
        let pub_path = scratch.join("id_test.pub");
        std::fs::write(
            &pub_path,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBnSCk/2pwG7QHQHIvF2UxYZsMP1qJ4XbJjT7mxBSBb1 test@host\n",
        )
        .unwrap();
        app.keys.list_mut().push(crate::ssh_keys::SshKeyInfo {
            name: "id_test".into(),
            display_path: pub_path.with_extension("").to_string_lossy().into_owned(),
            key_type: "ED25519".into(),
            bits: "256".into(),
            fingerprint: String::new(),
            comment: "test@host".into(),
            linked_hosts: vec![],
            bishop_art: String::new(),
            strength_score: 95,
            encrypted: false,
            agent_loaded: false,
            is_certificate: false,
            mtime_ts: None,
        });
        (app, scratch)
    }

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn n_returns_to_picker_with_key_index_preserved() {
        let (mut app, _scratch) = make_app();
        app.keys.push_mut().committed = vec!["h1".into()];
        app.screen = Screen::ConfirmKeyPush { key_index: 0 };
        let (tx, _rx) = mpsc::channel();
        handle_key_push_key(&mut app, k(KeyCode::Char('n')), &tx);
        match app.screen {
            Screen::KeyPushPicker { key_index } => assert_eq!(key_index, 0),
            ref other => panic!("expected KeyPushPicker, got {:?}", other),
        }
        assert!(
            app.keys.push().committed.is_empty(),
            "n should drop the frozen selection"
        );
    }

    #[test]
    fn esc_routes_through_route_confirm_key_and_returns_to_picker() {
        let (mut app, _scratch) = make_app();
        app.keys.push_mut().committed = vec!["h1".into()];
        app.screen = Screen::ConfirmKeyPush { key_index: 0 };
        let (tx, _rx) = mpsc::channel();
        handle_key_push_key(&mut app, k(KeyCode::Esc), &tx);
        assert!(matches!(app.screen, Screen::KeyPushPicker { .. }));
    }

    #[test]
    fn start_rejects_when_a_previous_run_is_still_in_flight() {
        let (mut app, _scratch) = make_app();
        app.keys.push_mut().expected_count = 2;
        app.keys
            .push_mut()
            .results
            .push(crate::key_push::KeyPushResult {
                alias: "h1".into(),
                outcome: crate::key_push::KeyPushOutcome::Appended,
            });
        let (tx, _rx) = mpsc::channel();
        start_key_push(&mut app, 0, vec!["h1".into()], false, &tx);
        assert_eq!(
            app.keys.push().expected_count,
            2,
            "guard must not reset in-flight state"
        );
        let toast = app.status_center.toast().expect("toast set");
        assert!(
            toast.text.contains("already running"),
            "expected 'already running' warning, got: {}",
            toast.text
        );
    }

    #[test]
    fn start_rejects_empty_aliases_and_does_not_spawn_worker() {
        let (mut app, _scratch) = make_app();
        let (tx, _rx) = mpsc::channel();
        start_key_push(&mut app, 0, Vec::new(), false, &tx);
        assert_eq!(app.keys.push().expected_count, 0);
        assert!(app.keys.push().worker.is_none());
        let toast = app.status_center.toast().expect("toast set");
        assert!(toast.is_error());
    }

    #[test]
    fn start_rejects_certificate_key() {
        let (mut app, _scratch) = make_app();
        app.keys.list_mut()[0].is_certificate = true;
        let (tx, _rx) = mpsc::channel();
        start_key_push(&mut app, 0, vec!["h1".into()], false, &tx);
        assert_eq!(app.keys.push().expected_count, 0);
        assert!(app.keys.push().worker.is_none());
        let toast = app.status_center.toast().expect("toast set");
        assert!(toast.is_error());
        assert!(toast.text.contains("Certificates"));
    }

    #[test]
    fn start_rejects_missing_pubkey_file() {
        let (mut app, _scratch) = make_app();
        app.keys.list_mut()[0].display_path = "/tmp/purple-this-file-does-not-exist".into();
        let (tx, _rx) = mpsc::channel();
        start_key_push(&mut app, 0, vec!["h1".into()], false, &tx);
        assert_eq!(app.keys.push().expected_count, 0);
        let toast = app.status_center.toast().expect("toast set");
        assert!(toast.is_error());
    }

    #[test]
    fn start_rejects_invalid_pubkey_content() {
        let (mut app, scratch) = make_app();
        // Multi-line pubkey: the canonical command-injection PoC. Must be
        // rejected without spawning the worker.
        let pub_path = scratch.join("id_bad.pub");
        std::fs::write(
            &pub_path,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBnSCk/2pwG7QHQHIvF2UxYZsMP1qJ4XbJjT7mxBSBb1 real\ncommand=\"evil\" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBnSCk/2pwG7QHQHIvF2UxYZsMP1qJ4XbJjT7mxBSBb2 hack\n",
        )
        .unwrap();
        app.keys.list_mut()[0].display_path =
            pub_path.with_extension("").to_string_lossy().into_owned();
        app.keys.list_mut()[0].name = "id_bad".into();
        let (tx, _rx) = mpsc::channel();
        start_key_push(&mut app, 0, vec!["h1".into()], false, &tx);
        assert_eq!(app.keys.push().expected_count, 0);
        assert!(app.keys.push().worker.is_none());
        let toast = app.status_center.toast().expect("toast set");
        assert!(toast.is_error());
        assert!(toast.text.contains("validation"));
    }

    // --- host key trust dialog ---

    fn trust_app() -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content("Host h1\n  HostName db.example.com\n"),
            path: scratch.join("cfg"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        app.screen = Screen::ConfirmHostKeyTrust {
            alias: "h1".into(),
            hostname: "db.example.com".into(),
            retry: crate::app::PendingRetry::KeyPush {
                key_path: "~/.ssh/id_test".into(),
                alias: "h1".into(),
            },
        };
        app
    }

    fn seed_key(app: &mut App) {
        app.keys.list_mut().push(crate::ssh_keys::SshKeyInfo {
            name: "id_test".into(),
            display_path: "~/.ssh/id_test".into(),
            key_type: "ED25519".into(),
            bits: "256".into(),
            fingerprint: String::new(),
            comment: String::new(),
            linked_hosts: vec![],
            bishop_art: String::new(),
            strength_score: 95,
            encrypted: false,
            agent_loaded: false,
            is_certificate: false,
            mtime_ts: None,
        });
    }

    #[test]
    fn trust_accepted_closes_the_dialog() {
        // The key list is empty, so the retry bails inside `start_key_push`
        // without spawning ssh. What matters here is that the dialog closes.
        let mut app = trust_app();
        let (tx, _rx) = mpsc::channel();
        handle_host_key_trust_key(&mut app, k(KeyCode::Char('y')), &tx);
        assert_eq!(app.screen, Screen::HostList);
        assert_eq!(app.keys.push().expected_count, 0);
    }

    #[test]
    fn trust_accepted_authorizes_the_retry_to_record_the_host_key() {
        // The whole point of answering `y`: the push that follows runs with
        // `StrictHostKeyChecking=accept-new`. A real key file is seeded so
        // `start_key_push` gets as far as starting the run, which is where
        // the authorization is recorded.
        let (mut app, _scratch) = make_app();
        app.screen = Screen::ConfirmHostKeyTrust {
            alias: "h1".into(),
            hostname: "db.example.com".into(),
            retry: crate::app::PendingRetry::KeyPush {
                key_path: app.keys.list()[0].display_path.clone(),
                alias: "h1".into(),
            },
        };
        let (tx, _rx) = mpsc::channel();
        handle_host_key_trust_key(&mut app, k(KeyCode::Char('y')), &tx);
        assert_eq!(app.keys.push().expected_count, 1, "the retry ran");
        assert!(
            app.keys.push().trust_new_host_key,
            "the retry must be allowed to record the host key"
        );
        // And that authorization is what puts accept-new on the command.
        let auth = push_auth_for(&app, "h1", app.keys.push().trust_new_host_key);
        assert!(auth.trust_new_host_key);
        app.keys.push_mut().cancel_run();
    }

    #[test]
    fn a_declined_trust_leaves_an_ordinary_push_unauthorized() {
        let mut app = trust_app();
        let (tx, _rx) = mpsc::channel();
        handle_host_key_trust_key(&mut app, k(KeyCode::Char('n')), &tx);
        assert!(!app.keys.push().trust_new_host_key);
    }

    #[test]
    fn push_auth_collects_every_input_the_worker_cannot_reach() {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(
                "Host h1\n  HostName 1.1.1.1\n  # purple:askpass keychain\n",
            ),
            path: scratch.join("cfg"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        app.bw_session = Some("tok".into());
        app.session_passwords
            .insert("h1".to_string(), "hunter2".to_string());
        let auth = push_auth_for(&app, "h1", false);
        assert_eq!(auth.askpass.as_deref(), Some("keychain"));
        assert_eq!(auth.session_password.as_deref(), Some("hunter2"));
        assert_eq!(auth.bw_session.as_deref(), Some("tok"));
        assert!(!auth.trust_new_host_key);
        // A host purple knows nothing about carries nothing.
        let bare = push_auth_for(&app, "ghost", false);
        assert!(bare.askpass.is_none());
        assert!(bare.session_password.is_none());
    }

    #[test]
    fn trust_declined_warns_and_leaves_the_dialog() {
        let mut app = trust_app();
        let (tx, _rx) = mpsc::channel();
        handle_host_key_trust_key(&mut app, k(KeyCode::Char('n')), &tx);
        assert_eq!(app.screen, Screen::HostList);
        let toast = app.status_center.toast().expect("toast");
        assert!(toast.text.contains("h1"), "got: {}", toast.text);
    }

    #[test]
    fn trust_esc_declines_like_n() {
        let mut app = trust_app();
        let (tx, _rx) = mpsc::channel();
        handle_host_key_trust_key(&mut app, k(KeyCode::Esc), &tx);
        assert_eq!(app.screen, Screen::HostList);
        assert!(app.status_center.toast().is_some());
    }

    #[test]
    fn trust_ignores_every_other_key() {
        // Accepting a host key is a security decision, so a mistyped key
        // next to y must neither accept nor dismiss it.
        for code in [
            KeyCode::Char('t'),
            KeyCode::Char('u'),
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Char(' '),
        ] {
            let mut app = trust_app();
            let (tx, _rx) = mpsc::channel();
            handle_host_key_trust_key(&mut app, k(code), &tx);
            assert!(
                matches!(app.screen, Screen::ConfirmHostKeyTrust { .. }),
                "{code:?} must leave the dialog open"
            );
            assert!(app.status_center.toast().is_none(), "{code:?}");
        }
    }

    #[test]
    fn trust_declined_hands_the_next_waiting_host_its_dialog() {
        let mut app = trust_app();
        seed_key(&mut app);
        app.keys.push_mut().key_path = "~/.ssh/id_test".to_string();
        app.keys
            .push_mut()
            .pending_prompts
            .push_back(crate::app::KeyPushPrompt::Password {
                alias: "h1".into(),
                key_path: "~/.ssh/id_test".into(),
            });
        let (tx, _rx) = mpsc::channel();
        handle_host_key_trust_key(&mut app, k(KeyCode::Char('n')), &tx);
        assert_eq!(app.screen, Screen::PasswordPrompt);
    }

    #[test]
    fn run_snippet_confirm_no_returns_to_picker() {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content("Host h1\n  HostName 1.1.1.1\n"),
            path: scratch.join("cfg"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        app.set_screen(Screen::ConfirmRunSnippet);
        let (tx, _rx) = mpsc::channel();
        handle_run_snippet_confirm_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
            &tx,
        );
        assert!(matches!(app.screen, Screen::SnippetHostPicker));
    }

    #[test]
    fn run_snippet_confirm_yes_with_params_opens_param_form() {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content("Host h1\n  HostName 1.1.1.1\n"),
            path: scratch.join("cfg"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        // A parametrised snippet: Yes prompts for params rather than spawning ssh.
        app.snippets.set_flow_snippet(Some(crate::snippet::Snippet {
            name: "deploy".into(),
            command: "echo {{msg}}".into(),
            description: String::new(),
            interactive: false,
        }));
        app.snippets.set_flow_targets(vec!["h1".into()]);
        app.set_screen(Screen::ConfirmRunSnippet);
        let (tx, _rx) = mpsc::channel();
        handle_run_snippet_confirm_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
            &tx,
        );
        assert!(matches!(app.screen, Screen::SnippetParamForm));
        assert!(app.snippets.form_return_to_tab());
    }
}
