use crossterm::event::KeyModifiers;
use ratatui::widgets::ListState;

use crate::history::ConnectionHistory;
use crate::ssh_config::model::SshConfigFile;

/// Case-insensitive substring check without allocation.
/// Uses a byte-window approach for ASCII strings (the common case for SSH
/// hostnames and aliases). Falls back to a char-based scan when either
/// string contains non-ASCII bytes to avoid false matches across UTF-8
/// character boundaries.
pub(crate) fn contains_ci(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle.is_ascii() {
        return haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()));
    }
    // Non-ASCII fallback: compare char-by-char (case fold ASCII only)
    let needle_lower: Vec<char> = needle.chars().map(|c| c.to_ascii_lowercase()).collect();
    let haystack_chars: Vec<char> = haystack.chars().collect();
    haystack_chars.windows(needle_lower.len()).any(|window| {
        window
            .iter()
            .zip(needle_lower.iter())
            .all(|(h, n)| h.to_ascii_lowercase() == *n)
    })
}

/// Case-insensitive equality check without allocation.
pub(super) fn eq_ci(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

mod baselines;
mod container_state;
mod containers_overview;
mod display_list;
mod file_browser_state;
mod form_state;
mod forms;
mod groups;
mod host_state;
mod hosts;
pub(crate) use hosts::migrate_renames_persistent_state;
pub(crate) mod jump;
mod key_push_state;
mod keys_state;
pub(crate) mod password_prompt;
mod pickers;
pub(crate) mod ping;
mod provider_state;
mod reload_state;
mod screen;
mod search;
mod security_key_pins;
mod selection;
mod snippet_state;
mod status_state;
mod tag_state;
mod tunnel_state;
mod ui_state;
mod update;
mod vault;

pub use baselines::{FormBaseline, ProviderFormBaseline, SnippetFormBaseline, TunnelFormBaseline};
pub use container_state::{ContainerSession, ContainerState, LogsView};
pub use containers_overview::{
    BulkConfirmContext, BulkConfirmKind, ContainerActionRequest, ContainerExecRequest,
    ContainerLogsRequest, ContainersOverviewState, ContainersSortMode, InspectCacheEntry,
    LIST_CACHE_TTL_SECS, LOGS_TAIL, LogsCacheEntry, REFRESH_MAX_PARALLEL, RefreshBatch,
    RefreshQueueItem,
};
pub use file_browser_state::FileBrowserState;
pub use form_state::FormState;
pub(crate) use forms::char_to_byte_pos;
pub use forms::{
    FormField, HostForm, ProviderFormField, ProviderFormFields, SnippetForm, SnippetFormField,
    SnippetHostOutput, SnippetOutputState, SnippetParamFormState, TunnelForm, TunnelFormField,
};
pub use host_state::{
    DeletedHost, GroupBy, HostListItem, HostState, ProxyJumpCandidate, SortMode, ViewMode,
    health_summary_spans, health_summary_spans_for,
};
pub use key_push_state::{KeyPushPrompt, KeyPushState};
pub use keys_state::KeysState;
pub use password_prompt::{
    PASSWORD_MAX_CHARS, PasswordPromptField, PasswordPromptState, PendingRetry, SessionPasswords,
    source_allows_prompt,
};
pub use ping::{
    PingState, PingStatus, classify_ping, ping_sort_key, propagate_ping_to_dependents, status_glyph,
};
pub use provider_state::{
    LabelMigrationField, PendingLabelMigration, PendingPurge, ProviderRow, ProviderState,
    SyncRecord,
};
pub(crate) use reload_state::config_changed;
pub use reload_state::{ConflictState, ReloadState};
pub use screen::{ContainerLogsSearch, Screen, StackMember, TopPage, WhatsNewState};
pub use search::SearchState;
pub use security_key_pins::{PinOutcome, PinTrial, RunEnd, SecurityKeyPins};
pub use snippet_state::{SnippetHostPick, SnippetHostPickPurpose, SnippetState};
pub use status_state::{MessageClass, StatusCenter, StatusMessage};
pub use tag_state::{
    BulkTagAction, BulkTagApplyResult, BulkTagEditorState, BulkTagRow, TagState,
    select_display_tags,
};
pub use tunnel_state::{TunnelExit, TunnelSortMode, TunnelState};
pub use ui_state::UiSelection;
pub use update::UpdateState;
pub use vault::VaultState;

/// Kill active tunnel processes when App is dropped (e.g. on panic).
impl Drop for App {
    fn drop(&mut self) {
        for (alias, mut tunnel) in self.tunnels.active.drain() {
            if let Err(e) = tunnel.child.kill() {
                log::debug!("[external] Failed to kill tunnel for {alias} on shutdown: {e}");
            }
            let _ = tunnel.child.wait();
        }
        // Cancel and join any in-flight Vault SSH bulk-sign worker so it
        // cannot keep writing to ~/.purple/certs/ after teardown (panic
        // unwind, normal exit, etc.).
        if let Some(handle) = self.vault.cancel_signing_run() {
            let _ = handle.join();
        }
        // Same dance for key-push workers: signal cancel, join, so a
        // panic or early exit cannot leave a thread writing to remote
        // authorized_keys after the App is gone.
        self.keys.push.shutdown();
    }
}

/// Main application state.
pub struct App {
    // Core
    /// Currently rendered screen identifier; navigation only, never carries state heaps.
    pub screen: Screen,
    /// Top-level page (Hosts, Tunnels, Containers). Selected by Tab/Shift+Tab
    /// in the navigation bar. Independent of `screen`, which tracks overlays.
    pub top_page: TopPage,
    /// App lifecycle flag; flip to false to exit the event loop.
    pub running: bool,
    /// All host entries plus selection state.
    pub(crate) hosts_state: HostState,

    // Sub-structs
    /// Toast queue, sticky messages, status routing.
    pub(crate) status_center: StatusCenter,
    /// Cursor reveal, detail-toggle, welcome timestamps and overlay meta.
    pub(crate) ui: UiSelection,
    /// Host-list incremental search query and matched hits.
    pub(crate) search: SearchState,
    /// Reload-from-disk state when ~/.ssh/config changes externally.
    pub(crate) reload: ReloadState,
    /// Conflict detection when an external edit clashes with our pending write.
    pub(crate) conflict: ConflictState,

    /// Keys-tab state: discovered keys, push runs, activity log.
    pub(crate) keys: KeysState,

    /// Tag library and per-host tag mappings.
    pub(crate) tags: TagState,

    /// Host form and bulk tag editor scratch state.
    pub(crate) forms: FormState,

    /// Connection history persisted to ~/.purple/history.
    pub(crate) history: ConnectionHistory,

    /// Provider configs, sync runs, host conflict resolution.
    pub(crate) providers: ProviderState,

    /// Ping/health-check state per host.
    pub(crate) ping: PingState,

    /// Vault SSH certificate cache and signing run state.
    pub(crate) vault: VaultState,

    /// Tunnel definitions per host and active tunnel processes.
    pub(crate) tunnels: TunnelState,

    /// Snippet library, parameter forms, output buffers.
    pub(crate) snippets: SnippetState,

    /// Self-update polling and badge state.
    pub(crate) update: UpdateState,

    /// askpass session token; not Keys-tab state.
    pub(crate) bw_session: Option<String>,

    /// Passwords typed in the TUI with "remember" off, per alias, for the
    /// lifetime of the process. Never written to disk.
    pub(crate) session_passwords: SessionPasswords,
    /// Security key PINs typed in the TUI, for the lifetime of the process.
    /// Never written to disk.
    pub(crate) security_key_pins: SecurityKeyPins,
    /// Whether the installed OpenSSH may receive PINs, asked once.
    pub(crate) ssh_allows_security_key_pins: std::cell::OnceCell<bool>,
    /// Payload of `Screen::PasswordPrompt`; Some while the prompt is open.
    pub(crate) password_prompt: Option<PasswordPromptState>,

    // File browser
    /// Persistent per-host last-visited paths; always present.
    pub(crate) file_browser_state: FileBrowserState,
    /// Per-host overlay session; Some when the file browser is open.
    pub(crate) file_browser_session: Option<crate::file_browser::FileBrowserSession>,

    // Containers
    /// Cache and cross-host pending operations; always present.
    pub(crate) container_state: ContainerState,
    /// Per-host overlay session state; Some when the containers overlay is open.
    pub(crate) container_session: Option<ContainerSession>,
    /// Containers tab data: per-host docker ps cache, selection.
    pub(crate) containers_overview: ContainersOverviewState,

    /// Demo mode: all mutations are in-memory only, no disk writes.
    pub demo_mode: bool,

    /// Resolved process environment and filesystem paths, injected once at
    /// construction. Every env/path read goes through here instead of ambient
    /// `std::env` / `dirs::home_dir`. `Arc` so worker closures clone cheaply.
    pub(crate) env: std::sync::Arc<crate::runtime::env::Env>,

    /// Jump state. Some when the jump bar is open.
    pub(crate) jump: Option<JumpState>,
}

/// The OpenSSH version gate for security key PINs. Test builds start it open
/// so no test runs the real `ssh -V`.
fn ssh_version_gate_for_build() -> std::cell::OnceCell<bool> {
    let gate = std::cell::OnceCell::new();
    #[cfg(test)]
    let _ = gate.set(true);
    gate
}

impl App {
    /// Construct with the process environment resolved here. In test builds the
    /// environment is a self-cleaning sandbox so fixtures never touch the real
    /// `~/.purple` or process env and need no lock.
    pub fn new(config: SshConfigFile) -> Self {
        #[cfg(test)]
        let env = std::sync::Arc::new(crate::runtime::env::Env::sandboxed());
        #[cfg(not(test))]
        let env = std::sync::Arc::new(crate::runtime::env::Env::from_process());
        Self::with_env(config, env)
    }

    /// Construct with an explicitly provided environment. The edge
    /// (`launcher::run`) uses this to share one `Env` snapshot with the
    /// pre-TUI CLI handlers.
    pub fn with_env(config: SshConfigFile, env: std::sync::Arc<crate::runtime::env::Env>) -> Self {
        let hosts = config.host_entries();
        let patterns = config.pattern_entries();
        let display_list = Self::build_display_list_from(&config, &hosts, &patterns);

        let initial_selection = display_list.iter().position(|item| {
            matches!(
                item,
                HostListItem::Host { .. } | HostListItem::Pattern { .. }
            )
        });

        let reload = ReloadState::from_config(&env, &config);
        let hosts_state = HostState::from_config(config, hosts, patterns, display_list);

        Self {
            screen: Screen::HostList,
            top_page: TopPage::default(),
            running: true,
            hosts_state,
            status_center: StatusCenter::default(),
            ui: UiSelection::new_with_initial_selection(initial_selection),
            search: SearchState::default(),
            reload,
            conflict: ConflictState::default(),
            keys: KeysState {
                list: Vec::new(),
                list_state: ratatui::widgets::ListState::default(),
                activity: crate::key_activity::KeyActivityLog::load(env.paths()),
                push: KeyPushState::default(),
            },
            tags: TagState::default(),
            forms: FormState::default(),
            history: ConnectionHistory::load(env.paths()),
            providers: ProviderState::load(env.paths()),
            ping: PingState::from_preferences(env.paths()),
            vault: VaultState::default(),
            tunnels: TunnelState::default(),
            snippets: SnippetState::with_store_loaded(env.paths()),
            update: UpdateState::with_current_hint(&env),
            bw_session: None,
            session_passwords: SessionPasswords::new(),
            security_key_pins: SecurityKeyPins::default(),
            ssh_allows_security_key_pins: ssh_version_gate_for_build(),
            password_prompt: None,
            file_browser_state: FileBrowserState::default(),
            file_browser_session: None,
            container_state: ContainerState {
                cache: crate::containers::load_container_cache(env.paths()),
                ..ContainerState::default()
            },
            container_session: None,
            containers_overview: ContainersOverviewState::default(),
            demo_mode: false,
            env,
            jump: None,
        }
    }

    /// The resolved process environment and filesystem paths for this run.
    pub(crate) fn env(&self) -> &crate::runtime::env::Env {
        &self.env
    }

    /// Record an SSH session against `alias` in the activity log. Appends
    /// in memory and flushes to `~/.purple/key_activity.json`. Failures
    /// during flush are logged at debug level only; an activity-log write
    /// failure must never interrupt the user's connect flow. Caller
    /// passes `now`; production call sites pass `key_activity::now_secs()`.
    pub fn record_key_use(&mut self, alias: &str, now: u64) {
        let paths = self.env.paths().cloned();
        crate::key_activity::record_and_flush(&mut self.keys.activity, alias, now, paths.as_ref());
    }

    /// Snapshot the alias of every host currently loaded. Used as
    /// the "before" set for `queue_new_aliases_since` after a
    /// reload that may have added or removed hosts.
    pub fn snapshot_alias_set(&self) -> std::collections::HashSet<String> {
        self.hosts_state
            .list
            .iter()
            .map(|h| h.alias.clone())
            .collect()
    }

    /// Push aliases that are in the current host list but were NOT
    /// in `before_aliases` to the auto-fetch queue. Sync handlers
    /// and external-config-edit detection use this so only freshly
    /// introduced hosts trigger an initial `docker ps`. pre-existing
    /// cache-missing hosts are explicitly left alone.
    pub fn queue_new_aliases_since(&mut self, before_aliases: &std::collections::HashSet<String>) {
        let new_aliases: Vec<String> = self
            .hosts_state
            .list
            .iter()
            .filter(|h| !before_aliases.contains(&h.alias))
            .map(|h| h.alias.clone())
            .collect();
        for alias in new_aliases {
            self.container_state.queue_fetch(alias);
        }
    }

    /// Reload hosts from config.
    ///
    /// Flushes any deferred vault config write, rebuilds the host list
    /// from `ssh_config`, then orchestrates orphan pruning across every
    /// sub-state that keys on host alias or container ID. Each sub-state
    /// owns the prune mechanics via `prune_orphans` (or
    /// `prune_by_container_ids` for the inspect/logs caches). This
    /// function still owns sequencing (container_id derivation requires
    /// the container_cache to be pruned first) and post-prune
    /// persistence (`save_container_cache`,
    /// `save_containers_collapsed_hosts`).
    pub fn reload_hosts(&mut self) {
        let had_pending_vault_write = self.vault.pending_config_write;
        // Synchronously flush any deferred vault config write before reloading,
        // so on-disk state matches in-memory state (no TOCTOU with auto-reload).
        // Skip when a form is open (flush handler would bail anyway) and do not
        // call flush_pending_vault_write() itself to avoid recursion.
        //
        // Before flushing, check whether the on-disk config changed since the
        // in-memory model was loaded. If so, the deferred write would overwrite
        // those external edits silently. Surface a notification and skip the
        // flush; the user can re-trigger vault sign after reviewing their
        // changes. The cert files themselves were already written by the bulk
        // sign worker. Only the config-side `CertificateFile` directives are
        // skipped, which the user can wire up via a fresh sign.
        let mut flushed_vault_write = false;
        if self.vault.pending_config_write && !self.is_form_open() {
            if self.external_config_changed() {
                self.notify_error(
                    crate::messages::vault_config_skipped_external_change().to_string(),
                );
                log::warn!(
                    "[config] reload_hosts: skipping deferred vault write. external config changed"
                );
            } else {
                match self.hosts_state.ssh_config.write() {
                    Ok(()) => flushed_vault_write = true,
                    Err(e) => self.notify_error(crate::messages::vault_config_write_after_sign(&e)),
                }
            }
        }
        // Always clear the flag: either we flushed, we surfaced a conflict, or
        // the form-submit path has already written the full config.
        self.vault.pending_config_write = false;
        log::debug!(
            "[config] reload_hosts: pending_vault_write={had_pending_vault_write} flushed={flushed_vault_write}"
        );
        let had_search = self.search.query.take();
        let selected_alias = self
            .selected_host()
            .map(|h| h.alias.clone())
            .or_else(|| self.selected_pattern().map(|p| p.pattern.clone()));

        self.tunnels.summaries_cache.clear();
        self.hosts_state.render_cache.invalidate();
        self.hosts_state.list = self.hosts_state.ssh_config.host_entries();
        self.hosts_state.patterns = self.hosts_state.ssh_config.pattern_entries();

        // Orphan-prune every per-host sub-state in one scoped block.
        // `valid_aliases` borrows from `self.hosts_state.list`, so the
        // scope ends before any `&mut self` call (apply_sort, set_screen)
        // can run. Within the scope only field-method calls are allowed.
        {
            let valid_aliases: std::collections::HashSet<&str> = self
                .hosts_state
                .list
                .iter()
                .map(|h| h.alias.as_str())
                .collect();

            self.vault.prune_orphans(&valid_aliases);

            // Per-host container cache; persist the trimmed cache when
            // anything dropped so `~/.purple/container_cache.jsonl` does
            // not keep serving orphan entries on the next purple start.
            // Demo mode skips disk writes via `save_container_cache` itself.
            if self.container_state.prune_orphans(&valid_aliases) {
                crate::containers::save_container_cache(
                    self.env().paths(),
                    self.container_state.cache(),
                );
            }

            // Inspect / logs caches key on container ID. Build the
            // valid-id set from the (just-pruned) container_cache and
            // prune both caches in one pass.
            let valid_container_ids: std::collections::HashSet<String> = self
                .container_state
                .cache()
                .values()
                .flat_map(|e| e.containers.iter().map(|c| c.id.clone()))
                .collect();
            self.containers_overview
                .prune_by_container_ids(&valid_container_ids);

            // Per-alias overview state (auto-list in-flight, refresh
            // batch, collapsed-hosts). Persist collapsed_hosts when it
            // shrank.
            if self.containers_overview.prune_orphans(&valid_aliases)
                && let Err(e) = crate::preferences::save_containers_collapsed_hosts(
                    self.env().paths(),
                    self.containers_overview.collapsed_hosts(),
                )
            {
                log::warn!("[config] failed to save collapsed_hosts after prune: {e}");
            }

            self.file_browser_state.prune_orphans(&valid_aliases);
            self.tunnels.prune_orphans(&valid_aliases);
            self.ping.prune_orphans(&valid_aliases);

            // A session password belongs to a host in the list. One whose
            // host is gone waits for whatever takes that alias next, so it
            // is dropped here.
            let before = self.session_passwords.len();
            self.session_passwords
                .retain(|alias, _| valid_aliases.contains(alias.as_str()));
            if self.session_passwords.len() != before {
                log::debug!(
                    "[purple] reload_hosts: dropped {} session password(s) for hosts that are gone",
                    before - self.session_passwords.len()
                );
            }
            if self.security_key_pins.prune_aliases(&valid_aliases) {
                log::debug!(
                    "[purple] reload_hosts: dropped a security key PIN trial for a host that is gone"
                );
            }
        }

        if self.hosts_state.sort_mode == SortMode::Original
            && matches!(self.hosts_state.group_by, GroupBy::None)
        {
            self.hosts_state.display_list = Self::build_display_list_from(
                &self.hosts_state.ssh_config,
                &self.hosts_state.list,
                &self.hosts_state.patterns,
            );
        } else {
            self.apply_sort();
        }

        // Close tag pickers if open. tags.list is stale after reload
        if matches!(self.screen, Screen::TagPicker | Screen::BulkTagEditor) {
            self.set_screen(Screen::HostList);
            self.forms.bulk_tag_editor = BulkTagEditorState::default();
        }

        // Multi-select stores indices into hosts; clear to avoid stale refs
        self.hosts_state.multi_select.clear();

        // Restore search if it was active, otherwise reset
        if let Some(query) = had_search {
            self.search.query = Some(query);
            self.apply_filter();
        } else {
            self.search.query = None;
            self.search.filtered_indices.clear();
            self.search.filtered_pattern_indices.clear();
            // Fix selection for display list mode
            if self.hosts_state.list.is_empty() && self.hosts_state.patterns.is_empty() {
                self.ui.list_state.select(None);
            } else if let Some(pos) = self.hosts_state.display_list.iter().position(|item| {
                matches!(
                    item,
                    HostListItem::Host { .. } | HostListItem::Pattern { .. }
                )
            }) {
                let current = self.ui.list_state.selected().unwrap_or(0);
                if current >= self.hosts_state.display_list.len()
                    || !matches!(
                        self.hosts_state.display_list.get(current),
                        Some(HostListItem::Host { .. } | HostListItem::Pattern { .. })
                    )
                {
                    self.ui.list_state.select(Some(pos));
                }
            } else {
                self.ui.list_state.select(None);
            }
        }

        // Restore selection by alias (e.g. after SSH connect changed sort order)
        if let Some(alias) = selected_alias {
            self.select_host_by_alias(&alias);
        }

        log::debug!(
            "[config] reload_hosts: hosts={} patterns={} display_items={}",
            self.hosts_state.list.len(),
            self.hosts_state.patterns.len(),
            self.hosts_state.display_list.len(),
        );
    }

    /// Synchronously re-check a host's Vault SSH certificate and update
    /// `vault.cert_cache` with fresh status + on-disk mtime.
    ///
    /// Every sign path (V-key bulk sign, host form submit, connect-time
    /// `ensure_vault_ssh_if_needed`, CLI) funnels through this helper so the
    /// detail panel never lies about cert state after a successful sign.
    ///
    /// No-op in demo mode. If the host is missing, has no resolvable vault
    /// role, or the cert path cannot be resolved, any stale entry for the
    /// alias is removed to avoid showing ghost status.
    pub fn refresh_cert_cache(&mut self, alias: &str) {
        if crate::demo_flag::is_demo() {
            return;
        }
        let Some(host) = self.hosts_state.list.iter().find(|h| h.alias == alias) else {
            self.vault.cert_cache.remove(alias);
            return;
        };
        let role_some = crate::vault_ssh::resolve_vault_role(
            host.vault_ssh.as_deref(),
            host.provider.as_deref(),
            host.provider_label.as_deref(),
            &self.providers.config,
        )
        .is_some();
        if !role_some {
            self.vault.cert_cache.remove(alias);
            return;
        }
        let cert_path = match crate::vault_ssh::resolve_cert_path(
            self.env().paths(),
            alias,
            &host.certificate_file,
        ) {
            Ok(p) => p,
            Err(_) => {
                self.vault.cert_cache.remove(alias);
                return;
            }
        };
        let status = crate::vault_ssh::check_cert_validity(self.env(), &cert_path);
        let mtime = std::fs::metadata(&cert_path)
            .ok()
            .and_then(|m| m.modified().ok());
        self.vault.cert_cache.insert(
            alias.to_string(),
            (std::time::Instant::now(), status, mtime),
        );
    }

    // --- Search methods ---

    /// Shim. Routes to `ProviderState::sorted_names`.
    /// Test-only: production code uses `provider_list_rows()` for the
    /// tree-style list, so this wrapper exists to keep older test fixtures
    /// concise.
    #[cfg(test)]
    pub fn sorted_provider_names(&self) -> Vec<String> {
        self.providers.sorted_names()
    }

    /// Check whether a form screen is currently open (host or provider forms).
    pub fn is_form_open(&self) -> bool {
        matches!(
            self.screen,
            Screen::AddHost | Screen::EditHost { .. } | Screen::ProviderForm { .. }
        )
    }

    /// Open the unified jump in the given mode. Loads recents
    /// from disk and seeds the empty-query view. Recomputes hits.
    pub fn open_jump(&mut self, mode: JumpMode) {
        log::debug!("[purple] jump: open mode={:?}", mode);
        let mut state = JumpState::for_mode(mode);
        let recents_file = jump::load_recents(self.env.paths());
        state.recents = self.resolve_recents(&recents_file);
        self.jump = Some(state);
        self.recompute_jump_hits();
    }

    /// Close the unified jump overlay. Idempotent: a no-op when no jump
    /// is open. Pairs with `open_jump`; the three handler arms (Esc,
    /// Enter-after-dispatch, Backspace-on-empty) all route through here.
    pub(crate) fn close_jump(&mut self) {
        self.jump = None;
    }

    /// Translate the on-disk recents log into live `JumpHit`s, dropping
    /// dangling references silently.
    fn resolve_recents(&self, file: &RecentsFile) -> Vec<JumpHit> {
        let mode = self
            .jump
            .as_ref()
            .map(|p| p.mode)
            .unwrap_or(JumpMode::Hosts);
        let mut out = Vec::with_capacity(file.entries.len());
        for entry in &file.entries {
            if let Some(hit) = self.resolve_recent_ref(&entry.target, mode) {
                out.push(hit);
            }
        }
        out
    }

    /// Test seam: exposes `resolve_recent_ref` as `pub(crate)` so the unit
    /// tests in `app::tests` can drive each `SourceKind` branch without
    /// going through `open_jump`.
    #[cfg(test)]
    pub(crate) fn resolve_recent_ref_for_test(
        &self,
        r: &RecentRef,
        mode: JumpMode,
    ) -> Option<JumpHit> {
        self.resolve_recent_ref(r, mode)
    }

    fn resolve_recent_ref(&self, r: &RecentRef, mode: JumpMode) -> Option<JumpHit> {
        match r.kind {
            SourceKind::Action => {
                let key_char = r.key.chars().next()?;
                let actions = JumpAction::for_mode(mode);
                actions
                    .iter()
                    .find(|a| a.key == key_char)
                    .copied()
                    .map(JumpHit::Action)
            }
            SourceKind::Host => {
                let host = self.hosts_state.list.iter().find(|h| h.alias == r.key)?;
                Some(JumpHit::Host(HostHit {
                    alias: host.alias.clone(),
                    hostname: host.hostname.clone(),
                    tags: host.tags.clone(),
                    provider: host.provider.clone(),
                    user: host.user.clone(),
                    identity_file: host.identity_file.clone(),
                    proxy_jump: host.proxy_jump.clone(),
                    vault_ssh: host.vault_ssh.clone(),
                }))
            }
            SourceKind::Tunnel => {
                let (alias, port_str) = r.key.split_once(':')?;
                let port: u16 = port_str.parse().ok()?;
                let rules = self.hosts_state.ssh_config.find_tunnel_directives(alias);
                let rule = rules.iter().find(|r| r.bind_port == port)?;
                Some(JumpHit::Tunnel(TunnelHit {
                    alias: alias.to_string(),
                    bind_port: rule.bind_port,
                    bind_port_str: rule.bind_port.to_string(),
                    destination: rule.display(),
                    active: self.tunnels.active.contains_key(alias),
                }))
            }
            SourceKind::Container => {
                let (alias, name) = r.key.split_once('/')?;
                let entry = self.container_state.cache.get(alias)?;
                let info = entry.containers.iter().find(|c| c.names == name)?;
                Some(JumpHit::Container(ContainerHit {
                    alias: alias.to_string(),
                    container_name: info.names.clone(),
                    container_id: info.id.clone(),
                    state: info.state.clone(),
                }))
            }
            SourceKind::Snippet => {
                let snippet = self.snippets.store.get(&r.key)?;
                Some(JumpHit::Snippet(SnippetHit {
                    name: snippet.name.clone(),
                    command_preview: preview(&snippet.command, 40),
                }))
            }
        }
    }

    /// Recompute the jump bar hit list against the current query. Pulls
    /// candidates from every live source and ranks them with nucleo-matcher.
    /// Preserves the previously-selected hit's identity across the
    /// recompute so mid-typing arrow-key navigation does not jump back to
    /// row 0.
    pub fn recompute_jump_hits(&mut self) {
        let Some(mut state) = self.jump.take() else {
            return;
        };
        // Identity of the row the user was on before the recompute. We
        // re-resolve it after rebuilding `hits` to keep selection stable
        // when the user types and the matched row is still in the list.
        let prior_identity = state
            .visible_hits()
            .get(state.selected)
            .map(|h| h.identity());

        let candidates = self.collect_jump_candidates(state.mode);
        if state.query.is_empty() {
            state.hits = candidates;
            state.selected = restore_selection(&state.visible_hits(), prior_identity.as_ref(), 0);
            self.jump = Some(state);
            return;
        }

        // Field-prefix syntax: `user:eric` scopes to one field. Empty
        // remainder after the prefix is treated as no query (empty
        // scope-search). Mode is held in `query_scope` for the row
        // renderer to surface a "via <field>" hint.
        let (scope, effective_query) = parse_query_scope(&state.query);

        use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
        use nucleo_matcher::{Config, Matcher, Utf32Str};
        let matcher_state = state
            .matcher
            .get_or_insert_with(|| Matcher::new(Config::DEFAULT));
        let pattern = Pattern::parse(effective_query, CaseMatching::Smart, Normalization::Smart);
        let mut buf: Vec<char> = Vec::new();
        let mut scored: Vec<(JumpHit, u32)> = Vec::with_capacity(candidates.len());
        for hit in candidates {
            let mut best: u32 = 0;
            // Score over the right haystack set: scoped queries narrow to
            // a single field; unscoped queries score over everything the
            // hit advertises.
            let scoped_haystacks = scoped_haystacks_for(&hit, scope);
            let haystacks: Vec<&str> = if let Some(hs) = scoped_haystacks {
                hs
            } else {
                hit.haystacks()
            };
            for haystack in haystacks {
                buf.clear();
                let chars = Utf32Str::new(haystack, &mut buf);
                if let Some(score) = pattern.score(chars, matcher_state) {
                    best = best.max(score);
                }
            }
            // Boost: a single-char query that exactly matches an action's
            // hotkey letter (case-insensitive) lands the action at the top.
            // When two actions share the same hotkey (e.g. 'a' for `Hosts:
            // Add host` and `Tunnels: Add tunnel`), the one whose target
            // matches the current mode wins, so muscle memory survives.
            // Multi-char queries get a smaller mode-match nudge so that
            // typing `sort` from the Tunnels tab still prefers
            // `Tunnels: Sort` over `Hosts: Sort` even though both labels
            // contain the same substring.
            if let JumpHit::Action(a) = &hit {
                let mode_match = matches!(
                    (state.mode, a.target),
                    (JumpMode::Hosts, JumpActionTarget::Hosts)
                        | (JumpMode::Tunnels, JumpActionTarget::Tunnels)
                        | (JumpMode::Containers, JumpActionTarget::Containers)
                        | (JumpMode::Keys, JumpActionTarget::Keys)
                );
                let single = effective_query.chars().next();
                let exact_hotkey = effective_query.chars().count() == 1
                    && single
                        .map(|c| c.eq_ignore_ascii_case(&a.key))
                        .unwrap_or(false);
                let bump = if exact_hotkey && mode_match {
                    20_000
                } else if exact_hotkey {
                    10_000
                } else if mode_match && best > 0 {
                    // Only nudge when the action already matched on its own.
                    // Without this guard the boost would float every
                    // current-mode action above the score floor and surface
                    // it for unrelated queries.
                    1_000
                } else {
                    0
                };
                if bump > 0 {
                    best = best.saturating_add(bump);
                }
            }
            // Score floor: actions need to clear a higher bar than data
            // rows. Stops query 'eric' from dragging in 'Containers: List
            // containers' on stray e/r/i/c char overlap.
            let floor = match &hit {
                JumpHit::Action(_) => jump::PALETTE_ACTION_FLOOR,
                _ => 1,
            };
            if best >= floor {
                scored.push((hit, best));
            }
        }
        // Stable sort: higher score first, ties broken by render-order kind so
        // hosts come before actions when scores tie.
        scored.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| kind_rank(a.0.kind()).cmp(&kind_rank(b.0.kind())))
        });
        // Cap per-section using a fixed-size array so a broad query (one
        // char that matches everything) cannot blow the visible list.
        let mut per_kind: [usize; 5] = [0; 5];
        let mut filtered: Vec<JumpHit> = Vec::with_capacity(scored.len().min(160));
        for (hit, _) in scored {
            let slot = kind_rank(hit.kind()) as usize;
            if per_kind[slot] < PALETTE_PER_SECTION_CAP {
                per_kind[slot] += 1;
                filtered.push(hit);
            }
        }
        state.hits = filtered;
        // `state.hits` is score-sorted but `visible_hits()` reorders into
        // fixed render-section order. Default the cursor to the top-scored
        // hit's position in that display order so the boosted best match
        // stays pre-selected and the highlight lands on the row that Enter
        // will dispatch, even when a lower-scored host renders above it. The
        // top-scored hit is the first of its kind in display order, so its
        // position is the first row of that section. Resolving by kind
        // sidesteps the non-unique action/tunnel/container identities.
        let display = state.visible_hits();
        let top_display = state
            .hits
            .first()
            .map(|h| h.kind())
            .and_then(|k| display.iter().position(|h| h.kind() == k))
            .unwrap_or(0);
        state.selected = restore_selection(&display, prior_identity.as_ref(), top_display);
        log::debug!(
            "[purple] jump: recompute selected={} of {} hits (top_display={})",
            state.selected,
            state.hits.len(),
            top_display
        );
        self.jump = Some(state);
    }

    fn collect_jump_candidates(&self, mode: JumpMode) -> Vec<JumpHit> {
        let mut out: Vec<JumpHit> = Vec::new();
        // Hosts
        for h in &self.hosts_state.list {
            out.push(JumpHit::Host(HostHit {
                alias: h.alias.clone(),
                hostname: h.hostname.clone(),
                tags: h.tags.clone(),
                provider: h.provider.clone(),
                user: h.user.clone(),
                identity_file: h.identity_file.clone(),
                proxy_jump: h.proxy_jump.clone(),
                vault_ssh: h.vault_ssh.clone(),
            }));
        }
        // Tunnels: every configured rule from every host with a directive.
        for h in &self.hosts_state.list {
            let rules = self.hosts_state.ssh_config.find_tunnel_directives(&h.alias);
            for rule in rules {
                out.push(JumpHit::Tunnel(TunnelHit {
                    alias: h.alias.clone(),
                    bind_port: rule.bind_port,
                    bind_port_str: rule.bind_port.to_string(),
                    destination: rule.display(),
                    active: self.tunnels.active.contains_key(&h.alias),
                }));
            }
        }
        // Containers: cached only. Triggering an SSH fetch on jump bar open
        // would be unbounded latency.
        for (alias, entry) in &self.container_state.cache {
            for info in &entry.containers {
                out.push(JumpHit::Container(ContainerHit {
                    alias: alias.clone(),
                    container_name: info.names.clone(),
                    container_id: info.id.clone(),
                    state: info.state.clone(),
                }));
            }
        }
        // Snippets
        for snippet in &self.snippets.store.snippets {
            out.push(JumpHit::Snippet(SnippetHit {
                name: snippet.name.clone(),
                command_preview: preview(&snippet.command, 40),
            }));
        }
        // Actions last
        for a in JumpAction::for_mode(mode) {
            out.push(JumpHit::Action(*a));
        }
        out
    }

    /// Persist a jump dispatch to the on-disk MRU log. Best-effort; a
    /// write error logs and is otherwise swallowed so user navigation is
    /// never blocked by a recents-file failure. Takes `&mut self` so the
    /// type system reflects that this performs I/O and mutates persistent
    /// state, even though `jump::save_recents` only needs `&File`.
    pub fn record_jump_hit(&mut self, hit: &JumpHit) {
        if self.demo_mode {
            log::debug!("[purple] jump: record skipped (demo mode)");
            return;
        }
        let paths = self.env.paths().cloned();
        let mut file = jump::load_recents(paths.as_ref());
        jump::touch_recent(&mut file, hit.identity());
        if let Err(e) = jump::save_recents(&file, paths.as_ref()) {
            log::warn!("[purple] failed to save recents: {e}");
        }
    }

    /// Open the file-browser overlay with the given session. Stores the
    /// session and switches to `Screen::FileBrowser` for the session's
    /// alias. Any previously-open session is replaced.
    pub(crate) fn open_file_browser(&mut self, session: crate::file_browser::FileBrowserSession) {
        let alias = session.alias.clone();
        self.file_browser_session = Some(session);
        self.set_screen(Screen::FileBrowser { alias });
    }

    /// Close the file-browser overlay. Persists the current pane paths to
    /// `file_browser_state.host_paths` so the next open re-seeds them,
    /// drops the session, and returns to the host list.
    pub(crate) fn close_file_browser(&mut self) {
        if let Some(fb) = self.file_browser_session.take() {
            self.abandon_security_key_trial(
                crate::security_key::PinFlow::FileBrowser,
                Some(&fb.alias),
            );
            self.file_browser_state
                .host_paths
                .insert(fb.alias, (fb.local_path, fb.remote_path));
        }
        self.set_screen(Screen::HostList);
    }

    /// Flush a deferred vault config write if one is pending and no form is open.
    /// Returns true if a write was performed.
    pub fn flush_pending_vault_write(&mut self) -> bool {
        if !self.vault.pending_config_write || self.is_form_open() {
            return false;
        }
        // reload_hosts() performs the write and clears the flag.
        self.reload_hosts();
        true
    }

    /// Run once after App::new: queue the upgrade toast if the user just
    /// upgraded past their last-seen version, otherwise seed the preference
    /// so the next launch is silent.
    pub fn post_init(&mut self) {
        let outcome = crate::onboarding::evaluate(self.env().paths());
        if let Some(text) = outcome.upgrade_toast {
            self.enqueue_sticky_toast(text);
        }
        // Seed the Keys tab so the first Tab navigation lands on a
        // populated list. Subsequent reloads run via R or after a host
        // form save / provider sync.
        self.scan_keys();
    }

    fn enqueue_sticky_toast(&mut self, text: String) {
        log::debug!("[purple] enqueue sticky toast: {}", text);
        let msg = StatusMessage {
            text,
            class: MessageClass::Success,
            tick_count: 0,
            sticky: true,
            created_at: std::time::Instant::now(),
        };
        self.status_center.toast = Some(msg);
    }

    /// User action feedback. Success toast, length-proportional timeout.
    pub fn notify(&mut self, text: impl Into<String>) {
        self.status_center.set_status(text, false);
    }

    /// User action error. Error toast, sticky by default, queued.
    pub fn notify_error(&mut self, text: impl Into<String>) {
        self.status_center.set_status(text, true);
    }

    /// Background event. Info footer, suppressed if sticky active.
    pub fn notify_background(&mut self, text: impl Into<String>) {
        self.status_center.set_background_status(text, false);
    }

    /// Background error. Sticky toast, bypasses sticky suppression.
    pub fn notify_background_error(&mut self, text: impl Into<String>) {
        self.status_center.set_background_status(text, true);
    }

    /// Caution / degraded state → Warning toast (length-proportional
    /// timeout, queued). For: precondition violations ("Nothing to undo."),
    /// validation hints ("Project ID can't be empty."), empty-state
    /// notices ("No stale hosts."), stale-host warnings, deprecated
    /// config detected, partial sync results. Warnings are NOT sticky;
    /// the user acknowledges them by continuing to interact.
    ///
    /// Use `notify_error` only for system-level failures (I/O, network,
    /// subprocess) that require explicit acknowledgement. Use
    /// `notify_warning` for everything that is "this can't happen given
    /// current state" or "you forgot something".
    pub fn notify_warning(&mut self, text: impl Into<String>) {
        let msg = StatusMessage {
            text: text.into(),
            class: MessageClass::Warning,
            tick_count: 0,
            sticky: false,
            created_at: std::time::Instant::now(),
        };
        log::debug!("[purple] toast <- Warning: {}", msg.text);
        self.status_center.push_toast(msg);
    }

    /// Long-running progress. Footer sticky, never expires automatically.
    pub fn notify_progress(&mut self, text: impl Into<String>) {
        self.status_center.set_sticky_status(text, false);
    }

    /// Sticky error. Footer sticky, never expires automatically.
    pub fn notify_sticky_error(&mut self, text: impl Into<String>) {
        self.status_center.set_sticky_status(text, true);
    }

    /// Explicit info. Footer, 4s timeout, not suppressed by sticky.
    pub fn notify_info(&mut self, text: impl Into<String>) {
        self.status_center.set_info_status(text);
    }

    /// Drop the footer status unconditionally. Use when a new user action
    /// makes the prior status stale. Symmetric with the `notify_*` family
    /// so handlers stay on the App surface instead of reaching into
    /// `status_center` directly.
    pub(crate) fn clear_status(&mut self) {
        self.status_center.clear_status();
    }

    /// Tick the footer status message timer. Uses wall-clock time.
    /// Sticky/Progress messages never expire automatically.
    ///
    /// Stays on `App` (not moved to `StatusCenter`) because expiry is
    /// suppressed while any provider sync is in flight, which requires
    /// reading `self.providers.syncing`.
    pub fn tick_status(&mut self) {
        // Don't expire status while providers are still syncing
        if !self.providers.syncing.is_empty() {
            return;
        }
        if let Some(ref status) = self.status_center.status {
            if status.sticky {
                return;
            }
            let timeout_ms = status.timeout_ms();
            if timeout_ms != u64::MAX && status.created_at.elapsed().as_millis() as u64 > timeout_ms
            {
                log::debug!("[purple] footer status expired: {}", status.text);
                self.status_center.status = None;
            }
        }
    }

    /// Shim. Routes to `StatusCenter::tick_toast`.
    pub fn tick_toast(&mut self) {
        self.status_center.tick_toast();
    }

    /// Check if config or any Include file has changed externally and reload if so.
    /// Skips reload when the user is in a form (AddHost/EditHost) to avoid
    /// overwriting in-memory config while the user is editing.
    pub fn check_config_changed(&mut self) {
        if matches!(
            self.screen,
            Screen::AddHost
                | Screen::EditHost { .. }
                | Screen::ProviderForm { .. }
                | Screen::TunnelList { .. }
                | Screen::TunnelForm { .. }
                | Screen::HostDetail { .. }
                | Screen::SnippetPicker
                | Screen::SnippetForm
                | Screen::SnippetOutput
                | Screen::SnippetParamForm
                | Screen::FileBrowser { .. }
                | Screen::Containers { .. }
                | Screen::ConfirmDelete { .. }
                | Screen::ConfirmHostKeyReset { .. }
                | Screen::ConfirmHostKeyTrust { .. }
                | Screen::PasswordPrompt
                | Screen::ConfirmPurgeStale
                | Screen::ConfirmImport { .. }
                | Screen::ConfirmVaultSign
                | Screen::TagPicker
                | Screen::BulkTagEditor
                | Screen::ThemePicker
                | Screen::WhatsNew(_)
        ) || self.tags.input.is_some()
        {
            return;
        }
        let current_mtime = reload_state::get_mtime(&self.reload.config_path);
        let changed = current_mtime != self.reload.last_modified
            || self
                .reload
                .include_mtimes
                .iter()
                .any(|(path, old_mtime)| reload_state::get_mtime(path) != *old_mtime)
            || self
                .reload
                .include_dir_mtimes
                .iter()
                .any(|(path, old_mtime)| reload_state::get_mtime(path) != *old_mtime);
        if changed {
            log::debug!(
                "[config] check_config_changed: mtime drift detected on {} -> reloading",
                self.reload.config_path.display()
            );
            if let Ok(new_config) =
                SshConfigFile::parse_with_env(&self.reload.config_path, &self.env)
            {
                let before_aliases = self.snapshot_alias_set();
                self.hosts_state.ssh_config = new_config;
                // Invalidate undo state. config structure may have changed externally
                self.hosts_state.undo_stack.clear();
                // Clear stale ping status. hosts may have changed
                log::debug!(
                    "[config] external config change: clearing {} ping result(s) + timestamps",
                    self.ping.status.len()
                );
                self.ping.status.clear();
                self.ping.last_checked.clear();
                self.ping.filter_down_only = false;
                self.ping.checked_at = None;
                self.reload_hosts();
                self.reload.last_modified = current_mtime;
                self.reload.include_mtimes =
                    reload_state::snapshot_include_mtimes(&self.hosts_state.ssh_config);
                self.reload.include_dir_mtimes = reload_state::snapshot_include_dir_mtimes(
                    &self.env,
                    &self.hosts_state.ssh_config,
                );
                let count = self.hosts_state.list.len();
                self.notify_background(crate::messages::config_reloaded(count));
                self.queue_new_aliases_since(&before_aliases);
            }
        }
    }

    /// Detect external changes to `~/.ssh/` keys and refresh `self.keys.list`
    /// when something has moved. Mirrors `check_config_changed` for the
    /// keys tab so users see new key files (or deletions, or rotations)
    /// without pressing R. Cheap: a single dir stat plus one stat per
    /// tracked key. Called from the 4-second throttle in `handle_tick`.
    ///
    /// Skips during demo mode (the demo seeds a fixed key list and never
    /// reads from disk) and when a form is open that could be mutating
    /// the same data.
    pub fn check_keys_changed(&mut self) {
        if self.demo_mode {
            return;
        }
        if matches!(
            self.screen,
            Screen::AddHost | Screen::EditHost { .. } | Screen::ProviderForm { .. }
        ) {
            return;
        }
        let Some(ssh_dir) = self.env().paths().map(crate::runtime::env::Paths::ssh_dir) else {
            return;
        };
        let current_dir_mtime = reload_state::get_mtime(&ssh_dir);
        let dir_changed = current_dir_mtime != self.reload.keys_dir_mtime;
        let files_changed = self
            .reload
            .key_file_mtimes
            .iter()
            .any(|(path, old)| reload_state::get_mtime(path) != *old);
        if !dir_changed && !files_changed {
            return;
        }
        log::debug!(
            "[purple] check_keys_changed: drift detected on {} (dir={} files={}) -> rescan",
            ssh_dir.display(),
            dir_changed,
            files_changed,
        );
        let previous = self.keys.list.len();
        self.scan_keys();
        let after = self.keys.list.len();
        // Keep the selection valid after a rescan: clamp to the new list
        // length, or land on the first row when the list grew from empty.
        if let Some(sel) = self.keys.list_state.selected() {
            if sel >= after {
                let next = after.checked_sub(1);
                self.keys.list_state.select(next);
            }
        } else if after > 0 {
            self.keys.list_state.select(Some(0));
        }
        if previous != after {
            log::debug!(
                "[purple] check_keys_changed: rescan {} -> {} keys",
                previous,
                after
            );
        }
    }

    /// Non-mutating check: has the on-disk config (or any tracked Include)
    /// been modified since `self.reload.last_modified` was captured? Used by
    /// async write paths (e.g. the Vault SSH bulk-sign completion handler)
    /// to refuse writing when an external editor changed the file underneath
    /// us. overwriting those edits would silently discard user work. The
    /// backup-on-write mechanism in `SshConfigFile::write()` would still
    /// recover them, but detecting the conflict BEFORE writing is strictly
    /// better than after.
    pub fn external_config_changed(&self) -> bool {
        let current_mtime = reload_state::get_mtime(&self.reload.config_path);
        current_mtime != self.reload.last_modified
            || self
                .reload
                .include_mtimes
                .iter()
                .any(|(path, old_mtime)| reload_state::get_mtime(path) != *old_mtime)
            || self
                .reload
                .include_dir_mtimes
                .iter()
                .any(|(path, old_mtime)| reload_state::get_mtime(path) != *old_mtime)
    }

    /// Update the last_modified timestamp (call after writing config).
    pub fn update_last_modified(&mut self) {
        self.reload.last_modified = reload_state::get_mtime(&self.reload.config_path);
        self.reload.include_mtimes =
            reload_state::snapshot_include_mtimes(&self.hosts_state.ssh_config);
        self.reload.include_dir_mtimes =
            reload_state::snapshot_include_dir_mtimes(&self.env, &self.hosts_state.ssh_config);
    }

    /// Returns true if any host or provider has a vault role configured.
    pub fn has_any_vault_role(&self) -> bool {
        for host in &self.hosts_state.list {
            if host.vault_ssh.is_some() {
                return true;
            }
        }
        for section in &self.providers.config.sections {
            if !section.vault_role.is_empty() {
                return true;
            }
        }
        false
    }

    /// Poll active tunnels for exit. Returns one `TunnelExit` per ended tunnel.
    pub fn poll_tunnels(&mut self) -> Vec<TunnelExit> {
        self.tunnels.poll()
    }

    /// Recompute the lsof poller's bind-port list from the current
    /// `active` map plus each host's directives in the SSH config.
    /// Called after every tunnel start/stop. The poller picks up the
    /// new list on its next iteration.
    pub fn refresh_tunnel_bind_ports(&mut self) {
        let mut ports: Vec<(String, u16, u32)> = Vec::new();
        for (alias, tunnel) in &self.tunnels.active {
            let pid = tunnel.child.id();
            for rule in self.hosts_state.ssh_config.find_tunnel_directives(alias) {
                ports.push((alias.clone(), rule.bind_port, pid));
            }
        }
        self.tunnels.set_lsof_ports(ports);
    }
}

