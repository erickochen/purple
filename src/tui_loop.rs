//! TUI event loop and the per-iteration helpers that drive it.
//!
//! Everything that runs while the TUI is on the alternate screen lives
//! here: the main `run_tui` orchestrator, its six tick-scoped helpers
//! (startup tasks, event dispatch, lazy cert check, pending SSH connect,
//! pending snippet run, teardown), plus Vault cert-cache helpers used by
//! the dispatch logic.

use anyhow::Result;

use crate::app::{self, App};
use crate::event::{self, AppEvent, EventHandler};

/// Minimum gap between consecutive `stat()` calls on a host's vault cert
/// inside `lazy_cert_check`. The probe runs on every TUI iteration; without
/// a throttle it would syscall on every 16ms animation tick. A quarter
/// second still detects external writes within a blink while cutting the
/// syscall count by ~95%.
const CERT_STAT_THROTTLE: std::time::Duration = std::time::Duration::from_millis(250);
use crate::ssh_config::model::SshConfigFile;
use crate::{
    animation, askpass, connection, ensure_bw_session, ensure_keychain_password,
    ensure_proton_login, ensure_vault_ssh_chain_if_needed, first_launch_init, handler, import,
    key_activity, ping, preferences, snippet, tui, update, vault_ssh,
};

pub fn run_tui(mut app: App) -> Result<()> {
    // First-launch welcome hint (one-shot: backs up the SSH config so it won't show again)
    if app.status_center.status().is_none() && !app.demo_mode {
        let paths = app.env().paths().cloned();
        if let Some(paths) = paths
            && let Some(has_backup) = first_launch_init(&paths, app.reload.config_path())
        {
            let host_count = app.hosts_state.list().len();
            let known_hosts_count = if host_count == 0 {
                import::count_known_hosts_candidates(Some(&paths))
            } else {
                0
            };
            app.ui.set_known_hosts_count(known_hosts_count);
            app.screen = app::Screen::Welcome {
                has_backup,
                host_count,
                known_hosts_count,
            };
        }
    }

    let mut terminal = tui::Tui::new()?;
    terminal.enter()?;
    let events = EventHandler::new(50);
    let events_tx = events.sender();
    let mut last_config_check = std::time::Instant::now();

    // Skip background tasks in demo mode (ping status is pre-populated).
    if !app.demo_mode {
        spawn_startup_tasks(&mut app, &events_tx);
    }

    let mut anim = animation::AnimationState::new();

    while app.running {
        anim.detect_transitions(&mut app);
        terminal.draw(&mut app, &mut anim)?;

        // During animation, use a short timeout for smooth frames (~60fps).
        // During ping checking, use 80ms timeout for spinner.
        // Otherwise, block until the next event arrives.
        let vault_signing = app.vault.is_signing();
        let provider_syncing = !app.providers.syncing().is_empty();
        // Tunnels tab drives the live chart animation. While at least
        // one tunnel is running we tick at 16ms (~60 fps) so the
        // swimlane bars and sparklines drift smoothly. The tick also
        // refreshes the uptime readout every frame.
        let tunnels_anim_tick =
            matches!(app.top_page, app::TopPage::Tunnels) && !app.tunnels.active().is_empty();
        let event = if anim.is_animating(&app) || tunnels_anim_tick {
            events.next_timeout(std::time::Duration::from_millis(16))?
        } else if anim.has_checking_hosts(&app)
            || vault_signing
            || provider_syncing
            || anim.has_reachable_hosts(&app)
        {
            events.next_timeout(std::time::Duration::from_millis(60))?
        } else {
            Some(events.next()?)
        };

        if dispatch_event(
            &mut app,
            event,
            &mut anim,
            vault_signing,
            &events_tx,
            &mut terminal,
            &mut last_config_check,
        )?
        .is_break()
        {
            continue;
        }

        lazy_cert_check(&mut app, &events_tx);

        handle_pending_connect(&mut app, &mut terminal, &events, &mut last_config_check)?;
        handle_pending_container_exec(&mut app, &mut terminal, &events, &mut last_config_check)?;
        handle_pending_container_logs(&mut app, &events_tx);
        handle_pending_container_action(&mut app, &events_tx);
        // Drain any aliases queued for an initial container-cache
        // fetch (form save, sync, external edit, restore). The
        // helper drains the queue itself and routes the items into
        // the existing `RefreshBatch` driver.
        if app.container_state.has_pending_fetches() {
            handler::containers_overview::auto_fetch_new_hosts(&mut app, &events_tx);
        }
        handle_pending_snippet(&mut app, &mut terminal, &events, &mut last_config_check)?;
    }

    tui_teardown(&mut app, &mut terminal)
}

