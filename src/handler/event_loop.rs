use std::sync::mpsc;
use std::time::Instant;

use crate::app::{self, App};
use crate::containers;
use crate::event::AppEvent;
use crate::file_browser;
use crate::providers;
use crate::ssh_config;
use crate::tui;
use crate::vault_ssh;

/// Handle `AppEvent::Tick` and `None` (timeout): spinner animation, ping TTL
/// expiry, config change detection and tunnel exit polling.
pub(crate) fn handle_tick(
    app: &mut App,
    anim: &mut crate::animation::AnimationState,
    vault_signing: bool,
    last_config_check: &mut Instant,
) {
    app.tick_status();
    app.tick_toast();
    if anim.has_checking_hosts(app) || vault_signing {
        anim.tick_spinner();
    }
    // Update the spinner character in the signing status text
    // so the spinner animates between VaultSignProgress events.
    if vault_signing {
        if let Some(ref mut status) = app.status {
            if status.sticky && !status.is_error() {
                let frame = crate::animation::SPINNER_FRAMES
                    [anim.spinner_tick as usize % crate::animation::SPINNER_FRAMES.len()];
                if let Some(updated) = crate::replace_spinner_frame(&status.text, frame) {
                    status.text = updated;
                }
            }
        }
    }
    // Expire ping results after 60s TTL
    if let Some(checked_at) = app.ping.checked_at {
        if checked_at.elapsed() > std::time::Duration::from_secs(60) {
            app.ping.status.clear();
            app.ping.checked_at = None;
            app.ping.generation += 1;
            if app.ping.filter_down_only {
                app.cancel_search();
            }
            app.set_background_status("Ping expired. Press P to refresh.", false);
        }
    }
    // Throttle config file stat() to every 4 seconds
    if last_config_check.elapsed() >= std::time::Duration::from_secs(4) {
        app.check_config_changed();
        *last_config_check = Instant::now();
    }
    // Poll active tunnels for exit
    let exited = app.poll_tunnels();
    for (_alias, msg, is_error) in exited {
        app.set_background_status(msg, is_error);
    }
}

/// Handle `AppEvent::PingResult`.
pub(crate) fn handle_ping_result(
    app: &mut App,
    alias: String,
    rtt_ms: Option<u32>,
    generation: u64,
) {
    if generation == app.ping.generation {
        let status = app::classify_ping(rtt_ms, app.ping.slow_threshold_ms);
        app.ping.status.insert(alias.clone(), status.clone());
        // Propagate bastion status to all ProxyJump dependents.
        app::propagate_ping_to_dependents(&app.hosts, &mut app.ping.status, &alias, &status);
        // Update live filter/sort as results arrive
        if app.ping.filter_down_only {
            app.apply_filter();
        }
        if app.sort_mode == app::SortMode::Status {
            app.apply_sort();
        }
        // Update "last checked" timestamp when all pings are done
        if !app.ping.status.is_empty()
            && app
                .ping
                .status
                .values()
                .all(|s| !matches!(s, app::PingStatus::Checking))
        {
            app.ping.checked_at = Some(Instant::now());
        }
    }
}

/// Handle `AppEvent::SyncProgress`.
pub(crate) fn handle_sync_progress(app: &mut App, provider: String, message: String) {
    // Only show per-provider progress while that provider is still syncing.
    // Late progress events (arriving after SyncComplete) are discarded.
    if app.syncing_providers.contains_key(&provider) && app.sync_done.is_empty() {
        let name = providers::provider_display_name(&provider);
        app.set_background_status(format!("{}: {}", name, message), false);
    }
}

/// Handle `AppEvent::SyncComplete`. Returns the new `last_config_check` value.
pub(crate) fn handle_sync_complete(
    app: &mut App,
    provider: String,
    hosts: Vec<crate::providers::ProviderHost>,
    last_config_check: &mut Instant,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let display_name = providers::provider_display_name(&provider);
    let (_msg, is_err, total, added, updated, stale) =
        app.apply_sync_result(&provider, hosts, false);
    if is_err {
        app.sync_history.insert(
            provider.clone(),
            app::SyncRecord {
                timestamp: now,
                message: format!("{}: sync failed", display_name),
                is_error: true,
            },
        );
        app.sync_had_errors = true;
    } else {
        let label = if total == 1 { "server" } else { "servers" };
        let message = format!(
            "{} {}{}",
            total,
            label,
            crate::format_sync_diff(added, updated, stale)
        );
        app.sync_history.insert(
            provider.clone(),
            app::SyncRecord {
                timestamp: now,
                message,
                is_error: false,
            },
        );
    }
    app.syncing_providers.remove(&provider);
    app.sync_done.push(display_name.to_string());
    crate::set_sync_summary(app);
    // Reset config check timer so auto-reload doesn't immediately
    // detect our own write as an "external" change
    *last_config_check = Instant::now();
}

