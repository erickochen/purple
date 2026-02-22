use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ratatui::widgets::ListState;

use crate::history::ConnectionHistory;
use crate::ssh_config::model::{ConfigElement, HostEntry, SshConfigFile};
use crate::ssh_keys::{self, SshKeyInfo};

/// Which screen is currently displayed.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    HostList,
    AddHost,
    EditHost { index: usize },
    ConfirmDelete { alias: String },
    Help,
    KeyList,
    KeyDetail { index: usize },
    HostDetail { index: usize },
    TagPicker,
}

/// Which form field is focused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FormField {
    Alias,
    Hostname,
    User,
    Port,
    IdentityFile,
    ProxyJump,
}

impl FormField {
    pub const ALL: [FormField; 6] = [
        FormField::Alias,
        FormField::Hostname,
        FormField::User,
        FormField::Port,
        FormField::IdentityFile,
        FormField::ProxyJump,
    ];

    pub fn next(self) -> Self {
        let idx = FormField::ALL.iter().position(|f| *f == self).unwrap_or(0);
        FormField::ALL[(idx + 1) % FormField::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = FormField::ALL.iter().position(|f| *f == self).unwrap_or(0);
        FormField::ALL[(idx + FormField::ALL.len() - 1) % FormField::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            FormField::Alias => "Alias",
            FormField::Hostname => "Host / IP",
            FormField::User => "User",
            FormField::Port => "Port",
            FormField::IdentityFile => "Identity File",
            FormField::ProxyJump => "ProxyJump",
        }
    }
}

/// Form state for adding/editing a host.
#[derive(Debug, Clone)]
pub struct HostForm {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: String,
    pub identity_file: String,
    pub proxy_jump: String,
    pub focused_field: FormField,
}

impl HostForm {
    pub fn new() -> Self {
        Self {
            alias: String::new(),
            hostname: String::new(),
            user: String::new(),
            port: "22".to_string(),
            identity_file: String::new(),
            proxy_jump: String::new(),
            focused_field: FormField::Alias,
        }
    }

    pub fn from_entry(entry: &HostEntry) -> Self {
        Self {
            alias: entry.alias.clone(),
            hostname: entry.hostname.clone(),
            user: entry.user.clone(),
            port: entry.port.to_string(),
            identity_file: entry.identity_file.clone(),
            proxy_jump: entry.proxy_jump.clone(),
            focused_field: FormField::Alias,
        }
    }

    /// Get a mutable reference to the currently focused field's value.
    pub fn focused_value_mut(&mut self) -> &mut String {
        match self.focused_field {
            FormField::Alias => &mut self.alias,
            FormField::Hostname => &mut self.hostname,
            FormField::User => &mut self.user,
            FormField::Port => &mut self.port,
            FormField::IdentityFile => &mut self.identity_file,
            FormField::ProxyJump => &mut self.proxy_jump,
        }
    }

    /// Validate the form. Returns an error message if invalid.
    pub fn validate(&self) -> Result<(), String> {
        if self.alias.trim().is_empty() {
            return Err("Alias can't be empty. Every host needs a name!".to_string());
        }
        if self.alias.contains(' ') {
            return Err("Alias can't contain spaces. Keep it simple.".to_string());
        }
        if self.hostname.trim().is_empty() {
            return Err("Hostname can't be empty. Where should we connect to?".to_string());
        }
        let port: u16 = self
            .port
            .parse()
            .map_err(|_| "That's not a port number. Ports are 1-65535, not poetry.".to_string())?;
        if port == 0 {
            return Err("Port 0? Bold choice, but no. Try 1-65535.".to_string());
        }
        Ok(())
    }

    /// Convert to a HostEntry.
    pub fn to_entry(&self) -> HostEntry {
        HostEntry {
            alias: self.alias.trim().to_string(),
            hostname: self.hostname.trim().to_string(),
            user: self.user.trim().to_string(),
            port: self.port.parse().unwrap_or(22),
            identity_file: self.identity_file.trim().to_string(),
            proxy_jump: self.proxy_jump.trim().to_string(),
            source_file: None,
            tags: Vec::new(),
        }
    }
}

/// Status message displayed at the bottom.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub is_error: bool,
    pub tick_count: u32,
}

/// An item in the display list (hosts + group headers).
#[derive(Debug, Clone)]
pub enum HostListItem {
    GroupHeader(String),
    Host { index: usize },
}

