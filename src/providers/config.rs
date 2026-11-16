use std::io;
use std::path::PathBuf;
use std::str::FromStr;

use crate::fs_util;
use crate::providers::ProviderKind;

/// Identifier for one provider-config section. Bare slug ("digitalocean") when
/// it is the only config for that provider; provider+label ("digitalocean:work")
/// when multiple configs coexist for the same provider.
///
/// The label charset is strict ([a-z0-9-], max 32) so the `:`-separator in the
/// `# purple:provider <id>:<server_id>` SSH marker stays unambiguous even if
/// future server IDs contain colons.
///
/// Fields are `pub(crate)` so external callers can't construct an invalid id
/// by direct field mutation. Use `bare()`, `labeled()` or `FromStr`. The
/// internal placeholder pattern in the add-flow (constructing with an empty
/// label, then filling it via the form) lives within the crate and is
/// validated again by `ProviderConfig::save()` before reaching disk.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderConfigId {
    pub(crate) provider: String,
    pub(crate) label: Option<String>,
}

impl ProviderConfigId {
    pub fn bare(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            label: None,
        }
    }

    pub fn labeled(provider: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            label: Some(label.into()),
        }
    }

    /// Typed provider kind, or None if the stored name does not match any known provider.
    pub fn kind(&self) -> Option<ProviderKind> {
        self.provider.parse().ok()
    }
}

impl Default for ProviderConfigId {
    fn default() -> Self {
        Self::bare(String::new())
    }
}

impl std::fmt::Display for ProviderConfigId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.label {
            None => f.write_str(&self.provider),
            Some(l) => write!(f, "{}:{}", self.provider, l),
        }
    }
}

impl FromStr for ProviderConfigId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once(':') {
            Some((p, l)) => {
                if p.is_empty() {
                    return Err("provider name is empty".to_string());
                }
                validate_label(l)?;
                Ok(Self::labeled(p, l))
            }
            None => {
                if s.is_empty() {
                    return Err("provider name is empty".to_string());
                }
                Ok(Self::bare(s))
            }
        }
    }
}

/// Validate a config label. Strict charset prevents collisions with marker
/// server_id parsing: [a-z0-9-]+, max 32 chars, no leading/trailing dash.
pub fn validate_label(label: &str) -> Result<(), String> {
    if label.is_empty() {
        return Err("label is empty".to_string());
    }
    if label.len() > 32 {
        return Err("label exceeds 32 characters".to_string());
    }
    if !label
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err("label contains illegal characters (only [a-z0-9-] allowed)".to_string());
    }
    if label.starts_with('-') || label.ends_with('-') {
        return Err("label must not start or end with a dash".to_string());
    }
    Ok(())
}

/// A configured provider section from ~/.purple/providers.
#[derive(Clone)]
pub struct ProviderSection {
    pub id: ProviderConfigId,
    pub token: String,
    pub alias_prefix: String,
    pub user: String,
    pub identity_file: String,
    pub url: String,
    pub verify_tls: bool,
    pub auto_sync: bool,
    pub profile: String,
    pub regions: String,
    pub project: String,
    pub compartment: String,
    /// Raw NetBox query parameters scoping which objects sync. NetBox only.
    pub filter: String,
    /// NetBox interface name whose assigned IP should be used when set.
    pub interface: String,
    /// Optional SSH port override for NetBox hosts.
    pub ssh_port: Option<u16>,
    pub vault_role: String,
    /// Optional `VAULT_ADDR` override passed to the `vault` CLI when signing
    /// SSH certs. Empty = inherit parent env. Stored as a plain string so an
    /// uninitialized field (via `..Default::default()`) stays innocuous.
    pub vault_addr: String,
    /// Whether synced hosts reach their instance through AWS Systems Manager
    /// Session Manager. AWS only.
    pub ssm: crate::providers::aws_ssm::SsmMode,
}

impl ProviderSection {
    /// Convenience accessor for the bare provider name (without label).
    pub fn provider(&self) -> &str {
        &self.id.provider
    }

