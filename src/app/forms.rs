//! Form state machines for host/provider/tunnel/snippet editing screens.
//!
//! Each form type owns its own field enum (for focus tracking), validation
//! rules, and cursor management. Forms are passive state — the handler module
//! drives them via key events and the ui module renders them.

use std::collections::HashMap;

use crate::ssh_config::model::{HostEntry, PatternEntry};
use crate::tunnel::{TunnelRule, TunnelType};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FormField {
    Alias,
    Hostname,
    User,
    Port,
    IdentityFile,
    ProxyJump,
    AskPass,
    VaultSsh,
    VaultAddr,
    Tags,
}

impl FormField {
    pub const ALL: [FormField; 10] = [
        FormField::Alias,
        FormField::Hostname,
        FormField::User,
        FormField::Port,
        FormField::IdentityFile,
        FormField::VaultSsh,
        FormField::VaultAddr,
        FormField::ProxyJump,
        FormField::AskPass,
        FormField::Tags,
    ];

    /// Walk the raw `ALL` array forward, ignoring visibility. Used only by
    /// tests that assert the schema order of the enum. Production code
    /// navigates via `HostForm::focus_next_visible` which respects the
    /// progressive-disclosure filter for `VaultAddr`.
    #[cfg(test)]
    pub fn next(self) -> Self {
        let idx = FormField::ALL.iter().position(|f| *f == self).unwrap_or(0);
        FormField::ALL[(idx + 1) % FormField::ALL.len()]
    }

    /// Walk the raw `ALL` array backward, test-only counterpart of `next()`.
    #[cfg(test)]
    pub fn prev(self) -> Self {
        let idx = FormField::ALL.iter().position(|f| *f == self).unwrap_or(0);
        FormField::ALL[(idx + FormField::ALL.len() - 1) % FormField::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            FormField::Alias => "Name",
            FormField::Hostname => "Host / IP",
            FormField::User => "User",
            FormField::Port => "Port",
            FormField::IdentityFile => "Identity File",
            FormField::ProxyJump => "ProxyJump",
            FormField::AskPass => "Password Source",
            FormField::VaultSsh => "Vault SSH Role",
            FormField::VaultAddr => "Vault SSH Address",
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
    pub vault_ssh: String,
    /// Optional VAULT_ADDR override. Progressive disclosure: the form field
    /// only renders and is only navigable when `vault_ssh` is non-empty. The
    /// stored value is preserved while hidden so re-enabling the role shows
    /// the previous address again.
    pub vault_addr: String,
    pub tags: String,
    pub focused_field: FormField,
    pub cursor_pos: usize,
    /// Real-time validation hint shown in footer.
    pub form_hint: Option<String>,
    /// When true, alias is a Host pattern (wildcards allowed, hostname optional).
    pub is_pattern: bool,
    /// Progressive disclosure: false = only required fields visible, true = all.
    /// Excluded from dirty detection (UI-only state).
    pub expanded: bool,
    /// Inherited values from matching patterns (value, source pattern).
    /// Shown as dimmed placeholders when the field is empty.
    pub inherited: crate::ssh_config::model::InheritedHints,
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
            vault_ssh: String::new(),
            vault_addr: String::new(),
            tags: String::new(),
            focused_field: FormField::Alias,
            cursor_pos: 0,
            form_hint: None,
            is_pattern: false,
            expanded: false,
            inherited: Default::default(),
        }
    }

    pub fn new_pattern() -> Self {
        Self {
            is_pattern: true,
            expanded: true,
            ..Self::new()
        }
    }