/// Ping status for a host.
#[derive(Debug, Clone, PartialEq)]
pub enum PingStatus {
    Checking,
    Reachable,
    Unreachable,
    Skipped,
}

/// Sort mode for the host list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortMode {
    Original,
    AlphaAlias,
    AlphaHostname,
    Frecency,
    MostRecent,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            SortMode::Original => SortMode::AlphaAlias,
            SortMode::AlphaAlias => SortMode::AlphaHostname,
            SortMode::AlphaHostname => SortMode::Frecency,
            SortMode::Frecency => SortMode::MostRecent,
            SortMode::MostRecent => SortMode::Original,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::Original => "config order",
            SortMode::AlphaAlias => "A-Z alias",
            SortMode::AlphaHostname => "A-Z hostname",
            SortMode::Frecency => "most used",
            SortMode::MostRecent => "most recent",
        }
    }

    pub fn to_key(self) -> &'static str {
        match self {
            SortMode::Original => "original",
            SortMode::AlphaAlias => "alpha_alias",
            SortMode::AlphaHostname => "alpha_hostname",
            SortMode::Frecency => "frecency",
            SortMode::MostRecent => "most_recent",
        }
    }

    pub fn from_key(s: &str) -> Self {
        match s {
            "alpha_alias" => SortMode::AlphaAlias,
            "alpha_hostname" => SortMode::AlphaHostname,
            "frecency" => SortMode::Frecency,
            "most_recent" => SortMode::MostRecent,
            _ => SortMode::Original,
        }
    }
}

/// Stores a deleted host for undo.
#[derive(Debug, Clone)]
pub struct DeletedHost {
    pub element: ConfigElement,
    pub position: usize,
}

/// Main application state.
pub struct App {
    pub screen: Screen,
    pub running: bool,
    pub config: SshConfigFile,
    pub hosts: Vec<HostEntry>,

    // Host list state
    pub list_state: ListState,

    // Display list (hosts + group headers)
    pub display_list: Vec<HostListItem>,

    // Search state
    pub search_query: Option<String>,
    pub filtered_indices: Vec<usize>,
    pre_search_selection: Option<usize>,

    // Form state
    pub form: HostForm,

    // Status bar
    pub status: Option<StatusMessage>,

    // Pending SSH connection
    pub pending_connect: Option<String>,

    // Key management state
    pub keys: Vec<SshKeyInfo>,
    pub key_list_state: ListState,
    pub show_key_picker: bool,
    pub key_picker_state: ListState,

    // Ping status
    pub ping_status: HashMap<String, PingStatus>,

    // Tag input
    pub tag_input: Option<String>,

    // Tag picker
    pub tag_list: Vec<String>,
    pub tag_picker_state: ListState,

    // Connection history
    pub history: ConnectionHistory,

    // Sort mode
    pub sort_mode: SortMode,

    // Undo state
    pub deleted_host: Option<DeletedHost>,

    // Learning hints
    pub has_pinged: bool,

    // Auto-reload state
    pub config_path: PathBuf,
    pub last_modified: Option<SystemTime>,
}

impl App {
    pub fn new(config: SshConfigFile) -> Self {
        let hosts = config.host_entries();
        let display_list = Self::build_display_list_from(&config, &hosts);
        let mut list_state = ListState::default();
        // Select first selectable item
        if let Some(pos) = display_list
            .iter()
            .position(|item| matches!(item, HostListItem::Host { .. }))
        {
            list_state.select(Some(pos));
        }

        let config_path = config.path.clone();
        let last_modified = Self::get_mtime(&config_path);

        Self {
            screen: Screen::HostList,
            running: true,
            config,
            hosts,
            list_state,
            display_list,
            search_query: None,
            filtered_indices: Vec::new(),
            pre_search_selection: None,
            form: HostForm::new(),
            status: None,
            pending_connect: None,
            keys: Vec::new(),
            key_list_state: ListState::default(),
            show_key_picker: false,
            key_picker_state: ListState::default(),
            ping_status: HashMap::new(),
            tag_input: None,
            tag_list: Vec::new(),
            tag_picker_state: ListState::default(),
            history: ConnectionHistory::load(),
            sort_mode: SortMode::Original,
            deleted_host: None,
            has_pinged: false,
            config_path,
            last_modified,
        }
    }

