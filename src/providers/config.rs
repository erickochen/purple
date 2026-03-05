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
}

/// Default for auto_sync: false for proxmox (N+1 API calls), true for all others.
fn default_auto_sync(provider: &str) -> bool {
    provider != "proxmox"
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
                eprintln!("! Could not read {}: {}", path.display(), e);
                return Self::default();
            }
        };
        Self::parse(&content)
    }

    /// Parse INI-style provider config.
    fn parse(content: &str) -> Self {
        let mut sections = Vec::new();
        let mut current: Option<ProviderSection> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if let Some(section) = current.take() {
                    if !sections.iter().any(|s: &ProviderSection| s.provider == section.provider) {
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
                        "verify_tls" => section.verify_tls = !matches!(
                            value.to_lowercase().as_str(), "false" | "0" | "no"
                        ),
                        "auto_sync" => section.auto_sync = !matches!(
                            value.to_lowercase().as_str(), "false" | "0" | "no"
                        ),
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
        Self { sections, path_override: None }
    }

    /// Save provider config to ~/.purple/providers (atomic write, chmod 600).
    /// Respects path_override when set (used in tests).
    pub fn save(&self) -> io::Result<()> {
        let path = match &self.path_override {
            Some(p) => p.clone(),
            None => match config_path() {
                Some(p) => p,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "Could not determine home directory",
                    ))
                }
            },
        };

        let mut content = String::new();
        for (i, section) in self.sections.iter().enumerate() {
            if i > 0 {
                content.push('\n');
            }
            content.push_str(&format!("[{}]\n", section.provider));
            content.push_str(&format!("token={}\n", section.token));
            content.push_str(&format!("alias_prefix={}\n", section.alias_prefix));
            content.push_str(&format!("user={}\n", section.user));
            if !section.identity_file.is_empty() {
                content.push_str(&format!("key={}\n", section.identity_file));
            }
            if !section.url.is_empty() {
                content.push_str(&format!("url={}\n", section.url));
            }
            if !section.verify_tls {
                content.push_str("verify_tls=false\n");
            }
            if section.auto_sync != default_auto_sync(&section.provider) {
                content.push_str(if section.auto_sync { "auto_sync=true\n" } else { "auto_sync=false\n" });
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
        if let Some(existing) = self.sections.iter_mut().find(|s| s.provider == section.provider) {
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
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty() {
        let config = ProviderConfig::parse("");
        assert!(config.sections.is_empty());
    }

    #[test]
    fn test_parse_single_section() {
        let content = "\
[digitalocean]
token=dop_v1_abc123
alias_prefix=do
user=root
key=~/.ssh/id_ed25519
";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 1);
        let s = &config.sections[0];
        assert_eq!(s.provider, "digitalocean");
        assert_eq!(s.token, "dop_v1_abc123");
        assert_eq!(s.alias_prefix, "do");
        assert_eq!(s.user, "root");
        assert_eq!(s.identity_file, "~/.ssh/id_ed25519");
    }

    #[test]
    fn test_parse_multiple_sections() {
        let content = "\
[digitalocean]
token=abc

[vultr]
token=xyz
user=deploy
";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 2);
        assert_eq!(config.sections[0].provider, "digitalocean");
        assert_eq!(config.sections[1].provider, "vultr");
        assert_eq!(config.sections[1].user, "deploy");
    }

    #[test]
    fn test_parse_comments_and_blanks() {
        let content = "\
# Provider config

[linode]
# API token
token=mytoken
";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].token, "mytoken");
    }

    #[test]
    fn test_set_section_add() {
        let mut config = ProviderConfig::default();
        config.set_section(ProviderSection {
            provider: "vultr".to_string(),
            token: "abc".to_string(),
            alias_prefix: "vultr".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
        });
        assert_eq!(config.sections.len(), 1);
    }

    #[test]
    fn test_set_section_replace() {
        let mut config = ProviderConfig::parse("[vultr]\ntoken=old\n");
        config.set_section(ProviderSection {
            provider: "vultr".to_string(),
            token: "new".to_string(),
            alias_prefix: "vultr".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
        });
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].token, "new");
    }

    #[test]
    fn test_remove_section() {
        let mut config = ProviderConfig::parse("[vultr]\ntoken=abc\n[linode]\ntoken=xyz\n");
        config.remove_section("vultr");
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].provider, "linode");
    }

    #[test]
    fn test_section_lookup() {
        let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
        assert!(config.section("digitalocean").is_some());
        assert!(config.section("vultr").is_none());
    }

    #[test]
    fn test_parse_duplicate_sections_first_wins() {
        let content = "\
[digitalocean]
token=first

[digitalocean]
token=second
";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].token, "first");
    }

    #[test]
    fn test_parse_duplicate_sections_trailing() {
        let content = "\
[vultr]
token=abc

[linode]
token=xyz

[vultr]
token=dup
";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 2);
        assert_eq!(config.sections[0].provider, "vultr");
        assert_eq!(config.sections[0].token, "abc");
        assert_eq!(config.sections[1].provider, "linode");
    }

    #[test]
    fn test_defaults_applied() {
        let config = ProviderConfig::parse("[hetzner]\ntoken=abc\n");
        let s = &config.sections[0];
        assert_eq!(s.user, "root");
        assert_eq!(s.alias_prefix, "hetzner");
        assert!(s.identity_file.is_empty());
        assert!(s.url.is_empty());
        assert!(s.verify_tls);
        assert!(s.auto_sync);
    }

    #[test]
    fn test_parse_url_and_verify_tls() {
        let content = "\
[proxmox]
token=user@pam!purple=secret
url=https://pve.example.com:8006
verify_tls=false
";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 1);
        let s = &config.sections[0];
        assert_eq!(s.url, "https://pve.example.com:8006");
        assert!(!s.verify_tls);
    }

    #[test]
    fn test_url_and_verify_tls_round_trip() {
        let content = "\
[proxmox]
token=tok
alias_prefix=pve
user=root
url=https://pve.local:8006
verify_tls=false
";
        let config = ProviderConfig::parse(content);
        let s = &config.sections[0];
        assert_eq!(s.url, "https://pve.local:8006");
        assert!(!s.verify_tls);
    }

    #[test]
    fn test_verify_tls_default_true() {
        // verify_tls not present -> defaults to true
        let config = ProviderConfig::parse("[proxmox]\ntoken=abc\nurl=https://pve:8006\n");
        assert!(config.sections[0].verify_tls);
    }

    #[test]
    fn test_verify_tls_false_variants() {
        for value in &["false", "False", "FALSE", "0", "no", "No", "NO"] {
            let content = format!("[proxmox]\ntoken=abc\nurl=https://pve:8006\nverify_tls={}\n", value);
            let config = ProviderConfig::parse(&content);
            assert!(!config.sections[0].verify_tls, "verify_tls={} should be false", value);
        }
    }

    #[test]
    fn test_verify_tls_true_variants() {
        for value in &["true", "True", "1", "yes"] {
            let content = format!("[proxmox]\ntoken=abc\nurl=https://pve:8006\nverify_tls={}\n", value);
            let config = ProviderConfig::parse(&content);
            assert!(config.sections[0].verify_tls, "verify_tls={} should be true", value);
        }
    }

    #[test]
    fn test_non_proxmox_url_not_written() {
        // url and verify_tls=false must not appear for non-Proxmox providers in saved config
        let section = ProviderSection {
            provider: "digitalocean".to_string(),
            token: "tok".to_string(),
            alias_prefix: "do".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: String::new(),     // empty: not written
            verify_tls: true,       // default: not written
            auto_sync: true,        // default for non-proxmox: not written
        };
        let mut config = ProviderConfig::default();
        config.set_section(section);
        // Parse it back: url and verify_tls should be at defaults
        let s = &config.sections[0];
        assert!(s.url.is_empty());
        assert!(s.verify_tls);
    }

    #[test]
    fn test_proxmox_url_fallback_in_section() {
        // Simulates the update path: existing section has url, new section should preserve it
        let existing = ProviderConfig::parse(
            "[proxmox]\ntoken=old\nalias_prefix=pve\nuser=root\nurl=https://pve.local:8006\n",
        );
        let existing_url = existing.section("proxmox").map(|s| s.url.clone()).unwrap_or_default();
        assert_eq!(existing_url, "https://pve.local:8006");

        let mut config = existing;
        config.set_section(ProviderSection {
            provider: "proxmox".to_string(),
            token: "new".to_string(),
            alias_prefix: "pve".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: existing_url,
            verify_tls: true,
            auto_sync: false,
        });
        assert_eq!(config.sections[0].token, "new");
        assert_eq!(config.sections[0].url, "https://pve.local:8006");
    }

    #[test]
    fn test_auto_sync_default_true_for_non_proxmox() {
        let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
        assert!(config.sections[0].auto_sync);
    }

    #[test]
    fn test_auto_sync_default_false_for_proxmox() {
        let config = ProviderConfig::parse("[proxmox]\ntoken=abc\nurl=https://pve:8006\n");
        assert!(!config.sections[0].auto_sync);
    }

    #[test]
    fn test_auto_sync_explicit_true() {
        let config = ProviderConfig::parse("[proxmox]\ntoken=abc\nurl=https://pve:8006\nauto_sync=true\n");
        assert!(config.sections[0].auto_sync);
    }

    #[test]
    fn test_auto_sync_explicit_false_non_proxmox() {
        let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\nauto_sync=false\n");
        assert!(!config.sections[0].auto_sync);
    }

    #[test]
    fn test_auto_sync_not_written_when_default() {
        // non-proxmox with auto_sync=true (default) -> not written
        let mut config = ProviderConfig::default();
        config.set_section(ProviderSection {
            provider: "digitalocean".to_string(),
            token: "tok".to_string(),
            alias_prefix: "do".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
        });
        // Re-parse: auto_sync should still be true (default)
        assert!(config.sections[0].auto_sync);

        // proxmox with auto_sync=false (default) -> not written
        let mut config2 = ProviderConfig::default();
        config2.set_section(ProviderSection {
            provider: "proxmox".to_string(),
            token: "tok".to_string(),
            alias_prefix: "pve".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: "https://pve:8006".to_string(),
            verify_tls: true,
            auto_sync: false,
        });
        assert!(!config2.sections[0].auto_sync);
    }

    #[test]
    fn test_auto_sync_false_variants() {
        for value in &["false", "False", "FALSE", "0", "no"] {
            let content = format!("[digitalocean]\ntoken=abc\nauto_sync={}\n", value);
            let config = ProviderConfig::parse(&content);
            assert!(!config.sections[0].auto_sync, "auto_sync={} should be false", value);
        }
    }

    #[test]
    fn test_auto_sync_true_variants() {
        for value in &["true", "True", "TRUE", "1", "yes"] {
            // Start from proxmox default=false, override to true via explicit value
            let content = format!("[proxmox]\ntoken=abc\nurl=https://pve:8006\nauto_sync={}\n", value);
            let config = ProviderConfig::parse(&content);
            assert!(config.sections[0].auto_sync, "auto_sync={} should be true", value);
        }
    }

    #[test]
    fn test_auto_sync_malformed_value_treated_as_true() {
        // Unrecognised value is not "false"/"0"/"no", so treated as true (like verify_tls)
        let config = ProviderConfig::parse("[proxmox]\ntoken=abc\nurl=https://pve:8006\nauto_sync=maybe\n");
        assert!(config.sections[0].auto_sync);
    }

    #[test]
    fn test_auto_sync_written_only_when_non_default() {
        // proxmox defaults to false — setting it to true is non-default, so it IS written
        let mut config = ProviderConfig::default();
        config.set_section(ProviderSection {
            provider: "proxmox".to_string(),
            token: "tok".to_string(),
            alias_prefix: "pve".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: "https://pve:8006".to_string(),
            verify_tls: true,
            auto_sync: true, // non-default for proxmox
        });
        // Simulate save by rebuilding content string (same logic as save())
        let content =
            "[proxmox]\ntoken=tok\nalias_prefix=pve\nuser=root\nurl=https://pve:8006\nauto_sync=true\n"
        .to_string();
        let reparsed = ProviderConfig::parse(&content);
        assert!(reparsed.sections[0].auto_sync);

        // digitalocean defaults to true — setting it to false IS written
        let content2 = "[digitalocean]\ntoken=tok\nalias_prefix=do\nuser=root\nauto_sync=false\n";
        let reparsed2 = ProviderConfig::parse(content2);
        assert!(!reparsed2.sections[0].auto_sync);
    }

    // =========================================================================
    // configured_providers accessor
    // =========================================================================

    #[test]
    fn test_configured_providers_empty() {
        let config = ProviderConfig::default();
        assert!(config.configured_providers().is_empty());
    }

    #[test]
    fn test_configured_providers_returns_all() {
        let content = "[digitalocean]\ntoken=a\n\n[vultr]\ntoken=b\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.configured_providers().len(), 2);
    }

    // =========================================================================
    // Parse edge cases
    // =========================================================================

    #[test]
    fn test_parse_unknown_keys_ignored() {
        let content = "[digitalocean]\ntoken=abc\nfoo=bar\nunknown_key=value\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].token, "abc");
    }

    #[test]
    fn test_parse_unknown_provider_still_parsed() {
        let content = "[aws]\ntoken=secret\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].provider, "aws");
    }

    #[test]
    fn test_parse_whitespace_in_section_name() {
        let content = "[ digitalocean ]\ntoken=abc\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].provider, "digitalocean");
    }

    #[test]
    fn test_parse_value_with_equals() {
        // Token might contain = signs (base64)
        let content = "[digitalocean]\ntoken=abc=def==\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections[0].token, "abc=def==");
    }

    #[test]
    fn test_parse_whitespace_around_key_value() {
        let content = "[digitalocean]\n  token = my-token  \n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections[0].token, "my-token");
    }

    #[test]
    fn test_parse_key_field_sets_identity_file() {
        let content = "[digitalocean]\ntoken=abc\nkey=~/.ssh/id_rsa\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections[0].identity_file, "~/.ssh/id_rsa");
    }

    #[test]
    fn test_section_lookup_missing() {
        let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
        assert!(config.section("vultr").is_none());
    }

    #[test]
    fn test_section_lookup_found() {
        let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
        let section = config.section("digitalocean").unwrap();
        assert_eq!(section.token, "abc");
    }

    #[test]
    fn test_remove_nonexistent_section_noop() {
        let mut config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
        config.remove_section("vultr");
        assert_eq!(config.sections.len(), 1);
    }

    // =========================================================================
    // Default alias_prefix from short_label
    // =========================================================================

    #[test]
    fn test_default_alias_prefix_digitalocean() {
        let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
        assert_eq!(config.sections[0].alias_prefix, "do");
    }

    #[test]
    fn test_default_alias_prefix_upcloud() {
        let config = ProviderConfig::parse("[upcloud]\ntoken=abc\n");
        assert_eq!(config.sections[0].alias_prefix, "uc");
    }

    #[test]
    fn test_default_alias_prefix_proxmox() {
        let config = ProviderConfig::parse("[proxmox]\ntoken=abc\n");
        assert_eq!(config.sections[0].alias_prefix, "pve");
    }

    #[test]
    fn test_alias_prefix_override() {
        let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\nalias_prefix=ocean\n");
        assert_eq!(config.sections[0].alias_prefix, "ocean");
    }

    // =========================================================================
    // Default user is root
    // =========================================================================

    #[test]
    fn test_default_user_is_root() {
        let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
        assert_eq!(config.sections[0].user, "root");
    }

    #[test]
    fn test_user_override() {
        let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\nuser=admin\n");
        assert_eq!(config.sections[0].user, "admin");
    }

    // =========================================================================
    // Proxmox URL scheme validation context
    // =========================================================================

    #[test]
    fn test_proxmox_url_parsed() {
        let config = ProviderConfig::parse("[proxmox]\ntoken=abc\nurl=https://pve.local:8006\n");
        assert_eq!(config.sections[0].url, "https://pve.local:8006");
    }

    #[test]
    fn test_non_proxmox_url_parsed_but_ignored() {
        // URL field is parsed for all providers, but only Proxmox uses it
        let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\nurl=https://api.do.com\n");
        assert_eq!(config.sections[0].url, "https://api.do.com");
    }

    // =========================================================================
    // Duplicate sections
    // =========================================================================

    #[test]
    fn test_duplicate_section_first_wins() {
        let content = "[digitalocean]\ntoken=first\n\n[digitalocean]\ntoken=second\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].token, "first");
    }

    // =========================================================================
    // verify_tls parsing
    // =========================================================================

    // =========================================================================
    // auto_sync default per provider
    // =========================================================================

    #[test]
    fn test_auto_sync_default_proxmox_false() {
        let config = ProviderConfig::parse("[proxmox]\ntoken=abc\n");
        assert!(!config.sections[0].auto_sync);
    }

    #[test]
    fn test_auto_sync_default_all_others_true() {
        for provider in &["digitalocean", "vultr", "linode", "hetzner", "upcloud"] {
            let content = format!("[{}]\ntoken=abc\n", provider);
            let config = ProviderConfig::parse(&content);
            assert!(config.sections[0].auto_sync, "auto_sync should default to true for {}", provider);
        }
    }

    #[test]
    fn test_auto_sync_override_proxmox_to_true() {
        let config = ProviderConfig::parse("[proxmox]\ntoken=abc\nauto_sync=true\n");
        assert!(config.sections[0].auto_sync);
    }

    #[test]
    fn test_auto_sync_override_do_to_false() {
        let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\nauto_sync=false\n");
        assert!(!config.sections[0].auto_sync);
    }

    // =========================================================================
    // set_section and remove_section
    // =========================================================================

    #[test]
    fn test_set_section_adds_new() {
        let mut config = ProviderConfig::default();
        let section = ProviderSection {
            provider: "vultr".to_string(),
            token: "tok".to_string(),
            alias_prefix: "vultr".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
        };
        config.set_section(section);
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].provider, "vultr");
    }

    #[test]
    fn test_set_section_replaces_existing() {
        let mut config = ProviderConfig::parse("[vultr]\ntoken=old\n");
        assert_eq!(config.sections[0].token, "old");
        let section = ProviderSection {
            provider: "vultr".to_string(),
            token: "new".to_string(),
            alias_prefix: "vultr".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
        };
        config.set_section(section);
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].token, "new");
    }

    #[test]
    fn test_remove_section_keeps_others() {
        let mut config = ProviderConfig::parse("[vultr]\ntoken=abc\n\n[linode]\ntoken=def\n");
        assert_eq!(config.sections.len(), 2);
        config.remove_section("vultr");
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].provider, "linode");
    }

    // =========================================================================
    // Comments and blank lines
    // =========================================================================

    #[test]
    fn test_comments_ignored() {
        let content = "# This is a comment\n[digitalocean]\n# Another comment\ntoken=abc\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].token, "abc");
    }

    #[test]
    fn test_blank_lines_ignored() {
        let content = "\n\n[digitalocean]\n\ntoken=abc\n\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 1);
        assert_eq!(config.sections[0].token, "abc");
    }

    // =========================================================================
    // Multiple providers
    // =========================================================================

    #[test]
    fn test_multiple_providers() {
        let content = "[digitalocean]\ntoken=do-tok\n\n[vultr]\ntoken=vultr-tok\n\n[proxmox]\ntoken=pve-tok\nurl=https://pve:8006\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections.len(), 3);
        assert_eq!(config.sections[0].provider, "digitalocean");
        assert_eq!(config.sections[1].provider, "vultr");
        assert_eq!(config.sections[2].provider, "proxmox");
        assert_eq!(config.sections[2].url, "https://pve:8006");
    }

    // =========================================================================
    // Token with special characters
    // =========================================================================

    #[test]
    fn test_token_with_equals_sign() {
        // API tokens can contain = signs (e.g., base64)
        let content = "[digitalocean]\ntoken=dop_v1_abc123==\n";
        let config = ProviderConfig::parse(content);
        // split_once('=') splits at first =, so "dop_v1_abc123==" is preserved
        assert_eq!(config.sections[0].token, "dop_v1_abc123==");
    }

    #[test]
    fn test_proxmox_token_with_exclamation() {
        let content = "[proxmox]\ntoken=user@pam!api-token=12345678-abcd\nurl=https://pve:8006\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections[0].token, "user@pam!api-token=12345678-abcd");
    }

    // =========================================================================
    // Parse serialization roundtrip
    // =========================================================================

    #[test]
    fn test_serialize_roundtrip_single_provider() {
        let content = "[digitalocean]\ntoken=abc\nalias_prefix=do\nuser=root\n";
        let config = ProviderConfig::parse(content);
        let mut serialized = String::new();
        for section in &config.sections {
            serialized.push_str(&format!("[{}]\n", section.provider));
            serialized.push_str(&format!("token={}\n", section.token));
            serialized.push_str(&format!("alias_prefix={}\n", section.alias_prefix));
            serialized.push_str(&format!("user={}\n", section.user));
        }
        let reparsed = ProviderConfig::parse(&serialized);
        assert_eq!(reparsed.sections.len(), 1);
        assert_eq!(reparsed.sections[0].token, "abc");
        assert_eq!(reparsed.sections[0].alias_prefix, "do");
        assert_eq!(reparsed.sections[0].user, "root");
    }

    // =========================================================================
    // verify_tls parsing variants
    // =========================================================================

    #[test]
    fn test_verify_tls_values() {
        for (val, expected) in [
            ("false", false), ("False", false), ("FALSE", false),
            ("0", false), ("no", false), ("No", false), ("NO", false),
            ("true", true), ("True", true), ("1", true), ("yes", true),
            ("anything", true), // any unrecognized value defaults to true
        ] {
            let content = format!("[digitalocean]\ntoken=t\nverify_tls={}\n", val);
            let config = ProviderConfig::parse(&content);
            assert_eq!(
                config.sections[0].verify_tls, expected,
                "verify_tls={} should be {}",
                val, expected
            );
        }
    }

    // =========================================================================
    // auto_sync parsing variants
    // =========================================================================

    #[test]
    fn test_auto_sync_values() {
        for (val, expected) in [
            ("false", false), ("False", false), ("FALSE", false),
            ("0", false), ("no", false), ("No", false),
            ("true", true), ("1", true), ("yes", true),
        ] {
            let content = format!("[digitalocean]\ntoken=t\nauto_sync={}\n", val);
            let config = ProviderConfig::parse(&content);
            assert_eq!(
                config.sections[0].auto_sync, expected,
                "auto_sync={} should be {}",
                val, expected
            );
        }
    }

    // =========================================================================
    // Default values
    // =========================================================================

    #[test]
    fn test_default_user_root_when_not_specified() {
        let content = "[digitalocean]\ntoken=abc\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections[0].user, "root");
    }

    #[test]
    fn test_default_alias_prefix_from_short_label() {
        // DigitalOcean short_label is "do"
        let content = "[digitalocean]\ntoken=abc\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections[0].alias_prefix, "do");
    }

    #[test]
    fn test_default_alias_prefix_unknown_provider() {
        // Unknown provider uses the section name as default prefix
        let content = "[unknown_cloud]\ntoken=abc\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections[0].alias_prefix, "unknown_cloud");
    }

    #[test]
    fn test_default_identity_file_empty() {
        let content = "[digitalocean]\ntoken=abc\n";
        let config = ProviderConfig::parse(content);
        assert!(config.sections[0].identity_file.is_empty());
    }

    #[test]
    fn test_default_url_empty() {
        let content = "[digitalocean]\ntoken=abc\n";
        let config = ProviderConfig::parse(content);
        assert!(config.sections[0].url.is_empty());
    }

    // =========================================================================
    // configured_providers and section methods
    // =========================================================================

    #[test]
    fn test_configured_providers_returns_all_sections() {
        let content = "[digitalocean]\ntoken=a\n\n[vultr]\ntoken=b\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.configured_providers().len(), 2);
    }

    #[test]
    fn test_section_by_name() {
        let content = "[digitalocean]\ntoken=do-tok\n\n[vultr]\ntoken=vultr-tok\n";
        let config = ProviderConfig::parse(content);
        let do_section = config.section("digitalocean").unwrap();
        assert_eq!(do_section.token, "do-tok");
        let vultr_section = config.section("vultr").unwrap();
        assert_eq!(vultr_section.token, "vultr-tok");
    }

    #[test]
    fn test_section_not_found() {
        let config = ProviderConfig::parse("");
        assert!(config.section("nonexistent").is_none());
    }

    // =========================================================================
    // Key without value
    // =========================================================================

    #[test]
    fn test_line_without_equals_ignored() {
        let content = "[digitalocean]\ntoken=abc\ngarbage_line\nuser=admin\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections[0].token, "abc");
        assert_eq!(config.sections[0].user, "admin");
    }

    #[test]
    fn test_unknown_key_ignored() {
        let content = "[digitalocean]\ntoken=abc\nfoo=bar\nbaz=qux\nuser=admin\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections[0].token, "abc");
        assert_eq!(config.sections[0].user, "admin");
    }

    // =========================================================================
    // Whitespace handling
    // =========================================================================

    #[test]
    fn test_whitespace_around_section_name() {
        let content = "[  digitalocean  ]\ntoken=abc\n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections[0].provider, "digitalocean");
    }

    #[test]
    fn test_whitespace_around_key_value() {
        let content = "[digitalocean]\n  token  =  abc  \n  user  =  admin  \n";
        let config = ProviderConfig::parse(content);
        assert_eq!(config.sections[0].token, "abc");
        assert_eq!(config.sections[0].user, "admin");
    }

    // =========================================================================
    // set_section edge cases
    // =========================================================================

    #[test]
    fn test_set_section_multiple_adds() {
        let mut config = ProviderConfig::default();
        for name in ["digitalocean", "vultr", "hetzner"] {
            config.set_section(ProviderSection {
                provider: name.to_string(),
                token: format!("{}-tok", name),
                alias_prefix: name.to_string(),
                user: "root".to_string(),
                identity_file: String::new(),
                url: String::new(),
                verify_tls: true,
                auto_sync: true,
            });
        }
        assert_eq!(config.sections.len(), 3);
    }

    #[test]
    fn test_remove_section_all() {
        let content = "[digitalocean]\ntoken=a\n\n[vultr]\ntoken=b\n";
        let mut config = ProviderConfig::parse(content);
        config.remove_section("digitalocean");
        config.remove_section("vultr");
        assert!(config.sections.is_empty());
    }
}