    /// Create form from a raw HostEntry (without pattern inheritance applied).
    /// The `inherited` hints are computed separately and passed in.
    pub fn from_entry(
        entry: &HostEntry,
        inherited: crate::ssh_config::model::InheritedHints,
    ) -> Self {
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
            vault_ssh: entry.vault_ssh.clone().unwrap_or_default(),
            vault_addr: entry.vault_addr.clone().unwrap_or_default(),
            tags: entry.tags.join(", "),
            focused_field: FormField::Alias,
            cursor_pos,
            form_hint: None,
            is_pattern: false,
            expanded: true,
            inherited,
        }
    }

    /// Create a HostForm from an existing host for the clone/duplicate flow.
    /// Clears fields that must not carry over to the copy: `vault_ssh` (the
    /// per-host Vault SSH override, which belongs to the original alias's
    /// certificate) and implicitly `certificate_file` (not stored on the form
    /// since it is derived from the alias). The caller is still responsible
    /// for setting a unique alias on the returned form.
    /// Returns the cloned form and a flag indicating whether a per-host
    /// Vault SSH override was cleared. Callers display a status when the
    /// flag is true so the user understands why the clone does not inherit
    /// the source host's Vault SSH role.
    pub fn from_entry_duplicate(
        entry: &HostEntry,
        inherited: crate::ssh_config::model::InheritedHints,
    ) -> (Self, bool) {
        let mut form = Self::from_entry(entry, inherited);
        let had_vault_ssh = !form.vault_ssh.is_empty();
        form.vault_ssh.clear();
        // The Vault address is conceptually part of the Vault SSH setup: if
        // we clear the role for the clone (because it belongs to the original
        // alias's certificate), the address must be cleared too so the user
        // explicitly re-enters it when enabling Vault SSH on the copy.
        form.vault_addr.clear();
        (form, had_vault_ssh)
    }

    pub fn from_pattern_entry(entry: &PatternEntry) -> Self {
        let alias = entry.pattern.clone();
        let cursor_pos = alias.chars().count();
        Self {
            alias,
            hostname: entry.hostname.clone(),
            user: entry.user.clone(),
            port: entry.port.to_string(),
            identity_file: entry.identity_file.clone(),
            proxy_jump: entry.proxy_jump.clone(),
            askpass: entry.askpass.clone().unwrap_or_default(),
            vault_ssh: String::new(),
            vault_addr: String::new(),
            tags: entry.tags.join(", "),
            focused_field: FormField::Alias,
            cursor_pos,
            form_hint: None,
            is_pattern: true,
            expanded: true,
            inherited: Default::default(),
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
            FormField::VaultSsh => &self.vault_ssh,
            FormField::VaultAddr => &self.vault_addr,
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
            FormField::VaultSsh => &mut self.vault_ssh,
            FormField::VaultAddr => &mut self.vault_addr,
            FormField::Tags => &mut self.tags,
        }
    }

    /// Returns the fields that are currently visible in the rendered form.
    ///
    /// `FormField::VaultAddr` is hidden (absent) unless the Vault SSH role
    /// field has a non-empty value on the same form. Navigation helpers must
    /// consult this list so Tab/Shift-Tab skip over hidden fields. The stored
    /// `vault_addr` value survives hiding, so toggling the role back on
    /// restores the previous input.
    pub fn visible_fields(&self) -> Vec<FormField> {
        let role_set = !self.vault_ssh.trim().is_empty();
        FormField::ALL
            .iter()
            .copied()
            .filter(|f| *f != FormField::VaultAddr || role_set)
            .collect()
    }

    /// Advance `focused_field` to the next visible field (wrapping).
    ///
    /// When the currently focused field is NOT in the visible set (e.g. the
    /// user toggled the role off while focused on VaultAddr, which the UI
    /// does not currently allow but defensive code must handle), fall back
    /// to the first visible field instead of silently skipping to index 1.
    pub fn focus_next_visible(&mut self) {
        let visible = self.visible_fields();
        if visible.is_empty() {
            return;
        }
        self.focused_field = match visible.iter().position(|f| *f == self.focused_field) {
            Some(idx) => visible[(idx + 1) % visible.len()],
            None => visible[0],
        };
    }

    /// Advance `focused_field` to the previous visible field (wrapping).
    ///
    /// Same fallback policy as `focus_next_visible`: an out-of-set focus
    /// state snaps to the last visible field, not the second-to-last.
    pub fn focus_prev_visible(&mut self) {
        let visible = self.visible_fields();
        if visible.is_empty() {
            return;
        }
        self.focused_field = match visible.iter().position(|f| *f == self.focused_field) {
            Some(idx) => visible[(idx + visible.len() - 1) % visible.len()],
            None => visible[visible.len() - 1],
        };
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

    /// Run lightweight validation on the focused field and update `form_hint`.
    pub fn update_hint(&mut self) {
        self.form_hint = match self.focused_field {
            FormField::Alias => {
                let v = self.alias.trim();
                if v.is_empty() {
                    None // Don't nag while empty (user may not have typed yet)
                } else if self.is_pattern {
                    if !crate::ssh_config::model::is_host_pattern(v) {
                        Some("Pattern needs a wildcard (*, ?, [) or multiple hosts".into())
                    } else {
                        None
                    }
                } else if v.contains(char::is_whitespace) {
                    Some("Alias can't contain whitespace".into())
                } else if v.contains('#') {
                    Some("Alias can't contain '#'".into())
                } else if crate::ssh_config::model::is_host_pattern(v) {
                    Some("Alias can't contain pattern characters".into())
                } else {
                    None
                }
            }
            FormField::Hostname => {
                let v = self.hostname.trim();
                if !v.is_empty() && v.contains(char::is_whitespace) {
                    Some("Hostname can't contain whitespace".into())
                } else {
                    None
                }
            }
            FormField::User => {
                let v = self.user.trim();
                if !v.is_empty() && v.contains(char::is_whitespace) {
                    Some("User can't contain whitespace".into())
                } else {
                    None
                }
            }
            FormField::Port => {
                let v = &self.port;
                if v.is_empty() || v == "22" {
                    None
                } else {
                    match v.parse::<u16>() {
                        Ok(0) => Some("Port must be 1-65535".into()),
                        Err(_) => Some("Not a valid port number".into()),
                        _ => None,
                    }
                }
            }
            _ => None,
        };
    }

    /// Validate the form. Returns an error message if invalid.
    pub fn validate(&self) -> Result<(), String> {
        let alias = self.alias.trim();
        if alias.is_empty() {
            return Err(if self.is_pattern {
                "Pattern can't be empty.".to_string()
            } else {
                "Alias can't be empty. Every host needs a name!".to_string()
            });
        }
        if self.is_pattern && !crate::ssh_config::model::is_host_pattern(alias) {
            return Err("Pattern needs a wildcard (*, ?, [) or multiple hosts.".to_string());
        } else if !self.is_pattern {
            if alias.contains(char::is_whitespace) {
                return Err("Alias can't contain whitespace. Keep it simple.".to_string());
            }
            if alias.contains('#') {
                return Err(
                    "Alias can't contain '#'. That's a comment character in SSH config."
                        .to_string(),
                );
            }
            // Catches *, ?, [, ! — whitespace overlap with the check above is intentional
            // (user gets the more specific whitespace message first)
            if crate::ssh_config::model::is_host_pattern(alias) {
                return Err(
                    "Alias can't contain pattern characters. That creates a match pattern, not a host."
                        .to_string(),
                );
            }
        }
        // Reject control characters in all fields
        let fields = [
            (
                &self.alias,
                if self.is_pattern { "Pattern" } else { "Alias" },
            ),
            (&self.hostname, "Hostname"),
            (&self.user, "User"),
            (&self.port, "Port"),
            (&self.identity_file, "Identity File"),
            (&self.proxy_jump, "ProxyJump"),
            (&self.askpass, "Password Source"),
            (&self.vault_ssh, "Vault SSH Role"),
            (&self.vault_addr, "Vault SSH Address"),
            (&self.tags, "Tags"),
        ];
        for (value, name) in &fields {
            if value.chars().any(|c| c.is_control()) {
                return Err(format!(
                    "{} contains control characters. That's not going to work.",
                    name
                ));
            }
        }
        if !self.is_pattern && self.hostname.trim().is_empty() {
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
        let vault_role = self.vault_ssh.trim();
        if !vault_role.is_empty() && !crate::vault_ssh::is_valid_role(vault_role) {
            return Err(
                "Vault SSH role: only letters, digits, /, _ and - are allowed \
                 (e.g. ssh-client-signer/sign/my-role)."
                    .to_string(),
            );
        }
        // vault_addr is only meaningful when a vault role is set. If the
        // user typed an address but then cleared the role we treat it as
        // leftover state and do not validate it (the submit path will not
        // persist it either, since visible_fields filters it out).
        if !vault_role.is_empty() {
            let addr = self.vault_addr.trim();
            if !addr.is_empty() && !crate::vault_ssh::is_valid_vault_addr(addr) {
                return Err("Vault SSH address: must be a non-empty URL without spaces \
                     or control characters (e.g. http://127.0.0.1:8200)."
                    .to_string());
            }
        }
        Ok(())
    }

    /// Convert to a HostEntry.
    pub fn to_entry(&self) -> HostEntry {
        let askpass_trimmed = self.askpass.trim().to_string();
        let vault_ssh_trimmed = self.vault_ssh.trim().to_string();
        // Drop vault_addr when the role is empty: without a role the address
        // has no effect, and persisting leftover state would be confusing.
        let vault_addr_trimmed = if vault_ssh_trimmed.is_empty() {
            String::new()
        } else {
            self.vault_addr.trim().to_string()
        };
        HostEntry {
            alias: self.alias.trim().to_string(),
            hostname: self.hostname.trim().to_string(),
            user: self.user.trim().to_string(),
            port: self.port.parse().unwrap_or(22),
            identity_file: self.identity_file.trim().to_string(),
            proxy_jump: self.proxy_jump.trim().to_string(),
            tags: self
                .tags
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect(),
            askpass: if askpass_trimmed.is_empty() {
                None
            } else {
                Some(askpass_trimmed)
            },
            vault_ssh: if vault_ssh_trimmed.is_empty() {
                None
            } else {
                Some(vault_ssh_trimmed)
            },
            vault_addr: if vault_addr_trimmed.is_empty() {
                None
            } else {
                Some(vault_addr_trimmed)
            },
            ..Default::default()
        }
    }
}

/// Which provider form field is focused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProviderFormField {
    Url,
    Token,
    Profile,
    Project,
    Compartment,
    Regions,
    AliasPrefix,
    User,
    IdentityFile,
    VerifyTls,
    VaultRole,
    VaultAddr,
    AutoSync,
}