    /// One-line summary of what a save wrote, for the state-change log.
    /// Optional fields only show up when set, so each provider's record names
    /// the fields it actually uses. Credential values stay out of it: the
    /// token reads as set or empty and `vault_addr` is left out entirely,
    /// matching what the `Debug` impl redacts.
    pub fn log_record(&self) -> String {
        let mut parts = vec![
            format!("prefix='{}'", self.alias_prefix),
            format!("user='{}'", self.user),
            // Trimmed, because every gate that reads the token trims first.
            // A whitespace-only token logged as set would contradict the
            // sync, which treats it as absent.
            format!(
                "token={}",
                if self.token.trim().is_empty() {
                    "empty"
                } else {
                    "set"
                }
            ),
        ];
        for (name, value) in [
            ("url", &self.url),
            ("profile", &self.profile),
            ("regions", &self.regions),
            ("project", &self.project),
            ("compartment", &self.compartment),
            ("filter", &self.filter),
            ("interface", &self.interface),
            ("identity_file", &self.identity_file),
            ("vault_role", &self.vault_role),
        ] {
            if !value.is_empty() {
                parts.push(format!("{}='{}'", name, value));
            }
        }
        if let Some(port) = self.ssh_port {
            parts.push(format!("ssh_port={port}"));
        }
        if self.ssm.is_enabled() {
            parts.push(format!("ssm={}", self.ssm));
        }
        parts.push(format!("verify_tls={}", self.verify_tls));
        parts.push(format!("auto_sync={}", self.auto_sync));
        parts.join(" ")
    }
}

/// Manual `Debug` so secret-bearing fields never leak into log lines,
/// panic messages, or test failure output via `{:?}`. Redacts the API
/// `token` and the `vault_addr` (which reveals internal Vault topology).
impl std::fmt::Debug for ProviderSection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderSection")
            .field("id", &self.id)
            .field("token", &redacted(&self.token))
            .field("alias_prefix", &self.alias_prefix)
            .field("user", &self.user)
            .field("identity_file", &self.identity_file)
            .field("url", &self.url)
            .field("verify_tls", &self.verify_tls)
            .field("auto_sync", &self.auto_sync)
            .field("profile", &self.profile)
            .field("regions", &self.regions)
            .field("project", &self.project)
            .field("compartment", &self.compartment)
            .field("filter", &self.filter)
            .field("interface", &self.interface)
            .field("ssh_port", &self.ssh_port)
            .field("vault_role", &self.vault_role)
            .field("vault_addr", &redacted(&self.vault_addr))
            .field("ssm", &self.ssm)
            .finish()
    }
}

pub(super) fn redacted(value: &str) -> &'static str {
    if value.is_empty() {
        "<empty>"
    } else {
        "<redacted>"
    }
}

impl Default for ProviderSection {
    fn default() -> Self {
        Self {
            id: ProviderConfigId::default(),
            token: String::new(),
            alias_prefix: String::new(),
            user: String::new(),
            identity_file: String::new(),
            url: String::new(),
            // verify_tls defaults to true (secure). A user who wants to sync
            // against self-signed Proxmox must opt in explicitly.
            verify_tls: true,
            auto_sync: false,
            profile: String::new(),
            regions: String::new(),
            project: String::new(),
            compartment: String::new(),
            filter: String::new(),
            interface: String::new(),
            ssh_port: None,
            vault_role: String::new(),
            vault_addr: String::new(),
            ssm: crate::providers::aws_ssm::SsmMode::default(),
        }
    }
}

/// Default for auto_sync. Delegates to `ProviderKind::default_auto_sync`;
/// unknown provider names default to true.
fn default_auto_sync(provider: &str) -> bool {
    provider
        .parse::<ProviderKind>()
        .ok()
        .is_none_or(ProviderKind::default_auto_sync)
}

/// Parsed provider configuration from ~/.purple/providers.
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    pub sections: Vec<ProviderSection>,
    /// Override path for save(). None uses the default ~/.purple/providers.
    /// Set to Some in tests to avoid writing to the real config.
    pub path_override: Option<PathBuf>,
}

fn config_path(paths: Option<&crate::runtime::env::Paths>) -> Option<PathBuf> {
    paths.map(crate::runtime::env::Paths::providers_config)
}

