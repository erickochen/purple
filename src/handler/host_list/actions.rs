//! Sub-handlers for the largest key actions in `handle_host_list`.
//!
//! Extracted from the main key dispatcher so the parent function stays below
//! the project file-size limit. Each function corresponds to one key press
//! and owns the full side-effect flow (status updates, state transitions,
//! thread spawning).

use std::sync::mpsc;

use crate::app::{App, HostForm, Screen};
use crate::event::AppEvent;

/// `c` — duplicate the selected host or pattern into a new AddHost form.
pub(super) fn clone_selected(app: &mut App) {
    if let Some(pattern) = app.selected_pattern() {
        if pattern.source_file.is_some() {
            app.set_status(
                format!(
                    "{} is in an included file. Clone it there.",
                    pattern.pattern
                ),
                true,
            );
            return;
        }
        let mut form = HostForm::from_pattern_entry(pattern);
        form.alias.clear();
        form.cursor_pos = 0;
        app.form = form;
        app.screen = Screen::AddHost;
        app.capture_form_mtime();
        app.capture_form_baseline();
        return;
    }

    if let Some(host) = app.selected_host() {
        if let Some(ref source) = host.source_file {
            let alias = host.alias.clone();
            let path = source.display();
            app.set_status(
                format!("{} lives in {}. Clone it there.", alias, path),
                true,
            );
            return;
        }
        let stale_hint = if host.stale.is_some() {
            Some(crate::handler::stale_provider_hint(host))
        } else {
            None
        };
        let copy_alias = format!("{}-copy", host.alias);
        // Clone uses the enriched entry (with inheritance) so the copy is
        // self-contained. from_entry_duplicate clears vault_ssh so the copy
        // does not inherit a per-host override tied to the original alias's
        // certificate.
        let (mut form, vault_cleared) = HostForm::from_entry_duplicate(host, Default::default());
        form.alias = copy_alias;
        form.cursor_pos = form.alias.chars().count();
        if let Some(hint) = stale_hint {
            app.set_status(format!("Stale host.{}", hint), true);
        } else if vault_cleared {
            app.set_status("Cloned. Vault SSH role cleared on copy.".to_string(), false);
        }
        app.form = form;
        app.screen = Screen::AddHost;
        app.capture_form_mtime();
        app.capture_form_baseline();
    }
}

/// `V` — collect all hosts with a Vault SSH role, filter the ones that need
/// renewal, and transition to the bulk-sign confirmation screen. Cancels an
/// in-progress signing thread if one is already running.
pub(super) fn initiate_bulk_vault_sign(app: &mut App) {
    if !app.has_any_vault_role() {
        app.set_status(
            "No Vault SSH role configured. Set one in the host form \
             (Vault SSH role field) or on a provider for shared defaults."
                .to_string(),
            false,
        );
        return;
    }
    if app.demo_mode {
        app.set_status("Demo mode. Vault SSH signing disabled.".to_string(), false);
        return;
    }
    // Cancel any in-progress vault signing thread
    if let Some(ref cancel) = app.vault_signing_cancel {
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        app.vault_signing_cancel = None;
        app.set_status("Vault SSH signing cancelled.".to_string(), false);
        return;
    }
    let provider_config = crate::providers::config::ProviderConfig::load();
    let entries = app.config.host_entries();
    let mut signable: Vec<(String, String, String, std::path::PathBuf, Option<String>)> =
        Vec::new();
    let mut pubkey_error: Option<String> = None;
    for e in &entries {
        let Some(role) = crate::vault_ssh::resolve_vault_role(
            e.vault_ssh.as_deref(),
            e.provider.as_deref(),
            &provider_config,
        ) else {
            continue;
        };
        let vault_addr = crate::vault_ssh::resolve_vault_addr(
            e.vault_addr.as_deref(),
            e.provider.as_deref(),
            &provider_config,
        );
        match crate::vault_ssh::resolve_pubkey_path(&e.identity_file) {
            Ok(pubkey) => signable.push((
                e.alias.clone(),
                role,
                e.certificate_file.clone(),
                pubkey,
                vault_addr,
            )),
            Err(err) => {
                if pubkey_error.is_none() {
                    pubkey_error = Some(err.to_string());
                }
            }
        }
    }
    if let Some(msg) = pubkey_error {
        app.set_status(format!("Vault SSH: {}", msg), true);
        return;
    }

    if signable.is_empty() {
        app.set_status(
            "No hosts with a Vault SSH role configured.".to_string(),
            false,
        );
        return;
    }

    // Pre-check: if any signable host has no resolved VAULT_ADDR and the
    // process env also has none, the vault CLI will fail with a cryptic
    // error only after the user confirms the dialog. Surface this upfront
    // with a clear, actionable message.
    let env_vault_addr = std::env::var("VAULT_ADDR").ok();
    let host_addrs: Vec<Option<&str>> = signable
        .iter()
        .map(|(_, _, _, _, a)| a.as_deref())
        .collect();
    if crate::handler::vault_addr_missing(&host_addrs, env_vault_addr.as_deref()) {
        app.set_status(
            "No Vault address set. Edit the host (e) or provider \
             and fill in the Vault SSH Address field."
                .to_string(),
            true,
        );
        return;
    }

    // Pre-filter to hosts that actually need renewal, so the confirm
    // dialog count matches what will actually be signed. Hosts with a
    // valid cached cert are skipped silently.
    let mut needs_signing: Vec<(String, String, String, std::path::PathBuf, Option<String>)> =
        Vec::with_capacity(signable.len());
    for entry in &signable {
        let (alias, _role, cert_file, _pubkey, _vault_addr) = entry;
        let check_path = match crate::vault_ssh::resolve_cert_path(alias, cert_file) {
            Ok(p) => p,
            Err(_) => {
                needs_signing.push(entry.clone());
                continue;
            }
        };
        let status = crate::vault_ssh::check_cert_validity(&check_path);
        if crate::vault_ssh::needs_renewal(&status) {
            needs_signing.push(entry.clone());
        }
    }

    if needs_signing.is_empty() {
        app.set_status(
            "All Vault SSH certificates are still valid.".to_string(),
            false,
        );
        return;
    }

    app.screen = Screen::ConfirmVaultSign {
        signable: needs_signing,
    };
}