impl ProviderFormField {
    const CLOUD_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::VaultRole,
        ProviderFormField::VaultAddr,
        ProviderFormField::AutoSync,
    ];

    const PROXMOX_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Url,
        ProviderFormField::Token,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::VerifyTls,
        ProviderFormField::VaultRole,
        ProviderFormField::VaultAddr,
        ProviderFormField::AutoSync,
    ];

    const AWS_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::Profile,
        ProviderFormField::Regions,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::VaultRole,
        ProviderFormField::VaultAddr,
        ProviderFormField::AutoSync,
    ];

    const SCALEWAY_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::Regions,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::VaultRole,
        ProviderFormField::VaultAddr,
        ProviderFormField::AutoSync,
    ];

    const GCP_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::Project,
        ProviderFormField::Regions,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::VaultRole,
        ProviderFormField::VaultAddr,
        ProviderFormField::AutoSync,
    ];

    const AZURE_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::Regions,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::VaultRole,
        ProviderFormField::VaultAddr,
        ProviderFormField::AutoSync,
    ];

    const ORACLE_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::Compartment,
        ProviderFormField::Regions,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::VaultRole,
        ProviderFormField::VaultAddr,
        ProviderFormField::AutoSync,
    ];

    const OVH_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Token,
        ProviderFormField::Project,
        ProviderFormField::Regions,
        ProviderFormField::AliasPrefix,
        ProviderFormField::User,
        ProviderFormField::IdentityFile,
        ProviderFormField::VaultRole,
        ProviderFormField::VaultAddr,
        ProviderFormField::AutoSync,
    ];

    pub fn fields_for(provider: &str) -> &'static [ProviderFormField] {
        match provider {
            "proxmox" => Self::PROXMOX_FIELDS,
            "aws" => Self::AWS_FIELDS,
            "scaleway" => Self::SCALEWAY_FIELDS,
            "gcp" => Self::GCP_FIELDS,
            "azure" => Self::AZURE_FIELDS,
            "oracle" => Self::ORACLE_FIELDS,
            "ovh" => Self::OVH_FIELDS,
            _ => Self::CLOUD_FIELDS,
        }
    }

    /// Required fields for this provider (always visible in collapsed mode).
    /// Used in tests only; runtime code slices `fields_for()` directly.
    #[cfg(test)]
    pub fn required_fields_for(provider: &str) -> Vec<ProviderFormField> {
        let all = Self::fields_for(provider);
        all.iter()
            .filter(|f| Self::is_required_field(**f, provider))
            .copied()
            .collect()
    }

    /// Optional fields for this provider (shown after expansion).
    /// Used in tests only; runtime code slices `fields_for()` directly.
    #[cfg(test)]
    pub fn optional_fields_for(provider: &str) -> Vec<ProviderFormField> {
        let all = Self::fields_for(provider);
        all.iter()
            .filter(|f| !Self::is_required_field(**f, provider))
            .copied()
            .collect()
    }

    /// Whether a field is mandatory for form submission (asterisk in renderer).
    /// Distinct from `is_required_field` which controls progressive disclosure.
    ///
    /// AWS: Token and Profile both get an asterisk — at least one must be filled
    /// (Token for inline keys, Profile for ~/.aws/credentials).
    /// Tailscale: Token is optional (empty = local CLI mode).
    /// OVH: Regions (= Endpoint) is mandatory (unlike GCP/Oracle where it has
    /// a meaningful default).
    pub fn is_mandatory_field(field: ProviderFormField, provider: &str) -> bool {
        match field {
            ProviderFormField::Url => true,
            ProviderFormField::Token => provider != "tailscale",
            ProviderFormField::Profile => provider == "aws",
            ProviderFormField::Project => matches!(provider, "gcp" | "ovh"),
            ProviderFormField::Compartment => provider == "oracle",
            ProviderFormField::Regions => {
                matches!(provider, "aws" | "scaleway" | "azure" | "ovh")
            }
            _ => false,
        }
    }

    /// Whether a field is shown in collapsed mode (progressive disclosure).
    pub fn is_required_field(field: ProviderFormField, provider: &str) -> bool {
        match field {
            ProviderFormField::Token => true,
            ProviderFormField::Url => provider == "proxmox",
            ProviderFormField::Profile => provider == "aws",
            ProviderFormField::Project => matches!(provider, "gcp" | "ovh"),
            ProviderFormField::Compartment => provider == "oracle",
            ProviderFormField::Regions => {
                matches!(
                    provider,
                    "aws" | "scaleway" | "gcp" | "azure" | "oracle" | "ovh"
                )
            }
            _ => false,
        }
    }

    pub fn next(self, fields: &[Self]) -> Self {
        debug_assert!(
            fields.contains(&self),
            "focused field {:?} not in fields slice",
            self
        );
        let idx = fields.iter().position(|f| *f == self).unwrap_or(0);
        fields[(idx + 1) % fields.len()]
    }

    pub fn prev(self, fields: &[Self]) -> Self {
        debug_assert!(
            fields.contains(&self),
            "focused field {:?} not in fields slice",
            self
        );
        let idx = fields.iter().position(|f| *f == self).unwrap_or(0);
        fields[(idx + fields.len() - 1) % fields.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            ProviderFormField::Url => "URL",
            ProviderFormField::Token => "Token",
            ProviderFormField::Profile => "Profile",
            ProviderFormField::Project => "Project ID",
            ProviderFormField::Compartment => "Compartment",
            ProviderFormField::Regions => "Regions",
            ProviderFormField::AliasPrefix => "Alias Prefix",
            ProviderFormField::User => "User",
            ProviderFormField::IdentityFile => "Identity File",
            ProviderFormField::VerifyTls => "Verify TLS",
            ProviderFormField::VaultRole => "Vault SSH Role",
            ProviderFormField::VaultAddr => "Vault SSH Address",
            ProviderFormField::AutoSync => "Auto Sync",
        }
    }
}

