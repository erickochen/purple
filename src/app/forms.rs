//! Form state machines for host/provider/tunnel/snippet editing screens.
//!
//! Each form type owns its own field enum (for focus tracking), validation
//! rules, and cursor management. Forms are passive state — the handler module
//! drives them via key events and the ui module renders them.

use std::collections::HashMap;

use crate::providers::ProviderKind;
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
            FormField::Alias => crate::i18n::text("Name"),
            FormField::Hostname => crate::i18n::text("Host / IP"),
            FormField::User => crate::i18n::text("User"),
            FormField::Port => crate::i18n::text("Port"),
            FormField::IdentityFile => crate::i18n::text("Identity File"),
            FormField::ProxyJump => crate::i18n::text("ProxyJump"),
            FormField::AskPass => crate::i18n::text("Password Source"),
            FormField::VaultSsh => crate::i18n::text("Vault SSH Role"),
            FormField::VaultAddr => crate::i18n::text("Vault SSH Address"),
            FormField::Tags => crate::i18n::text("Tags"),
        }
    }

    /// Whether this field opens a picker overlay when activated with Space.
    ///
    /// Picker fields: IdentityFile, ProxyJump, AskPass, VaultSsh. All other
    /// fields are plain text inputs. The handler dispatches Space to a
    /// picker for these fields and falls through to literal-character
    /// insertion for the rest.
    ///
    /// Note: `VaultSsh` is a picker only when the host's role candidates
    /// list is non-empty. The handler must consult `App::vault_role_candidates`
    /// before opening the picker; with no candidates, Space inserts a
    /// literal space (the user types the role manually).
    pub fn is_picker(self) -> bool {
        matches!(
            self,
            FormField::IdentityFile
                | FormField::ProxyJump
                | FormField::AskPass
                | FormField::VaultSsh
        )
    }

    /// Field kind for [`crate::ui::design::FieldKind`]. Drives dynamic
    /// footer hints in the host form (`Space pick` vs no hint).
    pub fn kind(self) -> crate::ui::design::FieldKind {
        if self.is_picker() {
            crate::ui::design::FieldKind::Picker
        } else {
            crate::ui::design::FieldKind::Text
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
    pub(crate) fn new() -> Self {
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

    /// Absorb a password-source selection from the password picker. Sets
    /// `askpass` to `value`, optionally moves focus to the AskPass field
    /// when the user must type extra characters (custom command or prefix
    /// completion), and parks the cursor at the end of the askpass value
    /// regardless of which field currently has focus.
    pub fn apply_password_source(&mut self, value: String, refocus_askpass: bool) {
        self.askpass = value;
        if refocus_askpass {
            self.focused_field = FormField::AskPass;
        }
        self.cursor_pos = self.askpass.chars().count();
    }

    /// Fill parsed `user@host:port` fields into the form, skipping any
    /// field already at non-default state. Alias is always overwritten
    /// with the caller-supplied short form.
    pub fn apply_smart_paste(
        &mut self,
        parsed: crate::quick_add::ParsedTarget,
        clean_alias: String,
    ) {
        if self.hostname.is_empty() {
            self.hostname = parsed.hostname;
        }
        if self.user.is_empty() && !parsed.user.is_empty() {
            self.user = parsed.user;
        }
        if self.port == "22" && parsed.port != 22 {
            self.port = parsed.port.to_string();
        }
        self.alias = clean_alias;
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
                crate::i18n::text(crate::messages::HOST_PATTERN_EMPTY).to_string()
            } else {
                crate::i18n::text(crate::messages::HOST_ALIAS_EMPTY).to_string()
            });
        }
        if self.is_pattern && !crate::ssh_config::model::is_host_pattern(alias) {
            return Err(
                crate::i18n::text(crate::messages::HOST_PATTERN_NEEDS_WILDCARD).to_string(),
            );
        } else if !self.is_pattern {
            if alias.contains(char::is_whitespace) {
                return Err(crate::i18n::text(crate::messages::HOST_ALIAS_WHITESPACE).to_string());
            }
            if alias.contains('#') {
                return Err(crate::i18n::text(crate::messages::HOST_ALIAS_HASH).to_string());
            }
            // Catches *, ?, [, ! — whitespace overlap with the check above is intentional
            // (user gets the more specific whitespace message first)
            if crate::ssh_config::model::is_host_pattern(alias) {
                return Err(
                    crate::i18n::text(crate::messages::HOST_ALIAS_PATTERN_CHARS).to_string()
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
                return Err(crate::messages::field_control_chars(name));
            }
        }
        if !self.is_pattern && self.hostname.trim().is_empty() {
            return Err(crate::i18n::text(crate::messages::HOST_HOSTNAME_EMPTY).to_string());
        }
        if self.hostname.trim().contains(char::is_whitespace) {
            return Err(crate::i18n::text(crate::messages::HOST_HOSTNAME_WHITESPACE).to_string());
        }
        if self.user.trim().contains(char::is_whitespace) {
            return Err(crate::i18n::text(crate::messages::USER_NO_WHITESPACE).to_string());
        }
        let port: u16 = self
            .port
            .parse()
            .map_err(|_| crate::i18n::text(crate::messages::HOST_PORT_INVALID).to_string())?;
        if port == 0 {
            return Err(crate::i18n::text(crate::messages::HOST_PORT_ZERO).to_string());
        }
        let vault_role = self.vault_ssh.trim();
        if !vault_role.is_empty() && !crate::vault_ssh::is_valid_role(vault_role) {
            return Err(crate::i18n::text(crate::messages::HOST_VAULT_ROLE_INVALID).to_string());
        }
        // vault_addr is only meaningful when a vault role is set. If the
        // user typed an address but then cleared the role we treat it as
        // leftover state and do not validate it (the submit path will not
        // persist it either, since visible_fields filters it out).
        if !vault_role.is_empty() {
            let addr = self.vault_addr.trim();
            if !addr.is_empty() && !crate::vault_ssh::is_valid_vault_addr(addr) {
                return Err(crate::i18n::text(crate::messages::HOST_VAULT_ADDR_INVALID).to_string());
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
///
/// `Label` is dynamic. The static per-provider arrays (`PROXMOX_FIELDS` etc.)
/// never list it; `ProviderFormFields::visible_fields()` prepends it when the
/// form was opened to collect a label for a new labeled config. This keeps
/// `fields_for(provider)` provider-pure and prevents Label from leaking into
/// bare-add or edit flows where the label is fixed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProviderFormField {
    Label,
    Url,
    Token,
    Profile,
    Project,
    Compartment,
    Regions,
    Filter,
    Ssm,
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

    const NETBOX_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::Url,
        ProviderFormField::Token,
        ProviderFormField::Filter,
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
        ProviderFormField::Ssm,
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

    /// Teleport has no token and issues its own certificates, so neither the
    /// Token nor the Vault fields apply. User (the Teleport login) leads so the
    /// collapsed form has a field to show.
    const TELEPORT_FIELDS: &[ProviderFormField] = &[
        ProviderFormField::User,
        ProviderFormField::AliasPrefix,
        ProviderFormField::IdentityFile,
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
        let Ok(kind) = provider.parse::<ProviderKind>() else {
            return Self::CLOUD_FIELDS;
        };
        match kind {
            ProviderKind::Proxmox => Self::PROXMOX_FIELDS,
            ProviderKind::NetBox => Self::NETBOX_FIELDS,
            ProviderKind::Aws => Self::AWS_FIELDS,
            ProviderKind::Scaleway => Self::SCALEWAY_FIELDS,
            ProviderKind::Gcp => Self::GCP_FIELDS,
            ProviderKind::Azure => Self::AZURE_FIELDS,
            ProviderKind::Oracle => Self::ORACLE_FIELDS,
            ProviderKind::Ovh => Self::OVH_FIELDS,
            ProviderKind::Teleport => Self::TELEPORT_FIELDS,
            ProviderKind::DigitalOcean
            | ProviderKind::Hetzner
            | ProviderKind::I3d
            | ProviderKind::Leaseweb
            | ProviderKind::Linode
            | ProviderKind::Tailscale
            | ProviderKind::Transip
            | ProviderKind::UpCloud
            | ProviderKind::Vultr => Self::CLOUD_FIELDS,
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
    /// Token: asterisked unless the provider can resolve credentials elsewhere
    /// (`ProviderKind::token_is_optional`).
    /// Profile: never asterisked. Only AWS shows it. AWS also takes a token or
    /// environment credentials, so no single one of those fields is mandatory.
    /// OVH: Regions (= Endpoint) is mandatory (unlike GCP/Oracle where it has
    /// a meaningful default).
    /// Label: only rendered when the form is in label-entry mode, and always
    /// mandatory there. `validate_label("")` rejects empties at save time.
    pub fn is_mandatory_field(field: ProviderFormField, provider: &str) -> bool {
        let kind = provider.parse::<ProviderKind>().ok();
        match field {
            ProviderFormField::Label => true,
            ProviderFormField::Url => true,
            ProviderFormField::Token => !kind.is_some_and(ProviderKind::token_is_optional),
            ProviderFormField::Profile => false,
            ProviderFormField::Project => kind.is_some_and(ProviderKind::has_project_field),
            ProviderFormField::Compartment => kind == Some(ProviderKind::Oracle),
            ProviderFormField::Regions => {
                kind.is_some_and(ProviderKind::regions_field_is_mandatory)
            }
            _ => false,
        }
    }

    /// Whether a field is shown in collapsed mode (progressive disclosure).
    /// Label is always required when present so the user is forced to fill it
    /// before reaching the optional tail.
    pub fn is_required_field(field: ProviderFormField, provider: &str) -> bool {
        let kind = provider.parse::<ProviderKind>().ok();
        match field {
            ProviderFormField::Label => true,
            ProviderFormField::Token => true,
            ProviderFormField::Url => kind.is_some_and(ProviderKind::requires_url),
            ProviderFormField::Profile => kind == Some(ProviderKind::Aws),
            ProviderFormField::Project => kind.is_some_and(ProviderKind::has_project_field),
            ProviderFormField::Compartment => kind == Some(ProviderKind::Oracle),
            ProviderFormField::Regions => kind.is_some_and(ProviderKind::has_regions_field),
            // Teleport shows the login user up front; every other provider
            // keeps User behind the expand step.
            ProviderFormField::User => kind == Some(ProviderKind::Teleport),
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
            ProviderFormField::Label => crate::i18n::text("Name"),
            ProviderFormField::Url => "URL",
            ProviderFormField::Token => crate::i18n::text("Token"),
            ProviderFormField::Profile => crate::i18n::text("Profile"),
            ProviderFormField::Project => crate::i18n::text("Project ID"),
            ProviderFormField::Compartment => crate::i18n::text("Compartment"),
            ProviderFormField::Regions => crate::i18n::text("Regions"),
            ProviderFormField::Filter => crate::i18n::text("Filter"),
            ProviderFormField::Ssm => crate::i18n::text("Session Manager"),
            ProviderFormField::AliasPrefix => crate::i18n::text("Alias Prefix"),
            ProviderFormField::User => crate::i18n::text("User"),
            ProviderFormField::IdentityFile => crate::i18n::text("Identity File"),
            ProviderFormField::VerifyTls => crate::i18n::text("Verify TLS"),
            ProviderFormField::VaultRole => crate::i18n::text("Vault SSH Role"),
            ProviderFormField::VaultAddr => crate::i18n::text("Vault SSH Address"),
            ProviderFormField::AutoSync => crate::i18n::text("Auto Sync"),
        }
    }

    /// Whether Space activates this field in place instead of typing into it.
    /// Most such fields hold a yes/no and flip; `Ssm` steps through three
    /// values. Same element, same key, same footer.
    pub fn is_toggle(self) -> bool {
        matches!(
            self,
            ProviderFormField::VerifyTls | ProviderFormField::AutoSync | ProviderFormField::Ssm
        )
    }

    /// Whether this field opens a picker overlay when activated with Space.
    ///
    /// `IdentityFile` always opens the SSH key picker. `Regions` opens a
    /// region picker only for providers with structured region lists
    /// (aws/scaleway/gcp/oracle/ovh). Other providers (azure, proxmox, ...)
    /// take Regions as free-form text input, so Space inserts a literal space.
    /// `Profile` lists the profiles found in `~/.aws`, so the name never has
    /// to be typed from memory.
    pub fn is_picker(self, provider: &str) -> bool {
        match self {
            ProviderFormField::IdentityFile => true,
            ProviderFormField::Profile => provider.parse::<ProviderKind>() == Ok(ProviderKind::Aws),
            ProviderFormField::Regions => provider
                .parse::<ProviderKind>()
                .ok()
                .is_some_and(ProviderKind::regions_field_is_picker),
            _ => false,
        }
    }

    /// Field kind for [`crate::ui::design::FieldKind`]. Toggles take precedence
    /// over pickers (no field is both).
    pub fn kind(self, provider: &str) -> crate::ui::design::FieldKind {
        if self.is_toggle() {
            crate::ui::design::FieldKind::Toggle
        } else if self.is_picker(provider) {
            crate::ui::design::FieldKind::Picker
        } else {
            crate::ui::design::FieldKind::Text
        }
    }
}

/// Form state for configuring a provider.
#[derive(Debug, Clone)]
pub struct ProviderFormFields {
    /// Label being entered for a new labeled config. Only visible in the form
    /// when `label_entry` is true (the `_ =>` branch of `open_add_config_flow`).
    /// Migration, bare add, and edit flows keep this empty and `label_entry`
    /// false so the field stays hidden and the label is sourced from `form_id`.
    pub label: String,
    /// Whether the form was opened to collect a label from the user. Drives
    /// `visible_fields` prepending `Label`, the initial focus, and the
    /// submit-time write of `label` back into `form_id.label`.
    pub label_entry: bool,
    pub url: String,
    pub token: String,
    pub profile: String,
    pub project: String,
    pub compartment: String,
    pub regions: String,
    /// Raw NetBox query parameters scoping which objects sync.
    pub filter: String,
    pub alias_prefix: String,
    pub user: String,
    pub identity_file: String,
    pub verify_tls: bool,
    pub auto_sync: bool,
    pub vault_role: String,
    /// Whether synced AWS hosts route through Session Manager.
    pub ssm: crate::providers::aws_ssm::SsmMode,
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
    pub(crate) fn new() -> Self {
        Self {
            label: String::new(),
            label_entry: false,
            url: String::new(),
            token: String::new(),
            profile: String::new(),
            project: String::new(),
            compartment: String::new(),
            regions: String::new(),
            filter: String::new(),
            alias_prefix: String::new(),
            user: "root".to_string(),
            identity_file: String::new(),
            verify_tls: true,
            auto_sync: true,
            vault_role: String::new(),
            ssm: crate::providers::aws_ssm::SsmMode::default(),
            vault_addr: String::new(),
            focused_field: ProviderFormField::Token,
            cursor_pos: 0,
            expanded: false,
        }
    }

    pub fn focused_value(&self) -> &str {
        match self.focused_field {
            ProviderFormField::Label => &self.label,
            ProviderFormField::Url => &self.url,
            ProviderFormField::Token => &self.token,
            ProviderFormField::Profile => &self.profile,
            ProviderFormField::Project => &self.project,
            ProviderFormField::Compartment => &self.compartment,
            ProviderFormField::Regions => &self.regions,
            ProviderFormField::Filter => &self.filter,
            ProviderFormField::AliasPrefix => &self.alias_prefix,
            ProviderFormField::User => &self.user,
            ProviderFormField::IdentityFile => &self.identity_file,
            ProviderFormField::VaultRole => &self.vault_role,
            ProviderFormField::VaultAddr => &self.vault_addr,
            ProviderFormField::Ssm => self.ssm.as_str(),
            ProviderFormField::VerifyTls | ProviderFormField::AutoSync => "",
        }
    }

    pub fn focused_value_mut(&mut self) -> Option<&mut String> {
        match self.focused_field {
            ProviderFormField::Label => Some(&mut self.label),
            ProviderFormField::Url => Some(&mut self.url),
            ProviderFormField::Token => Some(&mut self.token),
            ProviderFormField::Profile => Some(&mut self.profile),
            ProviderFormField::Project => Some(&mut self.project),
            ProviderFormField::Compartment => Some(&mut self.compartment),
            ProviderFormField::Regions => Some(&mut self.regions),
            ProviderFormField::Filter => Some(&mut self.filter),
            ProviderFormField::AliasPrefix => Some(&mut self.alias_prefix),
            ProviderFormField::User => Some(&mut self.user),
            ProviderFormField::IdentityFile => Some(&mut self.identity_file),
            ProviderFormField::VaultRole => Some(&mut self.vault_role),
            ProviderFormField::VaultAddr => Some(&mut self.vault_addr),
            // Ssm steps through a fixed list rather than being typed into.
            ProviderFormField::Ssm | ProviderFormField::VerifyTls | ProviderFormField::AutoSync => {
                None
            }
        }
    }

    /// Filter `fields_for(provider)` to the fields that should actually be
    /// rendered and receive focus. Two dynamic adjustments:
    /// 1. `Label` is prepended when `label_entry` is true so the user is asked
    ///    for a new config name on the third-and-onward add (issue #51).
    /// 2. `VaultAddr` is only visible when `vault_role` is non-empty,
    ///    mirroring the host form's progressive disclosure.
    pub fn visible_fields(&self, provider: &str) -> Vec<ProviderFormField> {
        let role_set = !self.vault_role.trim().is_empty();
        let base = ProviderFormField::fields_for(provider)
            .iter()
            .copied()
            .filter(|f| *f != ProviderFormField::VaultAddr || role_set);
        if self.label_entry {
            std::iter::once(ProviderFormField::Label)
                .chain(base)
                .collect()
        } else {
            base.collect()
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
            TunnelFormField::Type => crate::i18n::text("Type"),
            TunnelFormField::BindPort => crate::i18n::text("Bind Port"),
            TunnelFormField::RemoteHost => crate::i18n::text("Remote Host"),
            TunnelFormField::RemotePort => crate::i18n::text("Remote Port"),
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
    pub(crate) fn new() -> Self {
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

    /// Advance the focused field to the next one (skipping remote fields
    /// when tunnel_type is Dynamic) and sync the cursor to the end of the
    /// new focused value.
    pub fn focus_next(&mut self) {
        self.focused_field = self.focused_field.next(self.tunnel_type);
        self.sync_cursor_to_end();
    }

    /// Retreat the focused field to the previous one (skipping remote
    /// fields when tunnel_type is Dynamic) and sync the cursor to the end
    /// of the new focused value.
    pub fn focus_prev(&mut self) {
        self.focused_field = self.focused_field.prev(self.tunnel_type);
        self.sync_cursor_to_end();
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
                return Err(crate::messages::field_control_chars_short(name));
            }
        }
        let port: u16 = self.bind_port.parse().map_err(|_| {
            crate::i18n::text(crate::messages::TUNNEL_BIND_PORT_INVALID).to_string()
        })?;
        if port == 0 {
            return Err(crate::i18n::text(crate::messages::TUNNEL_BIND_PORT_ZERO).to_string());
        }
        if self.tunnel_type != TunnelType::Dynamic {
            let host = self.remote_host.trim();
            if host.is_empty() {
                return Err(
                    crate::i18n::text(crate::messages::TUNNEL_REMOTE_HOST_EMPTY).to_string()
                );
            }
            if host.contains(char::is_whitespace) {
                return Err(
                    crate::i18n::text(crate::messages::TUNNEL_REMOTE_HOST_SPACES).to_string(),
                );
            }
            let rport: u16 = self.remote_port.parse().map_err(|_| {
                crate::i18n::text(crate::messages::TUNNEL_REMOTE_PORT_INVALID).to_string()
            })?;
            if rport == 0 {
                return Err(crate::i18n::text(crate::messages::TUNNEL_REMOTE_PORT_ZERO).to_string());
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
    /// Interactive shell. Not a text field: Space flips `SnippetForm::interactive`.
    Interactive,
    /// Default target hosts. Not a text field: Space opens the host
    /// multi-select picker; the chosen aliases live in `SnippetForm::default_hosts`.
    DefaultHosts,
}

impl SnippetFormField {
    pub const ALL: &[SnippetFormField] = &[
        SnippetFormField::Name,
        SnippetFormField::Command,
        SnippetFormField::Description,
        SnippetFormField::Interactive,
        SnippetFormField::DefaultHosts,
    ];

    /// True for the picker fields (currently just `DefaultHosts`): Space opens a
    /// picker and the field takes no typed text. Mirrors `FormField::is_picker`.
    pub fn is_picker(self) -> bool {
        matches!(self, SnippetFormField::DefaultHosts)
    }

    /// True for the toggle fields (currently just `Interactive`): Space flips
    /// the value. Mirrors `ProviderFormField::is_toggle`.
    pub fn is_toggle(self) -> bool {
        matches!(self, SnippetFormField::Interactive)
    }

    /// Field kind for [`crate::ui::design::FieldKind`], driving the dynamic
    /// footer hint (`Space toggle`, `Space pick` or none). Mirrors `FormField::kind`.
    pub fn kind(self) -> crate::ui::design::FieldKind {
        if self.is_toggle() {
            crate::ui::design::FieldKind::Toggle
        } else if self.is_picker() {
            crate::ui::design::FieldKind::Picker
        } else {
            crate::ui::design::FieldKind::Text
        }
    }

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
            SnippetFormField::Name => crate::i18n::text("Name"),
            SnippetFormField::Command => crate::i18n::text("Command"),
            SnippetFormField::Description => crate::i18n::text("Description"),
            SnippetFormField::Interactive => crate::i18n::text("Interactive shell"),
            SnippetFormField::DefaultHosts => crate::i18n::text("Default hosts"),
        }
    }
}

/// Form state for adding/editing a snippet.
#[derive(Debug, Clone)]
pub struct SnippetForm {
    pub name: String,
    pub command: String,
    pub description: String,
    pub interactive: bool,
    /// Default target host aliases, chosen via the host picker (the
    /// `DefaultHosts` field). Seeded from the snippet's saved targets on edit;
    /// persisted on submit. Empty means no default hosts.
    pub default_hosts: Vec<String>,
    pub focused_field: SnippetFormField,
    pub cursor_pos: usize,
}

impl SnippetForm {
    pub(crate) fn new() -> Self {
        Self {
            name: String::new(),
            command: String::new(),
            description: String::new(),
            interactive: false,
            default_hosts: Vec::new(),
            focused_field: SnippetFormField::Name,
            cursor_pos: 0,
        }
    }

    pub fn from_snippet(snippet: &crate::snippet::Snippet) -> Self {
        Self {
            name: snippet.name.clone(),
            command: snippet.command.clone(),
            description: snippet.description.clone(),
            interactive: snippet.interactive,
            // The caller seeds default_hosts from the store's saved targets;
            // a Snippet carries no host association of its own.
            default_hosts: Vec::new(),
            focused_field: SnippetFormField::Name,
            cursor_pos: snippet.name.chars().count(),
        }
    }

    /// Advance the focused field to the next one and sync the cursor to
    /// the end of the new focused value.
    pub fn focus_next(&mut self) {
        self.focused_field = self.focused_field.next();
        self.sync_cursor_to_end();
    }

    /// Retreat the focused field to the previous one and sync the cursor
    /// to the end of the new focused value.
    pub fn focus_prev(&mut self) {
        self.focused_field = self.focused_field.prev();
        self.sync_cursor_to_end();
    }

    pub fn focused_value(&self) -> &str {
        match self.focused_field {
            SnippetFormField::Name => &self.name,
            SnippetFormField::Command => &self.command,
            SnippetFormField::Description => &self.description,
            // Not text fields; no editable value.
            SnippetFormField::Interactive | SnippetFormField::DefaultHosts => "",
        }
    }

    /// Mutable text value of the focused field, or `None` for the non-text
    /// `Interactive` and `DefaultHosts` fields (so keystroke handlers no-op there).
    pub fn focused_value_mut(&mut self) -> Option<&mut String> {
        match self.focused_field {
            SnippetFormField::Name => Some(&mut self.name),
            SnippetFormField::Command => Some(&mut self.command),
            SnippetFormField::Description => Some(&mut self.description),
            SnippetFormField::Interactive | SnippetFormField::DefaultHosts => None,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let pos = self.cursor_pos;
        let Some(val) = self.focused_value_mut() else {
            return;
        };
        let byte_pos = char_to_byte_pos(val, pos);
        val.insert(byte_pos, c);
        self.cursor_pos = pos + 1;
    }

    pub fn delete_char_before_cursor(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let pos = self.cursor_pos;
        let Some(val) = self.focused_value_mut() else {
            return;
        };
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
            return Err(
                crate::i18n::text(crate::messages::SNIPPET_DESCRIPTION_CONTROL_CHARS).to_string(),
            );
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

impl SnippetHostOutput {
    /// True when the host reported "command not found" for a snippet that
    /// runs in a plain shell, so the output can point at the Interactive
    /// shell toggle.
    pub fn shows_not_found_hint(&self, interactive: bool) -> bool {
        crate::snippet::not_found_hint_applies(interactive, self.exit_code)
    }
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
    /// Whether the running snippet uses the Interactive shell toggle.
    pub interactive: bool,
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
        let Some(val) = self.values.get_mut(idx) else {
            return;
        };
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
        let Some(val) = self.values.get_mut(idx) else {
            return;
        };
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

#[cfg(test)]
mod host_form_method_tests {
    use super::*;
    use crate::quick_add::ParsedTarget;

    #[test]
    fn apply_password_source_sets_value_and_keeps_focus_when_refocus_false() {
        let mut f = HostForm::new();
        f.focused_field = FormField::Alias;
        f.apply_password_source("vault:secret/ssh#pw".to_string(), false);
        assert_eq!(f.askpass, "vault:secret/ssh#pw");
        assert_eq!(f.focused_field, FormField::Alias);
        assert_eq!(f.cursor_pos, "vault:secret/ssh#pw".chars().count());
    }

    #[test]
    fn apply_password_source_moves_focus_to_askpass_when_refocus_true() {
        let mut f = HostForm::new();
        f.focused_field = FormField::Hostname;
        f.apply_password_source("op://Vault/Item/pw".to_string(), true);
        assert_eq!(f.askpass, "op://Vault/Item/pw");
        assert_eq!(f.focused_field, FormField::AskPass);
        assert_eq!(f.cursor_pos, "op://Vault/Item/pw".chars().count());
    }

    #[test]
    fn apply_password_source_clears_askpass_with_empty_value() {
        let mut f = HostForm::new();
        f.askpass = "old".into();
        f.focused_field = FormField::AskPass;
        f.apply_password_source(String::new(), false);
        assert_eq!(f.askpass, "");
        assert_eq!(f.cursor_pos, 0);
    }

    #[test]
    fn apply_smart_paste_fills_empty_fields_and_overwrites_alias() {
        let mut f = HostForm::new();
        f.alias = "user@host:2222".into();
        let parsed = ParsedTarget {
            user: "alice".into(),
            hostname: "db.example.com".into(),
            port: 2222,
        };
        f.apply_smart_paste(parsed, "db".into());
        assert_eq!(f.alias, "db");
        assert_eq!(f.hostname, "db.example.com");
        assert_eq!(f.user, "alice");
        assert_eq!(f.port, "2222");
    }

    #[test]
    fn apply_smart_paste_does_not_overwrite_non_empty_hostname() {
        let mut f = HostForm::new();
        f.hostname = "existing.com".into();
        let parsed = ParsedTarget {
            user: "u".into(),
            hostname: "parsed.com".into(),
            port: 22,
        };
        f.apply_smart_paste(parsed, "x".into());
        assert_eq!(f.hostname, "existing.com");
    }

    #[test]
    fn apply_smart_paste_does_not_overwrite_non_empty_user() {
        let mut f = HostForm::new();
        f.user = "bob".into();
        let parsed = ParsedTarget {
            user: "alice".into(),
            hostname: "host".into(),
            port: 22,
        };
        f.apply_smart_paste(parsed, "x".into());
        assert_eq!(f.user, "bob");
    }

    #[test]
    fn apply_smart_paste_keeps_default_port_when_parsed_port_is_default() {
        let mut f = HostForm::new();
        let parsed = ParsedTarget {
            user: "u".into(),
            hostname: "h".into(),
            port: 22,
        };
        f.apply_smart_paste(parsed, "x".into());
        assert_eq!(f.port, "22");
    }

    #[test]
    fn apply_smart_paste_does_not_overwrite_user_when_parsed_user_is_empty() {
        let mut f = HostForm::new();
        let parsed = ParsedTarget {
            user: String::new(),
            hostname: "h".into(),
            port: 22,
        };
        f.apply_smart_paste(parsed, "x".into());
        assert_eq!(f.user, "");
    }

    #[test]
    fn apply_smart_paste_keeps_user_custom_port_even_when_parsed_port_differs() {
        let mut f = HostForm::new();
        f.port = "8022".into();
        let parsed = ParsedTarget {
            user: "u".into(),
            hostname: "h".into(),
            port: 2222,
        };
        f.apply_smart_paste(parsed, "x".into());
        assert_eq!(f.port, "8022");
    }

    #[test]
    fn apply_password_source_empty_value_with_refocus_moves_focus_and_zeros_cursor() {
        let mut f = HostForm::new();
        f.focused_field = FormField::Hostname;
        f.apply_password_source(String::new(), true);
        assert_eq!(f.askpass, "");
        assert_eq!(f.focused_field, FormField::AskPass);
        assert_eq!(f.cursor_pos, 0);
    }

    #[test]
    fn tunnel_form_focus_next_from_type_goes_to_bind_port_and_resets_cursor() {
        let mut f = TunnelForm::new();
        f.bind_port = "8080".into();
        f.cursor_pos = 99;
        assert_eq!(f.focused_field, TunnelFormField::Type);
        f.focus_next();
        assert_eq!(f.focused_field, TunnelFormField::BindPort);
        // sync_cursor_to_end moves to end of focused value (chars count).
        assert_eq!(f.cursor_pos, 4);
    }

    #[test]
    fn tunnel_form_focus_next_wraps_from_remote_port_back_to_type() {
        let mut f = TunnelForm::new();
        f.focused_field = TunnelFormField::RemotePort;
        // Non-empty source field so a wrong-ordering regression
        // (sync_cursor_to_end before the field assignment) would
        // produce cursor_pos = 3 instead of 0.
        f.remote_port = "443".into();
        f.cursor_pos = 99;
        f.focus_next();
        assert_eq!(f.focused_field, TunnelFormField::Type);
        assert_eq!(f.cursor_pos, 0);
    }

    #[test]
    fn tunnel_form_focus_next_dynamic_skips_remote_fields() {
        let mut f = TunnelForm::new();
        f.tunnel_type = TunnelType::Dynamic;
        f.focused_field = TunnelFormField::BindPort;
        // Non-empty source field so a wrong-ordering regression would
        // yield cursor_pos = 4, distinguishing it from the correct 0.
        f.bind_port = "8080".into();
        f.cursor_pos = 99;
        f.focus_next();
        // Dynamic skips RemoteHost + RemotePort.
        assert_eq!(f.focused_field, TunnelFormField::Type);
        assert_eq!(f.cursor_pos, 0);
    }

    #[test]
    fn tunnel_form_focus_prev_from_bind_port_goes_to_type() {
        let mut f = TunnelForm::new();
        f.focused_field = TunnelFormField::BindPort;
        // Non-empty source field so a wrong-ordering regression would
        // yield cursor_pos = 4, distinguishing it from the correct 0.
        f.bind_port = "8080".into();
        f.cursor_pos = 99;
        f.focus_prev();
        assert_eq!(f.focused_field, TunnelFormField::Type);
        assert_eq!(f.cursor_pos, 0);
    }

    #[test]
    fn tunnel_form_focus_next_preserves_field_values_and_tunnel_type() {
        let mut f = TunnelForm {
            tunnel_type: TunnelType::Local,
            bind_port: "1234".into(),
            remote_host: "example.com".into(),
            remote_port: "5678".into(),
            bind_address: "127.0.0.1".into(),
            focused_field: TunnelFormField::Type,
            cursor_pos: 0,
        };
        f.focus_next();
        assert_eq!(f.bind_port, "1234");
        assert_eq!(f.remote_host, "example.com");
        assert_eq!(f.remote_port, "5678");
        assert_eq!(f.bind_address, "127.0.0.1");
        assert_eq!(f.tunnel_type, TunnelType::Local);
    }

    #[test]
    fn snippet_form_focus_next_advances_field_and_syncs_cursor_to_target() {
        let mut f = SnippetForm::new();
        // Distinct lengths so a wrong-ordering regression (sync before
        // assignment) would land cursor at 3 instead of the correct 2.
        f.name = "abc".into();
        f.command = "de".into();
        f.focused_field = SnippetFormField::Name;
        f.cursor_pos = 99;
        f.focus_next();
        assert_eq!(f.focused_field, SnippetFormField::Command);
        assert_eq!(f.cursor_pos, 2);
    }

    #[test]
    fn snippet_form_focus_prev_retreats_field_and_syncs_cursor_to_target() {
        let mut f = SnippetForm::new();
        // Source and target have distinct lengths: wrong ordering would
        // yield cursor_pos = 2 (end of source command), correct is 3.
        f.name = "xyz".into();
        f.command = "ab".into();
        f.focused_field = SnippetFormField::Command;
        f.cursor_pos = 99;
        f.focus_prev();
        assert_eq!(f.focused_field, SnippetFormField::Name);
        assert_eq!(f.cursor_pos, 3);
    }

    #[test]
    fn snippet_form_focus_next_preserves_field_values() {
        let mut f = SnippetForm::new();
        f.name = "foo".into();
        f.command = "bar".into();
        f.description = "baz".into();
        f.focus_next();
        assert_eq!(f.name, "foo");
        assert_eq!(f.command, "bar");
        assert_eq!(f.description, "baz");
    }
}