/// Handle `AppEvent::SyncPartial`.
pub(crate) fn handle_sync_partial(
    app: &mut App,
    provider: String,
    hosts: Vec<crate::providers::ProviderHost>,
    failures: usize,
    total: usize,
    last_config_check: &mut Instant,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let display_name = providers::provider_display_name(provider.as_str());
    let (msg, is_err, synced, added, updated, stale) =
        app.apply_sync_result(&provider, hosts, true);
    if is_err {
        app.sync_history.insert(
            provider.clone(),
            app::SyncRecord {
                timestamp: now,
                message: msg,
                is_error: true,
            },
        );
    } else {
        let label = if synced == 1 { "server" } else { "servers" };
        app.sync_history.insert(
            provider.clone(),
            app::SyncRecord {
                timestamp: now,
                message: format!(
                    "{} {}{} ({} of {} failed)",
                    synced,
                    label,
                    crate::format_sync_diff(added, updated, stale),
                    failures,
                    total
                ),
                is_error: true,
            },
        );
    }
    app.sync_had_errors = true;
    app.syncing_providers.remove(&provider);
    app.sync_done.push(display_name.to_string());
    crate::set_sync_summary(app);
    *last_config_check = Instant::now();
}

/// Handle `AppEvent::SyncError`.
pub(crate) fn handle_sync_error(
    app: &mut App,
    provider: String,
    message: String,
    last_config_check: &mut Instant,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let display_name = providers::provider_display_name(provider.as_str());
    app.sync_history.insert(
        provider.clone(),
        app::SyncRecord {
            timestamp: now,
            message: message.clone(),
            is_error: true,
        },
    );
    app.sync_had_errors = true;
    app.syncing_providers.remove(&provider);
    app.sync_done.push(display_name.to_string());
    crate::set_sync_summary(app);
    *last_config_check = Instant::now();
}

/// Handle `AppEvent::UpdateAvailable`.
pub(crate) fn handle_update_available(app: &mut App, version: String, headline: Option<String>) {
    app.update.available = Some(version);
    app.update.headline = headline;
}

/// Handle `AppEvent::FileBrowserListing`.
pub(crate) fn handle_file_browser_listing(
    app: &mut App,
    alias: String,
    path: String,
    entries: Result<Vec<crate::file_browser::FileEntry>, String>,
    terminal: &mut tui::Tui,
) {
    let mut record_connection = false;
    if let Some(ref mut fb) = app.file_browser {
        if fb.alias == alias {
            fb.remote_loading = false;
            match entries {
                Ok(listing) => {
                    if !fb.connection_recorded {
                        fb.connection_recorded = true;
                        record_connection = true;
                    }
                    if fb.remote_path.is_empty() || fb.remote_path != path {
                        fb.remote_path = path;
                    }
                    fb.remote_entries = listing;
                    fb.remote_error = None;
                    fb.remote_list_state = ratatui::widgets::ListState::default();
                    fb.remote_list_state.select(Some(0));
                }
                Err(msg) => {
                    if fb.remote_path.is_empty() {
                        fb.remote_path = path;
                    }
                    fb.remote_error = Some(msg);
                    fb.remote_entries.clear();
                }
            }
        }
    }
    if record_connection {
        app.history.record(&alias);
        app.apply_sort();
    }
    // Force full redraw: ssh may have written to /dev/tty
    terminal.force_redraw();
}

