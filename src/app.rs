use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use ratatui::widgets::ListState;

use crate::history::ConnectionHistory;
use crate::providers::config::ProviderConfig;
use crate::ssh_config::model::{HostEntry, PatternEntry, SshConfigFile};
use crate::ssh_keys::SshKeyInfo;
use crate::tunnel::TunnelRule;

/// Case-insensitive substring check without allocation.
/// Uses a byte-window approach for ASCII strings (the common case for SSH
/// hostnames and aliases). Falls back to a char-based scan when either
/// string contains non-ASCII bytes to avoid false matches across UTF-8
/// character boundaries.
pub(super) fn contains_ci(haystack: &str, needle: &str) -> bool {
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
mod display_list;
mod forms;
mod groups;
mod hosts;
mod ping;
mod search;
mod selection;
mod types;
mod update;
mod vault;

pub(crate) use forms::char_to_byte_pos;
pub use forms::{
    FormField, HostForm, ProviderFormField, ProviderFormFields, SnippetForm, SnippetFormField,
    SnippetHostOutput, SnippetOutputState, SnippetParamFormState, TunnelForm, TunnelFormField,
};
pub use ping::PingState;
pub use types::{
    ConflictState, ContainerState, DeletedHost, FormBaseline, GroupBy, HostListItem, MessageClass,
    PingStatus, ProviderFormBaseline, ReloadState, Screen, SearchState, SnippetFormBaseline,
    SortMode, StatusMessage, SyncRecord, TagState, TunnelFormBaseline, UiSelection, ViewMode,
    classify_ping, health_summary_spans, health_summary_spans_for, ping_sort_key,
    propagate_ping_to_dependents, select_display_tags, status_glyph,
};
pub use update::UpdateState;
pub use vault::VaultState;

/// Kill active tunnel processes when App is dropped (e.g. on panic).
impl Drop for App {
    fn drop(&mut self) {
        for (alias, mut tunnel) in self.active_tunnels.drain() {
            if let Err(e) = tunnel.child.kill() {
                log::debug!("[external] Failed to kill tunnel for {alias} on shutdown: {e}");
            }
            let _ = tunnel.child.wait();
        }
        // Cancel and join any in-flight Vault SSH bulk-sign worker so it
        // cannot keep writing to ~/.purple/certs/ after teardown (panic
        // unwind, normal exit, etc.).
        if let Some(ref cancel) = self.vault.signing_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(handle) = self.vault.sign_thread.take() {
            let _ = handle.join();
        }
    }
}

/// Main application state.
pub struct App {
    // Core
    pub screen: Screen,
    pub running: bool,
    pub config: SshConfigFile,
    pub hosts: Vec<HostEntry>,
    pub patterns: Vec<PatternEntry>,
    pub display_list: Vec<HostListItem>,
    pub form: HostForm,
    pub status: Option<StatusMessage>,
    pub toast: Option<StatusMessage>,
    pub toast_queue: VecDeque<StatusMessage>,
    pub pending_connect: Option<(String, Option<String>)>,

    // Sub-structs
    pub ui: UiSelection,
    pub search: SearchState,
    pub reload: ReloadState,
    pub conflict: ConflictState,

    // Keys
    pub keys: Vec<SshKeyInfo>,

    // Tags
    pub tags: TagState,

    // History + preferences
    pub history: ConnectionHistory,
    pub sort_mode: SortMode,
    pub group_by: GroupBy,
    pub view_mode: ViewMode,

    /// Signal for animation layer: detail panel toggle requested.
    /// Set by handler, consumed by AnimationState.detect_transitions().
    pub detail_toggle_pending: bool,

    // Undo (multi-level, capped at 50)
    pub undo_stack: Vec<DeletedHost>,

    // Providers
    pub provider_config: ProviderConfig,
    pub provider_form: ProviderFormFields,
    pub syncing_providers: HashMap<String, Arc<AtomicBool>>,
    /// Names of providers that completed during this sync batch.
    pub sync_done: Vec<String>,
    /// Whether any provider in the current batch had errors.
    pub sync_had_errors: bool,
    pub pending_provider_delete: Option<String>,
    pub pending_snippet_delete: Option<usize>,
    pub pending_tunnel_delete: Option<usize>,

    // Ping / health-check
    pub ping: PingState,

    // Vault SSH certificate and signing state
    pub vault: VaultState,

    // Tunnels
    pub tunnel_list: Vec<TunnelRule>,
    pub tunnel_form: TunnelForm,
    pub active_tunnels: HashMap<String, crate::tunnel::ActiveTunnel>,

    // Snippets
    pub snippet_store: crate::snippet::SnippetStore,
    pub snippet_form: SnippetForm,
    pub pending_snippet: Option<(crate::snippet::Snippet, Vec<String>)>,
    /// Host indices selected for multi-host snippet execution (space to toggle).
    pub multi_select: HashSet<usize>,
    /// Currently active group filter (tab navigation). None = show all groups.
    pub group_filter: Option<String>,
    /// Index into group_tab_order for tab navigation.
    pub group_tab_index: usize,
    /// Ordered list of group names from the current display list.
    pub group_tab_order: Vec<String>,
    /// Host/pattern counts per group (computed before group filtering).
    pub group_host_counts: HashMap<String, usize>,
    pub snippet_output: Option<SnippetOutputState>,
    pub snippet_param_form: Option<SnippetParamFormState>,
    /// When true, the snippet param form submits to terminal-exit mode (! key).
    pub pending_snippet_terminal: bool,

    // Update
    pub update: UpdateState,

    // Cached tunnel summaries (invalidated on config reload)
    pub tunnel_summaries_cache: HashMap<String, String>,

    // Sync history
    pub sync_history: HashMap<String, SyncRecord>,

    // Bitwarden session
    pub bw_session: Option<String>,

    // File browser
    pub file_browser: Option<crate::file_browser::FileBrowserState>,
    pub file_browser_paths: HashMap<String, (PathBuf, String)>,

    // Containers
    pub container_state: Option<ContainerState>,
    pub container_cache: HashMap<String, crate::containers::ContainerCacheEntry>,

    // First-run hints
    pub known_hosts_count: usize,
    pub welcome_opened: Option<std::time::Instant>,

    /// Demo mode: all mutations are in-memory only, no disk writes.
    pub demo_mode: bool,

    // Form dirty-check baselines
    pub form_baseline: Option<FormBaseline>,
    pub tunnel_form_baseline: Option<TunnelFormBaseline>,
    pub snippet_form_baseline: Option<SnippetFormBaseline>,
    pub provider_form_baseline: Option<ProviderFormBaseline>,
    /// When true, the Esc key shows a "Discard changes?" dialog instead of closing.
    pub pending_discard_confirm: bool,

    /// Deferred config write from VaultSignAllDone (guarded while forms are open).
    pub pending_vault_config_write: bool,

    /// Command palette state. Some when palette is open.
    pub palette: Option<CommandPaletteState>,
}

impl App {
    pub fn new(config: SshConfigFile) -> Self {
        let hosts = config.host_entries();
        let patterns = config.pattern_entries();
        let display_list = Self::build_display_list_from(&config, &hosts, &patterns);
        let mut list_state = ListState::default();
        // Select first selectable item
        if let Some(pos) = display_list.iter().position(|item| {
            matches!(
                item,
                HostListItem::Host { .. } | HostListItem::Pattern { .. }
            )
        }) {
            list_state.select(Some(pos));
        }

        let config_path = config.path.clone();
        let last_modified = Self::get_mtime(&config_path);
        let include_mtimes = Self::snapshot_include_mtimes(&config);
        let include_dir_mtimes = Self::snapshot_include_dir_mtimes(&config);

        Self {
            screen: Screen::HostList,
            running: true,
            config,
            hosts,
            patterns,
            display_list,
            form: HostForm::new(),
            status: None,
            toast: None,
            toast_queue: VecDeque::new(),
            pending_connect: None,
            ui: UiSelection {
                list_state,
                key_list_state: ListState::default(),
                show_key_picker: false,
                key_picker_state: ListState::default(),
                show_password_picker: false,
                password_picker_state: ListState::default(),
                show_proxyjump_picker: false,
                proxyjump_picker_state: ListState::default(),
                tag_picker_state: ListState::default(),
                theme_picker_state: ListState::default(),
                theme_picker_builtins: Vec::new(),
                theme_picker_custom: Vec::new(),
                theme_picker_saved_name: String::new(),
                theme_picker_original: None,
                provider_list_state: ListState::default(),
                tunnel_list_state: ListState::default(),
                snippet_picker_state: ListState::default(),
                snippet_search: None,
                show_region_picker: false,
                region_picker_cursor: 0,
                help_scroll: 0,
                detail_scroll: 0,
            },
            search: SearchState {
                query: None,
                filtered_indices: Vec::new(),
                filtered_pattern_indices: Vec::new(),
                pre_search_selection: None,
                scope_indices: None,
            },
            reload: ReloadState {
                config_path,
                last_modified,
                include_mtimes,
                include_dir_mtimes,
            },
            conflict: ConflictState {
                form_mtime: None,
                form_include_mtimes: Vec::new(),
                form_include_dir_mtimes: Vec::new(),
                provider_form_mtime: None,
            },
            keys: Vec::new(),
            tags: TagState::default(),
            history: ConnectionHistory::load(),
            sort_mode: SortMode::Original,
            group_by: GroupBy::None,
            view_mode: ViewMode::Compact,
            detail_toggle_pending: false,
            undo_stack: Vec::new(),
            provider_config: ProviderConfig::load(),
            provider_form: ProviderFormFields::new(),
            syncing_providers: HashMap::new(),
            sync_done: Vec::new(),
            sync_had_errors: false,
            pending_provider_delete: None,
            pending_snippet_delete: None,
            pending_tunnel_delete: None,
            ping: PingState {
                slow_threshold_ms: crate::preferences::load_slow_threshold(),
                auto_ping: crate::preferences::load_auto_ping(),
                ..PingState::default()
            },
            vault: VaultState::default(),
            tunnel_list: Vec::new(),
            tunnel_form: TunnelForm::new(),
            active_tunnels: HashMap::new(),
            snippet_store: crate::snippet::SnippetStore::load(),
            snippet_form: SnippetForm::new(),
            pending_snippet: None,
            multi_select: HashSet::new(),
            group_filter: None,
            group_tab_index: 0,
            group_tab_order: Vec::new(),
            group_host_counts: HashMap::new(),
            snippet_output: None,
            snippet_param_form: None,
            pending_snippet_terminal: false,
            tunnel_summaries_cache: HashMap::new(),
            update: UpdateState {
                hint: crate::update::update_hint(),
                ..UpdateState::default()
            },
            sync_history: SyncRecord::load_all(),
            bw_session: None,
            file_browser: None,
            file_browser_paths: HashMap::new(),
            container_state: None,
            container_cache: crate::containers::load_container_cache(),
            known_hosts_count: 0,
            welcome_opened: None,
            demo_mode: false,
            form_baseline: None,
            tunnel_form_baseline: None,
            snippet_form_baseline: None,
            provider_form_baseline: None,
            pending_discard_confirm: false,
            pending_vault_config_write: false,
            palette: None,
        }
    }

    /// Reload hosts from config.
    pub fn reload_hosts(&mut self) {
        // Synchronously flush any deferred vault config write before reloading,
        // so on-disk state matches in-memory state (no TOCTOU with auto-reload).
        // Skip when a form is open (flush handler would bail anyway) and do not
        // call flush_pending_vault_write() itself to avoid recursion.
        if self.pending_vault_config_write && !self.is_form_open() {
            if let Err(e) = self.config.write() {
                self.set_status(
                    format!("Failed to update config after vault signing: {}", e),
                    true,
                );
            }
        }
        // Always clear the flag: either we flushed, or the form-submit path
        // has already written the full config.
        self.pending_vault_config_write = false;
        let had_search = self.search.query.take();
        let selected_alias = self
            .selected_host()
            .map(|h| h.alias.clone())
            .or_else(|| self.selected_pattern().map(|p| p.pattern.clone()));

        self.tunnel_summaries_cache.clear();
        self.hosts = self.config.host_entries();
        self.patterns = self.config.pattern_entries();
        // Prune cert status cache and in-flight set: retain only entries whose
        // host alias still exists after the reload.
        let valid_for_certs: std::collections::HashSet<&str> =
            self.hosts.iter().map(|h| h.alias.as_str()).collect();
        self.vault
            .cert_cache
            .retain(|alias, _| valid_for_certs.contains(alias.as_str()));
        self.vault
            .cert_checks_in_flight
            .retain(|alias| valid_for_certs.contains(alias.as_str()));
        if self.sort_mode == SortMode::Original && matches!(self.group_by, GroupBy::None) {
            self.display_list =
                Self::build_display_list_from(&self.config, &self.hosts, &self.patterns);
        } else {
            self.apply_sort();
        }

        // Close tag pickers if open — tags.list is stale after reload
        if matches!(self.screen, Screen::TagPicker) {
            self.screen = Screen::HostList;
        }

        // Multi-select stores indices into hosts; clear to avoid stale refs
        self.multi_select.clear();

        // Prune ping status for hosts that no longer exist
        let valid_aliases: std::collections::HashSet<&str> =
            self.hosts.iter().map(|h| h.alias.as_str()).collect();
        self.ping
            .status
            .retain(|alias, _| valid_aliases.contains(alias.as_str()));

        // Restore search if it was active, otherwise reset
        if let Some(query) = had_search {
            self.search.query = Some(query);
            self.apply_filter();
        } else {
            self.search.query = None;
            self.search.filtered_indices.clear();
            self.search.filtered_pattern_indices.clear();
            // Fix selection for display list mode
            if self.hosts.is_empty() && self.patterns.is_empty() {
                self.ui.list_state.select(None);
            } else if let Some(pos) = self.display_list.iter().position(|item| {
                matches!(
                    item,
                    HostListItem::Host { .. } | HostListItem::Pattern { .. }
                )
            }) {
                let current = self.ui.list_state.selected().unwrap_or(0);
                if current >= self.display_list.len()
                    || !matches!(
                        self.display_list.get(current),
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
        let Some(host) = self.hosts.iter().find(|h| h.alias == alias) else {
            self.vault.cert_cache.remove(alias);
            return;
        };
        let role_some = crate::vault_ssh::resolve_vault_role(
            host.vault_ssh.as_deref(),
            host.provider.as_deref(),
            &self.provider_config,
        )
        .is_some();
        if !role_some {
            self.vault.cert_cache.remove(alias);
            return;
        }
        let cert_path = match crate::vault_ssh::resolve_cert_path(alias, &host.certificate_file) {
            Ok(p) => p,
            Err(_) => {
                self.vault.cert_cache.remove(alias);
                return;
            }
        };
        let status = crate::vault_ssh::check_cert_validity(&cert_path);
        let mtime = std::fs::metadata(&cert_path)
            .ok()
            .and_then(|m| m.modified().ok());
        self.vault.cert_cache.insert(
            alias.to_string(),
            (std::time::Instant::now(), status, mtime),
        );
    }

    // --- Search methods ---

    /// Provider names sorted by last sync (most recent first), then configured, then unconfigured.
    /// Includes any unknown provider names found in the config file (e.g. typos or future providers).
    pub fn sorted_provider_names(&self) -> Vec<String> {
        use crate::providers;
        let mut names: Vec<String> = providers::PROVIDER_NAMES
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Append configured providers not in the known list so they are visible and removable
        for section in &self.provider_config.sections {
            if !names.contains(&section.provider) {
                names.push(section.provider.clone());
            }
        }
        names.sort_by(|a, b| {
            let conf_a = self.provider_config.section(a.as_str()).is_some();
            let conf_b = self.provider_config.section(b.as_str()).is_some();
            let ts_a = self.sync_history.get(a.as_str()).map_or(0, |r| r.timestamp);
            let ts_b = self.sync_history.get(b.as_str()).map_or(0, |r| r.timestamp);
            // Configured first (by most recent sync), then unconfigured alphabetically
            conf_b.cmp(&conf_a).then(ts_b.cmp(&ts_a)).then(a.cmp(b))
        });
        names
    }

    /// Check whether a form screen is currently open (host or provider forms).
    pub fn is_form_open(&self) -> bool {
        matches!(
            self.screen,
            Screen::AddHost | Screen::EditHost { .. } | Screen::ProviderForm { .. }
        )
    }

    /// Flush a deferred vault config write if one is pending and no form is open.
    /// Returns true if a write was performed.
    pub fn flush_pending_vault_write(&mut self) -> bool {
        if !self.pending_vault_config_write || self.is_form_open() {
            return false;
        }
        // reload_hosts() performs the write and clears the flag.
        self.reload_hosts();
        true
    }

    pub fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        let class = if is_error {
            MessageClass::Alert
        } else {
            MessageClass::Confirmation
        };
        let msg = StatusMessage {
            text: text.into(),
            class,
            tick_count: 0,
            sticky: false,
        };
        if msg.is_toast() {
            self.push_toast(msg);
        } else {
            log::debug!("footer <- {:?}: {}", msg.class, msg.text);
            self.status = Some(msg);
        }
    }

    /// Push a toast message. Confirmation toasts replace the current toast
    /// immediately (last-write-wins). Alert toasts are queued (max 5) so
    /// errors are never lost.
    fn push_toast(&mut self, msg: StatusMessage) {
        log::debug!("toast <- {:?}: {}", msg.class, msg.text);
        if msg.class == MessageClass::Confirmation {
            // Confirmation replaces any active toast and clears the queue.
            self.toast = Some(msg);
            self.toast_queue.clear();
        } else if self.toast.is_some() {
            if self.toast_queue.len() >= 5 {
                if let Some(dropped) = self.toast_queue.front() {
                    log::debug!("toast queue full, dropping: {}", dropped.text);
                }
                self.toast_queue.pop_front();
            }
            self.toast_queue.push_back(msg);
        } else {
            self.toast = Some(msg);
        }
    }

    /// Set an Info-class status message that displays in the footer only.
    pub fn set_info_status(&mut self, text: impl Into<String>) {
        let text = text.into();
        log::debug!("footer <- Info: {}", text);
        self.status = Some(StatusMessage {
            text,
            class: MessageClass::Info,
            tick_count: 0,
            sticky: false,
        });
    }

    /// Like `set_status` but skips the write when a sticky message is active.
    /// Use for background/timer events (ping expiry, sync ticks) that must
    /// not clobber an in-progress or critical sticky message.
    /// Routes to Info (footer) by default, Alert to toast if is_error.
    pub fn set_background_status(&mut self, text: impl Into<String>, is_error: bool) {
        if is_error {
            let msg = StatusMessage {
                text: text.into(),
                class: MessageClass::Alert,
                tick_count: 0,
                sticky: false,
            };
            self.push_toast(msg);
            return;
        }
        if self.status.as_ref().is_some_and(|s| s.sticky) {
            log::debug!("background status suppressed (sticky active)");
            return;
        }
        self.set_info_status(text);
    }

    /// Sticky messages always go to the footer (`self.status`), even when the
    /// class is Alert. The `sticky` flag overrides the normal toast routing
    /// because sticky messages (Vault SSH signing summaries, progress spinners)
    /// must remain visible in the footer until explicitly replaced.
    pub fn set_sticky_status(&mut self, text: impl Into<String>, is_error: bool) {
        let text = text.into();
        let class = if is_error {
            MessageClass::Alert
        } else {
            MessageClass::Progress
        };
        log::debug!("footer <- sticky {:?}: {}", class, text);
        self.status = Some(StatusMessage {
            text,
            class,
            tick_count: 0,
            sticky: true,
        });
    }

    /// Tick the footer status message timer. Info shows for 3s.
    /// Sticky/Progress messages never expire automatically.
    pub fn tick_status(&mut self) {
        // Don't expire status while providers are still syncing
        if !self.syncing_providers.is_empty() {
            return;
        }
        if let Some(ref mut status) = self.status {
            if status.sticky {
                return;
            }
            status.tick_count += 1;
            if status.tick_count > status.timeout() {
                log::debug!("footer status expired: {}", status.text);
                self.status = None;
            }
        }
    }

    /// Tick the toast message timer. Expires current toast and pops queue.
    pub fn tick_toast(&mut self) {
        if let Some(ref mut toast) = self.toast {
            if toast.sticky {
                return;
            }
            toast.tick_count += 1;
            if toast.tick_count > toast.timeout() {
                log::debug!("toast expired: {}", toast.text);
                self.toast = self.toast_queue.pop_front();
            }
        }
    }

    /// Get the modification time of a file.
    fn get_mtime(path: &Path) -> Option<SystemTime> {
        std::fs::metadata(path).ok()?.modified().ok()
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
                | Screen::SnippetPicker { .. }
                | Screen::SnippetForm { .. }
                | Screen::SnippetOutput { .. }
                | Screen::SnippetParamForm { .. }
                | Screen::FileBrowser { .. }
                | Screen::Containers { .. }
                | Screen::ConfirmDelete { .. }
                | Screen::ConfirmHostKeyReset { .. }
                | Screen::ConfirmPurgeStale { .. }
                | Screen::ConfirmImport { .. }
                | Screen::ConfirmVaultSign { .. }
                | Screen::TagPicker
                | Screen::ThemePicker
        ) || self.tags.input.is_some()
        {
            return;
        }
        let current_mtime = Self::get_mtime(&self.reload.config_path);
        let changed = current_mtime != self.reload.last_modified
            || self
                .reload
                .include_mtimes
                .iter()
                .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
            || self
                .reload
                .include_dir_mtimes
                .iter()
                .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime);
        if changed {
            if let Ok(new_config) = SshConfigFile::parse(&self.reload.config_path) {
                self.config = new_config;
                // Invalidate undo state — config structure may have changed externally
                self.undo_stack.clear();
                // Clear stale ping status — hosts may have changed
                self.ping.status.clear();
                self.ping.filter_down_only = false;
                self.ping.checked_at = None;
                self.reload_hosts();
                self.reload.last_modified = current_mtime;
                self.reload.include_mtimes = Self::snapshot_include_mtimes(&self.config);
                self.reload.include_dir_mtimes = Self::snapshot_include_dir_mtimes(&self.config);
                let count = self.hosts.len();
                self.set_background_status(format!("Config reloaded. {} hosts.", count), false);
            }
        }
    }

    /// Non-mutating check: has the on-disk config (or any tracked Include)
    /// been modified since `self.reload.last_modified` was captured? Used by
    /// async write paths (e.g. the Vault SSH bulk-sign completion handler)
    /// to refuse writing when an external editor changed the file underneath
    /// us — overwriting those edits would silently discard user work. The
    /// backup-on-write mechanism in `SshConfigFile::write()` would still
    /// recover them, but detecting the conflict BEFORE writing is strictly
    /// better than after.
    pub fn external_config_changed(&self) -> bool {
        let current_mtime = Self::get_mtime(&self.reload.config_path);
        current_mtime != self.reload.last_modified
            || self
                .reload
                .include_mtimes
                .iter()
                .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
            || self
                .reload
                .include_dir_mtimes
                .iter()
                .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
    }

    /// Update the last_modified timestamp (call after writing config).
    pub fn update_last_modified(&mut self) {
        self.reload.last_modified = Self::get_mtime(&self.reload.config_path);
        self.reload.include_mtimes = Self::snapshot_include_mtimes(&self.config);
        self.reload.include_dir_mtimes = Self::snapshot_include_dir_mtimes(&self.config);
    }

    /// Returns true if any host or provider has a vault role configured.
    pub fn has_any_vault_role(&self) -> bool {
        for host in &self.hosts {
            if host.vault_ssh.is_some() {
                return true;
            }
        }
        for section in &self.provider_config.sections {
            if !section.vault_role.is_empty() {
                return true;
            }
        }
        false
    }

    /// Poll active tunnels for exit. Returns (alias, message, is_error) tuples.
    pub fn poll_tunnels(&mut self) -> Vec<(String, String, bool)> {
        if self.active_tunnels.is_empty() {
            return Vec::new();
        }
        let mut exited = Vec::new();
        let mut to_remove = Vec::new();
        for (alias, tunnel) in &mut self.active_tunnels {
            match tunnel.child.try_wait() {
                Ok(Some(status)) => {
                    let stderr_msg = tunnel.child.stderr.take().and_then(|mut stderr| {
                        use std::io::Read;
                        let mut buf = vec![0u8; 1024];
                        match stderr.read(&mut buf) {
                            Ok(n) if n > 0 => {
                                let s = String::from_utf8_lossy(&buf[..n]);
                                let trimmed = s.trim();
                                if trimmed.is_empty() {
                                    None
                                } else {
                                    Some(trimmed.to_string())
                                }
                            }
                            _ => None,
                        }
                    });
                    let exit_code = status.code().unwrap_or(-1);
                    if !status.success() {
                        log::error!(
                            "[external] Tunnel exited unexpectedly: alias={alias} exit={exit_code}"
                        );
                        if let Some(ref err) = stderr_msg {
                            log::debug!("[external] Tunnel stderr: {}", err.trim());
                        }
                    }
                    let (msg, is_error) = if status.success() {
                        (format!("Tunnel for {} closed.", alias), false)
                    } else if let Some(err) = stderr_msg {
                        (format!("Tunnel for {}: {}", alias, err), true)
                    } else {
                        (
                            format!("Tunnel for {} exited with code {}.", alias, exit_code),
                            true,
                        )
                    };
                    exited.push((alias.clone(), msg, is_error));
                    to_remove.push(alias.clone());
                }
                Ok(None) => {}
                Err(e) => {
                    exited.push((
                        alias.clone(),
                        format!("Tunnel for {} lost: {}", alias, e),
                        true,
                    ));
                    to_remove.push(alias.clone());
                }
            }
        }
        for alias in to_remove {
            self.active_tunnels.remove(&alias);
        }
        exited
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

/// A command that can be executed from the command palette.
#[derive(Debug, Clone, Copy)]
pub struct PaletteCommand {
    pub key: char,
    pub label: &'static str,
    /// Section for future grouped display. Not yet used by the renderer.
    #[allow(dead_code)]
    pub section: &'static str,
}

static ALL_PALETTE_COMMANDS: &[PaletteCommand] = &[
    PaletteCommand {
        key: 'a',
        label: "add host",
        section: "manage",
    },
    PaletteCommand {
        key: 'A',
        label: "add pattern",
        section: "manage",
    },
    PaletteCommand {
        key: 'e',
        label: "edit",
        section: "manage",
    },
    PaletteCommand {
        key: 'd',
        label: "del",
        section: "manage",
    },
    PaletteCommand {
        key: 'c',
        label: "clone",
        section: "manage",
    },
    PaletteCommand {
        key: 'u',
        label: "undo del",
        section: "manage",
    },
    PaletteCommand {
        key: 't',
        label: "tag (inline)",
        section: "manage",
    },
    PaletteCommand {
        key: 'i',
        label: "all directives",
        section: "manage",
    },
    PaletteCommand {
        key: 'y',
        label: "copy ssh command",
        section: "clipboard",
    },
    PaletteCommand {
        key: 'x',
        label: "copy config block",
        section: "clipboard",
    },
    PaletteCommand {
        key: 'X',
        label: "purge stale",
        section: "clipboard",
    },
    PaletteCommand {
        key: 'F',
        label: "file explorer",
        section: "tools",
    },
    PaletteCommand {
        key: 'T',
        label: "tunnels",
        section: "tools",
    },
    PaletteCommand {
        key: 'C',
        label: "containers",
        section: "tools",
    },
    PaletteCommand {
        key: 'K',
        label: "SSH keys",
        section: "tools",
    },
    PaletteCommand {
        key: 'S',
        label: "providers",
        section: "tools",
    },
    PaletteCommand {
        key: 'V',
        label: "vault sign",
        section: "tools",
    },
    PaletteCommand {
        key: 'I',
        label: "import known_hosts",
        section: "tools",
    },
    PaletteCommand {
        key: 'm',
        label: "theme",
        section: "tools",
    },
    PaletteCommand {
        key: 'r',
        label: "run snippet",
        section: "connect",
    },
    PaletteCommand {
        key: 'R',
        label: "run on all visible",
        section: "connect",
    },
    PaletteCommand {
        key: 'p',
        label: "ping",
        section: "connect",
    },
    PaletteCommand {
        key: 'P',
        label: "ping all",
        section: "connect",
    },
    PaletteCommand {
        key: '!',
        label: "down-only filter",
        section: "connect",
    },
];

impl PaletteCommand {
    pub fn all() -> &'static [PaletteCommand] {
        ALL_PALETTE_COMMANDS
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommandPaletteState {
    pub query: String,
    pub selected: usize,
}

impl CommandPaletteState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_query(&mut self, c: char) {
        if self.query.len() < 64 {
            self.query.push(c);
        }
        self.selected = 0;
    }

    pub fn pop_query(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    /// Return commands filtered by the current query (substring match on label).
    /// Returns a borrowed static slice when the query is empty (no allocation).
    pub fn filtered_commands(&self) -> std::borrow::Cow<'static, [PaletteCommand]> {
        let all = PaletteCommand::all();
        if self.query.is_empty() {
            return std::borrow::Cow::Borrowed(all);
        }
        let q = self.query.to_lowercase();
        std::borrow::Cow::Owned(
            all.iter()
                .filter(|cmd| cmd.label.to_lowercase().contains(&q))
                .copied()
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests;