/// Spawn auto-sync, auto-ping and the background version check on TUI startup.
fn spawn_startup_tasks(app: &mut App, events_tx: &std::sync::mpsc::Sender<AppEvent>) {
    for section in app.providers.config().configured_providers().to_vec() {
        if !section.auto_sync {
            continue;
        }
        let key = section.id.to_string();
        if !app.providers.syncing().contains_key(&key) {
            app.providers.reset_batch_if_idle();
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            app.providers.syncing_mut().insert(key, cancel.clone());
            app.providers.bump_batch_total();
            handler::spawn_provider_sync(
                &section,
                events_tx.clone(),
                cancel,
                std::sync::Arc::clone(&app.env),
            );
            crate::set_sync_summary(app);
        }
    }

    if app.ping.auto_ping() {
        let hosts_to_ping: Vec<(String, String, u16)> = app
            .hosts_state
            .list()
            .iter()
            .filter(|h| !h.hostname.is_empty() && h.proxy_jump.is_empty() && !h.has_proxy_command)
            .map(|h| (h.alias.clone(), h.hostname.clone(), h.port))
            .collect();
        for h in app.hosts_state.list() {
            if !h.proxy_jump.is_empty() || h.has_proxy_command {
                app.ping
                    .insert_status(h.alias.clone(), app::PingStatus::Skipped);
            }
        }
        if !hosts_to_ping.is_empty() {
            for (alias, _, _) in &hosts_to_ping {
                app.ping
                    .insert_status(alias.clone(), app::PingStatus::Checking);
            }
            ping::ping_all(&hosts_to_ping, events_tx.clone(), app.ping.generation());
        }
    }

    update::spawn_version_check(events_tx.clone(), std::sync::Arc::clone(&app.env));

    // Kick off a one-shot cert check for every vault-managed host so the
    // Keys-tab strip populates on startup without the user having to
    // navigate through each host first (or hit R). The actual validation
    // is `ssh-keygen -L`, cheap, runs off-thread, and reuses the same
    // `CertCheckResult` event path as the lazy selection-driven check.
    let paths = app.env().paths().cloned();
    let vault_aliases: Vec<(String, String)> = app
        .hosts_state
        .list()
        .iter()
        .filter(|h| vault_ssh::has_purple_vault_context(h, paths.as_ref()))
        .filter(|h| !app.vault.is_cert_check_in_flight(&h.alias))
        .filter(|h| !app.vault.has_cert(&h.alias))
        .map(|h| (h.alias.clone(), h.certificate_file.clone()))
        .collect();
    for (alias, cert_file) in vault_aliases {
        app.vault.mark_cert_check_started(alias.clone());
        let tx = events_tx.clone();
        let env = std::sync::Arc::clone(&app.env);
        std::thread::spawn(move || {
            let check_path = match vault_ssh::resolve_cert_path(env.paths(), &alias, &cert_file) {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(event::AppEvent::CertCheckError {
                        alias,
                        message: e.to_string(),
                    });
                    return;
                }
            };
            let status = vault_ssh::check_cert_validity(&env, &check_path);
            let _ = tx.send(event::AppEvent::CertCheckResult { alias, status });
        });
    }
}

