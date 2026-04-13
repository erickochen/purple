use std::io;
use std::path::PathBuf;

use crate::fs_util;

/// A configured provider section from ~/.purple/providers.
#[derive(Debug, Clone)]
pub struct ProviderSection {
    pub provider: String,
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
    pub vault_role: String,
    /// Optional `VAULT_ADDR` override passed to the `vault` CLI when signing
    /// SSH certs. Empty = inherit parent env. Stored as a plain string so an
    /// uninitialized field (via `..Default::default()`) stays innocuous.
    pub vault_addr: String,
}

/// Default for auto_sync: false for proxmox (N+1 API calls), true for all others.
fn default_auto_sync(provider: &str) -> bool {
    !matches!(provider, "proxmox")
}

/// Parsed provider configuration from ~/.purple/providers.
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    pub sections: Vec<ProviderSection>,
    /// Override path for save(). None uses the default ~/.purple/providers.
    /// Set to Some in tests to avoid writing to the real config.
    pub path_override: Option<PathBuf>,
}

fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".purple/providers"))
}

impl ProviderConfig {
    /// Load provider config from ~/.purple/providers.
    /// Returns empty config if file doesn't exist (normal first-use).
    /// Prints a warning to stderr on real IO errors (permissions, etc.).
    pub fn load() -> Self {
        let path = match config_path() {
            Some(p) => p,
            None => return Self::default(),
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                log::warn!("[config] Could not read {}: {}", path.display(), e);
                return Self::default();
            }
        };
        Self::parse(&content)
    }

    /// Parse INI-style provider config.
    pub(crate) fn parse(content: &str) -> Self {
        let mut sections = Vec::new();
        let mut current: Option<ProviderSection> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if let Some(section) = current.take() {
                    if !sections
                        .iter()
                        .any(|s: &ProviderSection| s.provider == section.provider)
                    {
                        sections.push(section);
                    }
                }
                let name = trimmed[1..trimmed.len() - 1].trim().to_string();
                if sections.iter().any(|s| s.provider == name) {
                    current = None;
                    continue;
                }
                let short_label = super::get_provider(&name)
                    .map(|p| p.short_label().to_string())
                    .unwrap_or_else(|| name.clone());
                let auto_sync_default = default_auto_sync(&name);
                current = Some(ProviderSection {
                    provider: name,
                    token: String::new(),
                    alias_prefix: short_label,
                    user: "root".to_string(),
                    identity_file: String::new(),
                    url: String::new(),
                    verify_tls: true,
                    auto_sync: auto_sync_default,
                    profile: String::new(),
                    regions: String::new(),
                    project: String::new(),
                    compartment: String::new(),
                    vault_role: String::new(),
                    vault_addr: String::new(),
                });
            } else if let Some(ref mut section) = current {
                if let Some((key, value)) = trimmed.split_once('=') {
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
                        "vault_role" => {
                            // Silently drop invalid roles so parsing stays infallible.
                            section.vault_role = if crate::vault_ssh::is_valid_role(&value) {
                                value
                            } else {
                                String::new()
                            };
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
        }
        if let Some(section) = current {
            if !sections.iter().any(|s| s.provider == section.provider) {
                sections.push(section);
            }
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
        if crate::demo_flag::is_demo() {
            return Ok(());
        }
        let path = match &self.path_override {
            Some(p) => p.clone(),
            None => match config_path() {
                Some(p) => p,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "Could not determine home directory",
                    ));
                }
            },
        };

        let mut content = String::new();
        for (i, section) in self.sections.iter().enumerate() {
            if i > 0 {
                content.push('\n');
            }
            content.push_str(&format!("[{}]\n", Self::sanitize_value(&section.provider)));
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
            if section.auto_sync != default_auto_sync(&section.provider) {
                content.push_str(if section.auto_sync {
                    "auto_sync=true\n"
                } else {
                    "auto_sync=false\n"
                });
            }
        }

        fs_util::atomic_write(&path, content.as_bytes())
    }

    /// Get a configured provider section by name.
    pub fn section(&self, provider: &str) -> Option<&ProviderSection> {
        self.sections.iter().find(|s| s.provider == provider)
    }

    /// Add or replace a provider section.
    pub fn set_section(&mut self, section: ProviderSection) {
        if let Some(existing) = self
            .sections
            .iter_mut()
            .find(|s| s.provider == section.provider)
        {
            *existing = section;
        } else {
            self.sections.push(section);
        }
    }

    /// Remove a provider section.
    pub fn remove_section(&mut self, provider: &str) {
        self.sections.retain(|s| s.provider != provider);
    }

    /// Get all configured provider sections.
    pub fn configured_providers(&self) -> &[ProviderSection] {
        &self.sections
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
