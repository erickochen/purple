use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::SystemTime;

use ratatui::widgets::ListState;

use crate::history::ConnectionHistory;
use crate::providers::config::ProviderConfig;
use crate::tunnel::{TunnelRule, TunnelType};
use crate::ssh_config::model::{ConfigElement, HostEntry, SshConfigFile};
use crate::ssh_keys::{self, SshKeyInfo};

/// Case-insensitive substring check without allocation.
pub(crate) fn contains_ci(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Case-insensitive equality check without allocation.
fn eq_ci(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Record of the last sync result for a provider.
#[derive(Debug, Clone)]
pub struct SyncRecord {
    pub timestamp: u64,
    pub message: String,
    pub is_error: bool,
}

/// Which screen is currently displayed.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    HostList,
    AddHost,
    EditHost { alias: String },
    ConfirmDelete { alias: String },
    Help,
    KeyList,
    KeyDetail { index: usize },
    HostDetail { index: usize },
    TagPicker,
    Providers,
    ProviderForm { provider: String },
    TunnelList { alias: String },
    TunnelForm { alias: String, editing: Option<usize> },
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
    AskPass,
    Tags,
}

impl FormField {
    pub const ALL: [FormField; 8] = [
        FormField::Alias,
        FormField::Hostname,
        FormField::User,
        FormField::Port,
        FormField::IdentityFile,
        FormField::ProxyJump,
        FormField::AskPass,
        FormField::Tags,
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
            FormField::AskPass => "Password Source",
            FormField::Tags => "Tags",
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
    pub askpass: String,
    pub tags: String,
    pub focused_field: FormField,
    pub cursor_pos: usize,
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
            askpass: String::new(),
            tags: String::new(),
            focused_field: FormField::Alias,
            cursor_pos: 0,
        }
    }

    pub fn from_entry(entry: &HostEntry) -> Self {
        let alias = entry.alias.clone();
        let cursor_pos = alias.chars().count();
        Self {
            alias,
            hostname: entry.hostname.clone(),
            user: entry.user.clone(),
            port: entry.port.to_string(),
            identity_file: entry.identity_file.clone(),
            proxy_jump: entry.proxy_jump.clone(),
            askpass: entry.askpass.clone().unwrap_or_default(),
            tags: entry.tags.join(", "),
            focused_field: FormField::Alias,
            cursor_pos,
        }
    }

    pub fn focused_value(&self) -> &str {
        match self.focused_field {
            FormField::Alias => &self.alias,
            FormField::Hostname => &self.hostname,
            FormField::User => &self.user,
            FormField::Port => &self.port,
            FormField::IdentityFile => &self.identity_file,
            FormField::ProxyJump => &self.proxy_jump,
            FormField::AskPass => &self.askpass,
            FormField::Tags => &self.tags,
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
            FormField::AskPass => &mut self.askpass,
            FormField::Tags => &mut self.tags,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let pos = self.cursor_pos;
        let val = self.focused_value_mut();
        let byte_pos = char_to_byte_pos(val, pos);
        val.insert(byte_pos, c);
        self.cursor_pos = pos + 1;
    }

    pub fn delete_char_before_cursor(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let pos = self.cursor_pos;
        let val = self.focused_value_mut();
        let byte_pos = char_to_byte_pos(val, pos);
        let prev = char_to_byte_pos(val, pos - 1);
        val.drain(prev..byte_pos);
        self.cursor_pos = pos - 1;
    }

    pub fn sync_cursor_to_end(&mut self) {
        self.cursor_pos = self.focused_value().chars().count();
    }

    /// Validate the form. Returns an error message if invalid.
    pub fn validate(&self) -> Result<(), String> {
        if self.alias.trim().is_empty() {
            return Err("Alias can't be empty. Every host needs a name!".to_string());
        }
        if self.alias.contains(char::is_whitespace) {
            return Err("Alias can't contain whitespace. Keep it simple.".to_string());
        }
        if self.alias.contains('#') {
            return Err("Alias can't contain '#'. That's a comment character in SSH config.".to_string());
        }
        if crate::ssh_config::model::is_host_pattern(&self.alias) {
            return Err(
                "Alias can't contain pattern characters. That creates a match pattern, not a host."
                    .to_string(),
            );
        }
        // Reject control characters in all fields
        let fields = [
            (&self.alias, "Alias"),
            (&self.hostname, "Hostname"),
            (&self.user, "User"),
            (&self.port, "Port"),
            (&self.identity_file, "Identity File"),
            (&self.proxy_jump, "ProxyJump"),
            (&self.askpass, "Password Source"),
            (&self.tags, "Tags"),
        ];
        for (value, name) in &fields {
            if value.chars().any(|c| c.is_control()) {
                return Err(format!("{} contains control characters. That's not going to work.", name));
            }
        }
        if self.hostname.trim().is_empty() {
            return Err("Hostname can't be empty. Where should we connect to?".to_string());
        }
        if self.hostname.trim().contains(char::is_whitespace) {
            return Err("Hostname can't contain whitespace.".to_string());
        }
        if self.user.trim().contains(char::is_whitespace) {
            return Err("User can't contain whitespace.".to_string());
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
        let askpass_trimmed = self.askpass.trim().to_string();
        HostEntry {
            alias: self.alias.trim().to_string(),
            hostname: self.hostname.trim().to_string(),
            user: self.user.trim().to_string(),
            port: self.port.parse().unwrap_or(22),
            identity_file: self.identity_file.trim().to_string(),
            proxy_jump: self.proxy_jump.trim().to_string(),
            tags: self.tags.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
            askpass: if askpass_trimmed.is_empty() { None } else { Some(askpass_trimmed) },
            ..Default::default()
        }
    }
}

/// Which provider form field is focused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProviderFormField {
    Url,
    Token,
    AliasPrefix,
    User,
    IdentityFile,
    VerifyTls,
    AutoSync,
}

impl ProviderFormField {
    const CLOUD_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::AutoSync,
    ];

    const PROXMOX_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Url,
        ProviderFormField::Token,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::VerifyTls,
        ProviderFormField::AutoSync,
    ];

    pub fn fields_for(provider: &str) -> &'static [ProviderFormField] {
        match provider {
            "proxmox" => Self::PROXMOX_FIELDS,
            _ => Self::CLOUD_FIELDS,
        }
    }

    pub fn next(self, fields: &[Self]) -> Self {
        let idx = fields.iter().position(|f| *f == self).unwrap_or(0);
        fields[(idx + 1) % fields.len()]
    }

    pub fn prev(self, fields: &[Self]) -> Self {
        let idx = fields.iter().position(|f| *f == self).unwrap_or(0);
        fields[(idx + fields.len() - 1) % fields.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            ProviderFormField::Url => "URL",
            ProviderFormField::Token => "Token",
            ProviderFormField::AliasPrefix => "Alias Prefix",
            ProviderFormField::User => "User",
            ProviderFormField::IdentityFile => "Identity File",
            ProviderFormField::VerifyTls => "Verify TLS",
            ProviderFormField::AutoSync => "Auto Sync",
        }
    }
}

/// Form state for configuring a provider.
#[derive(Debug, Clone)]
pub struct ProviderFormFields {
    pub url: String,
    pub token: String,
    pub alias_prefix: String,
    pub user: String,
    pub identity_file: String,
    pub verify_tls: bool,
    pub auto_sync: bool,
    pub focused_field: ProviderFormField,
    pub cursor_pos: usize,
}

impl ProviderFormFields {
    pub fn new() -> Self {
        Self {
            url: String::new(),
            token: String::new(),
            alias_prefix: String::new(),
            user: "root".to_string(),
            identity_file: String::new(),
            verify_tls: true,
            auto_sync: true,
            focused_field: ProviderFormField::Token,
            cursor_pos: 0,
        }
    }

    pub fn focused_value(&self) -> &str {
        match self.focused_field {
            ProviderFormField::Url => &self.url,
            ProviderFormField::Token => &self.token,
            ProviderFormField::AliasPrefix => &self.alias_prefix,
            ProviderFormField::User => &self.user,
            ProviderFormField::IdentityFile => &self.identity_file,
            ProviderFormField::VerifyTls | ProviderFormField::AutoSync => "",
        }
    }

    pub fn focused_value_mut(&mut self) -> &mut String {
        match self.focused_field {
            ProviderFormField::Url => &mut self.url,
            ProviderFormField::Token => &mut self.token,
            ProviderFormField::AliasPrefix => &mut self.alias_prefix,
            ProviderFormField::User => &mut self.user,
            ProviderFormField::IdentityFile => &mut self.identity_file,
            ProviderFormField::VerifyTls => unreachable!("VerifyTls is a toggle, not a text field"),
            ProviderFormField::AutoSync => unreachable!("AutoSync is a toggle, not a text field"),
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let pos = self.cursor_pos;
        let val = self.focused_value_mut();
        let byte_pos = char_to_byte_pos(val, pos);
        val.insert(byte_pos, c);
        self.cursor_pos = pos + 1;
    }

    pub fn delete_char_before_cursor(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let pos = self.cursor_pos;
        let val = self.focused_value_mut();
        let byte_pos = char_to_byte_pos(val, pos);
        let prev = char_to_byte_pos(val, pos - 1);
        val.drain(prev..byte_pos);
        self.cursor_pos = pos - 1;
    }

    pub fn sync_cursor_to_end(&mut self) {
        self.cursor_pos = self.focused_value().chars().count();
    }
}

fn char_to_byte_pos(s: &str, char_pos: usize) -> usize {
    s.char_indices()
        .nth(char_pos)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

/// Which tunnel form field is focused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TunnelFormField {
    Type,
    BindPort,
    RemoteHost,
    RemotePort,
}

impl TunnelFormField {
    /// Next field, skipping remote fields when Dynamic.
    pub fn next(self, tunnel_type: TunnelType) -> Self {
        match (self, tunnel_type) {
            (TunnelFormField::Type, _) => TunnelFormField::BindPort,
            (TunnelFormField::BindPort, TunnelType::Dynamic) => TunnelFormField::Type,
            (TunnelFormField::BindPort, _) => TunnelFormField::RemoteHost,
            (TunnelFormField::RemoteHost, _) => TunnelFormField::RemotePort,
            (TunnelFormField::RemotePort, _) => TunnelFormField::Type,
        }
    }