/// Dispatch a single tick's event. Returns `Break` when the outer loop
/// should `continue` without running the post-dispatch helpers.
#[allow(clippy::too_many_arguments)]
fn dispatch_event(
    app: &mut App,
    event: Option<AppEvent>,
    anim: &mut animation::AnimationState,
    vault_signing: bool,
    events_tx: &std::sync::mpsc::Sender<AppEvent>,
    terminal: &mut tui::Tui,
    last_config_check: &mut std::time::Instant,
) -> Result<std::ops::ControlFlow<()>> {
    match event {
        Some(AppEvent::Key(key)) => {
            handler::handle_key_event(app, key, events_tx)?;
        }
        Some(AppEvent::Tick) | None => {
            handler::event_loop::handle_tick(app, anim, vault_signing, last_config_check);
        }
        Some(AppEvent::PingResult {
            alias,
            rtt_ms,
            generation,
        }) => {
            handler::event_loop::handle_ping_result(app, alias, rtt_ms, generation);
        }
        Some(AppEvent::SyncProgress { provider, message }) => {
            handler::event_loop::handle_sync_progress(app, provider, message);
        }
        Some(AppEvent::SyncComplete { provider, hosts }) => {
            handler::event_loop::handle_sync_complete(app, provider, hosts, last_config_check);
        }
        Some(AppEvent::SyncPartial {
            provider,
            hosts,
            failures,
            total,
        }) => {
            handler::event_loop::handle_sync_partial(
                app,
                provider,
                hosts,
                failures,
                total,
                last_config_check,
            );
        }
        Some(AppEvent::SyncError { provider, message }) => {
            handler::event_loop::handle_sync_error(app, provider, message, last_config_check);
        }
        Some(AppEvent::UpdateAvailable { version, headline }) => {
            handler::event_loop::handle_update_available(app, version, headline);
        }
        Some(AppEvent::FileBrowserListing {
            alias,
            path,
            entries,
        }) => {
            handler::event_loop::handle_file_browser_listing(app, alias, path, entries, terminal);
        }
        Some(AppEvent::ScpComplete {
            alias,
            success,
            message,
        }) => {
            handler::event_loop::handle_scp_complete(
                app, alias, success, message, events_tx, terminal,
            );
        }
        Some(AppEvent::SnippetHostDone {
            run_id,
            alias,
            stdout,
            stderr,
            exit_code,
        }) => {
            handler::event_loop::handle_snippet_host_done(
                app, run_id, alias, stdout, stderr, exit_code,
            );
        }
        Some(AppEvent::SnippetProgress {
            run_id,
            completed,
            total,
        }) => {
            handler::event_loop::handle_snippet_progress(app, run_id, completed, total);
        }
        Some(AppEvent::SnippetAllDone { run_id }) => {
            handler::event_loop::handle_snippet_all_done(app, run_id);
        }
        Some(AppEvent::KeyPushResult { run_id, result }) => {
            handler::event_loop::handle_key_push_result(app, run_id, result);
        }
        Some(AppEvent::ContainerListing { alias, result }) => {
            handler::event_loop::handle_container_listing(app, alias, result, events_tx);
        }
        Some(AppEvent::ContainerActionComplete {
            alias,
            action,
            result,
        }) => {
            handler::event_loop::handle_container_action_complete(
                app, alias, action, result, events_tx,
            );
        }
        Some(AppEvent::ContainerLogsComplete {
            alias,
            container_id,
            container_name,
            result,
        }) => {
            handler::event_loop::handle_container_logs_complete(
                app,
                alias,
                container_id,
                container_name,
                result,
            );
        }
        Some(AppEvent::ContainerInspectComplete {
            alias,
            container_id,
            result,
        }) => {
            handler::event_loop::handle_container_inspect_complete(
                app,
                alias,
                container_id,
                *result,
            );
        }
        Some(AppEvent::ContainerLogsTailComplete {
            alias,
            container_id,
            result,
        }) => {
            handler::event_loop::handle_container_logs_tail_complete(
                app,
                alias,
                container_id,
                *result,
            );
        }
        Some(AppEvent::VaultSignResult {
            alias,
            certificate_file: existing_cert_file,
            success,
            message,
        }) => {
            handler::event_loop::handle_vault_sign_result(
                app,
                alias,
                existing_cert_file,
                success,
                message,
            );
        }
        Some(AppEvent::VaultSignProgress { alias, done, total }) => {
            handler::event_loop::handle_vault_sign_progress(
                app,
                alias,
                done,
                total,
                anim.spinner_tick,
            );
        }
        Some(AppEvent::VaultSignAllDone {
            signed,
            failed,
            skipped,
            cancelled,
            aborted_message,
            first_error,
        }) => {
            if handler::event_loop::handle_vault_sign_all_done(
                app,
                signed,
                failed,
                skipped,
                cancelled,
                aborted_message,
                first_error,
            )
            .is_break()
            {
                return Ok(std::ops::ControlFlow::Break(()));
            }
        }
        Some(AppEvent::CertCheckResult { alias, status }) => {
            handler::event_loop::handle_cert_check_result(app, alias, status);
        }
        Some(AppEvent::CertCheckError { alias, message }) => {
            handler::event_loop::handle_cert_check_error(app, alias, message);
        }
        Some(AppEvent::PollError) => {
            app.running = false;
        }
    }
    Ok(std::ops::ControlFlow::Continue(()))
}

