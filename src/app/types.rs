//! Non-App types used across the app module tree: Screen enum, sync record,
//! ui state wrappers, and form baselines. These are separated from app.rs so
//! the main file can focus on the `App` struct itself and its `impl` block.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use ratatui::text::Span;
use ratatui::widgets::ListState;

use crate::ssh_config::model::{ConfigElement, HostEntry};
use crate::ui::theme;

/// Record of the last sync result for a provider.
#[derive(Debug, Clone)]
pub struct SyncRecord {
    pub timestamp: u64,
    pub message: String,
    pub is_error: bool,
}

impl SyncRecord {
    /// Load sync history from ~/.purple/sync_history.tsv.
    /// Format: provider\ttimestamp\tis_error\tmessage
    pub fn load_all() -> HashMap<String, SyncRecord> {
        let mut map = HashMap::new();
        let Some(home) = dirs::home_dir() else {
            return map;
        };
        let path = home.join(".purple").join("sync_history.tsv");
        let Ok(content) = std::fs::read_to_string(&path) else {
            return map;
        };
        for line in content.lines() {
            let parts: Vec<&str> = line.splitn(4, '\t').collect();
            if parts.len() < 4 {
                continue;
            }
            let Some(ts) = parts[1].parse::<u64>().ok() else {
                continue;
            };
            let is_error = parts[2] == "1";
            map.insert(
                parts[0].to_string(),
                SyncRecord {
                    timestamp: ts,
                    message: parts[3].to_string(),
                    is_error,
                },
            );
        }
        map
    }

    /// Save sync history to ~/.purple/sync_history.tsv.
    pub fn save_all(history: &HashMap<String, SyncRecord>) {
        if crate::demo_flag::is_demo() {
            return;
        }
        let Some(home) = dirs::home_dir() else { return };
        let dir = home.join(".purple");
        let path = dir.join("sync_history.tsv");
        let mut lines = Vec::new();
        for (provider, record) in history {
            lines.push(format!(
                "{}\t{}\t{}\t{}",
                provider,
                record.timestamp,
                if record.is_error { "1" } else { "0" },
                record.message
            ));
        }
        let _ = crate::fs_util::atomic_write(&path, lines.join("\n").as_bytes());
    }

    /// Parse sync history from TSV content string (for demo/test use).
    pub fn load_from_content(content: &str) -> HashMap<String, SyncRecord> {
        let mut map = HashMap::new();
        for line in content.lines() {
            let parts: Vec<&str> = line.splitn(4, '\t').collect();
            if parts.len() < 4 {
                continue;
            }
            let Some(ts) = parts[1].parse::<u64>().ok() else {
                continue;
            };
            let is_error = parts[2] == "1";
            map.insert(
                parts[0].to_string(),
                SyncRecord {
                    timestamp: ts,
                    message: parts[3].to_string(),
                    is_error,
                },
            );
        }
        map
    }
}

/// Which screen is currently displayed.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    HostList,
    AddHost,
    EditHost {
        alias: String,
    },
    ConfirmDelete {
        alias: String,
    },
    Help {
        return_screen: Box<Screen>,
    },
    KeyList,
    KeyDetail {
        index: usize,
    },
    HostDetail {
        index: usize,
    },
    TagPicker,
    ThemePicker,
    Providers,
    ProviderForm {
        provider: String,
    },
    TunnelList {
        alias: String,
    },
    TunnelForm {
        alias: String,
        editing: Option<usize>,
    },
    SnippetPicker {
        target_aliases: Vec<String>,
    },
    SnippetForm {
        target_aliases: Vec<String>,
        editing: Option<usize>,
    },
    SnippetOutput {
        snippet_name: String,
        target_aliases: Vec<String>,
    },
    SnippetParamForm {
        snippet: crate::snippet::Snippet,
        target_aliases: Vec<String>,
    },
    ConfirmHostKeyReset {
        alias: String,
        hostname: String,
        known_hosts_path: String,
        askpass: Option<String>,
    },
    FileBrowser {
        alias: String,
    },
    Containers {
        alias: String,
    },
    ConfirmImport {
        count: usize,
    },
    ConfirmPurgeStale {
        aliases: Vec<String>,
        provider: Option<String>,
    },
    ConfirmVaultSign {
        /// Precomputed list of (alias, role, certificate_file, pubkey_path) for
        /// hosts that resolve to a vault SSH role. Computed when the user
        /// presses `V`. `certificate_file` is the host's existing
        /// `CertificateFile` directive (empty when unset) and is needed so the
        /// background worker checks renewal status against the actually
        /// configured cert path rather than purple's default.
        signable: Vec<(String, String, String, std::path::PathBuf, Option<String>)>,
    },
    Welcome {
        has_backup: bool,
        host_count: usize,
        known_hosts_count: usize,
    },
}