    /// Build the display list with group headers from comments above host blocks.
    /// Comments are associated with the host block directly below them (no blank line between).
    /// Because the parser puts inter-block comments inside the preceding block's directives,
    /// we also extract trailing comments from each HostBlock.
    fn build_display_list_from(config: &SshConfigFile, hosts: &[HostEntry]) -> Vec<HostListItem> {
        let mut display_list = Vec::new();
        let mut host_index = 0;
        let mut pending_comment: Option<String> = None;

        for element in &config.elements {
            match element {
                ConfigElement::GlobalLine(line) => {
                    let trimmed = line.trim();
                    if trimmed.starts_with('#') {
                        let text = trimmed.trim_start_matches('#').trim().to_string();
                        if !text.is_empty() {
                            pending_comment = Some(text);
                        }
                    } else if trimmed.is_empty() {
                        // Blank line breaks the comment-to-host association
                        pending_comment = None;
                    } else {
                        pending_comment = None;
                    }
                }
                ConfigElement::HostBlock(block) => {
                    // Skip wildcards (same logic as host_entries)
                    if block.host_pattern.contains('*')
                        || block.host_pattern.contains('?')
                        || block.host_pattern.contains(' ')
                    {
                        pending_comment = None;
                        continue;
                    }

                    if host_index < hosts.len() {
                        if let Some(header) = pending_comment.take() {
                            display_list.push(HostListItem::GroupHeader(header));
                        }
                        display_list.push(HostListItem::Host { index: host_index });
                        host_index += 1;
                    }

                    // Extract trailing comments from this block for the next host
                    pending_comment = Self::extract_trailing_comment(&block.directives);
                }
                ConfigElement::Include(include) => {
                    pending_comment = None;
                    for file in &include.resolved_files {
                        Self::build_display_list_from_included(
                            &file.elements,
                            &file.path,
                            hosts,
                            &mut host_index,
                            &mut display_list,
                        );
                    }
                }
            }
        }

        display_list
    }

    /// Extract a trailing comment from a block's directives.
    /// If the last non-blank line in the directives is a comment, return it as
    /// a potential group header for the next host block.
    fn extract_trailing_comment(directives: &[crate::ssh_config::model::Directive]) -> Option<String> {
        let d = directives.iter().next_back()?;
        if !d.is_non_directive {
            return None;
        }
        let trimmed = d.raw_line.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.starts_with('#') {
            let text = trimmed.trim_start_matches('#').trim().to_string();
            if !text.is_empty() {
                return Some(text);
            }
        }
        None
    }

    fn build_display_list_from_included(
        elements: &[ConfigElement],
        file_path: &std::path::Path,
        hosts: &[HostEntry],
        host_index: &mut usize,
        display_list: &mut Vec<HostListItem>,
    ) {
        let mut pending_comment: Option<String> = None;
        let file_name = file_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();

        // Add file header for included files
        if !file_name.is_empty() {
            let has_hosts = elements.iter().any(|e| {
                matches!(e, ConfigElement::HostBlock(b)
                    if !b.host_pattern.contains('*')
                    && !b.host_pattern.contains('?')
                    && !b.host_pattern.contains(' ')
                )
            });
            if has_hosts {
                display_list.push(HostListItem::GroupHeader(file_name));
            }
        }

        for element in elements {
            match element {
                ConfigElement::GlobalLine(line) => {
                    let trimmed = line.trim();
                    if trimmed.starts_with('#') {
                        let text = trimmed.trim_start_matches('#').trim().to_string();
                        if !text.is_empty() {
                            pending_comment = Some(text);
                        }
                    } else {
                        pending_comment = None;
                    }
                }
                ConfigElement::HostBlock(block) => {
                    if block.host_pattern.contains('*')
                        || block.host_pattern.contains('?')
                        || block.host_pattern.contains(' ')
                    {
                        pending_comment = None;
                        continue;
                    }

                    if *host_index < hosts.len() {
                        if let Some(header) = pending_comment.take() {
                            display_list.push(HostListItem::GroupHeader(header));
                        }
                        display_list.push(HostListItem::Host { index: *host_index });
                        *host_index += 1;
                    }
                }
                ConfigElement::Include(include) => {
                    pending_comment = None;
                    for file in &include.resolved_files {
                        Self::build_display_list_from_included(
                            &file.elements,
                            &file.path,
                            hosts,
                            host_index,
                            display_list,
                        );
                    }
                }
            }
        }
    }