/// When the selected host has a vault role and the cached cert status is
/// missing, stale or has been touched externally, spawn a background check.
fn lazy_cert_check(app: &mut App, events_tx: &std::sync::mpsc::Sender<AppEvent>) {
    // Snapshot the selected host's vault-relevant fields so the immutable
    // borrow on `app` ends here, freeing `app.vault` for the throttle write.
    // `has_purple_cert_file` also covers CLI-signed certs that lack the role
    // marker: without that branch the TTL gauge would stay empty for them.
    let Some((alias, certificate_file, has_vault_role, has_purple_cert_file)) =
        app.selected_host().map(|s| {
            let role = vault_ssh::resolve_vault_role(
                s.vault_ssh.as_deref(),
                s.provider.as_deref(),
                s.provider_label.as_deref(),
                app.providers.config(),
            )
            .is_some();
            let cert_file =
                vault_ssh::cert_file_in_purple_dir(&s.certificate_file, app.env().paths());
            (s.alias.clone(), s.certificate_file.clone(), role, cert_file)
        })
    else {
        return;
    };
    if !(has_vault_role || has_purple_cert_file) {
        return;
    }

    // Stat the cert file at most once per `CERT_STAT_THROTTLE` so the
    // per-frame freshness probe detects external writes (CLI sign,
    // another purple instance) without a syscall on every 16ms tick.
    // Compared against the mtime recorded when the cache entry was
    // populated; any mismatch forces a re-check, no matter the TTL.
    let now = std::time::Instant::now();
    let recently_stat = app
        .vault
        .last_cert_stat(&alias)
        .is_some_and(|t| now.duration_since(t) < CERT_STAT_THROTTLE);
    let current_mtime = if recently_stat {
        app.vault
            .cert_entry(&alias)
            .and_then(|(_, _, mtime)| *mtime)
    } else {
        let m = vault_ssh::resolve_cert_path(app.env().paths(), &alias, &certificate_file)
            .ok()
            .and_then(|p| std::fs::metadata(&p).ok())
            .and_then(|m| m.modified().ok());
        app.vault.note_cert_stat(alias.clone(), now);
        m
    };
    let cache_stale = cache_entry_is_stale(app.vault.cert_entry(&alias), current_mtime, |t| {
        t.elapsed().as_secs()
    });

    let sign_in_flight = app
        .vault
        .sign_in_flight()
        .lock()
        .map(|g| g.contains(&alias))
        .unwrap_or(false);
    if cache_stale && !app.vault.is_cert_check_in_flight(&alias) && !sign_in_flight {
        app.vault.mark_cert_check_started(alias.clone());
        let tx = events_tx.clone();
        let env = std::sync::Arc::clone(&app.env);
        std::thread::spawn(move || {
            let check_path =
                match vault_ssh::resolve_cert_path(env.paths(), &alias, &certificate_file) {
                    Ok(p) => p,
                    Err(e) => {
                        let _ = tx.send(event::AppEvent::CertCheckError {
                            alias,
                            message: e.to_string(),
                        });
                        return;
                    }
                };
            let status = vault_ssh::check_cert_validity(&env, &check_path);
            let _ = tx.send(event::AppEvent::CertCheckResult { alias, status });
        });
    }
}