/// Form state for configuring a provider.
#[derive(Debug, Clone)]
pub struct ProviderFormFields {
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
    /// Optional `VAULT_ADDR` override. Empty = inherit parent env. The
    /// rendered input is progressively disclosed: the field is only visible
    /// in the provider form when `vault_role` is non-empty.
    pub vault_addr: String,
    pub focused_field: ProviderFormField,
    pub cursor_pos: usize,
    /// Progressive disclosure: false = required fields only, true = all fields.
    /// Excluded from dirty detection (UI-only state).
    pub expanded: bool,
}

impl ProviderFormFields {
    pub fn new() -> Self {
        Self {
            url: String::new(),
            token: String::new(),
            profile: String::new(),
            project: String::new(),
            compartment: String::new(),
            regions: String::new(),
            alias_prefix: String::new(),
            user: "root".to_string(),
            identity_file: String::new(),
            verify_tls: true,
            auto_sync: true,
            vault_role: String::new(),
            vault_addr: String::new(),
            focused_field: ProviderFormField::Token,
            cursor_pos: 0,
            expanded: false,
        }
    }

    pub fn focused_value(&self) -> &str {
        match self.focused_field {
            ProviderFormField::Url => &self.url,
            ProviderFormField::Token => &self.token,
            ProviderFormField::Profile => &self.profile,
            ProviderFormField::Project => &self.project,
            ProviderFormField::Compartment => &self.compartment,
            ProviderFormField::Regions => &self.regions,
            ProviderFormField::AliasPrefix => &self.alias_prefix,
            ProviderFormField::User => &self.user,
            ProviderFormField::IdentityFile => &self.identity_file,
            ProviderFormField::VaultRole => &self.vault_role,
            ProviderFormField::VaultAddr => &self.vault_addr,
            ProviderFormField::VerifyTls | ProviderFormField::AutoSync => "",
        }
    }

