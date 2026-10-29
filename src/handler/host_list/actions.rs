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
            app.notify_error(crate::messages::included_file_clone(&pattern.pattern));
            return;
        }
        let mut form = HostForm::from_pattern_entry(pattern);
        form.alias.clear();
        form.cursor_pos = 0;
        *app.forms.host_mut() = form;
        app.set_screen(Screen::AddHost);
        app.capture_form_mtime();
        app.capture_form_baseline();
        return;
    }

    if let Some(host) = app.selected_host() {
        if let Some(ref source) = host.source_file {
            let alias = host.alias.clone();
            let path = source.display();
            app.notify_warning(crate::messages::included_host_clone_there(&alias, &path));
            return;
        }
        let stale_hint = super::super::host_form::stale_hint_for(host);
        let copy_alias = format!("{}-copy", host.alias);
        // Clone uses the enriched entry (with inheritance) so the copy is
        // self-contained. from_entry_duplicate clears vault_ssh so the copy
        // does not inherit a per-host override tied to the original alias's
        // certificate.
        let (mut form, vault_cleared) = HostForm::from_entry_duplicate(host, Default::default());
        form.alias = copy_alias;
        form.cursor_pos = form.alias.chars().count();
        if let Some(hint) = stale_hint {
            app.notify_warning(crate::messages::stale_host(&hint));
        } else if vault_cleared {
            app.notify(crate::i18n::text(crate::messages::CLONED_VAULT_CLEARED));
        }
        *app.forms.host_mut() = form;
        app.set_screen(Screen::AddHost);
        app.capture_form_mtime();
        app.capture_form_baseline();
    }
}

/// `V` — collect all hosts with a Vault SSH role, filter the ones that need
/// renewal, and transition to the bulk-sign confirmation screen. Cancels an
/// in-progress signing thread if one is already running. Shared between the
/// Host list tab and the Keys tab; both surfaces operate on the global host
/// list so the entry point lives here.
pub(crate) fn initiate_bulk_vault_sign(app: &mut App) {
    if !app.has_any_vault_role() {
        app.notify(crate::i18n::text(crate::messages::VAULT_NO_ROLE_CONFIGURED));
        return;
    }
    if app.demo_mode {
        app.notify_warning(crate::i18n::text(
            crate::messages::DEMO_VAULT_SIGNING_DISABLED,
        ));
        return;
    }
    // Cancel any in-progress vault signing thread
    if let Some(cancel) = app.vault.signing_cancel() {
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        app.vault.clear_signing_cancel();
        app.notify(crate::i18n::text(crate::messages::VAULT_SIGNING_CANCELLED));
        return;
    }
    let provider_config = crate::providers::config::ProviderConfig::load(app.env.paths());
    let entries = app.hosts_state.ssh_config().host_entries();
    let mut signable: Vec<crate::vault_ssh::VaultSignTarget> = Vec::new();
    let mut pubkey_error: Option<String> = None;
    for e in &entries {
        let Some(role) = crate::vault_ssh::resolve_vault_role(
            e.vault_ssh.as_deref(),
            e.provider.as_deref(),
            e.provider_label.as_deref(),
            &provider_config,
        ) else {
            continue;
        };
        let vault_addr = crate::vault_ssh::resolve_vault_addr(
            e.vault_addr.as_deref(),
            e.provider.as_deref(),
            e.provider_label.as_deref(),
            &provider_config,
        );
        match crate::vault_ssh::resolve_pubkey_path(app.env().paths(), &e.identity_file) {
            Ok(pubkey) => signable.push(crate::vault_ssh::VaultSignTarget {
                alias: e.alias.clone(),
                role,
                certificate_file: e.certificate_file.clone(),
                pubkey,
                vault_addr,
            }),
            Err(err) => {
                if pubkey_error.is_none() {
                    pubkey_error = Some(err.to_string());
                }
            }
        }
    }
    if let Some(msg) = pubkey_error {
        app.notify_error(crate::messages::vault_error(&msg));
        return;
    }

    if signable.is_empty() {
        app.notify(crate::i18n::text(crate::messages::VAULT_NO_HOSTS_WITH_ROLE));
        return;
    }

    // Pre-check: if any signable host has no resolved VAULT_ADDR and the
    // process env also has none, the vault CLI will fail with a cryptic
    // error only after the user confirms the dialog. Surface this upfront
    // with a clear, actionable message.
    let env_vault_addr = app.env().vault_addr().map(str::to_string);
    let host_addrs: Vec<Option<&str>> = signable.iter().map(|t| t.vault_addr.as_deref()).collect();
    if vault_addr_missing(&host_addrs, env_vault_addr.as_deref()) {
        app.notify_error(crate::i18n::text(crate::messages::VAULT_NO_ADDRESS));
        return;
    }

    // Pre-filter to hosts that actually need renewal, so the confirm
    // dialog count matches what will actually be signed. Hosts with a
    // valid cached cert are skipped silently.
    let mut needs_signing: Vec<crate::vault_ssh::VaultSignTarget> =
        Vec::with_capacity(signable.len());
    for entry in &signable {
        let check_path = match crate::vault_ssh::resolve_cert_path(
            app.env().paths(),
            &entry.alias,
            &entry.certificate_file,
        ) {
            Ok(p) => p,
            Err(_) => {
                needs_signing.push(entry.clone());
                continue;
            }
        };
        let status = crate::vault_ssh::check_cert_validity(app.env(), &check_path);
        if crate::vault_ssh::needs_renewal(&status) {
            needs_signing.push(entry.clone());
        }
    }

    if needs_signing.is_empty() {
        app.notify(crate::i18n::text(crate::messages::VAULT_ALL_CERTS_VALID));
        return;
    }

    app.vault.set_pending_sign(needs_signing);
    app.set_screen(Screen::ConfirmVaultSign);
}