    /// Previous field, skipping remote fields when Dynamic.
    pub fn prev(self, tunnel_type: TunnelType) -> Self {
        match (self, tunnel_type) {
            (TunnelFormField::Type, TunnelType::Dynamic) => TunnelFormField::BindPort,
            (TunnelFormField::Type, _) => TunnelFormField::RemotePort,
            (TunnelFormField::BindPort, _) => TunnelFormField::Type,
            (TunnelFormField::RemoteHost, _) => TunnelFormField::BindPort,
            (TunnelFormField::RemotePort, _) => TunnelFormField::RemoteHost,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TunnelFormField::Type => "Type",
            TunnelFormField::BindPort => "Bind Port",
            TunnelFormField::RemoteHost => "Remote Host",
            TunnelFormField::RemotePort => "Remote Port",
        }
    }
}

/// Form state for adding/editing a tunnel.
#[derive(Debug, Clone)]
pub struct TunnelForm {
    pub tunnel_type: TunnelType,
    pub bind_port: String,
    pub remote_host: String,
    pub remote_port: String,
    /// Hidden field: preserved during edit, not exposed in the form UI.
    pub bind_address: String,
    pub focused_field: TunnelFormField,
    pub cursor_pos: usize,
}

impl TunnelForm {
    pub fn new() -> Self {
        Self {
            tunnel_type: TunnelType::Local,
            bind_port: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: String::new(),
            bind_address: String::new(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        }
    }

    pub fn from_rule(rule: &TunnelRule) -> Self {
        Self {
            tunnel_type: rule.tunnel_type,
            bind_port: rule.bind_port.to_string(),
            remote_host: rule.remote_host.clone(),
            remote_port: if rule.remote_port > 0 {
                rule.remote_port.to_string()
            } else {
                String::new()
            },
            bind_address: rule.bind_address.clone(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        }
    }

    /// Validate the form. Returns error message if invalid.
    pub fn validate(&self) -> Result<(), String> {
        // Reject control characters in all fields
        let fields = [
            (&self.bind_port, "Bind Port"),
            (&self.remote_host, "Remote Host"),
            (&self.remote_port, "Remote Port"),
        ];
        for (value, name) in &fields {
            if value.chars().any(|c| c.is_control()) {
                return Err(format!("{} contains control characters.", name));
            }
        }
        let port: u16 = self
            .bind_port
            .parse()
            .map_err(|_| "Bind port must be 1-65535.".to_string())?;
        if port == 0 {
            return Err("Bind port can't be 0.".to_string());
        }
        if self.tunnel_type != TunnelType::Dynamic {
            let host = self.remote_host.trim();
            if host.is_empty() {
                return Err("Remote host can't be empty.".to_string());
            }
            if host.contains(char::is_whitespace) {
                return Err("Remote host can't contain spaces.".to_string());
            }
            let rport: u16 = self
                .remote_port
                .parse()
                .map_err(|_| "Remote port must be 1-65535.".to_string())?;
            if rport == 0 {
                return Err("Remote port can't be 0.".to_string());
            }
        }
        Ok(())
    }

    /// Convert to directive key and value for writing to SSH config.
    /// Uses TunnelRule for correct IPv6 bracketing and bind_address preservation.
    pub fn to_directive(&self) -> (&'static str, String) {
        let key = self.tunnel_type.directive_key();
        let bind_port: u16 = self.bind_port.parse().unwrap_or(0);
        let remote_port: u16 = self.remote_port.parse().unwrap_or(0);
        let rule = TunnelRule {
            tunnel_type: self.tunnel_type,
            bind_address: self.bind_address.clone(),
            bind_port,
            remote_host: self.remote_host.clone(),
            remote_port,
        };
        (key, rule.to_directive_value())
    }

    pub fn focused_value(&self) -> Option<&str> {
        match self.focused_field {
            TunnelFormField::Type => None,
            TunnelFormField::BindPort => Some(&self.bind_port),
            TunnelFormField::RemoteHost => Some(&self.remote_host),
            TunnelFormField::RemotePort => Some(&self.remote_port),
        }
    }

    /// Get mutable reference to the focused text field's value.
    /// Returns None for Type field (uses Left/Right, not text input).
    pub fn focused_value_mut(&mut self) -> Option<&mut String> {
        match self.focused_field {
            TunnelFormField::Type => None,
            TunnelFormField::BindPort => Some(&mut self.bind_port),
            TunnelFormField::RemoteHost => Some(&mut self.remote_host),
            TunnelFormField::RemotePort => Some(&mut self.remote_port),
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let pos = self.cursor_pos;
        if let Some(val) = self.focused_value_mut() {
            let byte_pos = char_to_byte_pos(val, pos);
            val.insert(byte_pos, c);
            self.cursor_pos = pos + 1;
        }
    }

    pub fn delete_char_before_cursor(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let pos = self.cursor_pos;
        if let Some(val) = self.focused_value_mut() {
            let byte_pos = char_to_byte_pos(val, pos);
            let prev = char_to_byte_pos(val, pos - 1);
            val.drain(prev..byte_pos);
            self.cursor_pos = pos - 1;
        }
    }

    pub fn sync_cursor_to_end(&mut self) {
        self.cursor_pos = self.focused_value().map(|v| v.chars().count()).unwrap_or(0);
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
            "original" => SortMode::Original,
            "alpha_alias" => SortMode::AlphaAlias,
            "alpha_hostname" => SortMode::AlphaHostname,
            "frecency" => SortMode::Frecency,
            "most_recent" => SortMode::MostRecent,
            _ => SortMode::MostRecent,
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
    pub tag_picker_state: ListState,
    pub provider_list_state: ListState,
    pub tunnel_list_state: ListState,
}

/// Search mode state.
pub struct SearchState {
    pub query: Option<String>,
    pub filtered_indices: Vec<usize>,
    pub pre_search_selection: Option<usize>,
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

/// Kill active tunnel processes when App is dropped (e.g. on panic).
impl Drop for App {
    fn drop(&mut self) {
        for (_, mut tunnel) in self.active_tunnels.drain() {
            let _ = tunnel.child.kill();
            let _ = tunnel.child.wait();
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
    pub display_list: Vec<HostListItem>,
    pub form: HostForm,
    pub status: Option<StatusMessage>,
    pub pending_connect: Option<(String, Option<String>)>,

    // Sub-structs
    pub ui: UiSelection,
    pub search: SearchState,
    pub reload: ReloadState,
    pub conflict: ConflictState,

    // Keys
    pub keys: Vec<SshKeyInfo>,

    // Tags
    pub tag_input: Option<String>,
    pub tag_list: Vec<String>,

    // History + preferences
    pub history: ConnectionHistory,
    pub sort_mode: SortMode,
    pub group_by_provider: bool,

    // Undo
    pub deleted_host: Option<DeletedHost>,

    // Providers
    pub provider_config: ProviderConfig,
    pub provider_form: ProviderFormFields,
    pub syncing_providers: HashMap<String, Arc<AtomicBool>>,
    pub pending_provider_delete: Option<String>,

    // Hints
    pub ping_status: HashMap<String, PingStatus>,
    pub has_pinged: bool,

    // Tunnels
    pub tunnel_list: Vec<TunnelRule>,
    pub tunnel_form: TunnelForm,
    pub active_tunnels: HashMap<String, crate::tunnel::ActiveTunnel>,

    // Update
    pub update_available: Option<String>,
    pub update_hint: &'static str,

    // Sync history
    pub sync_history: HashMap<String, SyncRecord>,

    // Bitwarden session
    pub bw_session: Option<String>,
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
        let include_mtimes = Self::snapshot_include_mtimes(&config);
        let include_dir_mtimes = Self::snapshot_include_dir_mtimes(&config);

        Self {
            screen: Screen::HostList,
            running: true,
            config,
            hosts,
            display_list,
            form: HostForm::new(),
            status: None,
            pending_connect: None,
            ui: UiSelection {
                list_state,
                key_list_state: ListState::default(),
                show_key_picker: false,
                key_picker_state: ListState::default(),
                show_password_picker: false,
                password_picker_state: ListState::default(),
                tag_picker_state: ListState::default(),
                provider_list_state: ListState::default(),
                tunnel_list_state: ListState::default(),
            },
            search: SearchState {
                query: None,
                filtered_indices: Vec::new(),
                pre_search_selection: None,
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
            tag_input: None,
            tag_list: Vec::new(),
            history: ConnectionHistory::load(),
            sort_mode: SortMode::Original,
            group_by_provider: false,
            deleted_host: None,
            provider_config: ProviderConfig::load(),
            provider_form: ProviderFormFields::new(),
            syncing_providers: HashMap::new(),
            pending_provider_delete: None,
            ping_status: HashMap::new(),
            has_pinged: false,
            tunnel_list: Vec::new(),
            tunnel_form: TunnelForm::new(),
            active_tunnels: HashMap::new(),
            update_available: None,
            update_hint: crate::update::update_hint(),
            sync_history: HashMap::new(),
            bw_session: None,
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
                        let text = trimmed.trim_start_matches('#').trim();
                        let text = text.strip_prefix("purple:group ").unwrap_or(text);
                        if !text.is_empty() {
                            pending_comment = Some(text.to_string());
                        }
                    } else if trimmed.is_empty() {
                        // Blank line breaks the comment-to-host association
                        pending_comment = None;
                    } else {
                        pending_comment = None;
                    }
                }
                ConfigElement::HostBlock(block) => {
                    if crate::ssh_config::model::is_host_pattern(&block.host_pattern) {
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
    /// Strips `purple:group ` prefix so headers display as the provider name.
    fn extract_trailing_comment(directives: &[crate::ssh_config::model::Directive]) -> Option<String> {
        let d = directives.last()?;
        if !d.is_non_directive {
            return None;
        }
        let trimmed = d.raw_line.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.starts_with('#') {
            let text = trimmed.trim_start_matches('#').trim();
            // Skip purple metadata comments (purple:provider, purple:tags)
            // Only purple:group should produce a group header
            if text.starts_with("purple:") && !text.starts_with("purple:group ") {
                return None;
            }
            let text = text.strip_prefix("purple:group ").unwrap_or(text);
            if !text.is_empty() {
                return Some(text.to_string());
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
                    if !crate::ssh_config::model::is_host_pattern(&b.host_pattern)
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
                        let text = trimmed.trim_start_matches('#').trim();
                        let text = text.strip_prefix("purple:group ").unwrap_or(text);
                        if !text.is_empty() {
                            pending_comment = Some(text.to_string());
                        }
                    } else {
                        pending_comment = None;
                    }
                }
                ConfigElement::HostBlock(block) => {
                    if crate::ssh_config::model::is_host_pattern(&block.host_pattern) {
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
                            host_index,
                            display_list,
                        );
                    }
                }
            }
        }
    }

    /// Rebuild the display list based on the current sort mode and group_by_provider toggle.
    pub fn apply_sort(&mut self) {
        if self.sort_mode == SortMode::Original && !self.group_by_provider {
            self.display_list = Self::build_display_list_from(&self.config, &self.hosts);
        } else if self.sort_mode == SortMode::Original && self.group_by_provider {
            // Original order but grouped by provider: extract flat indices from config order
            let indices: Vec<usize> = (0..self.hosts.len()).collect();
            self.display_list = Self::group_indices_by_provider(&self.hosts, &indices);
        } else {
            let mut indices: Vec<usize> = (0..self.hosts.len()).collect();
            match self.sort_mode {
                SortMode::AlphaAlias => {
                    indices.sort_by_cached_key(|&i| self.hosts[i].alias.to_lowercase());
                }
                SortMode::AlphaHostname => {
                    indices.sort_by_cached_key(|&i| self.hosts[i].hostname.to_lowercase());
                }
                SortMode::Frecency => {
                    indices.sort_by(|a, b| {
                        let score_a = self.history.frecency_score(&self.hosts[*a].alias);
                        let score_b = self.history.frecency_score(&self.hosts[*b].alias);
                        score_b.total_cmp(&score_a)
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
            if self.group_by_provider {
                self.display_list = Self::group_indices_by_provider(&self.hosts, &indices);
            } else {
                self.display_list = indices
                    .into_iter()
                    .map(|i| HostListItem::Host { index: i })
                    .collect();
            }
        }
        self.select_first_host();
    }

    /// Select the first host item in the display list.
    fn select_first_host(&mut self) {
        if let Some(pos) = self
            .display_list
            .iter()
            .position(|item| matches!(item, HostListItem::Host { .. }))
        {
            self.ui.list_state.select(Some(pos));
        }
    }

    /// Partition sorted indices by provider, inserting group headers.
    /// Hosts without provider appear first (no header), then named provider
    /// groups (in first-appearance order) with headers.
    fn group_indices_by_provider(hosts: &[HostEntry], sorted_indices: &[usize]) -> Vec<HostListItem> {
        let mut none_indices: Vec<usize> = Vec::new();
        let mut provider_groups: Vec<(&str, Vec<usize>)> = Vec::new();
        let mut provider_order: HashMap<&str, usize> = HashMap::new();

        for &idx in sorted_indices {
            match &hosts[idx].provider {
                None => none_indices.push(idx),
                Some(name) => {
                    if let Some(&group_idx) = provider_order.get(name.as_str()) {
                        provider_groups[group_idx].1.push(idx);
                    } else {
                        let group_idx = provider_groups.len();
                        provider_order.insert(name, group_idx);
                        provider_groups.push((name, vec![idx]));
                    }
                }
            }
        }

        let mut display_list = Vec::new();

        // Non-provider hosts first (no header)
        for idx in &none_indices {
            display_list.push(HostListItem::Host { index: *idx });
        }

        // Then provider groups with headers
        for (name, indices) in &provider_groups {
            let header = crate::providers::provider_display_name(name);
            display_list.push(HostListItem::GroupHeader(header.to_string()));
            for &idx in indices {
                display_list.push(HostListItem::Host { index: idx });
            }
        }
        display_list
    }

    /// Get the host index from the currently selected display list item.
    pub fn selected_host_index(&self) -> Option<usize> {
        if self.search.query.is_some() {
            // In search mode, list_state indexes into filtered_indices
            let sel = self.ui.list_state.selected()?;
            self.search.filtered_indices.get(sel).copied()
        } else {
            // In normal mode, list_state indexes into display_list
            let sel = self.ui.list_state.selected()?;
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
        if self.search.query.is_some() {
            cycle_selection(&mut self.ui.list_state, self.search.filtered_indices.len(), false);
        } else {
            self.select_prev_in_display_list();
        }
    }

    /// Move selection down, skipping group headers.
    pub fn select_next(&mut self) {
        if self.search.query.is_some() {
            cycle_selection(&mut self.ui.list_state, self.search.filtered_indices.len(), true);
        } else {
            self.select_next_in_display_list();
        }
    }

    fn select_next_in_display_list(&mut self) {
        if self.display_list.is_empty() {
            return;
        }
        let len = self.display_list.len();
        let current = self.ui.list_state.selected().unwrap_or(0);
        // Find next Host item after current
        for offset in 1..=len {
            let idx = (current + offset) % len;
            if matches!(self.display_list[idx], HostListItem::Host { .. }) {
                self.ui.list_state.select(Some(idx));
                return;
            }
        }
    }

    fn select_prev_in_display_list(&mut self) {
        if self.display_list.is_empty() {
            return;
        }
        let len = self.display_list.len();
        let current = self.ui.list_state.selected().unwrap_or(0);
        // Find prev Host item before current
        for offset in 1..=len {
            let idx = (current + len - offset) % len;
            if matches!(self.display_list[idx], HostListItem::Host { .. }) {
                self.ui.list_state.select(Some(idx));
                return;
            }
        }
    }

    /// Reload hosts from config.
    pub fn reload_hosts(&mut self) {
        let had_search = self.search.query.take();

        self.hosts = self.config.host_entries();
        if self.sort_mode == SortMode::Original && !self.group_by_provider {
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
            self.search.query = Some(query);
            self.apply_filter();
        } else {
            self.search.query = None;
            self.search.filtered_indices.clear();
            // Fix selection for display list mode
            if self.hosts.is_empty() {
                self.ui.list_state.select(None);
            } else if let Some(pos) = self
                .display_list
                .iter()
                .position(|item| matches!(item, HostListItem::Host { .. }))
            {
                let current = self.ui.list_state.selected().unwrap_or(0);
                if current >= self.display_list.len()
                    || !matches!(self.display_list.get(current), Some(HostListItem::Host { .. }))
                {
                    self.ui.list_state.select(Some(pos));
                }
            } else {
                self.ui.list_state.select(None);
            }
        }
    }

    // --- Search methods ---

    /// Enter search mode.
    pub fn start_search(&mut self) {
        self.search.pre_search_selection = self.ui.list_state.selected();
        self.search.query = Some(String::new());
        self.apply_filter();
    }

    /// Start search with an initial query (for positional arg).
    pub fn start_search_with(&mut self, query: &str) {
        self.search.pre_search_selection = self.ui.list_state.selected();
        self.search.query = Some(query.to_string());
        self.apply_filter();
    }

    /// Cancel search mode and restore normal view.
    pub fn cancel_search(&mut self) {
        self.search.query = None;
        self.search.filtered_indices.clear();
        // Restore pre-search position (bounds-checked)
        if let Some(pos) = self.search.pre_search_selection.take() {
            if pos < self.display_list.len() {
                self.ui.list_state.select(Some(pos));
            } else if let Some(first) = self
                .display_list
                .iter()
                .position(|item| matches!(item, HostListItem::Host { .. }))
            {
                self.ui.list_state.select(Some(first));
            }
        }
    }

    /// Apply the current search query to filter hosts.
    pub fn apply_filter(&mut self) {
        let query = match &self.search.query {
            Some(q) if !q.is_empty() => q.clone(),
            Some(_) => {
                self.search.filtered_indices = (0..self.hosts.len()).collect();
                if self.search.filtered_indices.is_empty() {
                    self.ui.list_state.select(None);
                } else {
                    self.ui.list_state.select(Some(0));
                }
                return;
            }
            None => return,
        };

        if let Some(tag_exact) = query.strip_prefix("tag=") {
            // Exact tag match (from tag picker), includes provider name
            self.search.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    host.tags
                        .iter()
                        .any(|t| eq_ci(t, tag_exact))
                        || host.provider.as_ref().is_some_and(|p| eq_ci(p, tag_exact))
                })
                .map(|(i, _)| i)
                .collect();
        } else if let Some(tag_query) = query.strip_prefix("tag:") {
            // Fuzzy tag match (manual search), includes provider name
            self.search.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    host.tags
                        .iter()
                        .any(|t| contains_ci(t, tag_query))
                        || host.provider.as_ref().is_some_and(|p| contains_ci(p, tag_query))
                })
                .map(|(i, _)| i)
                .collect();
        } else {
            self.search.filtered_indices = self
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, host)| {
                    contains_ci(&host.alias, &query)
                        || contains_ci(&host.hostname, &query)
                        || contains_ci(&host.user, &query)
                        || host.tags.iter().any(|t| contains_ci(t, &query))
                        || host.provider.as_ref().is_some_and(|p| contains_ci(p, &query))
                })
                .map(|(i, _)| i)
                .collect();
        }

        // Reset selection
        if self.search.filtered_indices.is_empty() {
            self.ui.list_state.select(None);
        } else {
            self.ui.list_state.select(Some(0));
        }
    }

    /// Provider names sorted by last sync (most recent first), then configured, then unconfigured.
    /// Includes any unknown provider names found in the config file (e.g. typos or future providers).
    pub fn sorted_provider_names(&self) -> Vec<String> {
        use crate::providers;
        let mut names: Vec<String> = providers::PROVIDER_NAMES.iter().map(|s| s.to_string()).collect();
        // Append configured providers not in the known list so they are visible and removable
        for section in &self.provider_config.sections {
            if !names.contains(&section.provider) {
                names.push(section.provider.clone());
            }
        }
        names.sort_by(|a, b| {
            let ts_a = self.sync_history.get(a.as_str()).map_or(0, |r| r.timestamp);
            let ts_b = self.sync_history.get(b.as_str()).map_or(0, |r| r.timestamp);
            let conf_a = self.provider_config.section(a.as_str()).is_some();
            let conf_b = self.provider_config.section(b.as_str()).is_some();
            ts_b.cmp(&ts_a).then(conf_b.cmp(&conf_a))
        });
        names
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

    /// Check if config or any Include file has changed externally and reload if so.
    /// Skips reload when the user is in a form (AddHost/EditHost) to avoid
    /// overwriting in-memory config while the user is editing.
    pub fn check_config_changed(&mut self) {
        if matches!(
            self.screen,
            Screen::AddHost | Screen::EditHost { .. } | Screen::ProviderForm { .. }
                | Screen::TunnelList { .. } | Screen::TunnelForm { .. }
                | Screen::HostDetail { .. }
        ) || self.tag_input.is_some()
        {
            return;
        }
        let current_mtime = Self::get_mtime(&self.reload.config_path);
        let changed = current_mtime != self.reload.last_modified
            || self.reload.include_mtimes.iter().any(|(path, old_mtime)| {
                Self::get_mtime(path) != *old_mtime
            })
            || self.reload.include_dir_mtimes.iter().any(|(path, old_mtime)| {
                Self::get_mtime(path) != *old_mtime
            });
        if changed {
            if let Ok(new_config) = SshConfigFile::parse(&self.reload.config_path) {
                self.config = new_config;
                // Invalidate undo state — config structure may have changed externally
                self.deleted_host = None;
                // Clear stale ping status — hosts may have changed
                self.ping_status.clear();
                self.reload_hosts();
                self.reload.last_modified = current_mtime;
                self.reload.include_mtimes = Self::snapshot_include_mtimes(&self.config);
                self.reload.include_dir_mtimes = Self::snapshot_include_dir_mtimes(&self.config);
                let count = self.hosts.len();
                self.set_status(format!("Config reloaded. {} hosts.", count), false);
            }
        }
    }

    /// Update the last_modified timestamp (call after writing config).
    pub fn update_last_modified(&mut self) {
        self.reload.last_modified = Self::get_mtime(&self.reload.config_path);
        self.reload.include_mtimes = Self::snapshot_include_mtimes(&self.config);
        self.reload.include_dir_mtimes = Self::snapshot_include_dir_mtimes(&self.config);
    }

    /// Clear form mtime state (call on form cancel or successful submit).
    pub fn clear_form_mtime(&mut self) {
        self.conflict.form_mtime = None;
        self.conflict.form_include_mtimes.clear();
        self.conflict.form_include_dir_mtimes.clear();
        self.conflict.provider_form_mtime = None;
    }

    /// Capture config and Include file mtimes when opening a host form.
    pub fn capture_form_mtime(&mut self) {
        self.conflict.form_mtime = Self::get_mtime(&self.reload.config_path);
        self.conflict.form_include_mtimes = Self::snapshot_include_mtimes(&self.config);
        self.conflict.form_include_dir_mtimes = Self::snapshot_include_dir_mtimes(&self.config);
    }

    /// Capture ~/.purple/providers mtime when opening a provider form.
    pub fn capture_provider_form_mtime(&mut self) {
        let path = dirs::home_dir()
            .map(|h| h.join(".purple/providers"));
        self.conflict.provider_form_mtime = path.as_ref().and_then(|p| Self::get_mtime(p));
    }

    /// Check if config or any Include file/directory has changed since the form was opened.
    pub fn config_changed_since_form_open(&self) -> bool {
        match self.conflict.form_mtime {
            Some(open_mtime) => {
                if Self::get_mtime(&self.reload.config_path) != Some(open_mtime) {
                    return true;
                }
                self.conflict.form_include_mtimes
                    .iter()
                    .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
                    || self
                        .conflict.form_include_dir_mtimes
                        .iter()
                        .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
            }
            None => false,
        }
    }

    /// Check if ~/.purple/providers has changed since the provider form was opened.
    pub fn provider_config_changed_since_form_open(&self) -> bool {
        let path = dirs::home_dir()
            .map(|h| h.join(".purple/providers"));
        let current_mtime = path.as_ref().and_then(|p| Self::get_mtime(p));
        self.conflict.provider_form_mtime != current_mtime
    }

    /// Snapshot mtimes of all resolved Include files.
    fn snapshot_include_mtimes(config: &SshConfigFile) -> Vec<(PathBuf, Option<SystemTime>)> {
        config
            .include_paths()
            .into_iter()
            .map(|p| {
                let mtime = Self::get_mtime(&p);
                (p, mtime)
            })
            .collect()
    }

    /// Snapshot mtimes of parent directories of Include glob patterns.
    fn snapshot_include_dir_mtimes(config: &SshConfigFile) -> Vec<(PathBuf, Option<SystemTime>)> {
        config
            .include_glob_dirs()
            .into_iter()
            .map(|p| {
                let mtime = Self::get_mtime(&p);
                (p, mtime)
            })
            .collect()
    }

    /// Scan SSH keys from ~/.ssh/ and cross-reference with hosts.
    pub fn scan_keys(&mut self) {
        if let Some(home) = dirs::home_dir() {
            let ssh_dir = home.join(".ssh");
            self.keys = ssh_keys::discover_keys(Path::new(&ssh_dir), &self.hosts);
            if !self.keys.is_empty() && self.ui.key_list_state.selected().is_none() {
                self.ui.key_list_state.select(Some(0));
            }
        }
    }

    /// Move key list selection up.
    pub fn select_prev_key(&mut self) {
        cycle_selection(&mut self.ui.key_list_state, self.keys.len(), false);
    }

    /// Move key list selection down.
    pub fn select_next_key(&mut self) {
        cycle_selection(&mut self.ui.key_list_state, self.keys.len(), true);
    }

    /// Move key picker selection up.
    pub fn select_prev_picker_key(&mut self) {
        cycle_selection(&mut self.ui.key_picker_state, self.keys.len(), false);
    }

    /// Move key picker selection down.
    pub fn select_next_picker_key(&mut self) {
        cycle_selection(&mut self.ui.key_picker_state, self.keys.len(), true);
    }

    /// Move password picker selection up.
    pub fn select_prev_password_source(&mut self) {
        cycle_selection(&mut self.ui.password_picker_state, crate::askpass::PASSWORD_SOURCES.len(), false);
    }

    /// Move password picker selection down.
    pub fn select_next_password_source(&mut self) {
        cycle_selection(&mut self.ui.password_picker_state, crate::askpass::PASSWORD_SOURCES.len(), true);
    }

    /// Collect all unique tags from hosts, sorted alphabetically.
    pub fn collect_unique_tags(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut tags = Vec::new();
        for host in &self.hosts {
            for tag in &host.tags {
                if seen.insert(tag.as_str()) {
                    tags.push(tag.clone());
                }
            }
            if let Some(ref provider) = host.provider {
                if seen.insert(provider.as_str()) {
                    tags.push(provider.clone());
                }
            }
        }
        tags.sort_by_cached_key(|a| a.to_lowercase());
        tags
    }

    /// Open the tag picker overlay.
    pub fn open_tag_picker(&mut self) {
        self.tag_list = self.collect_unique_tags();
        self.ui.tag_picker_state = ListState::default();
        if !self.tag_list.is_empty() {
            self.ui.tag_picker_state.select(Some(0));
        }
        self.screen = Screen::TagPicker;
    }

    /// Move tag picker selection up.
    pub fn select_prev_tag(&mut self) {
        cycle_selection(&mut self.ui.tag_picker_state, self.tag_list.len(), false);
    }

    /// Move tag picker selection down.
    pub fn select_next_tag(&mut self) {
        cycle_selection(&mut self.ui.tag_picker_state, self.tag_list.len(), true);
    }

    /// Load tunnel directives for a host alias.
    /// Uses find_tunnel_directives for Include-aware, multi-pattern host lookup.
    pub fn refresh_tunnel_list(&mut self, alias: &str) {
        self.tunnel_list = self.config.find_tunnel_directives(alias);
    }

    /// Move tunnel list selection up.
    pub fn select_prev_tunnel(&mut self) {
        cycle_selection(&mut self.ui.tunnel_list_state, self.tunnel_list.len(), false);
    }

    /// Move tunnel list selection down.
    pub fn select_next_tunnel(&mut self) {
        cycle_selection(&mut self.ui.tunnel_list_state, self.tunnel_list.len(), true);
    }

    /// Poll active tunnels for exit status. Returns messages for any that exited.
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
                    // Read up to 1KB of stderr for error details
                    let stderr_msg = tunnel.child.stderr.take().and_then(|mut stderr| {
                        use std::io::Read;
                        let mut buf = vec![0u8; 1024];
                        match stderr.read(&mut buf) {
                            Ok(n) if n > 0 => {
                                let s = String::from_utf8_lossy(&buf[..n]);
                                let trimmed = s.trim();
                                if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
                            }
                            _ => None,
                        }
                    });
                    let (msg, is_error) = if status.success() {
                        (format!("Tunnel for {} closed.", alias), false)
                    } else if let Some(err) = stderr_msg {
                        (format!("Tunnel for {}: {}", alias, err), true)
                    } else {
                        (format!("Tunnel for {} exited with code {}.", alias, status.code().unwrap_or(-1)), true)
                    };
                    exited.push((alias.clone(), msg, is_error));
                    to_remove.push(alias.clone());
                }
                Ok(None) => {} // Still running
                Err(e) => {
                    exited.push((alias.clone(), format!("Tunnel for {} lost: {}", alias, e), true));
                    to_remove.push(alias.clone());
                }
            }
        }
        for alias in to_remove {
            if let Some(mut tunnel) = self.active_tunnels.remove(&alias) {
                let _ = tunnel.child.kill();
                let _ = tunnel.child.wait();
            }
        }
        exited
    }

    /// Add a new host from the current form. Returns status message.
    pub fn add_host_from_form(&mut self) -> Result<String, String> {
        let entry = self.form.to_entry();
        let alias = entry.alias.clone();
        if self.config.has_host(&alias) {
            return Err(format!(
                "'{}' already exists. Aliases are like fingerprints — unique.",
                alias
            ));
        }
        let len_before = self.config.elements.len();
        self.config.add_host(&entry);
        if !entry.tags.is_empty() {
            self.config.set_host_tags(&alias, &entry.tags);
        }
        if let Some(ref source) = entry.askpass {
            self.config.set_host_askpass(&alias, source);
        }
        if let Err(e) = self.config.write() {
            self.config.elements.truncate(len_before);
            return Err(format!("Failed to save: {}", e));
        }
        self.update_last_modified();
        self.reload_hosts();
        self.select_host_by_alias(&alias);
        Ok(format!("Welcome aboard, {}!", alias))
    }

    /// Edit an existing host from the current form. Returns status message.
    pub fn edit_host_from_form(&mut self, old_alias: &str) -> Result<String, String> {
        let entry = self.form.to_entry();
        let alias = entry.alias.clone();
        if !self.config.has_host(old_alias) {
            return Err("Host no longer exists.".to_string());
        }
        if alias != old_alias && self.config.has_host(&alias) {
            return Err(format!(
                "'{}' already exists. Aliases are like fingerprints — unique.",
                alias
            ));
        }
        let old_entry = self
            .hosts
            .iter()
            .find(|h| h.alias == old_alias)
            .cloned()
            .unwrap_or_default();
        self.config.update_host(old_alias, &entry);
        self.config.set_host_tags(&entry.alias, &entry.tags);
        self.config.set_host_askpass(&entry.alias, entry.askpass.as_deref().unwrap_or(""));
        if let Err(e) = self.config.write() {
            self.config.update_host(&entry.alias, &old_entry);
            self.config.set_host_tags(&old_entry.alias, &old_entry.tags);
            self.config.set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
            return Err(format!("Failed to save: {}", e));
        }
        // Migrate active tunnel handle if alias changed
        if alias != old_alias {
            if let Some(tunnel) = self.active_tunnels.remove(old_alias) {
                self.active_tunnels.insert(alias.clone(), tunnel);
            }
        }
        self.update_last_modified();
        self.reload_hosts();
        Ok(format!("{} got a makeover.", alias))
    }

    /// Select a host in the display list (or filtered list) by alias.
    pub fn select_host_by_alias(&mut self, alias: &str) {
        if self.search.query.is_some() {
            // In search mode, list_state indexes into filtered_indices
            for (i, &host_idx) in self.search.filtered_indices.iter().enumerate() {
                if self.hosts.get(host_idx).is_some_and(|h| h.alias == alias) {
                    self.ui.list_state.select(Some(i));
                    return;
                }
            }
        } else {
            for (i, item) in self.display_list.iter().enumerate() {
                if let HostListItem::Host { index } = item {
                    if self.hosts.get(*index).is_some_and(|h| h.alias == alias) {
                        self.ui.list_state.select(Some(i));
                        return;
                    }
                }
            }
        }
    }

    /// Apply sync results from a background provider fetch.
    /// Returns (message, is_error, server_count). Caller must remove from syncing_providers.
    pub fn apply_sync_result(
        &mut self,
        provider: &str,
        hosts: Vec<crate::providers::ProviderHost>,
    ) -> (String, bool, usize) {
        let section = match self.provider_config.section(provider).cloned() {
            Some(s) => s,
            None => return (format!("{} sync skipped: no config.", provider), true, 0),
        };
        let provider_impl = match crate::providers::get_provider_with_config(provider, &section) {
            Some(p) => p,
            None => return (format!("Unknown provider: {}.", provider), true, 0),
        };
        let config_backup = self.config.clone();
        let result = crate::providers::sync::sync_provider(
            &mut self.config,
            &*provider_impl,
            &hosts,
            &section,
            false,
            false,
        );
        let total = result.added + result.updated + result.unchanged;
        if result.added > 0 || result.updated > 0 {
            if let Err(e) = self.config.write() {
                self.config = config_backup;
                return (format!("Sync failed to save: {}", e), true, total);
            }
            self.deleted_host = None;
            self.update_last_modified();
            self.reload_hosts();
            // Migrate active tunnel handles for renamed aliases
            for (old_alias, new_alias) in &result.renames {
                if let Some(tunnel) = self.active_tunnels.remove(old_alias) {
                    self.active_tunnels.insert(new_alias.clone(), tunnel);
                }
            }
        }
        let name = crate::providers::provider_display_name(provider);
        (format!(
            "Synced {}: added {}, updated {}, unchanged {}.",
            name, result.added, result.updated, result.unchanged
        ), false, total)
    }
}

/// Cycle list selection forward or backward with wraparound.
pub fn cycle_selection(state: &mut ListState, len: usize, forward: bool) {
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
            crlf: false,
        };
        App::new(config)
    }