/// Handle `AppEvent::ScpComplete`.
pub(crate) fn handle_scp_complete(
    app: &mut App,
    alias: String,
    success: bool,
    message: String,
    events_tx: &mpsc::Sender<AppEvent>,
    terminal: &mut tui::Tui,
) {
    // Track whether we need to spawn a remote refresh (can't do it inside the fb borrow
    // because spawn_remote_listing needs values from app too)
    let mut refresh_remote: Option<(
        String,
        Option<String>,
        String,
        bool,
        file_browser::BrowserSort,
    )> = None;
    let matched = if let Some(ref mut fb) = app.file_browser {
        if fb.alias == alias {
            fb.transferring = None;
            if success {
                app.history.record(&alias);
                fb.local_selected.clear();
                fb.remote_selected.clear();
                match file_browser::list_local(&fb.local_path, fb.show_hidden, fb.sort) {
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
                if !fb.remote_path.is_empty() {
                    fb.remote_loading = true;
                    fb.remote_entries.clear();
                    fb.remote_error = None;
                    fb.remote_list_state = ratatui::widgets::ListState::default();
                    refresh_remote = Some((
                        fb.alias.clone(),
                        fb.askpass.clone(),
                        fb.remote_path.clone(),
                        fb.show_hidden,
                        fb.sort,
                    ));
                }
            } else {
                fb.transfer_error = Some(message.clone());
            }
            true
        } else {
            false
        }
    } else {
        false
    };
    if matched && success {
        app.set_background_status("Transfer complete.", false);
        // Rebuild display list so frecency sort and LAST column reflect the transfer
        app.apply_sort();
    }
    if let Some((fb_alias, askpass_fb, path, show_hidden, sort)) = refresh_remote {
        let has_tunnel = app.active_tunnels.contains_key(&fb_alias);
        let ctx = crate::ssh_context::OwnedSshContext {
            alias: fb_alias,
            config_path: app.reload.config_path.clone(),
            askpass: askpass_fb,
            bw_session: app.bw_session.clone(),
            has_tunnel,
        };
        let tx = events_tx.clone();
        file_browser::spawn_remote_listing(ctx, path, show_hidden, sort, move |a, p, e| {
            let _ = tx.send(AppEvent::FileBrowserListing {
                alias: a,
                path: p,
                entries: e,
            });
        });
    }
    crate::askpass::cleanup_marker(&alias);
    // Force full redraw: ssh may have written to /dev/tty
    terminal.force_redraw();
}

/// Handle `AppEvent::SnippetHostDone`.
pub(crate) fn handle_snippet_host_done(
    app: &mut App,
    run_id: u64,
    alias: String,
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
) {
    if exit_code == Some(0) {
        app.history.record(&alias);
        app.apply_sort();
    }
    if let Some(ref mut state) = app.snippet_output {
        if state.run_id == run_id {
            state.results.push(app::SnippetHostOutput {
                alias,
                stdout,
                stderr,
                exit_code,
            });
        }
    }
}

/// Handle `AppEvent::SnippetProgress`.
pub(crate) fn handle_snippet_progress(app: &mut App, run_id: u64, completed: usize, total: usize) {
    if let Some(ref mut state) = app.snippet_output {
        if state.run_id == run_id {
            state.completed = completed;
            state.total = total;
        }
    }
}

/// Handle `AppEvent::SnippetAllDone`.
pub(crate) fn handle_snippet_all_done(app: &mut App, run_id: u64) {
    if let Some(ref mut state) = app.snippet_output {
        if state.run_id == run_id {
            state.all_done = true;
        }
    }
}

/// Handle `AppEvent::ContainerListing`.
pub(crate) fn handle_container_listing(
    app: &mut App,
    alias: String,
    result: Result<
        (containers::ContainerRuntime, Vec<containers::ContainerInfo>),
        containers::ContainerError,
    >,
) {
    // Always update cache, even if overlay is closed
    match &result {
        Ok((runtime, containers)) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            app.container_cache.insert(
                alias.clone(),
                containers::ContainerCacheEntry {
                    timestamp: now,
                    runtime: *runtime,
                    containers: containers.clone(),
                },
            );
            containers::save_container_cache(&app.container_cache);
        }
        Err(e) => {
            // Preserve runtime even on error
            if let Some(rt) = e.runtime {
                if let Some(entry) = app.container_cache.get_mut(&alias) {
                    entry.runtime = rt;
                }
            }
        }
    }
    // Update overlay state if open
    if let Some(ref mut state) = app.container_state {
        if state.alias == alias {
            match result {
                Ok((runtime, containers)) => {
                    state.runtime = Some(runtime);
                    state.containers = containers;
                    state.loading = false;
                    state.error = None;
                    if let Some(sel) = state.list_state.selected() {
                        if sel >= state.containers.len() && !state.containers.is_empty() {
                            state.list_state.select(Some(0));
                        }
                    } else if !state.containers.is_empty() {
                        state.list_state.select(Some(0));
                    }
                }
                Err(e) => {
                    if let Some(rt) = e.runtime {
                        state.runtime = Some(rt);
                    }
                    state.loading = false;
                    state.error = Some(e.message);
                }
            }
        }
    }
    crate::askpass::cleanup_marker(&alias);
}

