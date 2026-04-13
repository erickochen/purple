use std::sync::atomic::Ordering;
use std::sync::mpsc;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, HostForm, Screen};
use crate::event::AppEvent;
use crate::ssh_config::model::HostEntry;

mod command_palette;
mod confirm;
mod containers;
mod file_browser;
mod help;
mod host_detail;
mod host_form;
mod host_list;
mod picker;
mod ping;
mod provider;
mod snippet;
mod sync;
mod tag_picker;
mod theme_picker;
mod tunnel;

pub(crate) use provider::zone_data_for;
pub use sync::spawn_provider_sync;

/// Create a sender that maps SnippetEvent to AppEvent.
/// Returns true when every host in `host_addrs` has no per-host Vault address
/// and the process env also has no valid `VAULT_ADDR`. Extracted as a pure
/// function so the V-key pre-check can be unit tested without env mutation.
pub(super) fn vault_addr_missing(
    host_addrs: &[Option<&str>],
    env_vault_addr: Option<&str>,
) -> bool {
    let env_ok = env_vault_addr
        .map(crate::vault_ssh::is_valid_vault_addr)
        .unwrap_or(false);
    if env_ok || host_addrs.is_empty() {
        return false;
    }
    host_addrs.iter().all(|a| a.is_none())
}

pub(super) fn snippet_event_bridge(
    tx: &mpsc::Sender<AppEvent>,
) -> mpsc::Sender<crate::snippet::SnippetEvent> {
    let (stx, srx) = mpsc::channel::<crate::snippet::SnippetEvent>();
    let tx = tx.clone();
    std::thread::Builder::new()
        .name("snippet-bridge".into())
        .spawn(move || {
            while let Ok(evt) = srx.recv() {
                let app_evt = match evt {
                    crate::snippet::SnippetEvent::HostDone {
                        run_id,
                        alias,
                        stdout,
                        stderr,
                        exit_code,
                    } => AppEvent::SnippetHostDone {
                        run_id,
                        alias,
                        stdout,
                        stderr,
                        exit_code,
                    },
                    crate::snippet::SnippetEvent::Progress {
                        run_id,
                        completed,
                        total,
                    } => AppEvent::SnippetProgress {
                        run_id,
                        completed,
                        total,
                    },
                    crate::snippet::SnippetEvent::AllDone { run_id } => {
                        AppEvent::SnippetAllDone { run_id }
                    }
                };
                if tx.send(app_evt).is_err() {
                    break;
                }
            }
        })
        .expect("failed to spawn snippet bridge");
    stx
}

/// Handle a key event based on the current screen.
pub fn handle_key_event(
    app: &mut App,
    key: KeyEvent,
    events_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    // Global Ctrl+C handler — screen-conditional for SnippetOutput
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        if matches!(app.screen, Screen::SnippetOutput { .. }) {
            if let Some(ref state) = app.snippet_output {
                if !state.all_done {
                    if state.cancel.load(Ordering::Relaxed) {
                        // Second Ctrl+C: cancel already pending, force close
                    } else {
                        // First Ctrl+C: request cancellation
                        state.cancel.store(true, Ordering::Relaxed);
                        return Ok(());
                    }
                }
            }
            app.snippet_output = None;
            app.screen = Screen::HostList;
            return Ok(());
        }
        if let Some(ref cancel) = app.vault_signing_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        app.running = false;
        return Ok(());
    }

    // Command palette intercept
    if app.palette.is_some() {
        command_palette::handle_command_palette(app, key, events_tx);
        return Ok(());
    }

    match &app.screen {
        Screen::HostList => {
            if app.search.query.is_some() {
                host_list::handle_host_list_search(app, key, events_tx);
            } else {
                host_list::handle_host_list(app, key, events_tx);
            }
        }
        Screen::AddHost | Screen::EditHost { .. } => host_form::handle_form(app, key),
        Screen::ConfirmDelete { .. } => confirm::handle_confirm_delete(app, key),
        Screen::Help { .. } => help::handle_help(app, key),
        Screen::KeyList => help::handle_key_list(app, key),
        Screen::KeyDetail { .. } => help::handle_key_detail(app, key),
        Screen::HostDetail { .. } => host_detail::handle_host_detail(app, key),
        Screen::TagPicker => tag_picker::handle_tag_picker_screen(app, key),
        Screen::ThemePicker => theme_picker::handle_theme_picker(app, key),
        Screen::Providers => provider::handle_provider_list(app, key, events_tx),
        Screen::ProviderForm { .. } => provider::handle_provider_form(app, key, events_tx),
        Screen::TunnelList { .. } => tunnel::handle_tunnel_list(app, key),
        Screen::TunnelForm { .. } => tunnel::handle_tunnel_form(app, key),
        Screen::SnippetPicker { .. } => snippet::handle_snippet_picker(app, key, events_tx),
        Screen::SnippetForm { .. } => snippet::handle_snippet_form(app, key),
        Screen::SnippetOutput { .. } => snippet::handle_snippet_output(app, key),
        Screen::SnippetParamForm { .. } => snippet::handle_snippet_param_form(app, key, events_tx),
        Screen::ConfirmHostKeyReset { .. } => confirm::handle_confirm_host_key_reset(app, key),
        Screen::ConfirmVaultSign { .. } => confirm::handle_confirm_vault_sign(app, key, events_tx),
        Screen::ConfirmImport { .. } => {
            if key.code == KeyCode::Char('y') || key.code == KeyCode::Char('Y') {
                app.screen = Screen::HostList;
                execute_known_hosts_import(app);
            } else if key.code == KeyCode::Esc
                || key.code == KeyCode::Char('n')
                || key.code == KeyCode::Char('N')
            {
                app.screen = Screen::HostList;
            }
        }
        Screen::ConfirmPurgeStale { provider: p, .. } => {
            let provider = p.clone();
            if key.code == KeyCode::Char('y') || key.code == KeyCode::Char('Y') {
                execute_purge_stale(app, provider.as_deref());
                if provider.is_some() {
                    app.screen = Screen::Providers;
                } else {
                    app.screen = Screen::HostList;
                }
            } else if key.code == KeyCode::Esc
                || key.code == KeyCode::Char('n')
                || key.code == KeyCode::Char('N')
            {
                if provider.is_some() {
                    app.screen = Screen::Providers;
                } else {
                    app.screen = Screen::HostList;
                }
            }
        }
        Screen::FileBrowser { .. } => file_browser::handle_file_browser(app, key, events_tx),
        Screen::Containers { .. } => containers::handle_containers(app, key, events_tx)?,
        Screen::Welcome {
            known_hosts_count, ..
        } => {
            let known_hosts_count = *known_hosts_count;
            if key.code == KeyCode::Char('?') {
                app.screen = Screen::Help {
                    return_screen: Box::new(Screen::HostList),
                };
            } else if key.code == KeyCode::Char('I') && known_hosts_count > 0 {
                app.screen = Screen::HostList;
                execute_known_hosts_import(app);
            } else {
                app.screen = Screen::HostList;
            }
        }
    }
    Ok(())
}