/// `F` — open the file browser overlay for the selected host. Spawns a
/// background thread to fetch the remote home directory.
pub(super) fn open_file_browser(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.is_pattern_selected() {
        return;
    }
    if app.demo_mode {
        app.set_status("Demo mode. File browser disabled.".to_string(), false);
        return;
    }
    let Some(host) = app.selected_host() else {
        return;
    };
    let stale_hint = if host.stale.is_some() {
        Some(crate::handler::stale_provider_hint(host))
    } else {
        None
    };
    let alias = host.alias.clone();
    let askpass = host.askpass.clone();
    if let Some(hint) = stale_hint {
        app.set_status(format!("Stale host.{}", hint), true);
    }
    let has_tunnel = app.active_tunnels.contains_key(&alias);
    let (local_path, remote_path) =
        app.file_browser_paths
            .get(&alias)
            .cloned()
            .unwrap_or_else(|| {
                (
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/")),
                    String::new(),
                )
            });
    let (local_entries, local_error) = match crate::file_browser::list_local(
        &local_path,
        false,
        crate::file_browser::BrowserSort::Name,
    ) {
        Ok(entries) => (entries, None),
        Err(e) => (Vec::new(), Some(e.to_string())),
    };
    let mut local_list_state = ratatui::widgets::ListState::default();
    local_list_state.select(Some(0)); // Always select ".." entry
    let fb = crate::file_browser::FileBrowserState {
        alias: alias.clone(),
        askpass: askpass.clone(),
        active_pane: crate::file_browser::BrowserPane::Local,
        local_path,
        local_entries,
        local_list_state,
        local_selected: std::collections::HashSet::new(),
        local_error,
        remote_path: String::new(),
        remote_entries: Vec::new(),
        remote_list_state: ratatui::widgets::ListState::default(),
        remote_selected: std::collections::HashSet::new(),
        remote_error: None,
        remote_loading: true,
        show_hidden: false,
        sort: crate::file_browser::BrowserSort::Name,
        confirm_copy: None,
        transferring: None,
        transfer_error: None,
        connection_recorded: false,
    };
    app.file_browser = Some(fb);
    app.screen = Screen::FileBrowser {
        alias: alias.clone(),
    };
    // Fetch remote home dir in background
    let config_path = app.reload.config_path.clone();
    let tx = events_tx.clone();
    let bw = app.bw_session.clone();
    let remote = remote_path;
    std::thread::spawn(move || {
        let home = if remote.is_empty() {
            match crate::file_browser::get_remote_home(
                &alias,
                &config_path,
                askpass.as_deref(),
                bw.as_deref(),
                has_tunnel,
            ) {
                Ok(h) => h,
                Err(e) => {
                    let _ = tx.send(crate::event::AppEvent::FileBrowserListing {
                        alias,
                        path: String::new(),
                        entries: Err(e.to_string()),
                    });
                    return;
                }
            }
        } else {
            remote
        };
        crate::file_browser::spawn_remote_listing(
            alias,
            config_path,
            home,
            false,
            crate::file_browser::BrowserSort::Name,
            askpass,
            bw,
            has_tunnel,
            super::super::file_browser::fb_send(tx),
        );
    });
}

/// `C` — open the container overlay for the selected host. Spawns a
/// background listing thread unless the app is in demo mode.
pub(super) fn open_container_overlay(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.is_pattern_selected() {
        return;
    }
    let Some(host) = app.selected_host() else {
        return;
    };
    let stale_hint = if host.stale.is_some() {
        Some(crate::handler::stale_provider_hint(host))
    } else {
        None
    };
    let alias = host.alias.clone();
    let askpass = host.askpass.clone();
    if let Some(hint) = stale_hint {
        app.set_status(format!("Stale host.{}", hint), true);
    }
    let (cached_runtime, cached_containers) = if let Some(entry) = app.container_cache.get(&alias) {
        (Some(entry.runtime), entry.containers.clone())
    } else {
        (None, Vec::new())
    };
    let mut list_state = ratatui::widgets::ListState::default();
    if !cached_containers.is_empty() {
        list_state.select(Some(0));
    }
    app.container_state = Some(crate::app::ContainerState {
        alias: alias.clone(),
        askpass: askpass.clone(),
        runtime: cached_runtime,
        containers: cached_containers,
        list_state,
        loading: !app.demo_mode,
        error: None,
        action_in_progress: None,
        confirm_action: None,
    });
    app.screen = Screen::Containers {
        alias: alias.clone(),
    };
    if !app.demo_mode {
        let has_tunnel = app.active_tunnels.contains_key(&alias);
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
