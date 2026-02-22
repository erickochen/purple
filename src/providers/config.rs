use std::io;
use std::path::PathBuf;

/// A configured provider section from ~/.purple/providers.
#[derive(Debug, Clone)]
pub struct ProviderSection {
    pub provider: String,
    pub token: String,
    pub alias_prefix: String,
    pub user: String,
    pub identity_file: String,
}

/// Parsed provider configuration from ~/.purple/providers.
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    pub sections: Vec<ProviderSection>,
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
                    sections.push(section);
                }
                let name = trimmed[1..trimmed.len() - 1].trim().to_string();
                let short_label = super::get_provider(&name)
                    .map(|p| p.short_label().to_string())
                    .unwrap_or_else(|| name.clone());
                current = Some(ProviderSection {
                    provider: name,
                    token: String::new(),
                    alias_prefix: short_label,
                    user: "root".to_string(),
                    identity_file: String::new(),
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
                        _ => {}
                    }
                }
            }
        }
        if let Some(section) = current {
            sections.push(section);
        }
        Self { sections }
    }

    /// Save provider config to ~/.purple/providers (atomic write, chmod 600).
    pub fn save(&self) -> io::Result<()> {
        let path = match config_path() {
            Some(p) => p,
            None => return Ok(()),
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

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
        }

        let tmp_path = path.with_extension(format!("tmp.{}", std::process::id()));

        #[cfg(unix)]
        {
            use std::fs::OpenOptions;
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp_path)?;
            file.write_all(content.as_bytes())?;
        }

        #[cfg(not(unix))]
        std::fs::write(&tmp_path, &content)?;

        let result = std::fs::rename(&tmp_path, &path);
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp_path);
        }
        result?;
        Ok(())
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
    fn test_defaults_applied() {
        let config = ProviderConfig::parse("[hetzner]\ntoken=abc\n");
        let s = &config.sections[0];
        assert_eq!(s.user, "root");
        assert_eq!(s.alias_prefix, "hetzner");
        assert!(s.identity_file.is_empty());
    }
}