    pub fn focused_value_mut(&mut self) -> Option<&mut String> {
        match self.focused_field {
            ProviderFormField::Url => Some(&mut self.url),
            ProviderFormField::Token => Some(&mut self.token),
            ProviderFormField::Profile => Some(&mut self.profile),
            ProviderFormField::Project => Some(&mut self.project),
            ProviderFormField::Compartment => Some(&mut self.compartment),
            ProviderFormField::Regions => Some(&mut self.regions),
            ProviderFormField::AliasPrefix => Some(&mut self.alias_prefix),
            ProviderFormField::User => Some(&mut self.user),
            ProviderFormField::IdentityFile => Some(&mut self.identity_file),
            ProviderFormField::VaultRole => Some(&mut self.vault_role),
            ProviderFormField::VaultAddr => Some(&mut self.vault_addr),
            ProviderFormField::VerifyTls | ProviderFormField::AutoSync => None,
        }
    }

    /// Filter `fields_for(provider)` to the fields that should actually be
    /// rendered and receive focus. Progressive disclosure: `VaultAddr` is
    /// only visible when `vault_role` is non-empty, mirroring the host form.
    pub fn visible_fields(&self, provider: &str) -> Vec<ProviderFormField> {
        let role_set = !self.vault_role.trim().is_empty();
        ProviderFormField::fields_for(provider)
            .iter()
            .copied()
            .filter(|f| *f != ProviderFormField::VaultAddr || role_set)
            .collect()
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
        self.cursor_pos = self.focused_value().chars().count();
    }
}

