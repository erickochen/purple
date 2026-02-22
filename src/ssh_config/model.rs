use std::path::PathBuf;

/// Represents the entire SSH config file as a sequence of elements.
/// Preserves the original structure for round-trip fidelity.
#[derive(Debug, Clone)]
pub struct SshConfigFile {
    pub elements: Vec<ConfigElement>,
    pub path: PathBuf,
    /// Whether the original file used CRLF line endings.
    pub crlf: bool,
}

/// An Include directive that references other config files.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IncludeDirective {
    pub raw_line: String,
    pub pattern: String,
    pub resolved_files: Vec<IncludedFile>,
}

/// A file resolved from an Include directive.
#[derive(Debug, Clone)]
pub struct IncludedFile {
    pub path: PathBuf,
    pub elements: Vec<ConfigElement>,
}

/// A single element in the config file.
#[derive(Debug, Clone)]
pub enum ConfigElement {
    /// A Host block: the "Host <pattern>" line plus all indented directives.
    HostBlock(HostBlock),
    /// A comment, blank line, or global directive not inside a Host block.
    GlobalLine(String),
    /// An Include directive referencing other config files (read-only).
    Include(IncludeDirective),
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
    /// If this host comes from an included file, the file path.
    pub source_file: Option<PathBuf>,
    /// Tags from purple:tags comment.
    pub tags: Vec<String>,
}

impl HostEntry {
    /// Build the SSH command string for this host (e.g. "ssh -- 'myserver'").
    /// Shell-quotes the alias to prevent injection when pasted into a terminal.
    pub fn ssh_command(&self) -> String {
        let escaped = self.alias.replace('\'', "'\\''");
        format!("ssh -- '{}'", escaped)
    }
}

impl HostBlock {
    /// Index of the first trailing blank line (for inserting content before separators).
    fn content_end(&self) -> usize {
        let mut pos = self.directives.len();
        while pos > 0 {
            if self.directives[pos - 1].is_non_directive
                && self.directives[pos - 1].raw_line.trim().is_empty()
            {
                pos -= 1;
            } else {
                break;
            }
        }
        pos
    }

    /// Remove and return trailing blank lines.
    fn pop_trailing_blanks(&mut self) -> Vec<Directive> {
        let end = self.content_end();
        self.directives.drain(end..).collect()
    }

    /// Ensure exactly one trailing blank line.
    fn ensure_trailing_blank(&mut self) {
        self.pop_trailing_blanks();
        self.directives.push(Directive {
            key: String::new(),
            value: String::new(),
            raw_line: String::new(),
            is_non_directive: true,
        });
    }

    /// Detect indentation used by existing directives (falls back to "  ").
    fn detect_indent(&self) -> String {
        for d in &self.directives {
            if !d.is_non_directive && !d.raw_line.is_empty() {
                let trimmed = d.raw_line.trim_start();
                let indent_len = d.raw_line.len() - trimmed.len();
                if indent_len > 0 {
                    return d.raw_line[..indent_len].to_string();
                }
            }
        }
        "  ".to_string()
    }

    /// Extract tags from purple:tags comment in directives.
    pub fn tags(&self) -> Vec<String> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:tags ") {
                    return rest
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect();
                }
            }
        }
        Vec::new()
    }

    /// Set tags on a host block. Replaces existing purple:tags comment or adds one.
    pub fn set_tags(&mut self, tags: &[String]) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && d.raw_line.trim().starts_with("# purple:tags"))
        });
        if !tags.is_empty() {
            let pos = self.content_end();
            self.directives.insert(
                pos,
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: format!("{}# purple:tags {}", indent, tags.join(",")),
                    is_non_directive: true,
                },
            );
        }
    }

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
                "identityfile" => {
                    if entry.identity_file.is_empty() {
                        entry.identity_file = d.value.clone();
                    }
                }
                "proxyjump" => entry.proxy_jump = d.value.clone(),
                _ => {}
            }
        }
        entry.tags = self.tags();
        entry
    }
}

impl SshConfigFile {
    /// Get all host entries as convenience views (including from Include files).
    pub fn host_entries(&self) -> Vec<HostEntry> {
        Self::collect_host_entries(&self.elements)
    }