/// Drain any queued SSH connection request. In tmux mode we open a new
/// window and leave the TUI alive; otherwise we suspend the TUI, run ssh
/// inline, then restore it. Vault SSH signing and askpass pre-flight
/// (Bitwarden, keychain) run on the bare terminal to allow prompts.
fn handle_pending_connect(
    app: &mut App,
    terminal: &mut tui::Tui,
    events: &EventHandler,
    last_config_check: &mut std::time::Instant,
) -> Result<()> {
    let Some((alias, host_askpass)) = app.ui.take_pending_connect() else {
        return Ok(());
    };
    let vault_host = app
        .hosts_state
        .list()
        .iter()
        .find(|h| h.alias == alias)
        .cloned();
    let askpass = host_askpass.or_else(|| preferences::load_askpass_default(app.env().paths()));
    let has_active_tunnel = app.tunnels.active_contains(&alias);
    let use_tmux = connection::is_in_tmux(app.env()) && askpass.is_none();
    let launcher = crate::ssh_launcher::SshLauncher::resolve(app.env());

    if use_tmux {
        // Tmux mode: open SSH in a new tmux window. TUI stays alive.
        // Vault SSH cert signing runs first (eprintln warnings are harmless
        // on the alternate screen. ratatui repaints over them on the next
        // draw cycle). Sign the entire ProxyJump chain so the proxy hop's
        // cert is in place before ssh tries to use it.
        let vault_msg = if vault_host.is_some() {
            let env = std::sync::Arc::clone(&app.env);
            let msg = ensure_vault_ssh_chain_if_needed(
                &env,
                &alias,
                app.reload.config_path(),
                app.providers.config(),
                app.hosts_state.ssh_config_mut(),
            );
            if msg.is_some() {
                app.reload_hosts();
                for hop in vault_ssh::resolve_proxy_chain(app.reload.config_path(), &alias) {
                    app.refresh_cert_cache(&hop);
                }
            }
            msg
        } else {
            None
        };

        match connection::connect_tmux_window(
            &alias,
            app.reload.config_path(),
            has_active_tunnel,
            &launcher,
        ) {
            Ok(()) => {
                app.record_key_use(&alias, key_activity::now_secs());
                if let Some((ref msg, is_error)) = vault_msg {
                    if is_error {
                        app.notify_error(msg.clone());
                    } else {
                        app.notify(msg.clone());
                    }
                } else {
                    app.notify(crate::messages::opened_in_tmux(&alias));
                }
            }
            Err(e) => {
                app.notify_error(crate::messages::tmux_error(&e));
            }
        }
        return Ok(());
    }

    // Standard mode: suspend TUI, run SSH inline, restore TUI.
    // Order preserved: pause events, exit TUI, THEN run vault signing and
    // password setup (which may eprintln or prompt for input on the real
    // terminal). Sign the entire ProxyJump chain so the proxy hop's cert is
    // in place before ssh tries to use it.
    events.pause();
    terminal.suspend()?;
    let vault_msg = if vault_host.is_some() {
        let env = std::sync::Arc::clone(&app.env);
        let msg = ensure_vault_ssh_chain_if_needed(
            &env,
            &alias,
            app.reload.config_path(),
            app.providers.config(),
            app.hosts_state.ssh_config_mut(),
        );
        if msg.is_some() {
            app.reload_hosts();
            for hop in vault_ssh::resolve_proxy_chain(app.reload.config_path(), &alias) {
                app.refresh_cert_cache(&hop);
            }
        }
        msg
    } else {
        None
    };
    let env = std::sync::Arc::clone(&app.env);
    ensure_proton_login(&env, askpass.as_deref());
    if let Some(token) = ensure_bw_session(&env, app.bw_session.as_deref(), askpass.as_deref()) {
        app.bw_session = Some(token);
    }
    ensure_keychain_password(&env, &alias, askpass.as_deref());
    print!("{}", crate::messages::cli::beaming_up(&alias));
    let result = connection::connect(
        &alias,
        app.reload.config_path(),
        askpass.as_deref(),
        app.bw_session.as_deref(),
        has_active_tunnel,
        &launcher,
    );
    println!();
    match &result {
        Ok(cr) => {
            let code = cr.status.code().unwrap_or(1);
            if code != 255 {
                app.history.record(&alias);
                app.record_key_use(&alias, key_activity::now_secs());
                app.hosts_state.invalidate_render_cache();
            }
            if code != 0 {
                if let Some((hostname, known_hosts_path)) =
                    connection::parse_host_key_error(&cr.stderr_output)
                {
                    app.screen = app::Screen::ConfirmHostKeyReset {
                        alias: alias.clone(),
                        hostname,
                        known_hosts_path,
                        askpass,
                    };
                } else {
                    // A failed Vault sign that came alongside a failed SSH
                    // is almost always the CAUSE of the SSH failure (no cert
                    // → permission denied). Surface the vault error first so
                    // the user can fix the actual problem; otherwise they
                    // chase the generic ssh error.
                    if let Some((ref vmsg, true)) = vault_msg {
                        app.notify_error(vmsg.clone());
                    }
                    let reason = connection::stderr_summary(&cr.stderr_output);
                    let msg = if let Some(reason) = reason {
                        crate::messages::ssh_failed_with_reason(&alias, &reason)
                    } else {
                        crate::messages::ssh_exited_with_code(&alias, code)
                    };
                    app.notify_error(msg);
                }
            } else if let Some((ref msg, is_error)) = vault_msg {
                if is_error {
                    app.notify_error(msg.clone());
                } else {
                    app.notify(msg.clone());
                }
            }
        }
        Err(e) => {
            log::error!("[external] ssh connect failed: alias={alias}: {e}");
            eprintln!("{}", crate::messages::connection_spawn_failed(&e));
            app.notify_error(crate::messages::connection_failed(&alias));
        }
    }
    askpass::cleanup_marker(app.env.paths(), &alias);
    terminal.enter()?;
    events.resume();
    *last_config_check = std::time::Instant::now();
    let reloaded = SshConfigFile::parse_with_env(app.reload.config_path(), app.env())?;
    app.hosts_state.set_ssh_config(reloaded);
    app.reload_hosts();
    app.update_last_modified();
    Ok(())
}