/// Classification of status messages for routing to toast overlay vs footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageClass {
    /// User action succeeded (copy, sort, delete). Toast, 6 ticks.
    Confirmation,
    /// Background event (sync complete, config reload). Footer, 12 ticks.
    Info,
    /// Error or warning requiring attention. Toast, 20 ticks.
    Alert,
    /// Long-running operation with spinner. Footer, sticky.
    Progress,
}

/// Status message displayed as toast overlay or in the footer.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub class: MessageClass,
    pub tick_count: u32,
    /// When true the message never auto-expires and `set_background_status`
    /// will not overwrite it. Cleared by `set_status` or `set_sticky_status`.
    pub sticky: bool,
}

impl StatusMessage {
    /// Backward compat: is this an error-class message?
    pub fn is_error(&self) -> bool {
        matches!(self.class, MessageClass::Alert)
    }

    /// Timeout in ticks for this message class.
    pub fn timeout(&self) -> u32 {
        match self.class {
            MessageClass::Confirmation => 6,
            MessageClass::Info => 12,
            MessageClass::Alert => 20,
            MessageClass::Progress => u32::MAX,
        }
    }

    /// Should this message render as a toast overlay?
    pub fn is_toast(&self) -> bool {
        matches!(self.class, MessageClass::Confirmation | MessageClass::Alert)
    }
}

/// An item in the display list (hosts + group headers).
#[derive(Debug, Clone)]
pub enum HostListItem {
    GroupHeader(String),
    Host { index: usize },
    Pattern { index: usize },
}

/// Ping status for a host.
#[derive(Debug, Clone, PartialEq)]
pub enum PingStatus {
    Checking,
    Reachable { rtt_ms: u32 },
    Slow { rtt_ms: u32 },
    Unreachable,
    Skipped,
}

/// View mode for the host list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewMode {
    Compact,
    Detailed,
}

/// Sort mode for the host list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortMode {
    Original,
    AlphaAlias,
    AlphaHostname,
    Frecency,
    MostRecent,
    Status,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            SortMode::Original => SortMode::AlphaAlias,
            SortMode::AlphaAlias => SortMode::AlphaHostname,
            SortMode::AlphaHostname => SortMode::Frecency,
            SortMode::Frecency => SortMode::MostRecent,
            SortMode::MostRecent => SortMode::Status,
            SortMode::Status => SortMode::Original,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::Original => "config order",
            SortMode::AlphaAlias => "A-Z alias",
            SortMode::AlphaHostname => "A-Z hostname",
            SortMode::Frecency => "most used",
            SortMode::MostRecent => "most recent",
            SortMode::Status => "down first",
        }
    }

    pub fn to_key(self) -> &'static str {
        match self {
            SortMode::Original => "original",
            SortMode::AlphaAlias => "alpha_alias",
            SortMode::AlphaHostname => "alpha_hostname",
            SortMode::Frecency => "frecency",
            SortMode::MostRecent => "most_recent",
            SortMode::Status => "status",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "original" => SortMode::Original,
            "alpha_alias" => SortMode::AlphaAlias,
            "alpha_hostname" => SortMode::AlphaHostname,
            "frecency" => SortMode::Frecency,
            "most_recent" => SortMode::MostRecent,
            "status" => SortMode::Status,
            _ => SortMode::MostRecent,
        }
    }
}

/// Classify a ping result into a PingStatus based on RTT and threshold.
pub fn classify_ping(rtt_ms: Option<u32>, slow_threshold_ms: u16) -> PingStatus {
    match rtt_ms {
        Some(ms) if ms >= slow_threshold_ms as u32 => PingStatus::Slow { rtt_ms: ms },
        Some(ms) => PingStatus::Reachable { rtt_ms: ms },
        None => PingStatus::Unreachable,
    }
}

/// Propagate a ping result to all hosts that use the given alias as ProxyJump bastion.
pub fn propagate_ping_to_dependents(
    hosts: &[HostEntry],
    ping_status: &mut HashMap<String, PingStatus>,
    bastion_alias: &str,
    status: &PingStatus,
) {
    for h in hosts {
        if h.proxy_jump == bastion_alias {
            ping_status.insert(h.alias.clone(), status.clone());
        }
    }
}

/// Sort key for ping status: unreachable first, slow, reachable, unchecked last.
pub fn ping_sort_key(status: Option<&PingStatus>) -> u8 {
    match status {
        Some(PingStatus::Unreachable) => 0,
        Some(PingStatus::Slow { .. }) => 1,
        Some(PingStatus::Reachable { .. }) => 2,
        Some(PingStatus::Checking) => 3,
        Some(PingStatus::Skipped) | None => 4,
    }
}