/// Cycle list selection forward or backward with wraparound.
pub(crate) fn cycle_selection(state: &mut ListState, len: usize, forward: bool) {
    if len == 0 {
        return;
    }
    let i = match state.selected() {
        Some(i) => {
            if forward {
                if i >= len - 1 { 0 } else { i + 1 }
            } else if i == 0 {
                len - 1
            } else {
                i - 1
            }
        }
        None => 0,
    };
    state.select(Some(i));
}

/// Apply pending bulk-tag actions to the selected hosts and persist the
/// config. Slice-scoped: touches only host and form state, so the caller runs
/// the whole-App tails (mtime refresh, reload) when the result reports changed
/// hosts. Leaves the config untouched on a write failure so the user can retry.
pub(crate) fn apply_bulk_tags(
    hosts: &mut HostState,
    forms: &mut FormState,
) -> Result<BulkTagApplyResult, String> {
    if forms.bulk_tag_editor.aliases.is_empty() {
        return Err(crate::messages::BULK_TAG_NO_HOSTS_SELECTED.to_string());
    }
    let aliases = forms.bulk_tag_editor.aliases.clone();
    let rows = forms.bulk_tag_editor.rows.clone();
    let skipped_set: std::collections::HashSet<&str> = forms
        .bulk_tag_editor
        .skipped_included
        .iter()
        .map(|s| s.as_str())
        .collect();

    // Short-circuit when the user opened the editor but never changed any
    // row. Avoids a no-op config write and a confusing toast.
    let has_pending = rows.iter().any(|r| r.action != BulkTagAction::Leave);
    if !has_pending {
        return Ok(BulkTagApplyResult {
            skipped_included: skipped_set.len(),
            ..Default::default()
        });
    }

    let mut changed_hosts: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut skipped_included = 0usize;
    // Captured only when a host actually changes so `u` can undo the whole
    // bulk op in one keystroke. Collected before the write so a write failure
    // leaves the snapshot untouched (we roll back config anyway below).
    let mut undo_snapshot: Vec<(String, Vec<String>)> = Vec::new();

    for alias in &aliases {
        if skipped_set.contains(alias.as_str()) {
            skipped_included += 1;
            continue;
        }
        let Some(host) = hosts.list.iter().find(|h| &h.alias == alias) else {
            continue;
        };
        let original_tags = host.tags.clone();
        let mut new_tags = original_tags.clone();
        let mut host_changed = false;
        for row in &rows {
            match row.action {
                BulkTagAction::Leave => {}
                BulkTagAction::AddToAll => {
                    if !new_tags.iter().any(|t| t == &row.tag) {
                        new_tags.push(row.tag.clone());
                        added += 1;
                        host_changed = true;
                    }
                }
                BulkTagAction::RemoveFromAll => {
                    let before = new_tags.len();
                    new_tags.retain(|t| t != &row.tag);
                    if new_tags.len() != before {
                        removed += 1;
                        host_changed = true;
                    }
                }
            }
        }
        if host_changed {
            let _ = hosts.ssh_config.set_host_tags(alias, &new_tags);
            changed_hosts.insert(alias.clone());
            undo_snapshot.push((alias.clone(), original_tags));
        }
    }

    if changed_hosts.is_empty() {
        return Ok(BulkTagApplyResult {
            skipped_included,
            ..Default::default()
        });
    }

    // Clone only when we actually need to write, so no-op applies skip the
    // allocation entirely.
    let config_backup = hosts.ssh_config.clone();
    if let Err(e) = hosts.ssh_config.write() {
        log::error!("[purple] bulk tag apply write failed: {e}");
        hosts.ssh_config = config_backup;
        return Err(format!("Failed to save: {}", e));
    }

    log::debug!(
        "[purple] bulk tag apply: {} hosts, +{} -{}, skipped {}",
        changed_hosts.len(),
        added,
        removed,
        skipped_included
    );
    // Store the undo snapshot so `u` can restore previous tags. Cleared by a
    // successful undo or by the next config mutation.
    if !undo_snapshot.is_empty() {
        forms.bulk_tag_undo = Some(undo_snapshot);
    }

    Ok(BulkTagApplyResult {
        changed_hosts: changed_hosts.len(),
        added,
        removed,
        skipped_included,
    })
}