/// Run known_hosts import and set status. Used by both ConfirmImport and Welcome handlers.
fn execute_known_hosts_import(app: &mut App) {
    let config_backup = app.config.clone();
    match crate::import::import_from_known_hosts(&mut app.config, Some("known_hosts")) {
        Ok((imported, skipped, _, _)) => {
            if imported > 0 {
                if let Err(e) = app.config.write() {
                    app.config = config_backup;
                    app.set_status(format!("Failed to save: {}", e), true);
                    return;
                }
                app.reload_hosts();
                app.set_status(
                    format!(
                        "Imported {} host{}, skipped {} duplicate{}.",
                        imported,
                        if imported == 1 { "" } else { "s" },
                        skipped,
                        if skipped == 1 { "" } else { "s" },
                    ),
                    false,
                );
            } else {
                app.set_status(
                    if skipped == 1 {
                        "Host already exists.".to_string()
                    } else {
                        format!("All {} hosts already exist.", skipped)
                    },
                    false,
                );
            }
            app.known_hosts_count = 0;
        }
        Err(e) => {
            app.set_status(e, true);
        }
    }
}

fn execute_purge_stale(app: &mut App, provider: Option<&str>) {
    let stale = app.config.stale_hosts();
    if stale.is_empty() {
        return;
    }
    // Filter by provider if specified
    let targets: Vec<(String, u64)> = if let Some(prov) = provider {
        stale
            .into_iter()
            .filter(|(alias, _)| {
                app.config
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
    let config_backup = app.config.clone();
    let count = targets.len();
    for (alias, _) in &targets {
        app.config.delete_host(alias);
    }
    if let Err(e) = app.config.write() {
        app.config = config_backup;
        app.set_status(format!("Failed to save: {}", e), true);
        return;
    }
    // Kill active tunnels only after successful write (no rollback needed)
    for (alias, _) in &targets {
        if let Some(mut tunnel) = app.active_tunnels.remove(alias) {
            let _ = tunnel.child.kill();
            let _ = tunnel.child.wait();
        }
    }
    app.undo_stack.clear();
    app.update_last_modified();
    app.reload_hosts();
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
    app.set_status(msg, false);
}

/// Build a provider hint string for stale host messages, e.g. " gone from DigitalOcean".
pub(super) fn stale_provider_hint(host: &crate::ssh_config::model::HostEntry) -> String {
    host.provider
        .as_ref()
        .map(|p| format!(" gone from {}", crate::providers::provider_display_name(p)))
        .unwrap_or_default()
}

/// Open the edit form for `host`. Returns `true` if the form was opened,
/// `false` if the host is from an include file (status message set instead).
pub(super) fn open_edit_form(app: &mut App, host: HostEntry) -> bool {
    if let Some(ref source) = host.source_file {
        app.set_status(
            format!(
                "{} lives in {}. Edit it there.",
                host.alias,
                source.display()
            ),
            true,
        );
        return false;
    }
    let stale_hint = host.stale.is_some().then(|| stale_provider_hint(&host));
    // Load raw entry (without pattern inheritance) so inherited values are not
    // shown as editable own values. Compute inherited hints separately.
    let raw = match app.config.raw_host_entry(&host.alias) {
        Some(entry) => entry,
        None => {
            app.set_status("Host not found in config.".to_string(), true);
            return false;
        }
    };
    let inherited = app.config.inherited_hints(&host.alias);
    app.form = HostForm::from_entry(&raw, inherited);
    if let Some(hint) = stale_hint {
        app.set_status(format!("Stale host.{}", hint), true);
    }
    app.screen = Screen::EditHost { alias: host.alias };
    app.capture_form_mtime();
    app.capture_form_baseline();
    true
}

/// After a picker selection, try to auto-submit the host form if all
/// required fields are filled. Lives at the handler level so picker
/// submodules do not need a reverse dependency on host_form.
pub(super) fn try_auto_submit_after_picker(app: &mut App) {
    if !app.form.alias.is_empty() && !app.form.hostname.is_empty() {
        host_form::submit_form(app);
    }
}

#[cfg(test)]
mod tests;