/// Drain any queued container-exec request. Same lifecycle as
/// `handle_pending_connect` but the spawned command is
/// `ssh -t <alias> <runtime> exec -it <id> sh -c 'bash || sh'` instead
/// of a plain shell login.
fn handle_pending_container_exec(
    app: &mut App,
    terminal: &mut tui::Tui,
    events: &EventHandler,
    last_config_check: &mut std::time::Instant,
) -> Result<()> {
    let Some(req) = app.container_state.take_pending_exec() else {
        return Ok(());
    };

    // Defense-in-depth: container_id is currently gated by
    // `validate_running_row` (which calls validate_container_id)
    // before pending_container_exec is populated. This second validation
    // covers any future entry point (MCP tool call, paste-via-jump, etc.)
    // that might populate the request without going through that gate.
    if let Err(e) = crate::containers::validate_container_id(&req.container_id) {
        log::warn!(
            "[purple] container exec blocked on '{}': invalid container_id: {}",
            req.alias,
            e
        );
        app.notify(crate::messages::container_invalid_id(&e));
        return Ok(());
    }

    let askpass = req
        .askpass
        .or_else(|| preferences::load_askpass_default(app.env().paths()));
    let has_active_tunnel = app.tunnels.active_contains(&req.alias);
    let use_tmux = connection::is_in_tmux(app.env()) && askpass.is_none();
    let launcher = crate::ssh_launcher::SshLauncher::resolve(app.env());

    let remote_cmd = if let Some(ref user_cmd) = req.command {
        // User-typed exec command from the `e` prompt. The remote runs
        // `sh -c '<cmd>'`; embedded single-quotes are escaped as the
        // standard POSIX `'\''` so the wrapping quotes survive a token
        // like `it's-fine`. The prompt handler already rejects newlines.
        let escaped = user_cmd.replace('\'', "'\\''");
        format!(
            "{} exec -it {} sh -c '{}'",
            req.runtime.as_str(),
            req.container_id,
            escaped
        )
    } else {
        format!(
            "{} exec -it {} sh -c 'bash || sh'",
            req.runtime.as_str(),
            req.container_id
        )
    };

    if use_tmux {
        let label = format!("{}/{}", req.alias, req.container_name);
        match connection::connect_tmux_window_with_remote_command(
            &req.alias,
            app.reload.config_path(),
            app.env(),
            has_active_tunnel,
            &remote_cmd,
            &label,
            &launcher,
        ) {
            Ok(()) => {
                app.record_key_use(&req.alias, key_activity::now_secs());
                app.notify(crate::messages::container_exec_opened_in_tmux(
                    &req.container_name,
                    &req.alias,
                ));
            }
            Err(e) => {
                app.notify_error(crate::messages::tmux_error(&e));
            }
        }
        return Ok(());
    }

    events.pause();
    terminal.suspend()?;

    let result = connection::connect_with_remote_command(
        &req.alias,
        app.reload.config_path(),
        app.env(),
        askpass.as_deref(),
        app.bw_session.as_deref(),
        has_active_tunnel,
        &remote_cmd,
        &launcher,
    );

    match result {
        Ok(cr) => {
            let code = cr.status.code().unwrap_or(1);
            // SSH exit 255 = ssh itself failed (auth, network, host-key
            // mismatch); anything else means ssh connected and the
            // remote command exited with that code. Recording history
            // for non-255 mirrors the host-list connect flow so a
            // mid-shell crash still counts as a successful login.
            if code != 255 {
                app.history.record(&req.alias);
                app.record_key_use(&req.alias, key_activity::now_secs());
                app.hosts_state.invalidate_render_cache();
            }
            if code == 0 {
                app.notify(crate::messages::container_exec_ended(&req.container_name));
            } else if let Some((hostname, known_hosts_path)) =
                connection::parse_host_key_error(&cr.stderr_output)
            {
                // Same recovery surface as the host-list `i` path:
                // park the user on ConfirmHostKeyReset so they can
                // delete the stale known_hosts entry and retry.
                app.screen = app::Screen::ConfirmHostKeyReset {
                    alias: req.alias.clone(),
                    hostname,
                    known_hosts_path,
                    askpass: askpass.clone(),
                };
            } else {
                let reason = connection::stderr_summary(&cr.stderr_output);
                let msg = match reason {
                    Some(r) => {
                        crate::messages::container_exec_failed_with_reason(&req.container_name, &r)
                    }
                    None => {
                        crate::messages::container_exec_exited_with_code(&req.container_name, code)
                    }
                };
                app.notify_error(msg);
            }
        }
        Err(e) => {
            eprintln!("{}", crate::messages::connection_spawn_failed(&e));
            app.notify_error(crate::messages::container_exec_spawn_failed(
                &req.container_name,
            ));
        }
    }
    askpass::cleanup_marker(app.env.paths(), &req.alias);
    terminal.enter()?;
    events.resume();
    *last_config_check = std::time::Instant::now();
    Ok(())
}