/// Cycle the bulk-tag row under the cursor through its tri-state action.
/// Slice-scoped so the handler can call it without borrowing the whole App.
pub(crate) fn bulk_tag_cycle_current(ui: &UiSelection, forms: &mut FormState) {
    let Some(idx) = ui.bulk_tag_editor_state.selected() else {
        return;
    };
    if let Some(row) = forms.bulk_tag_editor.rows.get_mut(idx) {
        row.action = row.action.cycle();
    }
}

/// Append a freshly typed tag to the bulk-tag row list, marked `AddToAll`.
/// No-op on empty input; flips the existing row to `AddToAll` on a duplicate.
/// Slice-scoped so the handler can call it without borrowing the whole App.
pub(crate) fn bulk_tag_commit_new_tag(ui: &mut UiSelection, forms: &mut FormState) {
    let Some(input) = forms.bulk_tag_editor.new_tag_input.take() else {
        return;
    };
    forms.bulk_tag_editor.new_tag_cursor = 0;
    let tag = input.trim().to_string();
    if tag.is_empty() {
        return;
    }
    if let Some(existing) = forms.bulk_tag_editor.rows.iter().position(|r| r.tag == tag) {
        forms.bulk_tag_editor.rows[existing].action = BulkTagAction::AddToAll;
        ui.bulk_tag_editor_state.select(Some(existing));
        return;
    }
    let row = BulkTagRow {
        tag,
        initial_count: 0,
        action: BulkTagAction::AddToAll,
    };
    let insert_at = forms.bulk_tag_editor.rows.len();
    forms.bulk_tag_editor.rows.push(row);
    ui.bulk_tag_editor_state.select(Some(insert_at));
}