pub(crate) fn char_to_byte_pos(s: &str, char_pos: usize) -> usize {
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

/// Which snippet form field is focused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnippetFormField {
    Name,
    Command,
    Description,
}

impl SnippetFormField {
    pub const ALL: &[SnippetFormField] = &[
        SnippetFormField::Name,
        SnippetFormField::Command,
        SnippetFormField::Description,
    ];

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            SnippetFormField::Name => "Name",
            SnippetFormField::Command => "Command",
            SnippetFormField::Description => "Description",
        }
    }
}

/// Form state for adding/editing a snippet.
#[derive(Debug, Clone)]
pub struct SnippetForm {
    pub name: String,
    pub command: String,
    pub description: String,
    pub focused_field: SnippetFormField,
    pub cursor_pos: usize,
}

impl SnippetForm {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            command: String::new(),
            description: String::new(),
            focused_field: SnippetFormField::Name,
            cursor_pos: 0,
        }
    }

    pub fn from_snippet(snippet: &crate::snippet::Snippet) -> Self {
        Self {
            name: snippet.name.clone(),
            command: snippet.command.clone(),
            description: snippet.description.clone(),
            focused_field: SnippetFormField::Name,
            cursor_pos: snippet.name.chars().count(),
        }
    }

    pub fn focused_value(&self) -> &str {
        match self.focused_field {
            SnippetFormField::Name => &self.name,
            SnippetFormField::Command => &self.command,
            SnippetFormField::Description => &self.description,
        }
    }

    pub fn focused_value_mut(&mut self) -> &mut String {
        match self.focused_field {
            SnippetFormField::Name => &mut self.name,
            SnippetFormField::Command => &mut self.command,
            SnippetFormField::Description => &mut self.description,
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

    pub fn validate(&self) -> Result<(), String> {
        crate::snippet::validate_name(&self.name)?;
        crate::snippet::validate_command(&self.command)?;
        if self.description.contains(|c: char| c.is_control()) {
            return Err("Description contains control characters.".to_string());
        }
        Ok(())
    }
}

/// Output from snippet execution, per host.
#[derive(Debug, Clone)]
pub struct SnippetHostOutput {
    pub alias: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

/// State for the snippet output screen.
#[derive(Debug, Clone)]
pub struct SnippetOutputState {
    pub run_id: u64,
    pub results: Vec<SnippetHostOutput>,
    pub scroll_offset: usize,
    pub completed: usize,
    pub total: usize,
    pub all_done: bool,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Form state for snippet parameter input.
#[derive(Debug, Clone)]
pub struct SnippetParamFormState {
    pub params: Vec<crate::snippet::SnippetParam>,
    pub values: Vec<String>,
    pub focused_index: usize,
    pub cursor_pos: usize,
    pub scroll_offset: usize,
    /// How many params actually fit on screen (set by renderer).
    pub visible_count: usize,
}

impl SnippetParamFormState {
    pub fn new(params: &[crate::snippet::SnippetParam]) -> Self {
        let values: Vec<String> = params
            .iter()
            .map(|p| p.default.clone().unwrap_or_default())
            .collect();
        let cursor_pos = values.first().map(|v| v.chars().count()).unwrap_or(0);
        Self {
            params: params.to_vec(),
            values,
            focused_index: 0,
            cursor_pos,
            scroll_offset: 0,
            visible_count: params.len().min(8),
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let idx = self.focused_index;
        let pos = self.cursor_pos;
        let val = &mut self.values[idx];
        let byte_pos = char_to_byte_pos(val, pos);
        val.insert(byte_pos, c);
        self.cursor_pos = pos + 1;
    }

    pub fn delete_char_before_cursor(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let idx = self.focused_index;
        let pos = self.cursor_pos;
        let val = &mut self.values[idx];
        let byte_pos = char_to_byte_pos(val, pos);
        let prev = char_to_byte_pos(val, pos - 1);
        val.drain(prev..byte_pos);
        self.cursor_pos = pos - 1;
    }

    /// Build a map of param name to user-entered value for substitution.
    pub fn values_map(&self) -> HashMap<String, String> {
        self.params
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name.clone(), self.values[i].clone()))
            .collect()
    }

    /// Returns true if any parameter value differs from its default.
    pub fn is_dirty(&self) -> bool {
        self.params.iter().enumerate().any(|(i, p)| {
            let default = p.default.as_deref().unwrap_or("");
            self.values[i] != default
        })
    }
}