/// Drain `pending_container_logs`. Spawns a background SSH
/// thread that runs `<runtime> logs --tail N <id>` and emits an
/// `AppEvent::ContainerLogsComplete` with the captured output. The
/// receiving handler in `event_loop.rs` fills the open
/// `Screen::ContainerLogs` overlay's body.
fn handle_pending_container_logs(app: &mut App, events_tx: &std::sync::mpsc::Sender<AppEvent>) {
    let Some(req) = app.container_state.take_pending_logs() else {
        return;
    };
    let askpass = req
        .askpass
        .or_else(|| preferences::load_askpass_default(app.env().paths()));
    let ctx = app.ssh_context_for(req.alias, askpass);
    let tx = events_tx.clone();
    log::debug!(
        "[purple] container_logs_fetch: spawning alias={} id={}",
        ctx.alias,
        req.container_id
    );
    crate::containers::spawn_container_logs_fetch(
        ctx,
        req.runtime,
        req.container_id,
        req.container_name,
        crate::handler::container_logs::DEFAULT_TAIL,
        move |alias, container_id, container_name, result| {
            let _ = tx.send(AppEvent::ContainerLogsComplete {
                alias,
                container_id,
                container_name,
                result,
            });
        },
    );
}

/// Drain `pending_container_action`. Reuses the existing
/// `spawn_container_action` helper and `AppEvent::ContainerActionComplete`
/// event so the result handler can stay one path. The action's
/// container_id+name are logged here; the toast on completion uses
/// the alias because the existing event payload does not carry the
/// per-container labels.
fn handle_pending_container_action(app: &mut App, events_tx: &std::sync::mpsc::Sender<AppEvent>) {
    // Drain at most one action per tick. Stack-restart pushes N
    // requests but the SSH workers should not all sprint off the
    // same tick. staggering keeps load on the remote sshd lower.
    let Some(req) = app.container_state.pop_next_action() else {
        return;
    };
    let askpass = req
        .askpass
        .or_else(|| preferences::load_askpass_default(app.env().paths()));
    let ctx = app.ssh_context_for(req.alias.clone(), askpass);
    let tx = events_tx.clone();
    log::info!(
        "[purple] container_action_drain: spawning alias={} id={} action={:?} name={}",
        req.alias,
        req.container_id,
        req.action,
        req.container_name
    );
    crate::containers::spawn_container_action(
        ctx,
        req.runtime,
        req.action,
        req.container_id,
        move |alias, action, result| {
            let _ = tx.send(AppEvent::ContainerActionComplete {
                alias,
                action,
                result,
            });
        },
    );
}

