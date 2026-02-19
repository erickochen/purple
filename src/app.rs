use ratatui::widgets::ListState;

use crate::ssh_config::model::{HostEntry, SshConfigFile};

/// Which screen is currently displayed.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    HostList,
    AddHost,
    EditHost { index: usize },
    ConfirmDelete { index: usize },
    Help,
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
            FormField::Hostname => "Hostname",
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

/// Main application state.
pub struct App {
    pub screen: Screen,
    pub running: bool,
    pub config: SshConfigFile,
    pub hosts: Vec<HostEntry>,

    // Host list state
    pub list_state: ListState,

    // Form state
    pub form: HostForm,

    // Status bar
    pub status: Option<StatusMessage>,

    // Pending SSH connection
    pub pending_connect: Option<String>,
}

impl App {
    pub fn new(config: SshConfigFile) -> Self {
        let hosts = config.host_entries();
        let mut list_state = ListState::default();
        if !hosts.is_empty() {
            list_state.select(Some(0));
        }

        Self {
            screen: Screen::HostList,
            running: true,
            config,
            hosts,
            list_state,
            form: HostForm::new(),
            status: None,
            pending_connect: None,
        }
    }

    /// Get the currently selected host index.
    pub fn selected_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    /// Get the currently selected host entry.
    pub fn selected_host(&self) -> Option<&HostEntry> {
        self.list_state.selected().and_then(|i| self.hosts.get(i))
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        if self.hosts.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.hosts.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if self.hosts.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.hosts.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    /// Reload hosts from config.
    pub fn reload_hosts(&mut self) {
        self.hosts = self.config.host_entries();
        // Fix selection if it's out of bounds
        if self.hosts.is_empty() {
            self.list_state.select(None);
        } else if let Some(i) = self.list_state.selected() {
            if i >= self.hosts.len() {
                self.list_state.select(Some(self.hosts.len() - 1));
            }
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
}