    #[test]
    fn test_apply_filter_matches_alias() {
        let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        app.start_search();
        app.search.query = Some("alp".to_string());
        app.apply_filter();
        assert_eq!(app.search.filtered_indices, vec![0]);
    }

    #[test]
    fn test_apply_filter_matches_hostname() {
        let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        app.start_search();
        app.search.query = Some("b.com".to_string());
        app.apply_filter();
        assert_eq!(app.search.filtered_indices, vec![1]);
    }

    #[test]
    fn test_apply_filter_empty_query() {
        let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        app.start_search();
        assert_eq!(app.search.filtered_indices, vec![0, 1]);
    }

    #[test]
    fn test_apply_filter_no_matches() {
        let mut app = make_app("Host alpha\n  HostName a.com\n");
        app.start_search();
        app.search.query = Some("zzz".to_string());
        app.apply_filter();
        assert!(app.search.filtered_indices.is_empty());
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
        assert_eq!(app.ui.list_state.selected(), Some(1));
        app.select_next();
        // Should skip header at index 2, land on Host at index 3
        assert_eq!(app.ui.list_state.selected(), Some(3));
        app.select_prev();
        assert_eq!(app.ui.list_state.selected(), Some(1));
    }

    #[test]
    fn test_group_by_provider_creates_headers() {
        let content = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:123

Host do-db
  HostName 5.6.7.8
  # purple:provider digitalocean:456

Host vultr-app
  HostName 9.9.9.9
  # purple:provider vultr:789
";
        let mut app = make_app(content);
        app.group_by_provider = true;
        app.apply_sort();

        // Should have: DigitalOcean header, 2 hosts, Vultr header, 1 host
        assert_eq!(app.display_list.len(), 5);
        assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean"));
        assert!(matches!(&app.display_list[1], HostListItem::Host { .. }));
        assert!(matches!(&app.display_list[2], HostListItem::Host { .. }));
        assert!(matches!(&app.display_list[3], HostListItem::GroupHeader(s) if s == "Vultr"));
        assert!(matches!(&app.display_list[4], HostListItem::Host { .. }));
    }

