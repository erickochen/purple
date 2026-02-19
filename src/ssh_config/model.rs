use std::path::PathBuf;

/// Represents the entire SSH config file as a sequence of elements.
/// Preserves the original structure for round-trip fidelity.
#[derive(Debug, Clone)]
pub struct SshConfigFile {
    pub elements: Vec<ConfigElement>,
    pub path: PathBuf,
}

/// A single element in the config file.
#[derive(Debug, Clone)]
pub enum ConfigElement {
    /// A Host block: the "Host <pattern>" line plus all indented directives.
    HostBlock(HostBlock),
    /// A comment, blank line, or global directive not inside a Host block.
    GlobalLine(String),
}

/// A parsed Host block with its directives.
#[derive(Debug, Clone)]
pub struct HostBlock {
    /// The host alias/pattern (the value after "Host").
    pub host_pattern: String,
    /// The original raw "Host ..." line for faithful reproduction.
    pub raw_host_line: String,
    /// Parsed directives inside this block.
    pub directives: Vec<Directive>,
}

/// A directive line inside a Host block.
#[derive(Debug, Clone)]
pub struct Directive {
    /// The directive key (e.g., "HostName", "User", "Port").
    pub key: String,
    /// The directive value.
    pub value: String,
    /// The original raw line (preserves indentation, inline comments).
    pub raw_line: String,
    /// Whether this is a comment-only or blank line inside a host block.
    pub is_non_directive: bool,
}

/// Convenience view for the TUI — extracted from a HostBlock.
#[derive(Debug, Clone, Default)]
pub struct HostEntry {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub identity_file: String,
    pub proxy_jump: String,
}

impl HostBlock {
    /// Extract a convenience HostEntry view from this block.
    pub fn to_host_entry(&self) -> HostEntry {
        let mut entry = HostEntry {
            alias: self.host_pattern.clone(),
            port: 22,
            ..Default::default()
        };
        for d in &self.directives {
            if d.is_non_directive {
                continue;
            }
            match d.key.to_lowercase().as_str() {
                "hostname" => entry.hostname = d.value.clone(),
                "user" => entry.user = d.value.clone(),
                "port" => entry.port = d.value.parse().unwrap_or(22),
                "identityfile" => entry.identity_file = d.value.clone(),
                "proxyjump" => entry.proxy_jump = d.value.clone(),
                _ => {}
            }
        }
        entry
    }
}

impl SshConfigFile {
    /// Get all host entries as convenience views.
    pub fn host_entries(&self) -> Vec<HostEntry> {
        self.elements
            .iter()
            .filter_map(|e| match e {
                ConfigElement::HostBlock(block) => {
                    // Skip wildcard/multi patterns (*, ?, space-separated)
                    if block.host_pattern.contains('*')
                        || block.host_pattern.contains('?')
                        || block.host_pattern.contains(' ')
                    {
                        None
                    } else {
                        Some(block.to_host_entry())
                    }
                }
                ConfigElement::GlobalLine(_) => None,
            })
            .collect()
    }

    /// Find a host block by alias.
    pub fn find_host(&self, alias: &str) -> Option<&HostBlock> {
        self.elements.iter().find_map(|e| match e {
            ConfigElement::HostBlock(block) if block.host_pattern == alias => Some(block),
            _ => None,
        })
    }

    /// Check if a host alias already exists.
    pub fn has_host(&self, alias: &str) -> bool {
        self.find_host(alias).is_some()
    }

    /// Add a new host entry to the config.
    pub fn add_host(&mut self, entry: &HostEntry) {
        let block = Self::entry_to_block(entry);
        // Add a blank line separator if the file isn't empty
        if !self.elements.is_empty() {
            self.elements
                .push(ConfigElement::GlobalLine(String::new()));
        }
        self.elements.push(ConfigElement::HostBlock(block));
    }