    /// Rebuild the display list based on the current sort mode.
    pub fn apply_sort(&mut self) {
        if self.sort_mode == SortMode::Original {
            self.display_list = Self::build_display_list_from(&self.config, &self.hosts);
        } else {
            let mut indices: Vec<usize> = (0..self.hosts.len()).collect();
            match self.sort_mode {
                SortMode::AlphaAlias => {
                    indices.sort_by(|a, b| {
                        self.hosts[*a]
                            .alias
                            .to_lowercase()
                            .cmp(&self.hosts[*b].alias.to_lowercase())
                    });
                }
                SortMode::AlphaHostname => {
                    indices.sort_by(|a, b| {
                        self.hosts[*a]
                            .hostname
                            .to_lowercase()
                            .cmp(&self.hosts[*b].hostname.to_lowercase())
                    });
                }
                SortMode::Frecency => {
                    indices.sort_by(|a, b| {
                        let score_a = self.history.frecency_score(&self.hosts[*a].alias);
                        let score_b = self.history.frecency_score(&self.hosts[*b].alias);
                        score_b
                            .partial_cmp(&score_a)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
                SortMode::MostRecent => {
                    indices.sort_by(|a, b| {
                        let ts_a = self.history.last_connected(&self.hosts[*a].alias);
                        let ts_b = self.history.last_connected(&self.hosts[*b].alias);
                        ts_b.cmp(&ts_a)
                    });
                }
                _ => {}
            }
            self.display_list = indices
                .into_iter()
                .map(|i| HostListItem::Host { index: i })
                .collect();
        }
        // Select first host
        if let Some(pos) = self
            .display_list
            .iter()
            .position(|item| matches!(item, HostListItem::Host { .. }))
        {
            self.list_state.select(Some(pos));
        }
    }

    /// Get the host index from the currently selected display list item.
    pub fn selected_host_index(&self) -> Option<usize> {
        if self.search_query.is_some() {
            // In search mode, list_state indexes into filtered_indices
            let sel = self.list_state.selected()?;
            self.filtered_indices.get(sel).copied()
        } else {
            // In normal mode, list_state indexes into display_list
            let sel = self.list_state.selected()?;
            match self.display_list.get(sel) {
                Some(HostListItem::Host { index }) => Some(*index),
                _ => None,
            }
        }
    }

    /// Get the currently selected host entry.
    pub fn selected_host(&self) -> Option<&HostEntry> {
        self.selected_host_index()
            .and_then(|i| self.hosts.get(i))
    }

    /// Move selection up, skipping group headers.
    pub fn select_prev(&mut self) {
        if self.search_query.is_some() {
            cycle_selection(&mut self.list_state, self.filtered_indices.len(), false);
        } else {
            self.select_prev_in_display_list();
        }
    }

    /// Move selection down, skipping group headers.
    pub fn select_next(&mut self) {
        if self.search_query.is_some() {
            cycle_selection(&mut self.list_state, self.filtered_indices.len(), true);
        } else {
            self.select_next_in_display_list();
        }
    }

    fn select_next_in_display_list(&mut self) {
        if self.display_list.is_empty() {
            return;
        }
        let len = self.display_list.len();
        let current = self.list_state.selected().unwrap_or(0);
        // Find next Host item after current
        for offset in 1..=len {
            let idx = (current + offset) % len;
            if matches!(self.display_list[idx], HostListItem::Host { .. }) {
                self.list_state.select(Some(idx));
                return;
            }
        }
    }

    fn select_prev_in_display_list(&mut self) {
        if self.display_list.is_empty() {
            return;
        }
        let len = self.display_list.len();
        let current = self.list_state.selected().unwrap_or(0);
        // Find prev Host item before current
        for offset in 1..=len {
            let idx = (current + len - offset) % len;
            if matches!(self.display_list[idx], HostListItem::Host { .. }) {
                self.list_state.select(Some(idx));
                return;
            }
        }
    }

    /// Reload hosts from config.
    pub fn reload_hosts(&mut self) {
        let had_search = self.search_query.clone();

        self.hosts = self.config.host_entries();
        if self.sort_mode == SortMode::Original {
            self.display_list = Self::build_display_list_from(&self.config, &self.hosts);
        } else {
            self.apply_sort();
        }

        // Prune ping status for hosts that no longer exist
        let valid_aliases: std::collections::HashSet<&str> =
            self.hosts.iter().map(|h| h.alias.as_str()).collect();
        self.ping_status.retain(|alias, _| valid_aliases.contains(alias.as_str()));

        // Restore search if it was active, otherwise reset
        if let Some(query) = had_search {
            self.search_query = Some(query);
            self.apply_filter();
        } else {
            self.search_query = None;
            self.filtered_indices.clear();
            // Fix selection for display list mode
            if self.hosts.is_empty() {
                self.list_state.select(None);
            } else if let Some(pos) = self
                .display_list
                .iter()
                .position(|item| matches!(item, HostListItem::Host { .. }))
            {
                let current = self.list_state.selected().unwrap_or(0);
                if current >= self.display_list.len()
                    || !matches!(self.display_list.get(current), Some(HostListItem::Host { .. }))
                {
                    self.list_state.select(Some(pos));
                }
            } else {
                self.list_state.select(None);
            }
        }
    }

    // --- Search methods ---

    /// Enter search mode.
    pub fn start_search(&mut self) {
        self.pre_search_selection = self.list_state.selected();
        self.search_query = Some(String::new());
        self.apply_filter();
    }

    /// Start search with an initial query (for positional arg).
    pub fn start_search_with(&mut self, query: &str) {
        self.search_query = Some(query.to_string());
        self.apply_filter();
    }

    /// Cancel search mode and restore normal view.
    pub fn cancel_search(&mut self) {
        self.search_query = None;
        self.filtered_indices.clear();
        // Restore pre-search position (bounds-checked)
        if let Some(pos) = self.pre_search_selection.take() {
            if pos < self.display_list.len() {
                self.list_state.select(Some(pos));
            } else if let Some(first) = self
                .display_list
                .iter()
                .position(|item| matches!(item, HostListItem::Host { .. }))
            {
                self.list_state.select(Some(first));
            }
        }
    }

    /// Apply the current search query to filter hosts.
    pub fn apply_filter(&mut self) {
        let query = match &self.search_query {
            Some(q) => q.to_lowercase(),
            None => return,
        };

        if query.is_empty() {
            self.filtered_indices = (0..self.hosts.len()).collect();
        } else if let Some(tag_exact) = query.strip_prefix("tag=") {
            // Exact tag match (from tag picker)
            self.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    host.tags
                        .iter()
                        .any(|t| t.to_lowercase() == tag_exact)
                })
                .map(|(i, _)| i)
                .collect();
        } else if let Some(tag_query) = query.strip_prefix("tag:") {
            // Fuzzy tag match (manual search)
            self.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    host.tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(tag_query))
                })
                .map(|(i, _)| i)
                .collect();
        } else {
            self.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    host.alias.to_lowercase().contains(&query)
                        || host.hostname.to_lowercase().contains(&query)
                        || host.user.to_lowercase().contains(&query)
                        || host.tags.iter().any(|t| t.to_lowercase().contains(&query))
                })
                .map(|(i, _)| i)
                .collect();
        }

        // Reset selection
        if self.filtered_indices.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
    }

    /// Set a status message.
    pub fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status = Some(StatusMessage {
            text: text.into(),
            is_error,
            tick_count: 0,
        });
    }

    /// Tick the status message timer. Errors show for 5s, success for 3s.
    pub fn tick_status(&mut self) {
        if let Some(ref mut status) = self.status {
            status.tick_count += 1;
            let timeout = if status.is_error { 20 } else { 12 };
            if status.tick_count > timeout {
                self.status = None;
            }
        }
    }

    /// Get the modification time of a file.
    fn get_mtime(path: &Path) -> Option<SystemTime> {
        std::fs::metadata(path).ok()?.modified().ok()
    }

    /// Check if config has changed externally and reload if so.
    pub fn check_config_changed(&mut self) {
        let current_mtime = Self::get_mtime(&self.config_path);
        if current_mtime != self.last_modified {
            if let Ok(new_config) = SshConfigFile::parse(&self.config_path) {
                self.config = new_config;
                self.reload_hosts();
                self.last_modified = current_mtime;
                let count = self.hosts.len();
                self.set_status(format!("Config reloaded. {} hosts.", count), false);
            }
        }
    }

    /// Update the last_modified timestamp (call after writing config).
    pub fn update_last_modified(&mut self) {
        self.last_modified = Self::get_mtime(&self.config_path);
    }

    /// Scan SSH keys from ~/.ssh/ and cross-reference with hosts.
    pub fn scan_keys(&mut self) {
        if let Some(home) = dirs::home_dir() {
            let ssh_dir = home.join(".ssh");
            self.keys = ssh_keys::discover_keys(Path::new(&ssh_dir), &self.hosts);
            if !self.keys.is_empty() && self.key_list_state.selected().is_none() {
                self.key_list_state.select(Some(0));
            }
        }
    }

    /// Move key list selection up.
    pub fn select_prev_key(&mut self) {
        cycle_selection(&mut self.key_list_state, self.keys.len(), false);
    }

    /// Move key list selection down.
    pub fn select_next_key(&mut self) {
        cycle_selection(&mut self.key_list_state, self.keys.len(), true);
    }

    /// Move key picker selection up.
    pub fn select_prev_picker_key(&mut self) {
        cycle_selection(&mut self.key_picker_state, self.keys.len(), false);
    }

    /// Move key picker selection down.
    pub fn select_next_picker_key(&mut self) {
        cycle_selection(&mut self.key_picker_state, self.keys.len(), true);
    }

    /// Collect all unique tags from hosts, sorted alphabetically.
    pub fn collect_unique_tags(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut tags = Vec::new();
        for host in &self.hosts {
            for tag in &host.tags {
                if seen.insert(tag.clone()) {
                    tags.push(tag.clone());
                }
            }
        }
        tags.sort_by_key(|a| a.to_lowercase());
        tags
    }

    /// Open the tag picker overlay.
    pub fn open_tag_picker(&mut self) {
        self.tag_list = self.collect_unique_tags();
        self.tag_picker_state = ListState::default();
        if !self.tag_list.is_empty() {
            self.tag_picker_state.select(Some(0));
        }
        self.screen = Screen::TagPicker;
    }

    /// Move tag picker selection up.
    pub fn select_prev_tag(&mut self) {
        cycle_selection(&mut self.tag_picker_state, self.tag_list.len(), false);
    }

    /// Move tag picker selection down.
    pub fn select_next_tag(&mut self) {
        cycle_selection(&mut self.tag_picker_state, self.tag_list.len(), true);
    }
}