/// Drain any queued snippet-run request: suspend the TUI, run the command
/// across all selected hosts, record history on success, wait for Enter,
/// then restore the TUI and reload the SSH config.
fn handle_pending_snippet(
    app: &mut App,
    terminal: &mut tui::Tui,
    events: &EventHandler,
    last_config_check: &mut std::time::Instant,
) -> Result<()> {
    let Some((snip, aliases)) = app.snippets.take_pending() else {
        return Ok(());
    };
    events.pause();
    terminal.suspend()?;

    let multi = aliases.len() > 1;
    let mut ok_count = 0usize;
    for alias in &aliases {
        let askpass = app
            .hosts_state
            .list()
            .iter()
            .find(|h| h.alias == *alias)
            .and_then(|h| h.askpass.clone())
            .or_else(|| preferences::load_askpass_default(app.env().paths()));
        let env = std::sync::Arc::clone(&app.env);
        ensure_proton_login(&env, askpass.as_deref());
        if let Some(token) = ensure_bw_session(&env, app.bw_session.as_deref(), askpass.as_deref())
        {
            app.bw_session = Some(token);
        }
        ensure_keychain_password(&env, alias, askpass.as_deref());

        if multi {
            println!("{}", crate::messages::cli::host_separator(alias));
        } else {
            print!(
                "{}",
                crate::messages::cli::running_snippet_on(&snip.name, alias)
            );
        }
        let has_tunnel = app.tunnels.active_contains(alias);
        match snippet::run_snippet(
            alias,
            app.reload.config_path(),
            &env,
            &snip.remote_command(),
            askpass.as_deref(),
            app.bw_session.as_deref(),
            false,
            has_tunnel,
        ) {
            Ok(r) => {
                if snippet::not_found_hint_applies(snip.interactive, r.status.code()) {
                    eprintln!(
                        "{}",
                        crate::i18n::text(crate::messages::SNIPPET_NOT_FOUND_HINT)
                    );
                }
                if r.status.success() {
                    ok_count += 1;
                    app.history.record(alias);
                    app.record_key_use(alias, key_activity::now_secs());
                    app.hosts_state.invalidate_render_cache();
                } else if multi {
                    eprintln!(
                        "{}",
                        crate::messages::cli::exited_with_code(r.status.code().unwrap_or(1))
                    );
                } else {
                    println!(
                        "\n{}",
                        crate::messages::cli::exited_with_code(r.status.code().unwrap_or(1))
                    );
                }
            }
            Err(e) => eprintln!("{}", crate::messages::cli::host_failed(alias, &e)),
        }
        // This run is over, so drop the retry marker it may have armed. Left
        // behind, it would read as a rejected password on the next run within
        // the minute and send ssh to the tty instead of the stored one.
        askpass::cleanup_marker(app.env.paths(), alias);
        if multi {
            println!();
        }
    }

    // Record the terminal-mode run in the per-snippet ledger so the detail
    // TRACK RECORD verdict and trend chart reflect it.
    let now = key_activity::now_secs();
    let paths = app.env().paths().cloned();
    app.snippets.runs_mut().record_run_and_flush(
        &snip.name,
        aliases.len(),
        ok_count,
        now,
        paths.as_ref(),
    );

    if !multi {
        println!("\n{}", crate::i18n::text(crate::messages::cli::DONE));
    } else {
        println!(
            "{}",
            crate::messages::cli::done_multi(&snip.name, aliases.len())
        );
    }
    println!("\n{}", crate::i18n::text(crate::messages::cli::PRESS_ENTER));
    let _ = std::io::stdin().read_line(&mut String::new());
    terminal.enter()?;
    events.resume();
    *last_config_check = std::time::Instant::now();
    // Reload so sort order (e.g. most recent) reflects the new history.
    let reloaded = SshConfigFile::parse_with_env(app.reload.config_path(), app.env())?;
    app.hosts_state.set_ssh_config(reloaded);
    app.reload_hosts();
    app.update_last_modified();
    Ok(())
}

/// Flush any deferred vault-config writes, join the background signing
/// thread and kill active tunnels before leaving the TUI.
fn tui_teardown(app: &mut App, terminal: &mut tui::Tui) -> Result<()> {
    app.flush_pending_vault_write();

    if let Some(handle) = app.vault.cancel_signing_run() {
        let _ = handle.join();
    }

    for (_, mut tunnel) in app.tunnels.drain_active() {
        let _ = tunnel.child.kill();
        let _ = tunnel.child.wait();
    }

    terminal.exit()?;
    Ok(())
}

pub(crate) fn current_cert_mtime(alias: &str, app: &app::App) -> Option<std::time::SystemTime> {
    let host = app.hosts_state.list().iter().find(|h| h.alias == alias)?;
    let cert_path =
        vault_ssh::resolve_cert_path(app.env().paths(), alias, &host.certificate_file).ok()?;
    std::fs::metadata(&cert_path)
        .ok()
        .and_then(|m| m.modified().ok())
}

/// Decide whether a `vault.cert_cache` entry should be re-checked.
///
/// Returns true when:
/// - there is no cached entry at all, or
/// - the cert file's current mtime differs from the cached mtime
///   (an external actor signed or deleted the cert behind our back), or
/// - the entry's age exceeds its TTL. `CertStatus::Invalid` uses a shorter
///   backoff so transient errors recover quickly without hammering the
///   background check thread on every poll tick.
///
/// The `elapsed_secs` closure is taken as a parameter so tests can inject
/// deterministic elapsed times instead of calling the real clock.
pub(crate) fn cache_entry_is_stale<F>(
    entry: Option<&(
        std::time::Instant,
        vault_ssh::CertStatus,
        Option<std::time::SystemTime>,
    )>,
    current_mtime: Option<std::time::SystemTime>,
    elapsed_secs: F,
) -> bool
where
    F: FnOnce(std::time::Instant) -> u64,
{
    let Some((checked_at, status, cached_mtime)) = entry else {
        return true;
    };
    if current_mtime != *cached_mtime {
        return true;
    }
    let ttl = if matches!(status, vault_ssh::CertStatus::Invalid(_)) {
        vault_ssh::CERT_ERROR_BACKOFF_SECS
    } else {
        vault_ssh::CERT_STATUS_CACHE_TTL_SECS
    };
    elapsed_secs(*checked_at) > ttl
}