impl ProviderConfig {
    /// Load provider config from `~/.purple/providers`, resolved from the
    /// injected `paths`. The resolved path is stored as `path_override` so
    /// a later `save()` writes back to the same location. Returns an empty
    /// config when the file does not exist (normal first-use) or when no
    /// home directory is known. Logs a warning on real IO errors.
    pub fn load(paths: Option<&crate::runtime::env::Paths>) -> Self {
        let path = match config_path(paths) {
            Some(p) => p,
            None => return Self::default(),
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Self {
                    path_override: Some(path),
                    ..Self::default()
                };
            }
            Err(e) => {
                log::warn!("[config] Could not read {}: {}", path.display(), e);
                return Self {
                    path_override: Some(path),
                    ..Self::default()
                };
            }
        };
        Self {
            path_override: Some(path),
            ..Self::parse(&content)
        }
    }

    /// Parse INI-style provider config.
    ///
    /// Section headers are either `[provider]` (bare, single config) or
    /// `[provider:label]` (multi-config). Mixing both forms for the same
    /// provider is rejected (first wins, others dropped with a warn-log).
    pub(crate) fn parse(content: &str) -> Self {
        let mut sections: Vec<ProviderSection> = Vec::new();
        let mut current: Option<ProviderSection> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if let Some(section) = current.take()
                    && !sections.iter().any(|s| s.id == section.id)
                {
                    sections.push(section);
                }
                let raw = trimmed[1..trimmed.len() - 1].trim();
                let id = match ProviderConfigId::from_str(raw) {
                    Ok(id) => id,
                    Err(e) => {
                        log::warn!("[config] Skipping invalid section header [{}]: {}", raw, e);
                        current = None;
                        continue;
                    }
                };
                // Reject duplicates (same provider+label combo).
                if sections.iter().any(|s| s.id == id) {
                    log::warn!("[config] Skipping duplicate section header [{}]", id);
                    current = None;
                    continue;
                }
                // Reject mix of bare + labeled for the same provider.
                let has_bare = sections
                    .iter()
                    .any(|s| s.id.provider == id.provider && s.id.label.is_none());
                let has_labeled = sections
                    .iter()
                    .any(|s| s.id.provider == id.provider && s.id.label.is_some());
                if (id.label.is_some() && has_bare) || (id.label.is_none() && has_labeled) {
                    log::warn!(
                        "[config] Skipping [{}]: mixing bare and labeled sections for provider '{}' is not allowed",
                        id,
                        id.provider
                    );
                    current = None;
                    continue;
                }
                let short_label = super::get_provider(&id.provider)
                    .map(|p| p.short_label().to_string())
                    .unwrap_or_else(|| id.provider.clone());
                let auto_sync_default = default_auto_sync(&id.provider);
                let alias_prefix = match &id.label {
                    Some(l) => format!("{}-{}", short_label, l),
                    None => short_label,
                };
                current = Some(ProviderSection {
                    id,
                    token: String::new(),
                    alias_prefix,
                    user: "root".to_string(),
                    identity_file: String::new(),
                    url: String::new(),
                    verify_tls: true,
                    auto_sync: auto_sync_default,
                    profile: String::new(),
                    regions: String::new(),
                    project: String::new(),
                    compartment: String::new(),
                    filter: String::new(),
                    interface: String::new(),
                    ssh_port: None,
                    vault_role: String::new(),
                    vault_addr: String::new(),
                    ssm: crate::providers::aws_ssm::SsmMode::default(),
                });
            } else if let Some(ref mut section) = current
                && let Some((key, value)) = trimmed.split_once('=')
            {
                let key = key.trim();
                let value = value.trim().to_string();
                match key {
                    "token" => section.token = value,
                    "alias_prefix" => section.alias_prefix = value,
                    "user" => section.user = value,
                    "key" => section.identity_file = value,
                    "url" => section.url = value,
                    "verify_tls" => {
                        section.verify_tls =
                            !matches!(value.to_lowercase().as_str(), "false" | "0" | "no")
                    }
                    "auto_sync" => {
                        section.auto_sync =
                            !matches!(value.to_lowercase().as_str(), "false" | "0" | "no")
                    }
                    "profile" => section.profile = value,
                    "regions" => section.regions = value,
                    "project" => section.project = value,
                    "compartment" => section.compartment = value,
                    "filter" => section.filter = value,
                    "interface" => section.interface = value,
                    "ssh_port" => {
                        section.ssh_port = value.parse::<u16>().ok().filter(|port| *port > 0)
                    }
                    "vault_role" => {
                        // Silently drop invalid roles so parsing stays infallible.
                        section.vault_role = if crate::vault_ssh::is_valid_role(&value) {
                            value
                        } else {
                            String::new()
                        };
                    }
                    "ssm" => {
                        // Infallible: an unknown value parses as Off, so a
                        // hand-edited config still loads.
                        section.ssm = value.parse().unwrap_or_default();
                    }
                    "vault_addr" => {
                        // Same silent-drop policy as vault_role: a bad
                        // value is ignored on parse rather than crashing
                        // the whole config load.
                        section.vault_addr = if crate::vault_ssh::is_valid_vault_addr(&value) {
                            value
                        } else {
                            String::new()
                        };
                    }
                    _ => {}
                }
            }
        }
        if let Some(section) = current
            && !sections.iter().any(|s| s.id == section.id)
        {
            sections.push(section);
        }
        Self {
            sections,
            path_override: None,
        }
    }

    /// Strip control characters (newlines, tabs, etc.) from a config value
    /// to prevent INI format corruption from paste errors.
    fn sanitize_value(s: &str) -> String {
        s.chars().filter(|c| !c.is_control()).collect()
    }

    /// Save provider config to ~/.purple/providers (atomic write, chmod 600).
    /// Respects path_override when set (used in tests).
    pub fn save(&self) -> io::Result<()> {
        // Reject obviously broken in-memory state before touching disk.
        if let Err(e) = self.validate() {
            log::warn!("[config] Refusing to save invalid provider config: {}", e);
            return Err(io::Error::new(io::ErrorKind::InvalidData, e));
        }
        // Demo mode never writes to the user's real config. In production
        // builds that means an unconditional skip; under `#[cfg(test)]` the
        // resolved path is always an isolated sandbox or explicit override,
        // so tests write regardless of a parallel test toggling the global
        // demo flag.
        if crate::demo_flag::is_demo() {
            #[cfg(not(test))]
            return Ok(());
        }
        let Some(path) = self.path_override.clone() else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Could not determine home directory",
            ));
        };

        let mut content = String::new();
        for (i, section) in self.sections.iter().enumerate() {
            if i > 0 {
                content.push('\n');
            }
            content.push_str(&format!(
                "[{}]\n",
                Self::sanitize_value(&section.id.to_string())
            ));
            content.push_str(&format!("token={}\n", Self::sanitize_value(&section.token)));
            content.push_str(&format!(
                "alias_prefix={}\n",
                Self::sanitize_value(&section.alias_prefix)
            ));
            content.push_str(&format!("user={}\n", Self::sanitize_value(&section.user)));
            if !section.identity_file.is_empty() {
                content.push_str(&format!(
                    "key={}\n",
                    Self::sanitize_value(&section.identity_file)
                ));
            }
            if !section.url.is_empty() {
                content.push_str(&format!("url={}\n", Self::sanitize_value(&section.url)));
            }
            if !section.verify_tls {
                content.push_str("verify_tls=false\n");
            }
            if !section.profile.is_empty() {
                content.push_str(&format!(
                    "profile={}\n",
                    Self::sanitize_value(&section.profile)
                ));
            }
            if !section.regions.is_empty() {
                content.push_str(&format!(
                    "regions={}\n",
                    Self::sanitize_value(&section.regions)
                ));
            }
            if !section.project.is_empty() {
                content.push_str(&format!(
                    "project={}\n",
                    Self::sanitize_value(&section.project)
                ));
            }
            if !section.compartment.is_empty() {
                content.push_str(&format!(
                    "compartment={}\n",
                    Self::sanitize_value(&section.compartment)
                ));
            }
            if !section.filter.is_empty() {
                content.push_str(&format!(
                    "filter={}\n",
                    Self::sanitize_value(&section.filter)
                ));
            }
            if !section.interface.is_empty() {
                content.push_str(&format!(
                    "interface={}\n",
                    Self::sanitize_value(&section.interface)
                ));
            }
            if let Some(port) = section.ssh_port {
                content.push_str(&format!("ssh_port={port}\n"));
            }
            if !section.vault_role.is_empty()
                && crate::vault_ssh::is_valid_role(&section.vault_role)
            {
                content.push_str(&format!(
                    "vault_role={}\n",
                    Self::sanitize_value(&section.vault_role)
                ));
            }
            if !section.vault_addr.is_empty()
                && crate::vault_ssh::is_valid_vault_addr(&section.vault_addr)
            {
                content.push_str(&format!(
                    "vault_addr={}\n",
                    Self::sanitize_value(&section.vault_addr)
                ));
            }
            if section.ssm.is_enabled() {
                content.push_str(&format!("ssm={}\n", section.ssm));
            }
            if section.auto_sync != default_auto_sync(&section.id.provider) {
                content.push_str(if section.auto_sync {
                    "auto_sync=true\n"
                } else {
                    "auto_sync=false\n"
                });
            }
        }

        fs_util::atomic_write(&path, content.as_bytes())
    }

    /// Get the first section matching the given provider name.
    /// For multi-config use, prefer `section_by_id` or `sections_for_provider`.
    pub fn section(&self, provider: &str) -> Option<&ProviderSection> {
        self.sections.iter().find(|s| s.id.provider == provider)
    }

    /// Get all sections for a given provider name (multi-config support).
    pub fn sections_for_provider(&self, provider: &str) -> Vec<&ProviderSection> {
        self.sections
            .iter()
            .filter(|s| s.id.provider == provider)
            .collect()
    }

    /// Get a section by exact ProviderConfigId match.
    pub fn section_by_id(&self, id: &ProviderConfigId) -> Option<&ProviderSection> {
        self.sections.iter().find(|s| &s.id == id)
    }

    /// Add or replace a provider section. Matches on full ProviderConfigId so
    /// labeled configs are independent from each other and from a bare config.
    pub fn set_section(&mut self, section: ProviderSection) {
        if let Some(existing) = self.sections.iter_mut().find(|s| s.id == section.id) {
            *existing = section;
        } else {
            self.sections.push(section);
        }
    }

    /// Remove all sections matching the given provider name (any label).
    pub fn remove_section(&mut self, provider: &str) {
        self.sections.retain(|s| s.id.provider != provider);
    }

    /// Remove the section with the exact ProviderConfigId.
    pub fn remove_section_by_id(&mut self, id: &ProviderConfigId) {
        self.sections.retain(|s| &s.id != id);
    }

    /// Get all configured provider sections.
    pub fn configured_providers(&self) -> &[ProviderSection] {
        &self.sections
    }

    /// Validate the in-memory section set:
    /// - no duplicate ProviderConfigId
    /// - no mix of bare + labeled for the same provider
    /// - no duplicate alias_prefix anywhere (case-sensitive)
    /// - all labels pass `validate_label`
    pub fn validate(&self) -> Result<(), String> {
        let mut seen_ids: Vec<&ProviderConfigId> = Vec::new();
        for s in &self.sections {
            if let Some(label) = &s.id.label {
                validate_label(label).map_err(|e| format!("[{}]: {}", s.id, e))?;
            }
            if seen_ids.iter().any(|id| **id == s.id) {
                return Err(format!("duplicate section [{}]", s.id));
            }
            seen_ids.push(&s.id);
        }
        for s in &self.sections {
            let bare = self
                .sections
                .iter()
                .any(|o| o.id.provider == s.id.provider && o.id.label.is_none());
            let labeled = self
                .sections
                .iter()
                .any(|o| o.id.provider == s.id.provider && o.id.label.is_some());
            if bare && labeled {
                return Err(format!(
                    "provider '{}' has both bare and labeled sections",
                    s.id.provider
                ));
            }
        }
        let mut seen_prefixes: Vec<&str> = Vec::new();
        for s in &self.sections {
            if s.alias_prefix.is_empty() {
                continue;
            }
            if seen_prefixes.contains(&s.alias_prefix.as_str()) {
                return Err(format!(
                    "duplicate alias_prefix '{}' across sections",
                    s.alias_prefix
                ));
            }
            seen_prefixes.push(&s.alias_prefix);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