    /// Collect all resolved Include file paths (recursively).
    pub fn include_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        Self::collect_include_paths(&self.elements, &mut paths);
        paths
    }

    fn collect_include_paths(elements: &[ConfigElement], paths: &mut Vec<PathBuf>) {
        for e in elements {
            if let ConfigElement::Include(include) = e {
                for file in &include.resolved_files {
                    paths.push(file.path.clone());
                    Self::collect_include_paths(&file.elements, paths);
                }
            }
        }
    }

    /// Collect parent directories of Include glob patterns.
    /// When a file is added/removed under a glob dir, the directory's mtime changes.
    pub fn include_glob_dirs(&self) -> Vec<PathBuf> {
        let config_dir = self.path.parent();
        let mut dirs = Vec::new();
        Self::collect_include_glob_dirs(&self.elements, config_dir, &mut dirs);
        dirs
    }

    fn collect_include_glob_dirs(
        elements: &[ConfigElement],
        config_dir: Option<&std::path::Path>,
        dirs: &mut Vec<PathBuf>,
    ) {
        for e in elements {
            if let ConfigElement::Include(include) = e {
                let expanded = Self::expand_tilde(&include.pattern);
                let resolved = if expanded.starts_with('/') {
                    PathBuf::from(&expanded)
                } else if let Some(dir) = config_dir {
                    dir.join(&expanded)
                } else {
                    continue;
                };
                if let Some(parent) = resolved.parent() {
                    let parent = parent.to_path_buf();
                    if !dirs.contains(&parent) {
                        dirs.push(parent);
                    }
                }
                // Recurse into resolved files
                for file in &include.resolved_files {
                    Self::collect_include_glob_dirs(
                        &file.elements,
                        file.path.parent(),
                        dirs,
                    );
                }
            }
        }
    }


    /// Recursively collect host entries from a list of elements.
    fn collect_host_entries(elements: &[ConfigElement]) -> Vec<HostEntry> {
        let mut entries = Vec::new();
        for e in elements {
            match e {
                ConfigElement::HostBlock(block) => {
                    // Skip wildcard/multi patterns (*, ?, whitespace-separated)
                    if block.host_pattern.contains('*')
                        || block.host_pattern.contains('?')
                        || block.host_pattern.contains(' ')
                        || block.host_pattern.contains('\t')
                    {
                        continue;
                    }
                    entries.push(block.to_host_entry());
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        let mut file_entries = Self::collect_host_entries(&file.elements);
                        for entry in &mut file_entries {
                            if entry.source_file.is_none() {
                                entry.source_file = Some(file.path.clone());
                            }
                        }
                        entries.extend(file_entries);
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
        entries
    }

    /// Check if a host alias already exists (including in Include files).
    /// Walks the element tree directly without building HostEntry structs.
    pub fn has_host(&self, alias: &str) -> bool {
        Self::has_host_in_elements(&self.elements, alias)
    }

    fn has_host_in_elements(elements: &[ConfigElement], alias: &str) -> bool {
        for e in elements {
            match e {
                ConfigElement::HostBlock(block) => {
                    if block.host_pattern.split_whitespace().any(|p| p == alias) {
                        return true;
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        if Self::has_host_in_elements(&file.elements, alias) {
                            return true;
                        }
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
        false
    }

    /// Add a new host entry to the config.
    pub fn add_host(&mut self, entry: &HostEntry) {
        let block = Self::entry_to_block(entry);
        // Add a blank line separator if the file isn't empty and doesn't already end with one
        if !self.elements.is_empty() && !self.last_element_has_trailing_blank() {
            self.elements
                .push(ConfigElement::GlobalLine(String::new()));
        }
        self.elements.push(ConfigElement::HostBlock(block));
    }

    /// Check if the last element already ends with a blank line.
    pub fn last_element_has_trailing_blank(&self) -> bool {
        match self.elements.last() {
            Some(ConfigElement::HostBlock(block)) => block
                .directives
                .last()
                .is_some_and(|d| d.is_non_directive && d.raw_line.trim().is_empty()),
            Some(ConfigElement::GlobalLine(line)) => line.trim().is_empty(),
            _ => false,
        }
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
        let indent = block.detect_indent();
        for d in &mut block.directives {
            if !d.is_non_directive && d.key.to_lowercase() == key.to_lowercase() {
                // Only rebuild raw_line when value actually changed (preserves inline comments)
                if d.value != value {
                    d.value = value.to_string();
                    d.raw_line = format!("{}{} {}", indent, d.key, value);
                }
                return;
            }
        }
        // Not found — insert before trailing blanks
        let pos = block.content_end();
        block.directives.insert(
            pos,
            Directive {
                key: key.to_string(),
                value: value.to_string(),
                raw_line: format!("{}{} {}", indent, key, value),
                is_non_directive: false,
            },
        );
    }

    /// Set tags on a host block by alias.
    pub fn set_host_tags(&mut self, alias: &str, tags: &[String]) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    block.set_tags(tags);
                    return;
                }
            }
        }
    }

    /// Delete a host entry by alias.
    #[allow(dead_code)]
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

    /// Delete a host and return the removed element and its position for undo.
    /// Does NOT collapse blank lines so the position stays valid for re-insertion.
    pub fn delete_host_undoable(&mut self, alias: &str) -> Option<(ConfigElement, usize)> {
        let pos = self.elements.iter().position(|e| {
            matches!(e, ConfigElement::HostBlock(b) if b.host_pattern == alias)
        })?;
        let element = self.elements.remove(pos);
        Some((element, pos))
    }

    /// Insert a host block at a specific position (for undo).
    pub fn insert_host_at(&mut self, element: ConfigElement, position: usize) {
        let pos = position.min(self.elements.len());
        self.elements.insert(pos, element);
    }

    /// Swap two host blocks in the config by alias. Returns true if swap was performed.
    #[allow(dead_code)]
    pub fn swap_hosts(&mut self, alias_a: &str, alias_b: &str) -> bool {
        let pos_a = self.elements.iter().position(|e| {
            matches!(e, ConfigElement::HostBlock(b) if b.host_pattern == alias_a)
        });
        let pos_b = self.elements.iter().position(|e| {
            matches!(e, ConfigElement::HostBlock(b) if b.host_pattern == alias_b)
        });
        if let (Some(a), Some(b)) = (pos_a, pos_b) {
            let (first, second) = (a.min(b), a.max(b));

            // Strip trailing blanks from both blocks before swap
            if let ConfigElement::HostBlock(block) = &mut self.elements[first] {
                block.pop_trailing_blanks();
            }
            if let ConfigElement::HostBlock(block) = &mut self.elements[second] {
                block.pop_trailing_blanks();
            }

            // Swap
            self.elements.swap(first, second);

            // Add trailing blank to first block (separator between the two)
            if let ConfigElement::HostBlock(block) = &mut self.elements[first] {
                block.ensure_trailing_blank();
            }

            // Add trailing blank to second only if not the last element
            if second < self.elements.len() - 1 {
                if let ConfigElement::HostBlock(block) = &mut self.elements[second] {
                    block.ensure_trailing_blank();
                }
            }

            return true;
        }
        false
    }

    /// Convert a HostEntry into a new HostBlock with clean formatting.
    pub(crate) fn entry_to_block(entry: &HostEntry) -> HostBlock {
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