/// Status glyph for dual encoding (color + shape).
/// ● online, ▲ slow, ✖ down. Checking uses animated spinner via tick.
pub fn status_glyph(status: Option<&PingStatus>, tick: u64) -> &'static str {
    match status {
        Some(PingStatus::Reachable { .. }) => "\u{25CF}", // ●
        Some(PingStatus::Slow { .. }) => "\u{25B2}",      // ▲
        Some(PingStatus::Unreachable) => "\u{2716}",      // ✖
        Some(PingStatus::Checking) => {
            crate::animation::SPINNER_FRAMES
                [(tick as usize) % crate::animation::SPINNER_FRAMES.len()]
        }
        Some(PingStatus::Skipped) => "",
        None => "\u{25CB}", // ○
    }
}

/// A display tag with its source (user-defined or provider-synced).
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayTag {
    pub name: String,
    pub is_user: bool,
}

/// Select up to 3 tags for display based on view mode and grouping.
/// Returns a Vec of up to 3 DisplayTags (user tags first, then provider tags).
pub fn select_display_tags(
    host: &HostEntry,
    group_by: &GroupBy,
    detail_mode: bool,
) -> Vec<DisplayTag> {
    let group_name = match group_by {
        GroupBy::Provider => host.provider.clone(),
        GroupBy::Tag(t) => Some(t.clone()),
        GroupBy::None => None,
    };

    let not_group = |t: &&str| {
        group_name
            .as_ref()
            .is_none_or(|g| !t.eq_ignore_ascii_case(g))
    };

    // Collect user tags, filtering out the group name
    let user_tags: Vec<DisplayTag> = host
        .tags
        .iter()
        .map(|t| t.as_str())
        .filter(not_group)
        .map(|t| DisplayTag {
            name: t.to_string(),
            is_user: true,
        })
        .collect();

    let limit = if detail_mode { 1 } else { 3 };
    let is_grouped = !matches!(group_by, GroupBy::None);

    // Grouped view: user tags only. Flat view: user tags + provider tags.
    if is_grouped {
        user_tags.into_iter().take(limit).collect()
    } else {
        let provider_tags = host
            .provider_tags
            .iter()
            .chain(host.provider.iter())
            .map(|t| DisplayTag {
                name: t.to_string(),
                is_user: false,
            });
        user_tags
            .into_iter()
            .chain(provider_tags)
            .take(limit)
            .collect()
    }
}

/// Build health summary spans: ●23 ▲2 ✖1 ○1
/// Only includes states with count > 0. Returns empty vec if no pings.
pub fn health_summary_spans(
    ping_status: &HashMap<String, PingStatus>,
    hosts: &[HostEntry],
) -> Vec<Span<'static>> {
    health_summary_spans_for(ping_status, hosts.iter().map(|h| h.alias.as_str()))
}

/// Build health summary spans for a subset of host aliases.
/// Only includes states with count > 0. Returns empty vec if no pings.
pub fn health_summary_spans_for<'a>(
    ping_status: &HashMap<String, PingStatus>,
    aliases: impl Iterator<Item = &'a str>,
) -> Vec<Span<'static>> {
    if ping_status.is_empty() {
        return vec![];
    }
    let mut online = 0u32;
    let mut slow = 0u32;
    let mut down = 0u32;
    let mut unchecked = 0u32;
    for alias in aliases {
        match ping_status.get(alias) {
            Some(PingStatus::Reachable { .. }) => online += 1,
            Some(PingStatus::Slow { .. }) => slow += 1,
            Some(PingStatus::Unreachable) => down += 1,
            Some(PingStatus::Checking) | None => unchecked += 1,
            Some(PingStatus::Skipped) => {} // ProxyJump, excluded
        }
    }
    let mut spans = Vec::new();
    if online > 0 {
        spans.push(Span::styled(
            format!("\u{25CF}{online}"),
            theme::online_dot(),
        ));
    }
    if slow > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!("\u{25B2}{slow}"), theme::warning()));
    }
    if down > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!("\u{2716}{down}"), theme::error()));
    }
    if unchecked > 0 {
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!("\u{25CB}{unchecked}"), theme::muted()));
    }
    spans
}

/// Group mode for the host list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupBy {
    None,
    Provider,
    Tag(String),
}