    /// Update an existing host entry by alias.
    /// Merges changes into the existing block, preserving unknown directives.
    pub fn update_host(&mut self, old_alias: &str, entry: &HostEntry) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == old_alias {
                    // Update host pattern
                    block.host_pattern = entry.alias.clone();
                    block.raw_host_line = format!("Host {}", entry.alias);

                    // Merge known directives (update existing, add missing, remove empty)
                    Self::upsert_directive(block, "HostName", &entry.hostname);
                    Self::upsert_directive(block, "User", &entry.user);
                    if entry.port != 22 {
                        Self::upsert_directive(block, "Port", &entry.port.to_string());
                    } else {
                        // Remove explicit Port 22 (it's the default)
                        block
                            .directives
                            .retain(|d| d.is_non_directive || d.key.to_lowercase() != "port");
                    }
                    Self::upsert_directive(block, "IdentityFile", &entry.identity_file);
                    Self::upsert_directive(block, "ProxyJump", &entry.proxy_jump);
                    return;
                }
            }
        }
    }

    /// Update a directive in-place, add it if missing, or remove it if value is empty.
    fn upsert_directive(block: &mut HostBlock, key: &str, value: &str) {
        if value.is_empty() {
            block
                .directives
                .retain(|d| d.is_non_directive || d.key.to_lowercase() != key.to_lowercase());
            return;
        }
        for d in &mut block.directives {
            if !d.is_non_directive && d.key.to_lowercase() == key.to_lowercase() {
                d.value = value.to_string();
                d.raw_line = format!("  {} {}", key, value);
                return;
            }
        }
        // Not found — append
        block.directives.push(Directive {
            key: key.to_string(),
            value: value.to_string(),
            raw_line: format!("  {} {}", key, value),
            is_non_directive: false,
        });
    }

    /// Delete a host entry by alias.
    pub fn delete_host(&mut self, alias: &str) {
        self.elements.retain(|e| match e {
            ConfigElement::HostBlock(block) => block.host_pattern != alias,
            _ => true,
        });
        // Collapse consecutive blank lines left by deletion
        self.elements.dedup_by(|a, b| {
            matches!(
                (&*a, &*b),
                (ConfigElement::GlobalLine(x), ConfigElement::GlobalLine(y))
                if x.trim().is_empty() && y.trim().is_empty()
            )
        });
    }

    /// Convert a HostEntry into a new HostBlock with clean formatting.
    fn entry_to_block(entry: &HostEntry) -> HostBlock {
        let mut directives = Vec::new();

        if !entry.hostname.is_empty() {
            directives.push(Directive {
                key: "HostName".to_string(),
                value: entry.hostname.clone(),
                raw_line: format!("  HostName {}", entry.hostname),
                is_non_directive: false,
            });
        }
        if !entry.user.is_empty() {
            directives.push(Directive {
                key: "User".to_string(),
                value: entry.user.clone(),
                raw_line: format!("  User {}", entry.user),
                is_non_directive: false,
            });
        }
        if entry.port != 22 {
            directives.push(Directive {
                key: "Port".to_string(),
                value: entry.port.to_string(),
                raw_line: format!("  Port {}", entry.port),
                is_non_directive: false,
            });
        }
        if !entry.identity_file.is_empty() {
            directives.push(Directive {
                key: "IdentityFile".to_string(),
                value: entry.identity_file.clone(),
                raw_line: format!("  IdentityFile {}", entry.identity_file),
                is_non_directive: false,
            });
        }
        if !entry.proxy_jump.is_empty() {
            directives.push(Directive {
                key: "ProxyJump".to_string(),
                value: entry.proxy_jump.clone(),
                raw_line: format!("  ProxyJump {}", entry.proxy_jump),
                is_non_directive: false,
            });
        }

        HostBlock {
            host_pattern: entry.alias.clone(),
            raw_host_line: format!("Host {}", entry.alias),
            directives,
        }
    }
}