/// Cycle list selection forward or backward with wraparound.
fn cycle_selection(state: &mut ListState, len: usize, forward: bool) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_config::model::SshConfigFile;
    use std::path::PathBuf;

    fn make_app(content: &str) -> App {
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: PathBuf::from("/tmp/test_config"),
        };
        App::new(config)
    }

    #[test]
    fn test_apply_filter_matches_alias() {
        let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        app.start_search();
        app.search_query = Some("alp".to_string());
        app.apply_filter();
        assert_eq!(app.filtered_indices, vec![0]);
    }

    #[test]
    fn test_apply_filter_matches_hostname() {
        let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        app.start_search();
        app.search_query = Some("b.com".to_string());
        app.apply_filter();
        assert_eq!(app.filtered_indices, vec![1]);
    }

    #[test]
    fn test_apply_filter_empty_query() {
        let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        app.start_search();
        assert_eq!(app.filtered_indices, vec![0, 1]);
    }

    #[test]
    fn test_apply_filter_no_matches() {
        let mut app = make_app("Host alpha\n  HostName a.com\n");
        app.start_search();
        app.search_query = Some("zzz".to_string());
        app.apply_filter();
        assert!(app.filtered_indices.is_empty());
    }

    #[test]
    fn test_build_display_list_with_group_headers() {
        let content = "\
# Production
Host prod
  HostName prod.example.com

# Staging
Host staging
  HostName staging.example.com
";
        let app = make_app(content);
        assert_eq!(app.display_list.len(), 4);
        assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "Production"));
        assert!(matches!(&app.display_list[1], HostListItem::Host { index: 0 }));
        assert!(matches!(&app.display_list[2], HostListItem::GroupHeader(s) if s == "Staging"));
        assert!(matches!(&app.display_list[3], HostListItem::Host { index: 1 }));
    }

    #[test]
    fn test_build_display_list_blank_line_breaks_group() {
        let content = "\
# This comment is separated by blank line

Host nogroup
  HostName nogroup.example.com
";
        let app = make_app(content);
        // Blank line between comment and host means no group header
        assert_eq!(app.display_list.len(), 1);
        assert!(matches!(&app.display_list[0], HostListItem::Host { index: 0 }));
    }

    #[test]
    fn test_navigation_skips_headers() {
        let content = "\
# Group
Host alpha
  HostName a.com

# Group 2
Host beta
  HostName b.com
";
        let mut app = make_app(content);
        // Should start on first Host (index 1 in display_list)
        assert_eq!(app.list_state.selected(), Some(1));
        app.select_next();
        // Should skip header at index 2, land on Host at index 3
        assert_eq!(app.list_state.selected(), Some(3));
        app.select_prev();
        assert_eq!(app.list_state.selected(), Some(1));
    }
}