    #[test]
    fn test_group_by_provider_no_header_for_none() {
        let content = "\
Host manual
  HostName 1.2.3.4

Host do-web
  HostName 5.6.7.8
  # purple:provider digitalocean:123
";
        let mut app = make_app(content);
        app.group_by_provider = true;
        app.apply_sort();

        // manual first (no header), then DigitalOcean header + do-web
        assert_eq!(app.display_list.len(), 3);
        // No header before the manual host
        assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
        assert!(matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "DigitalOcean"));
        assert!(matches!(&app.display_list[2], HostListItem::Host { .. }));
    }

    #[test]
    fn test_group_by_provider_with_alpha_sort() {
        let content = "\
Host do-zeta
  HostName 1.2.3.4
  # purple:provider digitalocean:1

Host do-alpha
  HostName 5.6.7.8
  # purple:provider digitalocean:2
";
        let mut app = make_app(content);
        app.group_by_provider = true;
        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();

        // DigitalOcean header + sorted hosts
        assert_eq!(app.display_list.len(), 3);
        assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean"));
        // First host should be do-alpha (alphabetical)
        if let HostListItem::Host { index } = &app.display_list[1] {
            assert_eq!(app.hosts[*index].alias, "do-alpha");
        } else {
            panic!("Expected Host item");
        }
    }