/// Handle `AppEvent::ContainerActionComplete`.
pub(crate) fn handle_container_action_complete(
    app: &mut App,
    alias: String,
    action: containers::ContainerAction,
    result: Result<(), String>,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    // Check if overlay matches and extract refresh info before set_status
    let should_refresh = if let Some(ref mut state) = app.container_state {
        if state.alias == alias {
            state.action_in_progress = None;
            match result {
                Ok(()) => {
                    state.loading = true;
                    Some((state.alias.clone(), state.askpass.clone(), state.runtime))
                }
                Err(e) => {
                    state.error = Some(e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };
    if let Some((refresh_alias, askpass, cached_runtime)) = should_refresh {
        app.set_background_status(format!("Container {} complete.", action.as_str()), false);
        let has_tunnel = app.active_tunnels.contains_key(&refresh_alias);
        let ctx = crate::ssh_context::OwnedSshContext {
            alias: refresh_alias,
            config_path: app.reload.config_path.clone(),
            askpass,
            bw_session: app.bw_session.clone(),
            has_tunnel,
        };
        let tx = events_tx.clone();
        containers::spawn_container_listing(ctx, cached_runtime, move |a, r| {
            let _ = tx.send(AppEvent::ContainerListing {
                alias: a,
                result: r,
            });
        });
    }
    crate::askpass::cleanup_marker(&alias);
}

/// Handle `AppEvent::VaultSignResult`.
pub(crate) fn handle_vault_sign_result(
    app: &mut App,
    alias: String,
    existing_cert_file: String,
    success: bool,
    message: String,
) {
    if success {
        // The CertificateFile snapshot is carried in the event so
        // we never re-look up the host (which would be O(n) and
        // racy under concurrent renames).
        let mut host_missing = false;
        if crate::should_write_certificate_file(&existing_cert_file) {
            if let Ok(cert_path) = vault_ssh::cert_path_for(&alias) {
                let updated = app
                    .config
                    .set_host_certificate_file(&alias, &cert_path.to_string_lossy());
                if !updated {
                    host_missing = true;
                }
            }
        }
        app.refresh_cert_cache(&alias);
        if host_missing {
            app.set_status(
                format!(
                    "Vault SSH cert saved for {} but host no longer in config (renamed or deleted). CertificateFile NOT written.",
                    alias
                ),
                true,
            );
        } else {
            app.set_status(format!("Signed Vault SSH cert for {}", alias), false);
        }
    } else {
        app.set_status(
            format!("Vault SSH: failed to sign {}: {}", alias, message),
            true,
        );
    }
}

/// Handle `AppEvent::VaultSignProgress`.
pub(crate) fn handle_vault_sign_progress(
    app: &mut App,
    alias: String,
    done: usize,
    total: usize,
    spinner_tick: u64,
) {
    // Truncate long aliases so the status line fits even on
    // narrow terminals; the full alias is recoverable from the
    // host list.
    const ALIAS_BUDGET: usize = 40;
    let display_alias: String = if alias.chars().count() > ALIAS_BUDGET {
        let cut: String = alias.chars().take(ALIAS_BUDGET - 1).collect();
        format!("{}\u{2026}", cut)
    } else {
        alias.clone()
    };
    let spinner = crate::animation::SPINNER_FRAMES
        [spinner_tick as usize % crate::animation::SPINNER_FRAMES.len()];
    app.set_sticky_status(
        format!(
            "{} Signing {}/{}: {} (V to cancel)",
            spinner, done, total, display_alias
        ),
        false,
    );
}

/// Handle `AppEvent::VaultSignAllDone`. Returns `ControlFlow::Break(())` when
/// the caller should `continue` the event loop (skip the rest of the iteration),
/// or `ControlFlow::Continue(())` for normal processing.
pub(crate) fn handle_vault_sign_all_done(
    app: &mut App,
    signed: u32,
    failed: u32,
    skipped: u32,
    cancelled: bool,
    aborted_message: Option<String>,
    first_error: Option<String>,
) -> std::ops::ControlFlow<()> {
    app.vault.signing_cancel = None;
    // Join the background thread now that it has finished.
    if let Some(handle) = app.vault.sign_thread.take() {
        let _ = handle.join();
    }
    if let Some(msg) = aborted_message {
        app.set_sticky_status(msg, true);
        return std::ops::ControlFlow::Break(()); // caller should `continue`
    }
    if cancelled {
        let mut msg = format!(
            "Vault SSH signing cancelled ({} signed, {} failed)",
            signed, failed
        );
        if let Some(err) = first_error.as_ref() {
            msg.push_str(": ");
            msg.push_str(err);
        }
        if failed > 0 {
            app.set_sticky_status(msg, true);
        } else {
            app.set_info_status(msg);
        }
        return std::ops::ControlFlow::Break(()); // caller should `continue`
    }
    let summary_msg =
        crate::format_vault_sign_summary(signed, failed, skipped, first_error.as_deref());
    if signed > 0 {
        if app.is_form_open() {
            // Defer config write to avoid mtime conflict with open forms
            app.pending_vault_config_write = true;
            if failed > 0 {
                app.set_sticky_status(summary_msg, true);
            } else {
                app.set_info_status(summary_msg);
            }
        } else if app.external_config_changed() {
            // The on-disk ssh config (or an include) was modified
            // by an external editor while the bulk-sign worker was
            // running. Writing now would overwrite those edits.
            let reapply: Vec<(String, String)> = app
                .config
                .host_entries()
                .into_iter()
                .filter_map(|h| {
                    if h.vault_ssh.is_some()
                        && crate::should_write_certificate_file(&h.certificate_file)
                    {
                        vault_ssh::cert_path_for(&h.alias)
                            .ok()
                            .map(|p| (h.alias.clone(), p.to_string_lossy().into_owned()))
                    } else {
                        None
                    }
                })
                .collect();
            match ssh_config::model::SshConfigFile::parse(&app.reload.config_path) {
                Ok(fresh) => {
                    app.config = fresh;
                    let mut reapplied = 0usize;
                    for (alias, cert_path) in &reapply {
                        let entry = app
                            .config
                            .host_entries()
                            .into_iter()
                            .find(|h| &h.alias == alias);
                        if let Some(entry) = entry {
                            if crate::should_write_certificate_file(&entry.certificate_file)
                                && app.config.set_host_certificate_file(alias, cert_path)
                            {
                                reapplied += 1;
                            }
                        }
                    }
                    if reapplied > 0 {
                        if let Err(e) = app.config.write() {
                            app.set_sticky_status(
                                format!(
                                    "External edits detected; signed {} certs but failed to re-apply CertificateFile: {}",
                                    signed, e
                                ),
                                true,
                            );
                        } else {
                            app.update_last_modified();
                            app.reload_hosts();
                            if failed > 0 {
                                app.set_sticky_status(
                                    format!(
                                        "{} External ssh config edits detected, merged {} CertificateFile directives.",
                                        summary_msg, reapplied
                                    ),
                                    true,
                                );
                            } else {
                                app.set_info_status(format!(
                                    "{} External ssh config edits detected, merged {} CertificateFile directives.",
                                    summary_msg, reapplied
                                ));
                            }
                        }
                    } else {
                        app.reload_hosts();
                        app.set_sticky_status(
                            format!(
                                "{} External ssh config edits detected; certs on disk, no CertificateFile written.",
                                summary_msg
                            ),
                            true,
                        );
                    }
                }
                Err(e) => {
                    app.set_sticky_status(
                        format!(
                            "Signed {} certs but cannot re-parse ssh config after external edit: {}. Certs are on disk under ~/.purple/certs/.",
                            signed, e
                        ),
                        true,
                    );
                }
            }
        } else if let Err(e) = app.config.write() {
            app.set_sticky_status(
                format!(
                    "Signed {} certs but failed to update SSH config: {}",
                    signed, e
                ),
                true,
            );
        } else {
            app.update_last_modified();
            app.reload_hosts();
            if failed > 0 {
                app.set_sticky_status(summary_msg, true);
            } else {
                app.set_info_status(summary_msg);
            }
        }
    } else if failed > 0 {
        app.set_sticky_status(summary_msg, true);
    } else {
        app.set_info_status(summary_msg);
    }
    std::ops::ControlFlow::Continue(()) // normal flow
}

/// Handle `AppEvent::CertCheckResult`.
pub(crate) fn handle_cert_check_result(
    app: &mut App,
    alias: String,
    status: vault_ssh::CertStatus,
) {
    app.vault.cert_checks_in_flight.remove(&alias);
    let mtime = crate::current_cert_mtime(&alias, app);
    app.vault
        .cert_cache
        .insert(alias, (Instant::now(), status, mtime));
}

/// Handle `AppEvent::CertCheckError`.
pub(crate) fn handle_cert_check_error(app: &mut App, alias: String, message: String) {
    // Cache the error as Invalid so the lazy-check loop doesn't
    // re-spawn a background thread on every poll tick.
    app.vault.cert_checks_in_flight.remove(&alias);
    app.vault.cert_cache.insert(
        alias.clone(),
        (
            Instant::now(),
            vault_ssh::CertStatus::Invalid(message.clone()),
            None,
        ),
    );
    app.set_background_status(
        format!("Cert check failed for {}: {}", alias, message),
        true,
    );
}