impl GroupBy {
    pub fn to_key(&self) -> String {
        match self {
            GroupBy::None => "none".to_string(),
            GroupBy::Provider => "provider".to_string(),
            GroupBy::Tag(tag) => format!("tag:{}", tag),
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "none" => GroupBy::None,
            "provider" => GroupBy::Provider,
            s if s.starts_with("tag:") => match s.strip_prefix("tag:") {
                Some(tag) => GroupBy::Tag(tag.to_string()),
                _ => GroupBy::None,
            },
            _ => GroupBy::None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            GroupBy::None => "ungrouped".to_string(),
            GroupBy::Provider => "provider".to_string(),
            GroupBy::Tag(tag) => format!("tag: {}", tag),
        }
    }
}

/// Stores a deleted host for undo.
#[derive(Debug, Clone)]
pub struct DeletedHost {
    pub element: ConfigElement,
    pub position: usize,
}

/// Ratatui ListState fields for all list views.
pub struct UiSelection {
    pub list_state: ListState,
    pub key_list_state: ListState,
    pub show_key_picker: bool,
    pub key_picker_state: ListState,
    pub show_password_picker: bool,
    pub password_picker_state: ListState,
    pub show_proxyjump_picker: bool,
    pub proxyjump_picker_state: ListState,
    pub tag_picker_state: ListState,
    pub theme_picker_state: ListState,
    pub theme_picker_builtins: Vec<crate::ui::theme::ThemeDef>,
    pub theme_picker_custom: Vec<crate::ui::theme::ThemeDef>,
    pub theme_picker_saved_name: String,
    pub theme_picker_original: Option<crate::ui::theme::ThemeDef>,
    pub provider_list_state: ListState,
    pub tunnel_list_state: ListState,
    pub snippet_picker_state: ListState,
    pub snippet_search: Option<String>,
    pub show_region_picker: bool,
    pub region_picker_cursor: usize,
    pub help_scroll: u16,
    pub detail_scroll: u16,
}

/// State for the Containers overlay.
pub struct ContainerState {
    pub alias: String,
    pub askpass: Option<String>,
    pub runtime: Option<crate::containers::ContainerRuntime>,
    pub containers: Vec<crate::containers::ContainerInfo>,
    pub list_state: ratatui::widgets::ListState,
    pub loading: bool,
    pub error: Option<String>,
    pub action_in_progress: Option<String>,
    /// Pending confirmation for stop/restart actions: (action, container_name, container_id).
    pub confirm_action: Option<(crate::containers::ContainerAction, String, String)>,
}

/// Search mode state.
pub struct SearchState {
    pub query: Option<String>,
    pub filtered_indices: Vec<usize>,
    pub filtered_pattern_indices: Vec<usize>,
    pub pre_search_selection: Option<usize>,
    /// When a group tab is active, holds the host indices visible in that group.
    /// Search results are intersected with this set to scope the search.
    pub scope_indices: Option<std::collections::HashSet<usize>>,
}

/// Auto-reload mtime tracking.
pub struct ReloadState {
    pub config_path: PathBuf,
    pub last_modified: Option<SystemTime>,
    pub include_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
    pub include_dir_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
}

/// Form conflict detection mtimes.
pub struct ConflictState {
    pub form_mtime: Option<SystemTime>,
    pub form_include_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
    pub form_include_dir_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
    pub provider_form_mtime: Option<SystemTime>,
}
/// Baseline snapshot of host form content for dirty-check on Esc.
#[derive(Clone)]
pub struct FormBaseline {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: String,
    pub identity_file: String,
    pub proxy_jump: String,
    pub askpass: String,
    pub vault_ssh: String,
    pub vault_addr: String,
    pub tags: String,
}

/// Baseline snapshot of tunnel form content for dirty-check on Esc.
#[derive(Clone)]
pub struct TunnelFormBaseline {
    pub tunnel_type: crate::tunnel::TunnelType,
    pub bind_port: String,
    pub remote_host: String,
    pub remote_port: String,
    pub bind_address: String,
}

/// Baseline snapshot of snippet form content for dirty-check on Esc.
#[derive(Clone)]
pub struct SnippetFormBaseline {
    pub name: String,
    pub command: String,
    pub description: String,
}

/// Tag editor state.
#[derive(Default)]
pub struct TagState {
    pub input: Option<String>,
    pub cursor: usize,
    pub list: Vec<String>,
}

/// Baseline snapshot of provider form content for dirty-check on Esc.
#[derive(Clone)]
pub struct ProviderFormBaseline {
    pub url: String,
    pub token: String,
    pub profile: String,
    pub project: String,
    pub compartment: String,
    pub regions: String,
    pub alias_prefix: String,
    pub user: String,
    pub identity_file: String,
    pub verify_tls: bool,
    pub auto_sync: bool,
    pub vault_role: String,
    pub vault_addr: String,
}