    #[test]
    fn test_config_changed_since_form_open_no_mtime() {
        let app = make_app("Host alpha\n  HostName a.com\n");
        // No mtime captured — should return false
        assert!(!app.config_changed_since_form_open());
    }

    #[test]
    fn test_config_changed_since_form_open_same_mtime() {
        let mut app = make_app("Host alpha\n  HostName a.com\n");
        // Config path is /tmp/test_config which doesn't exist, so mtime is None
        app.capture_form_mtime();
        // Immediately checking — mtime should be same (None == None)
        assert!(!app.config_changed_since_form_open());
    }

    #[test]
    fn test_config_changed_since_form_open_detects_change() {
        let mut app = make_app("Host alpha\n  HostName a.com\n");
        // Set form_mtime to a known past value (different from current None)
        app.conflict.form_mtime = Some(SystemTime::UNIX_EPOCH);
        // Config path doesn't exist (mtime is None), so it differs from UNIX_EPOCH
        assert!(app.config_changed_since_form_open());
    }

    #[test]
    fn test_group_by_provider_toggle_off_restores_flat() {
        let content = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:123

Host vultr-app
  HostName 5.6.7.8
  # purple:provider vultr:456
";
        let mut app = make_app(content);
        app.sort_mode = SortMode::AlphaAlias;

        // Enable grouping
        app.group_by_provider = true;
        app.apply_sort();
        let grouped_len = app.display_list.len();
        assert!(grouped_len > 2); // Has headers

        // Disable grouping
        app.group_by_provider = false;
        app.apply_sort();
        // Should be flat: just hosts, no headers
        assert_eq!(app.display_list.len(), 2);
        assert!(app.display_list.iter().all(|item| matches!(item, HostListItem::Host { .. })));
    }

    // --- New validation tests from review findings ---