/// Jump forward by page_size items, clamping at the end (no wrap).
pub(crate) fn page_down(state: &mut ListState, len: usize, page_size: usize) {
    if len == 0 {
        return;
    }
    let current = state.selected().unwrap_or(0);
    let next = (current + page_size).min(len - 1);
    state.select(Some(next));
}

/// Jump backward by page_size items, clamping at 0 (no wrap).
pub(crate) fn page_up(state: &mut ListState, len: usize, page_size: usize) {
    if len == 0 {
        return;
    }
    let current = state.selected().unwrap_or(0);
    let prev = current.saturating_sub(page_size);
    state.select(Some(prev));
}

// Re-export the jump bar types so call sites keep referring to them via
// `crate::app::JumpHit` / `crate::app::JumpAction` without caring
// which submodule they live in.
pub use jump::{
    ContainerHit, HostHit, JumpAction, JumpActionTarget, JumpHit, JumpMode, JumpState, RecentRef,
    RecentsFile, SnippetHit, SourceKind, TunnelHit,
};

/// Backwards-compatible alias for the old `PaletteCommand` (now `JumpAction`) name. The
/// renamed type is `JumpAction`. Test-only. there is no production
/// caller.
#[cfg(test)]
pub type PaletteCommand = JumpAction;

/// Unified action set. Every action declares its `target` so dispatch
/// switches `top_page` first, then synthesises the hotkey for the right
/// handler. The jump bar shows this same list regardless of which
/// top-page was active when it opened. so the overlay size is
/// consistent and `Tunnels: Add tunnel` is reachable from the Hosts
/// tab and vice versa.
static ALL_JUMP_ACTIONS: &[JumpAction] = &[
    JumpAction {
        key: 'a',
        key_str: "a",
        label: "Hosts: Add host",
        aliases: &["new", "create"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'A',
        key_str: "A",
        label: "Hosts: Add pattern",
        aliases: &["new pattern", "wildcard"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'e',
        key_str: "e",
        label: "Hosts: Edit host",
        aliases: &["modify", "change"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'd',
        key_str: "d",
        label: "Hosts: Delete host",
        aliases: &["remove", "rm"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'c',
        key_str: "c",
        label: "Hosts: Clone host",
        aliases: &["duplicate", "copy"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'u',
        key_str: "u",
        label: "Hosts: Undo delete",
        aliases: &["restore"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 't',
        key_str: "t",
        label: "Hosts: Tag host",
        aliases: &["label", "category"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'i',
        key_str: "i",
        label: "Hosts: Show all directives",
        aliases: &["raw", "config", "settings"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'y',
        key_str: "y",
        label: "Clipboard: Copy SSH command",
        aliases: &["yank"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'x',
        key_str: "x",
        label: "Clipboard: Copy config block",
        aliases: &["yank config"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'X',
        key_str: "X",
        label: "Hosts: Purge stale hosts",
        aliases: &["clean", "cleanup"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'F',
        key_str: "F",
        label: "Files: Browse remote files",
        aliases: &[
            "browse",
            "filesystem",
            "scp",
            "sftp",
            "transfer",
            "explorer",
            "open",
        ],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'C',
        key_str: "C",
        label: "Containers: List containers",
        aliases: &["docker", "podman", "ps", "open"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'K',
        key_str: "K",
        label: "Keys: Manage SSH keys",
        aliases: &["identity", "id_rsa", "id_ed25519", "private key", "open"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'S',
        key_str: "S",
        label: "Providers: Manage cloud sync",
        aliases: &["cloud", "aws", "gcp", "azure", "hetzner", "sync", "open"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'V',
        key_str: "V",
        label: "Vault: Sign certificate",
        aliases: &["hashicorp", "ssh cert", "vault ssh"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'I',
        key_str: "I",
        label: "Hosts: Import from known_hosts",
        aliases: &["known", "import"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'm',
        key_str: "m",
        label: "Settings: Switch theme",
        aliases: &["color", "appearance", "dark", "light"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'n',
        key_str: "n",
        label: "Help: What's new",
        aliases: &["changelog", "news", "release notes"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'r',
        key_str: "r",
        label: "Snippets: Run snippet",
        aliases: &["execute", "command"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'R',
        key_str: "R",
        label: "Snippets: Run on all visible",
        aliases: &["batch", "execute all"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'p',
        key_str: "p",
        label: "Hosts: Ping host",
        aliases: &["health", "check"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'P',
        key_str: "P",
        label: "Hosts: Ping all hosts",
        aliases: &["health all"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: '!',
        key_str: "!",
        label: "Hosts: Show down only",
        aliases: &["filter offline", "down only"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 's',
        key_str: "s",
        label: "Hosts: Sort",
        aliases: &["order", "arrange", "sort by"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'g',
        key_str: "g",
        label: "Hosts: Cycle grouping",
        aliases: &["group", "group by", "category"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'v',
        key_str: "v",
        label: "Hosts: Toggle compact view",
        aliases: &["view mode", "detail panel", "compact", "detailed"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: '#',
        key_str: "#",
        label: "Hosts: Filter by tag",
        aliases: &["tag", "tag filter", "filter tag"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: ' ',
        key_str: " ",
        label: "Hosts: Mark host",
        aliases: &["select", "multi-select", "toggle mark", "checkbox"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'a',
        key_str: "a",
        label: "Hosts: Select all visible",
        aliases: &["select all", "mark all", "bulk select"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::CONTROL,
    },
    // Tunnel-tab actions. Disambiguated by label so they coexist with
    // hosts-tab hotkey letters in the same list. Dispatch switches to
    // Tunnels top-page before synthesising the keypress.
    JumpAction {
        key: 'T',
        key_str: "T",
        label: "Tunnels: Manage tunnels",
        aliases: &["forward", "port forward", "ssh -L", "ssh -R", "open"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'a',
        key_str: "a",
        label: "Tunnels: Add tunnel",
        aliases: &["new tunnel", "create tunnel", "forward"],
        target: JumpActionTarget::Tunnels,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'e',
        key_str: "e",
        label: "Tunnels: Edit tunnel",
        aliases: &["modify tunnel"],
        target: JumpActionTarget::Tunnels,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'd',
        key_str: "d",
        label: "Tunnels: Delete tunnel",
        aliases: &["remove tunnel"],
        target: JumpActionTarget::Tunnels,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 's',
        key_str: "s",
        label: "Tunnels: Sort",
        aliases: &["order tunnels"],
        target: JumpActionTarget::Tunnels,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'R',
        key_str: "R",
        label: "Containers: Refresh all hosts",
        aliases: &["reload containers", "fetch", "rescan"],
        target: JumpActionTarget::Containers,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 's',
        key_str: "s",
        label: "Containers: Cycle sort",
        aliases: &["order containers", "sort by host", "sort by name"],
        target: JumpActionTarget::Containers,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'v',
        key_str: "v",
        label: "Containers: Toggle detail panel",
        aliases: &["show details", "hide details", "compact view"],
        target: JumpActionTarget::Containers,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'e',
        key_str: "e",
        label: "Containers: Exec into container",
        aliases: &["shell", "bash", "sh", "terminal", "attach"],
        target: JumpActionTarget::Containers,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'l',
        key_str: "l",
        label: "Containers: View logs",
        aliases: &["log", "stdout", "stderr", "tail"],
        target: JumpActionTarget::Containers,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'K',
        key_str: "K",
        label: "Containers: Restart container",
        aliases: &["bounce", "reload"],
        target: JumpActionTarget::Containers,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'S',
        key_str: "S",
        label: "Containers: Stop container",
        aliases: &["halt", "kill", "down"],
        target: JumpActionTarget::Containers,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'a',
        key_str: "a",
        label: "Containers: Add host",
        aliases: &["pick host", "scan host", "track host", "new host"],
        target: JumpActionTarget::Containers,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'r',
        key_str: "r",
        label: "Containers: Refresh selected host",
        aliases: &["reload host", "rescan host", "refresh host"],
        target: JumpActionTarget::Containers,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: ' ',
        key_str: " ",
        label: "Containers: Fold or unfold host",
        aliases: &["collapse", "expand", "toggle host", "group"],
        target: JumpActionTarget::Containers,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'k',
        key_str: "k",
        label: "Containers: Restart compose stack",
        aliases: &["stack", "docker compose", "podman compose", "project"],
        target: JumpActionTarget::Containers,
        modifiers: KeyModifiers::CONTROL,
    },
    // Keys tab. Mirror the footer + handler bindings on the Keys tab so
    // typing `:` followed by part of a verb (e.g. `push`, `sign`, `copy`)
    // surfaces the same actions the keyboard shortcuts already trigger.
    JumpAction {
        key: 'c',
        key_str: "c",
        label: "Keys: Copy public key",
        aliases: &["yank", "clipboard", "pubkey"],
        target: JumpActionTarget::Keys,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'p',
        key_str: "p",
        label: "Keys: Push to host",
        aliases: &["install", "ssh-copy-id", "deploy", "upload"],
        target: JumpActionTarget::Keys,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: 'V',
        key_str: "V",
        label: "Keys: Sign Vault SSH certificate",
        aliases: &["vault", "renew cert", "sign"],
        target: JumpActionTarget::Keys,
        modifiers: KeyModifiers::NONE,
    },
    JumpAction {
        key: '?',
        key_str: "?",
        label: "Help: Show keybindings",
        aliases: &["help", "shortcuts", "manual", "keymap"],
        target: JumpActionTarget::Hosts,
        modifiers: KeyModifiers::NONE,
    },
];

/// Cap on hits rendered per section. Broad queries (e.g. one character)
/// match thousands of candidates; capping keeps the jump bar legible without
/// virtualizing the render. The selected hit always falls within the cap
/// because results are sorted by score before truncation.
pub const PALETTE_PER_SECTION_CAP: usize = 32;

/// Field-prefix parser: `user:eric` → (`Some(QueryScope::User)`, "eric").
/// Returns `(None, query)` for queries without a recognised scope.
pub fn parse_query_scope(query: &str) -> (Option<QueryScope>, &str) {
    if let Some((prefix, rest)) = query.split_once(':') {
        let scope = match prefix.trim() {
            "user" => Some(QueryScope::User),
            "host" => Some(QueryScope::Hostname),
            "proxy" => Some(QueryScope::ProxyJump),
            "vault" => Some(QueryScope::VaultSsh),
            "tag" => Some(QueryScope::Tag),
            _ => None,
        };
        if scope.is_some() {
            return (scope, rest.trim_start());
        }
    }
    (None, query)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryScope {
    User,
    Hostname,
    ProxyJump,
    VaultSsh,
    Tag,
}

/// Truncate a string to `max` characters, appending "..." if cut.
fn preview(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s
    } else {
        let mut out: String = chars.iter().take(max.saturating_sub(3)).collect();
        out.push_str("...");
        out
    }
}

/// Restrict scoring to a single field when the user prefixes the query
/// with `user:` / `host:` / `proxy:` / `vault:` / `tag:`. Returns `None`
/// when no scope is set OR when the scope does not apply to the hit
/// (e.g. `vault:` on a snippet). caller falls back to the full set.
fn scoped_haystacks_for(hit: &JumpHit, scope: Option<QueryScope>) -> Option<Vec<&str>> {
    let scope = scope?;
    match (hit, scope) {
        (JumpHit::Host(h), QueryScope::User) if !h.user.is_empty() => Some(vec![&h.user]),
        (JumpHit::Host(h), QueryScope::Hostname) if !h.hostname.is_empty() => {
            Some(vec![&h.hostname])
        }
        (JumpHit::Host(h), QueryScope::ProxyJump) if !h.proxy_jump.is_empty() => {
            Some(vec![&h.proxy_jump])
        }
        (JumpHit::Host(h), QueryScope::VaultSsh) => h.vault_ssh.as_deref().map(|s| vec![s]),
        (JumpHit::Host(h), QueryScope::Tag) => Some(h.tags.iter().map(|t| t.as_str()).collect()),
        // Scoped queries do not match other kinds.
        _ => None,
    }
}

/// Determine which field caused the host hit to match. The renderer uses
/// this to append a `via user`, `via proxy`, `vault: <role>` hint to the
/// row when the matched field is not part of the visible columns. Returns
/// `None` if the alias/hostname (already visible) matched.
pub fn match_source_for_host(host: &HostHit, query: &str) -> Option<MatchSource> {
    if query.is_empty() {
        return None;
    }
    let q = query.to_lowercase();
    let alias_hit = host.alias.to_lowercase().contains(&q);
    let hostname_hit = host.hostname.to_lowercase().contains(&q);
    if alias_hit || hostname_hit {
        return None;
    }
    if !host.user.is_empty() && host.user.to_lowercase().contains(&q) {
        return Some(MatchSource::User);
    }
    if !host.proxy_jump.is_empty() && host.proxy_jump.to_lowercase().contains(&q) {
        return Some(MatchSource::ProxyJump);
    }
    if let Some(role) = &host.vault_ssh
        && role.to_lowercase().contains(&q)
    {
        return Some(MatchSource::VaultSsh);
    }
    if !host.identity_file.is_empty() && host.identity_file.to_lowercase().contains(&q) {
        return Some(MatchSource::IdentityFile);
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchSource {
    User,
    ProxyJump,
    VaultSsh,
    IdentityFile,
}

fn kind_rank(k: SourceKind) -> u8 {
    match k {
        SourceKind::Host => 0,
        SourceKind::Tunnel => 1,
        SourceKind::Container => 2,
        SourceKind::Snippet => 3,
        SourceKind::Action => 4,
    }
}

/// Find `prior` in `hits` and return its index, or `fallback` if the prior
/// hit is gone (e.g. the typed query no longer matches it). Used by
/// `recompute_jump_hits` so mid-typing arrow navigation does not lose
/// the user's place.
fn restore_selection(hits: &[JumpHit], prior: Option<&RecentRef>, fallback: usize) -> usize {
    if let Some(target) = prior
        && let Some(idx) = hits.iter().position(|h| &h.identity() == target)
    {
        return idx;
    }
    fallback.min(hits.len().saturating_sub(1))
}

impl JumpAction {
    #[cfg(test)]
    pub fn all() -> &'static [JumpAction] {
        ALL_JUMP_ACTIONS
    }

    /// The jump bar surfaces the same action set regardless of mode now.
    /// `mode` is preserved on the API so the dispatcher and test helpers
    /// can still pass through, but it no longer narrows the visible list.
    pub fn for_mode(_mode: JumpMode) -> &'static [JumpAction] {
        ALL_JUMP_ACTIONS
    }
}

#[cfg(test)]
mod tests;