/// `F` — open the file browser overlay for the selected host. Spawns a
/// background thread to fetch the remote home directory.
pub(super) fn open_file_browser(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.is_pattern_selected() {
        return;
    }
    if app.demo_mode {
        app.notify_warning(crate::i18n::text(
            crate::messages::DEMO_FILE_BROWSER_DISABLED,
        ));
        return;
    }
    let Some(host) = app.selected_host() else {
        return;
    };
    let stale_hint = super::super::host_form::stale_hint_for(host);
    let alias = host.alias.clone();
    let askpass = host.askpass.clone();
    if let Some(hint) = stale_hint {
        app.notify_warning(crate::messages::stale_host(&hint));
    }
    let (local_path, remote_path) = app
        .file_browser_state
        .host_path(&alias)
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
    let fb = crate::file_browser::FileBrowserSession {
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
    app.open_file_browser(fb);
    // Resolve the remote home (when we have no saved path) and list it in
    // the background. Same call the retry after a password or trust dialog
    // makes, so both recover identically.
    let ctx = app.file_browser_ssh_context(alias, askpass);
    crate::file_browser::spawn_remote_open(
        ctx,
        remote_path,
        false,
        crate::file_browser::BrowserSort::Name,
        super::super::file_browser::fb_send(events_tx.clone()),
    );
}

/// `C` — open the container overlay for the selected host. Spawns a
/// background listing thread unless the app is in demo mode. State setup
/// lives in `crate::handler::containers::open_overlay_for_host` so the
/// containers overview tab and the host-list `C` shortcut share one
/// implementation.
pub(super) fn open_container_overlay(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if app.is_pattern_selected() {
        return;
    }
    let Some(host) = app.selected_host() else {
        return;
    };
    let stale_hint = super::super::host_form::stale_hint_for(host);
    let alias = host.alias.clone();
    let askpass = host.askpass.clone();
    if let Some(hint) = stale_hint {
        app.notify_warning(crate::messages::stale_host(&hint));
    }
    crate::handler::containers::open_overlay_for_host(app, alias, askpass, events_tx);
}

/// Returns true when every host in `host_addrs` has no per-host Vault address
/// and the process env also has no valid `VAULT_ADDR`.
pub(crate) fn vault_addr_missing(
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

/// `g` — cycle the host-list grouping: None -> Provider -> Tag -> None. Tag
/// mode is skipped when no user tags exist. Persists each transition.
pub(super) fn cycle_group_by(app: &mut App) {
    use crate::app::GroupBy;
    match app.hosts_state.group_by() {
        GroupBy::None => {
            app.hosts_state.set_group_by(GroupBy::Provider);
            app.apply_sort();
            if let Err(e) =
                crate::preferences::save_group_by(app.env().paths(), app.hosts_state.group_by())
            {
                app.notify_error(crate::messages::grouped_by_save_failed(
                    &app.hosts_state.group_by().label(),
                    &e,
                ));
            } else {
                app.notify(crate::messages::grouped_by(
                    &app.hosts_state.group_by().label(),
                ));
            }
        }
        GroupBy::Provider => {
            let user_tags: Vec<String> = {
                let mut seen = std::collections::HashSet::new();
                let mut tags = Vec::new();
                for host in app.hosts_state.list() {
                    for tag in &host.tags {
                        if seen.insert(tag.clone()) {
                            tags.push(tag.clone());
                        }
                    }
                }
                tags.sort_by_cached_key(|a| a.to_lowercase());
                tags
            };
            if user_tags.is_empty() {
                app.hosts_state.set_group_by(GroupBy::None);
                app.apply_sort();
                if let Err(e) =
                    crate::preferences::save_group_by(app.env().paths(), app.hosts_state.group_by())
                {
                    app.notify_error(crate::messages::ungrouped_save_failed(&e));
                } else {
                    app.notify(crate::i18n::text(crate::messages::UNGROUPED));
                }
            } else {
                // Switch to tag mode directly. The nav bar shows all tags as
                // tabs, no picker needed.
                app.hosts_state.set_group_by(GroupBy::Tag(String::new()));
                app.apply_sort();
                if let Err(e) =
                    crate::preferences::save_group_by(app.env().paths(), app.hosts_state.group_by())
                {
                    app.notify_error(crate::messages::grouped_by_tag_save_failed(&e));
                } else {
                    app.notify(crate::i18n::text(crate::messages::GROUPED_BY_TAG));
                }
            }
        }
        GroupBy::Tag(_) => {
            app.hosts_state.set_group_by(GroupBy::None);
            app.apply_sort();
            if let Err(e) =
                crate::preferences::save_group_by(app.env().paths(), app.hosts_state.group_by())
            {
                app.notify_error(crate::messages::ungrouped_save_failed(&e));
            } else {
                app.notify(crate::i18n::text(crate::messages::UNGROUPED));
            }
        }
    }
}

/// `u` — undo the most recent bulk-tag apply, else the most recent host
/// delete. Bulk-tag undo takes priority; after a successful tag restore the
/// snapshot is cleared so the next `u` falls through to the delete stack.
pub(super) fn undo_last(app: &mut App) {
    if let Some(snapshot) = app.forms.take_bulk_tag_undo() {
        let config_backup = app.hosts_state.ssh_config().clone();
        for (alias, tags) in &snapshot {
            let _ = app.hosts_state.ssh_config_mut().set_host_tags(alias, tags);
        }
        if let Err(e) = app.hosts_state.ssh_config().write() {
            app.hosts_state.set_ssh_config(config_backup);
            app.forms.set_bulk_tag_undo(Some(snapshot));
            app.notify_error(crate::messages::failed_to_save(&e));
        } else {
            let count = snapshot.len();
            app.update_last_modified();
            app.reload_hosts();
            log::debug!("[purple] undo: restored tags on {count} host(s)");
            app.notify(crate::messages::restored_tags(count));
        }
    } else if let Some(deleted) = app.hosts_state.pop_undo() {
        let alias = match &deleted.element {
            crate::ssh_config::model::ConfigElement::HostBlock(block) => block.host_pattern.clone(),
            _ => "host".to_string(),
        };
        app.hosts_state
            .ssh_config_mut()
            .insert_host_at(deleted.element, deleted.position);
        if let Err(e) = app.hosts_state.ssh_config().write() {
            // Rollback: remove re-inserted host and restore undo buffer
            if let Some((element, position)) = app
                .hosts_state
                .ssh_config_mut()
                .delete_host_undoable(&alias)
            {
                app.hosts_state
                    .undo_stack_mut()
                    .push(crate::app::DeletedHost { element, position });
            }
            app.notify_error(crate::messages::failed_to_save(&e));
        } else {
            app.update_last_modified();
            app.reload_hosts();
            // Restored host has no container_cache entry, queue an initial
            // fetch for THIS alias only.
            app.container_state.queue_fetch(alias.clone());
            log::debug!("[purple] undo: restored host {alias}");
            app.notify(crate::messages::host_restored(&alias));
        }
    } else {
        app.notify_warning(crate::i18n::text(crate::messages::NOTHING_TO_UNDO));
    }
}

/// `m` — open the theme picker, preselecting the active theme and
/// snapshotting the current theme so Esc can restore it.
pub(super) fn open_theme_picker(app: &mut App) {
    let current = crate::ui::theme::current_theme().name;
    let builtins = crate::ui::theme::ThemeDef::builtins();
    let custom = crate::ui::theme::ThemeDef::load_custom(app.env.paths());
    let idx = builtins
        .iter()
        .position(|t| t.name.eq_ignore_ascii_case(&current))
        .or_else(|| {
            if custom.is_empty() {
                None
            } else {
                custom
                    .iter()
                    .position(|t| t.name.eq_ignore_ascii_case(&current))
                    .map(|i| builtins.len() + 1 + i) // +1 for divider
            }
        })
        .unwrap_or(0);
    app.ui.theme_picker_mut().list.select(Some(idx));
    app.ui.theme_picker_mut().builtins = builtins;
    app.ui.theme_picker_mut().custom = custom;
    app.ui.theme_picker_mut().saved_name =
        crate::preferences::load_theme(app.env().paths()).unwrap_or_else(|| "Purple".to_string());
    app.ui.theme_picker_mut().original = Some(crate::ui::theme::current_theme());
    app.set_screen(Screen::ThemePicker);
}

/// `!` — toggle the down-only ping filter. Requires a prior ping; activates
/// search-mode filtering when enabled and tears it down cleanly when disabled.
pub(super) fn toggle_down_filter(app: &mut App) {
    if app.ping.status_is_empty() {
        app.notify_warning(crate::i18n::text(crate::messages::PING_FIRST));
        return;
    }
    app.ping.set_filter_down_only(!app.ping.filter_down_only());
    if app.ping.filter_down_only() {
        // Activate search mode to trigger filtering
        if app.search.query().is_none() {
            app.search.set_query(Some(String::new()));
        }
        app.apply_filter();
        let count = app.search.filtered_indices().len();
        app.notify(crate::messages::showing_unreachable(count));
    } else {
        // If search was only active for down-only, clear it
        if app.search.query().is_some_and(|q| q.is_empty()) {
            app.search.set_query(None);
            app.search.clear_filtered_indices();
            app.search.clear_filtered_pattern_indices();
        } else {
            app.apply_filter();
        }
        app.clear_status();
    }
}

/// `P` — ping every host. Clears existing results first when present
/// (toggle-clear), otherwise marks ProxyJump hosts as Checking and fans out
/// pings to all directly-reachable hosts.
pub(super) fn ping_all_hosts(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    if !app.ping.status_is_empty() {
        log::debug!(
            "[purple] P: clearing {} ping result(s) + timestamps",
            app.ping.status_len()
        );
        app.ping.clear_results();
        app.clear_status();
        return;
    }
    let hosts_to_ping: Vec<(String, String, u16)> = app
        .hosts_state
        .list()
        .iter()
        .filter(|h| !h.hostname.is_empty() && h.proxy_jump.is_empty() && !h.has_proxy_command)
        .map(|h| (h.alias.clone(), h.hostname.clone(), h.port))
        .collect();
    // Mark ProxyJump hosts as Checking (their status will be inherited from
    // the bastion once it responds). ProxyCommand hosts have no bastion to
    // probe, so they show as proxied.
    for h in app.hosts_state.list() {
        if h.has_proxy_command {
            app.ping
                .insert_status(h.alias.clone(), crate::app::PingStatus::Skipped);
        } else if !h.proxy_jump.is_empty() {
            app.ping
                .insert_status(h.alias.clone(), crate::app::PingStatus::Checking);
        }
    }
    if !hosts_to_ping.is_empty() {
        for (alias, _, _) in &hosts_to_ping {
            app.ping
                .insert_status(alias.clone(), crate::app::PingStatus::Checking);
        }
        app.notify_info(crate::i18n::text(crate::messages::PINGING_ALL));
        crate::ping::ping_all(&hosts_to_ping, events_tx.clone(), app.ping.generation());
    }
}

/// `r` — run a snippet on the current selection (multi-select set, else the
/// host under the cursor). Surfaces a stale-host warning and opens the snippet
/// picker; warns when nothing is selected.
pub(super) fn run_snippet_on_selection(app: &mut App) {
    if app.is_pattern_selected() {
        return;
    }
    let (aliases, stale_hint): (Vec<String>, Option<String>) =
        if app.hosts_state.multi_select().is_empty() {
            if let Some(host) = app.selected_host() {
                let hint = super::super::host_form::stale_hint_for(host);
                (vec![host.alias.clone()], hint)
            } else {
                (Vec::new(), None)
            }
        } else {
            let has_stale = app.hosts_state.multi_select().iter().any(|&idx| {
                app.hosts_state
                    .list()
                    .get(idx)
                    .is_some_and(|h| h.stale.is_some())
            });
            (
                app.hosts_state
                    .multi_select()
                    .iter()
                    .filter_map(|&idx| app.hosts_state.list().get(idx).map(|h| h.alias.clone()))
                    .collect(),
                if has_stale {
                    Some(" Selection includes stale hosts.".to_string())
                } else {
                    None
                },
            )
        };
    if let Some(hint) = stale_hint {
        app.notify_warning(crate::messages::stale_host(&hint));
    }
    if aliases.is_empty() {
        app.notify_warning(crate::i18n::text(crate::messages::NO_HOST_SELECTED));
    } else {
        super::super::snippet::open_snippet_picker(app, aliases);
    }
}