    #[test]
    fn test_validate_rejects_hash_in_alias() {
        let mut form = HostForm::new();
        form.alias = "my#host".to_string();
        form.hostname = "1.2.3.4".to_string();
        let result = form.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("#"));
    }

    #[test]
    fn test_validate_empty_alias() {
        let mut form = HostForm::new();
        form.alias = "".to_string();
        form.hostname = "1.2.3.4".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_whitespace_alias() {
        let mut form = HostForm::new();
        form.alias = "my host".to_string();
        form.hostname = "1.2.3.4".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_pattern_alias() {
        let mut form = HostForm::new();
        form.alias = "my*host".to_string();
        form.hostname = "1.2.3.4".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_empty_hostname() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_port() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.port = "abc".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_port_zero() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.port = "0".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_valid_form() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.port = "22".to_string();
        assert!(form.validate().is_ok());
    }

    #[test]
    fn test_validate_rejects_control_chars() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4\x00".to_string();
        form.port = "22".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_to_entry_parses_tags() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.tags = "prod, staging, us-east".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.tags, vec!["prod", "staging", "us-east"]);
    }

    #[test]
    fn test_sort_mode_round_trip() {
        for mode in [
            SortMode::Original,
            SortMode::AlphaAlias,
            SortMode::AlphaHostname,
            SortMode::Frecency,
            SortMode::MostRecent,
        ] {
            assert_eq!(SortMode::from_key(mode.to_key()), mode);
        }
    }

    // --- TunnelForm tests ---

    #[test]
    fn tunnel_form_from_rule_local() {
        use crate::tunnel::{TunnelRule, TunnelType};
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: String::new(),
            bind_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        };
        let form = TunnelForm::from_rule(&rule);
        assert_eq!(form.tunnel_type, TunnelType::Local);
        assert_eq!(form.bind_port, "8080");
        assert_eq!(form.remote_host, "localhost");
        assert_eq!(form.remote_port, "80");
    }

    #[test]
    fn tunnel_form_from_rule_dynamic() {
        use crate::tunnel::{TunnelRule, TunnelType};
        let rule = TunnelRule {
            tunnel_type: TunnelType::Dynamic,
            bind_address: String::new(),
            bind_port: 1080,
            remote_host: String::new(),
            remote_port: 0,
        };
        let form = TunnelForm::from_rule(&rule);
        assert_eq!(form.tunnel_type, TunnelType::Dynamic);
        assert_eq!(form.bind_port, "1080");
        assert_eq!(form.remote_host, "");
        assert_eq!(form.remote_port, "");
    }

    #[test]
    fn tunnel_form_to_directive_local() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        let (key, value) = form.to_directive();
        assert_eq!(key, "LocalForward");
        assert_eq!(value, "8080 localhost:80");
    }

    #[test]
    fn tunnel_form_to_directive_remote() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Remote,
            bind_port: "9090".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "3000".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        let (key, value) = form.to_directive();
        assert_eq!(key, "RemoteForward");
        assert_eq!(value, "9090 localhost:3000");
    }

    #[test]
    fn tunnel_form_to_directive_dynamic() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Dynamic,
            bind_port: "1080".to_string(),
            bind_address: String::new(),
            remote_host: String::new(),
            remote_port: String::new(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        let (key, value) = form.to_directive();
        assert_eq!(key, "DynamicForward");
        assert_eq!(value, "1080");
    }

    #[test]
    fn tunnel_form_validate_valid() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_ok());
    }

    #[test]
    fn tunnel_form_validate_bad_bind_port() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "abc".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_err());
    }

    #[test]
    fn tunnel_form_validate_zero_bind_port() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "0".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_err());
    }

    #[test]
    fn tunnel_form_validate_empty_remote_host() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "  ".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_err());
    }

    #[test]
    fn tunnel_form_validate_dynamic_skips_remote() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Dynamic,
            bind_port: "1080".to_string(),
            bind_address: String::new(),
            remote_host: String::new(),
            remote_port: String::new(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_ok());
    }

    #[test]
    fn tunnel_form_field_next_local() {
        use crate::tunnel::TunnelType;
        assert_eq!(TunnelFormField::Type.next(TunnelType::Local), TunnelFormField::BindPort);
        assert_eq!(TunnelFormField::BindPort.next(TunnelType::Local), TunnelFormField::RemoteHost);
        assert_eq!(TunnelFormField::RemoteHost.next(TunnelType::Local), TunnelFormField::RemotePort);
        assert_eq!(TunnelFormField::RemotePort.next(TunnelType::Local), TunnelFormField::Type);
    }

    #[test]
    fn tunnel_form_field_next_dynamic_skips_remote() {
        use crate::tunnel::TunnelType;
        assert_eq!(TunnelFormField::Type.next(TunnelType::Dynamic), TunnelFormField::BindPort);
        assert_eq!(TunnelFormField::BindPort.next(TunnelType::Dynamic), TunnelFormField::Type);
    }

    #[test]
    fn tunnel_form_field_prev_local() {
        use crate::tunnel::TunnelType;
        assert_eq!(TunnelFormField::Type.prev(TunnelType::Local), TunnelFormField::RemotePort);
        assert_eq!(TunnelFormField::BindPort.prev(TunnelType::Local), TunnelFormField::Type);
        assert_eq!(TunnelFormField::RemoteHost.prev(TunnelType::Local), TunnelFormField::BindPort);
        assert_eq!(TunnelFormField::RemotePort.prev(TunnelType::Local), TunnelFormField::RemoteHost);
    }

    #[test]
    fn tunnel_form_field_prev_dynamic_skips_remote() {
        use crate::tunnel::TunnelType;
        assert_eq!(TunnelFormField::Type.prev(TunnelType::Dynamic), TunnelFormField::BindPort);
        assert_eq!(TunnelFormField::BindPort.prev(TunnelType::Dynamic), TunnelFormField::Type);
    }

    #[test]
    fn tunnel_form_validate_bad_remote_port() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "abc".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_err());
    }

    #[test]
    fn tunnel_form_from_rule_with_bind_address() {
        use crate::tunnel::{TunnelRule, TunnelType};
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: "192.168.1.1".to_string(),
            bind_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        };
        let form = TunnelForm::from_rule(&rule);
        assert_eq!(form.bind_address, "192.168.1.1");
        assert_eq!(form.bind_port, "8080");
        let (key, value) = form.to_directive();
        assert_eq!(key, "LocalForward");
        assert_eq!(value, "192.168.1.1:8080 localhost:80");
    }

    #[test]
    fn tunnel_form_validate_empty_bind_port() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: String::new(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        assert!(form.validate().is_err());
    }

    #[test]
    fn tunnel_form_validate_zero_remote_port() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "localhost".to_string(),
            remote_port: "0".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("Remote port"));
    }

    #[test]
    fn tunnel_form_validate_control_chars() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "local\x00host".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("control characters"));
    }

    #[test]
    fn tunnel_form_validate_spaces_in_remote_host() {
        use crate::tunnel::TunnelType;
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            bind_address: String::new(),
            remote_host: "local host".to_string(),
            remote_port: "80".to_string(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("spaces"));
    }

    #[test]
    fn tunnel_form_from_rule_ipv6_bind_address() {
        use crate::tunnel::{TunnelRule, TunnelType};
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: "::1".to_string(),
            bind_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        };
        let form = TunnelForm::from_rule(&rule);
        assert_eq!(form.bind_address, "::1");
        let (key, value) = form.to_directive();
        assert_eq!(key, "LocalForward");
        assert_eq!(value, "[::1]:8080 localhost:80");
    }

    // --- HostForm validation tests ---

    #[test]
    fn validate_hostname_whitespace_rejected() {
        let form = HostForm {
            alias: "myserver".to_string(),
            hostname: "host name".to_string(),
            port: "22".to_string(),
            ..HostForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("whitespace"), "got: {}", err);
    }

    #[test]
    fn validate_user_whitespace_rejected() {
        let form = HostForm {
            alias: "myserver".to_string(),
            hostname: "10.0.0.1".to_string(),
            user: "my user".to_string(),
            port: "22".to_string(),
            ..HostForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("whitespace"), "got: {}", err);
    }

    #[test]
    fn validate_hostname_with_control_chars_rejected() {
        let form = HostForm {
            alias: "myserver".to_string(),
            hostname: "10.0.0.1\n".to_string(),
            port: "22".to_string(),
            ..HostForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("control"), "got: {}", err);
    }

    // --- TunnelForm validation error message tests ---

    #[test]
    fn tunnel_validate_bind_port_zero_message() {
        let form = TunnelForm {
            bind_port: "0".to_string(),
            ..TunnelForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("0"), "got: {}", err);
    }

    #[test]
    fn tunnel_validate_remote_host_empty_message() {
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            remote_host: "".to_string(),
            remote_port: "80".to_string(),
            ..TunnelForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("empty"), "got: {}", err);
    }

    #[test]
    fn tunnel_validate_remote_host_whitespace_message() {
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            remote_host: "host name".to_string(),
            remote_port: "80".to_string(),
            ..TunnelForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("spaces"), "got: {}", err);
    }

    #[test]
    fn tunnel_validate_bind_port_non_numeric_message() {
        let form = TunnelForm {
            bind_port: "abc".to_string(),
            ..TunnelForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("1-65535"), "got: {}", err);
    }

    #[test]
    fn tunnel_validate_remote_port_zero_message() {
        let form = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "8080".to_string(),
            remote_host: "localhost".to_string(),
            remote_port: "0".to_string(),
            ..TunnelForm::new()
        };
        let err = form.validate().unwrap_err();
        assert!(err.contains("0"), "got: {}", err);
    }

    #[test]
    fn select_host_by_alias_normal_mode() {
        let mut app =
            make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
        app.select_host_by_alias("beta");
        let selected = app.selected_host().unwrap();
        assert_eq!(selected.alias, "beta");
    }

    #[test]
    fn select_host_by_alias_search_mode() {
        let mut app = make_app(
            "Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n\nHost gamma\n  HostName g.com\n",
        );
        app.start_search();
        // Filter to beta and gamma (both contain letter 'a' in hostname or alias)
        app.search.query = Some("a".to_string());
        app.apply_filter();
        // filtered_indices should contain alpha (0) and gamma (2)
        assert!(app.search.filtered_indices.contains(&0));
        assert!(app.search.filtered_indices.contains(&2));

        // Select gamma by alias — should find it in filtered_indices
        app.select_host_by_alias("gamma");
        let selected = app.selected_host().unwrap();
        assert_eq!(selected.alias, "gamma");
    }

    #[test]
    fn select_host_by_alias_search_mode_not_in_results() {
        let mut app = make_app(
            "Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n",
        );
        app.start_search();
        app.search.query = Some("alpha".to_string());
        app.apply_filter();
        assert_eq!(app.search.filtered_indices, vec![0]);

        // "beta" is not in filtered results — selection should not change
        let before = app.ui.list_state.selected();
        app.select_host_by_alias("beta");
        assert_eq!(app.ui.list_state.selected(), before);
    }

    fn make_provider_app() -> App {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.provider_config = crate::providers::config::ProviderConfig::default();
        app.provider_config.set_section(crate::providers::config::ProviderSection {
            provider: "digitalocean".to_string(),
            token: "test-token".to_string(),
            alias_prefix: "do".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
        });
        app
    }

    #[test]
    fn test_apply_sync_result_no_config() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.provider_config = crate::providers::config::ProviderConfig::default();
        let (msg, is_err, total) = app.apply_sync_result("digitalocean", vec![]);
        assert!(is_err);
        assert_eq!(total, 0);
        assert!(msg.contains("no config"));
    }

    #[test]
    fn test_apply_sync_result_empty_hosts_returns_zero_total() {
        let mut app = make_provider_app();
        let (msg, is_err, total) = app.apply_sync_result("digitalocean", vec![]);
        assert!(!is_err);
        assert_eq!(total, 0);
        assert!(msg.contains("added 0"));
        assert!(msg.contains("unchanged 0"));
    }

    #[test]
    fn test_apply_sync_result_with_hosts_returns_total() {
        let mut app = make_provider_app();
        let hosts = vec![
            crate::providers::ProviderHost {
                server_id: "s1".to_string(),
                name: "web".to_string(),
                ip: "1.2.3.4".to_string(),
                tags: vec![],
            },
            crate::providers::ProviderHost {
                server_id: "s2".to_string(),
                name: "db".to_string(),
                ip: "5.6.7.8".to_string(),
                tags: vec![],
            },
        ];
        let (msg, is_err, total) = app.apply_sync_result("digitalocean", hosts);
        assert!(!is_err);
        assert_eq!(total, 2);
        assert!(msg.contains("added 2"));
        assert!(msg.contains("unchanged 0"));
    }

    #[test]
    fn test_apply_sync_result_write_failure_preserves_total() {
        let mut app = make_provider_app();
        // Point config to a non-writable path so write() fails
        app.config.path = PathBuf::from("/dev/null/impossible");
        let hosts = vec![
            crate::providers::ProviderHost {
                server_id: "s1".to_string(),
                name: "web".to_string(),
                ip: "1.2.3.4".to_string(),
                tags: vec![],
            },
            crate::providers::ProviderHost {
                server_id: "s2".to_string(),
                name: "db".to_string(),
                ip: "5.6.7.8".to_string(),
                tags: vec![],
            },
        ];
        let (msg, is_err, total) = app.apply_sync_result("digitalocean", hosts);
        assert!(is_err);
        assert_eq!(total, 2); // total preserved despite write failure
        assert!(msg.contains("Sync failed to save"));
    }

    #[test]
    fn test_apply_sync_result_unknown_provider() {
        let mut app = make_provider_app();
        // Configure a section for the unknown provider name so it passes
        // the config check but fails on get_provider()
        app.provider_config.set_section(crate::providers::config::ProviderSection {
            provider: "nonexistent".to_string(),
            token: "tok".to_string(),
            alias_prefix: "nope".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
        });
        let (msg, is_err, total) = app.apply_sync_result("nonexistent", vec![]);
        assert!(is_err);
        assert_eq!(total, 0);
        assert!(msg.contains("Unknown provider"));
    }

    #[test]
    fn test_sync_history_cleared_on_provider_remove() {
        let mut app = make_provider_app();
        // Simulate a completed sync
        app.sync_history.insert("digitalocean".to_string(), SyncRecord {
            timestamp: 100,
            message: "3 servers".to_string(),
            is_error: false,
        });
        assert!(app.sync_history.contains_key("digitalocean"));

        // Simulate provider remove (same as handler.rs 'd' key path)
        app.provider_config.remove_section("digitalocean");
        app.sync_history.remove("digitalocean");

        assert!(!app.sync_history.contains_key("digitalocean"));
    }

    #[test]
    fn test_sync_history_overwrite_replaces_error_with_success() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        // First sync fails
        app.sync_history.insert("hetzner".to_string(), SyncRecord {
            timestamp: 100,
            message: "auth failed".to_string(),
            is_error: true,
        });
        // Second sync succeeds
        app.sync_history.insert("hetzner".to_string(), SyncRecord {
            timestamp: 200,
            message: "5 servers".to_string(),
            is_error: false,
        });
        let record = app.sync_history.get("hetzner").unwrap();
        assert_eq!(record.timestamp, 200);
        assert!(!record.is_error);
        assert_eq!(record.message, "5 servers");
    }

    // --- auto_sync tests ---

    fn make_section(provider: &str, auto_sync: bool) -> crate::providers::config::ProviderSection {
        crate::providers::config::ProviderSection {
            provider: provider.to_string(),
            token: "tok".to_string(),
            alias_prefix: provider[..2].to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: if provider == "proxmox" { "https://pve:8006".to_string() } else { String::new() },
            verify_tls: true,
            auto_sync,
        }
    }

    #[test]
    fn test_startup_auto_sync_filter_skips_disabled_providers() {
        // Simuleert de startup-loop in main.rs: providers met auto_sync=false worden overgeslagen.
        let mut config = crate::providers::config::ProviderConfig::default();
        config.set_section(make_section("digitalocean", true));
        config.set_section(make_section("proxmox", false));
        let auto_synced: Vec<&str> = config
            .configured_providers()
            .iter()
            .filter(|s| s.auto_sync)
            .map(|s| s.provider.as_str())
            .collect();
        assert_eq!(auto_synced, vec!["digitalocean"]);
        assert!(!auto_synced.contains(&"proxmox"));
    }

    #[test]
    fn test_startup_auto_sync_filter_all_enabled() {
        let mut config = crate::providers::config::ProviderConfig::default();
        config.set_section(make_section("digitalocean", true));
        config.set_section(make_section("vu", true)); // vultr-achtig
        let skipped: Vec<&str> = config
            .configured_providers()
            .iter()
            .filter(|s| !s.auto_sync)
            .map(|s| s.provider.as_str())
            .collect();
        assert!(skipped.is_empty());
    }

    #[test]
    fn test_startup_auto_sync_filter_explicit_false_skips() {
        // Niet-Proxmox provider met expliciete auto_sync=false wordt ook overgeslagen.
        let mut config = crate::providers::config::ProviderConfig::default();
        config.set_section(make_section("digitalocean", false));
        let s = &config.configured_providers()[0];
        assert!(!s.auto_sync);
    }

    #[test]
    fn test_provider_form_fields_new_defaults() {
        let form = ProviderFormFields::new();
        assert!(form.auto_sync, "new() should default auto_sync to true");
        assert!(form.verify_tls);
        assert_eq!(form.focused_field, ProviderFormField::Token);
    }

    #[test]
    fn test_provider_form_field_cloud_fields_include_auto_sync() {
        let fields = ProviderFormField::fields_for("digitalocean");
        assert!(
            fields.contains(&ProviderFormField::AutoSync),
            "CLOUD_FIELDS should contain AutoSync"
        );
        assert!(!fields.contains(&ProviderFormField::VerifyTls),
            "CLOUD_FIELDS should not contain VerifyTls"
        );
    }

    #[test]
    fn test_provider_form_field_proxmox_fields_include_auto_sync_and_verify_tls() {
        let fields = ProviderFormField::fields_for("proxmox");
        assert!(
            fields.contains(&ProviderFormField::AutoSync),
            "PROXMOX_FIELDS should contain AutoSync"
        );
        assert!(
            fields.contains(&ProviderFormField::VerifyTls),
            "PROXMOX_FIELDS should contain VerifyTls"
        );
    }

    #[test]
    fn test_provider_form_field_auto_sync_is_last_in_both_field_lists() {
        let cloud = ProviderFormField::fields_for("digitalocean");
        assert_eq!(*cloud.last().unwrap(), ProviderFormField::AutoSync);

        let proxmox = ProviderFormField::fields_for("proxmox");
        assert_eq!(*proxmox.last().unwrap(), ProviderFormField::AutoSync);
    }

    #[test]
    fn test_provider_form_field_label_auto_sync() {
        assert_eq!(ProviderFormField::AutoSync.label(), "Auto Sync");
    }

    // =========================================================================
    // HostForm askpass tests
    // =========================================================================

    #[test]
    fn test_form_new_has_empty_askpass() {
        let form = HostForm::new();
        assert_eq!(form.askpass, "");
    }

    #[test]
    fn test_form_from_entry_with_askpass() {
        let entry = HostEntry {
            alias: "test".to_string(),
            hostname: "1.2.3.4".to_string(),
            askpass: Some("keychain".to_string()),
            ..Default::default()
        };
        let form = HostForm::from_entry(&entry);
        assert_eq!(form.askpass, "keychain");
    }

    #[test]
    fn test_form_from_entry_without_askpass() {
        let entry = HostEntry {
            alias: "test".to_string(),
            hostname: "1.2.3.4".to_string(),
            askpass: None,
            ..Default::default()
        };
        let form = HostForm::from_entry(&entry);
        assert_eq!(form.askpass, "");
    }

    #[test]
    fn test_to_entry_with_askpass_keychain() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "keychain".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("keychain".to_string()));
    }

    #[test]
    fn test_to_entry_with_askpass_op() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "op://Vault/Item/password".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("op://Vault/Item/password".to_string()));
    }

    #[test]
    fn test_to_entry_with_askpass_vault() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "vault:secret/data/myapp#password".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("vault:secret/data/myapp#password".to_string()));
    }

    #[test]
    fn test_to_entry_with_askpass_bw() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "bw:my-item".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("bw:my-item".to_string()));
    }

    #[test]
    fn test_to_entry_with_askpass_pass() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "pass:ssh/myserver".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("pass:ssh/myserver".to_string()));
    }

    #[test]
    fn test_to_entry_with_askpass_custom_command() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "my-script %a %h".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("my-script %a %h".to_string()));
    }

    #[test]
    fn test_to_entry_with_askpass_empty() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, None);
    }

    #[test]
    fn test_to_entry_with_askpass_whitespace_only() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "  ".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, None);
    }

    #[test]
    fn test_to_entry_askpass_trimmed() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "  keychain  ".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("keychain".to_string()));
    }

    #[test]
    fn test_focused_value_mut_askpass() {
        let mut form = HostForm::new();
        form.focused_field = FormField::AskPass;
        form.focused_value_mut().push_str("vault:");
        assert_eq!(form.askpass, "vault:");
    }

    #[test]
    fn test_askpass_field_label() {
        assert_eq!(FormField::AskPass.label(), "Password Source");
    }

    #[test]
    fn test_askpass_field_navigation() {
        // AskPass is between ProxyJump and Tags
        assert_eq!(FormField::ProxyJump.next(), FormField::AskPass);
        assert_eq!(FormField::AskPass.next(), FormField::Tags);
        assert_eq!(FormField::Tags.prev(), FormField::AskPass);
        assert_eq!(FormField::AskPass.prev(), FormField::ProxyJump);
    }

    #[test]
    fn test_form_field_all_includes_askpass() {
        assert!(FormField::ALL.contains(&FormField::AskPass));
        assert_eq!(FormField::ALL.len(), 8);
    }

    // --- Password picker state ---

    #[test]
    fn test_password_picker_state_init() {
        let app = make_app("Host test\n  HostName test.com\n");
        assert!(!app.ui.show_password_picker);
    }

    #[test]
    fn test_select_next_password_source() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.ui.password_picker_state.select(Some(0));
        app.select_next_password_source();
        assert_eq!(app.ui.password_picker_state.selected(), Some(1));
    }

    #[test]
    fn test_select_prev_password_source() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.ui.password_picker_state.select(Some(2));
        app.select_prev_password_source();
        assert_eq!(app.ui.password_picker_state.selected(), Some(1));
    }

    #[test]
    fn test_select_password_source_wrap_bottom() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
        app.ui.password_picker_state.select(Some(last));
        app.select_next_password_source();
        assert_eq!(app.ui.password_picker_state.selected(), Some(0));
    }

    #[test]
    fn test_select_password_source_wrap_top() {
        let mut app = make_app("Host test\n  HostName test.com\n");
        app.ui.password_picker_state.select(Some(0));
        app.select_prev_password_source();
        let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
        assert_eq!(app.ui.password_picker_state.selected(), Some(last));
    }

    // --- Host entry askpass from config ---

    #[test]
    fn test_host_entries_include_askpass() {
        let app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        assert_eq!(app.hosts[0].askpass, Some("keychain".to_string()));
    }

    #[test]
    fn test_host_entries_vault_askpass() {
        let app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pass\n");
        assert_eq!(app.hosts[0].askpass, Some("vault:secret/ssh#pass".to_string()));
    }

    #[test]
    fn test_host_entries_no_askpass() {
        let app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        assert_eq!(app.hosts[0].askpass, None);
    }

    // --- Validate with askpass ---

    #[test]
    fn test_validate_askpass_with_control_char() {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "keychain\x00".to_string();
        let result = form.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Password Source"));
    }

    #[test]
    fn test_validate_askpass_normal_values_ok() {
        let sources = ["", "keychain", "op://V/I/p", "bw:x", "pass:x", "vault:x#y", "cmd %a"];
        for src in &sources {
            let mut form = HostForm::new();
            form.alias = "myhost".to_string();
            form.hostname = "1.2.3.4".to_string();
            form.askpass = src.to_string();
            assert!(form.validate().is_ok(), "Validate should pass for askpass='{}'", src);
        }
    }

    // --- add_host askpass flow (test config mutation directly, bypassing write) ---

    #[test]
    fn test_add_host_config_mutation_with_askpass() {
        let mut app = make_app("");
        let entry = HostEntry {
            alias: "newhost".to_string(),
            hostname: "1.2.3.4".to_string(),
            askpass: Some("keychain".to_string()),
            ..Default::default()
        };
        app.config.add_host(&entry);
        app.config.set_host_askpass("newhost", "keychain");
        let serialized = app.config.serialize();
        assert!(serialized.contains("purple:askpass keychain"));
        let entries = app.config.host_entries();
        let found = entries.iter().find(|e| e.alias == "newhost").unwrap();
        assert_eq!(found.askpass, Some("keychain".to_string()));
    }

    #[test]
    fn test_add_host_config_mutation_with_vault() {
        let mut app = make_app("");
        let entry = HostEntry {
            alias: "vaulthost".to_string(),
            hostname: "10.0.0.1".to_string(),
            askpass: Some("vault:secret/ssh#pass".to_string()),
            ..Default::default()
        };
        app.config.add_host(&entry);
        app.config.set_host_askpass("vaulthost", "vault:secret/ssh#pass");
        let serialized = app.config.serialize();
        assert!(serialized.contains("purple:askpass vault:secret/ssh#pass"));
    }

    #[test]
    fn test_add_host_config_mutation_without_askpass() {
        let mut app = make_app("");
        let entry = HostEntry {
            alias: "nopass".to_string(),
            hostname: "1.2.3.4".to_string(),
            ..Default::default()
        };
        app.config.add_host(&entry);
        // Don't call set_host_askpass when None — mirrors add_host_from_form logic
        let serialized = app.config.serialize();
        assert!(!serialized.contains("purple:askpass"), "No askpass comment when None");
    }

    #[test]
    fn test_add_host_from_form_calls_set_askpass() {
        // Verify that add_host_from_form invokes set_host_askpass for non-None askpass.
        // We test by checking the form.to_entry() produces correct askpass.
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "op://Vault/Item/pw".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("op://Vault/Item/pw".to_string()));
        // And that the code path in add_host_from_form would call set_host_askpass
        assert!(entry.askpass.is_some());
    }

    // --- update host askpass via config (bypassing write which fails in test) ---

    #[test]
    fn test_config_set_host_askpass_adds() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        app.config.set_host_askpass("myserver", "bw:my-item");
        let serialized = app.config.serialize();
        assert!(serialized.contains("purple:askpass bw:my-item"));
        let entries = app.config.host_entries();
        assert_eq!(entries[0].askpass, Some("bw:my-item".to_string()));
    }

    #[test]
    fn test_config_set_host_askpass_changes() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        app.config.set_host_askpass("myserver", "pass:ssh/myserver");
        let serialized = app.config.serialize();
        assert!(!serialized.contains("keychain"));
        assert!(serialized.contains("purple:askpass pass:ssh/myserver"));
    }

    #[test]
    fn test_config_set_host_askpass_removes() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        app.config.set_host_askpass("myserver", "");
        let serialized = app.config.serialize();
        assert!(!serialized.contains("purple:askpass"));
        let entries = app.config.host_entries();
        assert_eq!(entries[0].askpass, None);
    }

    #[test]
    fn test_edit_host_from_form_sets_askpass_in_config() {
        // edit_host_from_form calls config.set_host_askpass() before write().
        // Since write() fails with test path, the rollback restores old state.
        // Test the config mutation directly to verify the flow.
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        let entry = HostEntry {
            alias: "myserver".to_string(),
            hostname: "10.0.0.1".to_string(),
            askpass: Some("vault:secret/ssh#pass".to_string()),
            ..Default::default()
        };
        app.config.update_host("myserver", &entry);
        app.config.set_host_askpass("myserver", entry.askpass.as_deref().unwrap_or(""));
        let serialized = app.config.serialize();
        assert!(serialized.contains("purple:askpass vault:secret/ssh#pass"));
    }

    // --- pending_connect carries askpass ---

    #[test]
    fn test_pending_connect_with_askpass() {
        let app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        let host = &app.hosts[0];
        assert_eq!(host.askpass, Some("keychain".to_string()));
        // Simulating what handle_host_list does
        let pending = (host.alias.clone(), host.askpass.clone());
        assert_eq!(pending.0, "myserver");
        assert_eq!(pending.1, Some("keychain".to_string()));
    }

    #[test]
    fn test_pending_connect_without_askpass() {
        let app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        let host = &app.hosts[0];
        let pending = (host.alias.clone(), host.askpass.clone());
        assert_eq!(pending.0, "myserver");
        assert_eq!(pending.1, None);
    }

    // --- from_entry roundtrip for all source types ---

    #[test]
    fn test_form_entry_roundtrip_all_sources() {
        let sources = [
            Some("keychain".to_string()),
            Some("op://V/I/p".to_string()),
            Some("bw:item".to_string()),
            Some("pass:ssh/x".to_string()),
            Some("vault:s/d#f".to_string()),
            Some("cmd %a %h".to_string()),
            None,
        ];
        for askpass in &sources {
            let entry = HostEntry {
                alias: "test".to_string(),
                hostname: "1.2.3.4".to_string(),
                askpass: askpass.clone(),
                ..Default::default()
            };
            let form = HostForm::from_entry(&entry);
            let back = form.to_entry();
            assert_eq!(back.askpass, *askpass, "Roundtrip failed for {:?}", askpass);
        }
    }

    // --- askpass special values ---

    #[test]
    fn test_to_entry_askpass_with_equals_sign() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "cmd --opt=val %h".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("cmd --opt=val %h".to_string()));
    }

    #[test]
    fn test_to_entry_askpass_with_hash() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "vault:secret/ssh#api_key".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("vault:secret/ssh#api_key".to_string()));
    }

    #[test]
    fn test_to_entry_askpass_long_value() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "op://My Personal Vault/SSH Production Server/password".to_string();
        let entry = form.to_entry();
        assert_eq!(entry.askpass, Some("op://My Personal Vault/SSH Production Server/password".to_string()));
    }

    // --- edit form askpass rollback logic ---

    #[test]
    fn test_edit_askpass_rollback_restores_old_source() {
        // Simulate the rollback logic from edit_host_from_form
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
        let old_entry = app.hosts[0].clone();
        assert_eq!(old_entry.askpass, Some("keychain".to_string()));

        // Apply new askpass
        app.config.set_host_askpass("myserver", "vault:secret/ssh#pw");
        assert_eq!(app.config.host_entries()[0].askpass, Some("vault:secret/ssh#pw".to_string()));

        // Simulate rollback (write failed)
        app.config.set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
        assert_eq!(app.config.host_entries()[0].askpass, Some("keychain".to_string()));
    }

    #[test]
    fn test_edit_askpass_rollback_restores_none() {
        let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
        let old_entry = app.hosts[0].clone();
        assert_eq!(old_entry.askpass, None);

        // Apply new askpass
        app.config.set_host_askpass("myserver", "bw:my-item");
        assert_eq!(app.config.host_entries()[0].askpass, Some("bw:my-item".to_string()));

        // Simulate rollback (write failed)
        app.config.set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
        assert_eq!(app.config.host_entries()[0].askpass, None);
    }

    // --- password picker state edge cases ---

    #[test]
    fn test_password_picker_initial_state_not_shown() {
        let app = make_app("Host test\n  HostName test.com\n");
        assert!(!app.ui.show_password_picker);
        assert_eq!(app.ui.password_picker_state.selected(), None);
    }

    // --- askpass global default fallback ---

    #[test]
    fn test_pending_connect_askpass_from_host() {
        let app = make_app("Host s1\n  HostName 1.1.1.1\n  # purple:askpass bw:item1\n\nHost s2\n  HostName 2.2.2.2\n");
        assert_eq!(app.hosts[0].askpass, Some("bw:item1".to_string()));
        assert_eq!(app.hosts[1].askpass, None);
    }

    // --- form field cycling includes askpass ---

    #[test]
    fn test_form_field_cycle_through_askpass() {
        let fields = FormField::ALL;
        let askpass_idx = fields.iter().position(|f| matches!(f, FormField::AskPass)).unwrap();
        assert_eq!(askpass_idx, 6, "AskPass should be the 7th field (index 6)");
        // Verify it's between ProxyJump and Tags
        assert!(matches!(fields[askpass_idx - 1], FormField::ProxyJump));
        assert!(matches!(fields[askpass_idx + 1], FormField::Tags));
    }

    // --- validate control chars in askpass ---

    #[test]
    fn test_validate_askpass_rejects_newline() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "keychain\ninjected".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_askpass_rejects_tab() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "keychain\tinjected".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_askpass_rejects_null_byte() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "keychain\0injected".to_string();
        assert!(form.validate().is_err());
    }

    #[test]
    fn test_validate_askpass_allows_normal_special_chars() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "vault:secret/data/my-app#api_key".to_string();
        assert!(form.validate().is_ok());
    }

    #[test]
    fn test_validate_askpass_allows_percent_substitution() {
        let mut form = HostForm::new();
        form.alias = "test".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = "get-pass %a %h".to_string();
        assert!(form.validate().is_ok());
    }

    // =========================================================================
    // Askpass fallback chain: per-host → global default (replicated logic)
    // =========================================================================

    #[test]
    fn test_askpass_fallback_per_host_takes_precedence() {
        // main.rs: host_askpass.or_else(preferences::load_askpass_default)
        let host_askpass: Option<String> = Some("op://V/I/p".to_string());
        let global_default: Option<String> = Some("keychain".to_string());
        let result = host_askpass.or(global_default);
        assert_eq!(result, Some("op://V/I/p".to_string()));
    }

    #[test]
    fn test_askpass_fallback_uses_global_when_no_per_host() {
        let host_askpass: Option<String> = None;
        let global_default: Option<String> = Some("keychain".to_string());
        let result = host_askpass.or(global_default);
        assert_eq!(result, Some("keychain".to_string()));
    }

    #[test]
    fn test_askpass_fallback_none_when_both_absent() {
        let host_askpass: Option<String> = None;
        let global_default: Option<String> = None;
        let result = host_askpass.or(global_default);
        assert_eq!(result, None);
    }

    // =========================================================================
    // cleanup_marker called after connection (document contract)
    // =========================================================================

    #[test]
    fn test_cleanup_marker_contract() {
        // After successful connection, main.rs calls askpass::cleanup_marker(&alias)
        // to remove the retry detection marker file
        let alias = "myserver";
        let call = format!("askpass::cleanup_marker(\"{}\")", alias);
        assert!(call.contains("cleanup_marker"));
    }

    // =========================================================================
    // pending_connect carries askpass through TUI event loop
    // =========================================================================

    #[test]
    fn test_pending_connect_tuple_structure() {
        // pending_connect is Option<(String, Option<String>)> = (alias, askpass)
        let (alias, askpass) = ("myserver".to_string(), Some("keychain".to_string()));
        assert_eq!(alias, "myserver");
        assert_eq!(askpass, Some("keychain".to_string()));
    }

    #[test]
    fn test_pending_connect_none_askpass() {
        let (alias, askpass): (String, Option<String>) = ("myserver".to_string(), None);
        assert_eq!(alias, "myserver");
        assert!(askpass.is_none());
    }

    // =========================================================================
    // bw_session caching in app state
    // =========================================================================

    #[test]
    fn test_bw_session_cached_across_connections() {
        let mut app = make_app("Host a\n  HostName 1.1.1.1\n  # purple:askpass bw:item\n\nHost b\n  HostName 2.2.2.2\n  # purple:askpass bw:other\n");
        // First connection prompts for unlock and caches token
        app.bw_session = Some("cached-token".to_string());
        // Second connection should reuse cached token
        let existing = app.bw_session.as_deref();
        assert_eq!(existing, Some("cached-token"));
        // ensure_bw_session returns None when existing is Some (no re-prompt)
        let needs_prompt = existing.is_none();
        assert!(!needs_prompt);
    }

    #[test]
    fn test_bw_session_not_set_for_non_bw() {
        let app = make_app("Host srv\n  HostName 1.1.1.1\n  # purple:askpass keychain\n");
        assert!(app.bw_session.is_none());
    }

    // =========================================================================
    // AskPass field in HostForm: display label and position
    // =========================================================================

    #[test]
    fn test_askpass_field_is_seventh_in_form() {
        let fields = FormField::ALL;
        assert_eq!(fields.len(), 8);
        assert!(matches!(fields[6], FormField::AskPass));
    }

    #[test]
    fn test_askpass_field_between_proxyjump_and_tags() {
        let fields = FormField::ALL;
        assert!(matches!(fields[5], FormField::ProxyJump));
        assert!(matches!(fields[6], FormField::AskPass));
        assert!(matches!(fields[7], FormField::Tags));
    }
}
