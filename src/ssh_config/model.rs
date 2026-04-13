use std::path::PathBuf;

/// Display name for a provider used in `# purple:group` headers.
/// Mirrors `providers::provider_display_name()` without a cross-module dependency.
fn provider_group_display_name(name: &str) -> &str {
    match name {
        "digitalocean" => "DigitalOcean",
        "vultr" => "Vultr",
        "linode" => "Linode",
        "hetzner" => "Hetzner",
        "upcloud" => "UpCloud",
        "proxmox" => "Proxmox VE",
        "aws" => "AWS EC2",
        "scaleway" => "Scaleway",
        "gcp" => "GCP",
        "azure" => "Azure",
        "tailscale" => "Tailscale",
        "oracle" => "Oracle Cloud",
        other => other,
    }
}

/// Represents the entire SSH config file as a sequence of elements.
/// Preserves the original structure for round-trip fidelity.
#[derive(Debug, Clone)]
pub struct SshConfigFile {
    pub elements: Vec<ConfigElement>,
    pub path: PathBuf,
    /// Whether the original file used CRLF line endings.
    pub crlf: bool,
    /// Whether the original file started with a UTF-8 BOM.
    pub bom: bool,
}

/// An Include directive that references other config files.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct HostEntry {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub identity_file: String,
    pub proxy_jump: String,
    /// If this host comes from an included file, the file path.
    pub source_file: Option<PathBuf>,
    /// User-added tags from purple:tags comment.
    pub tags: Vec<String>,
    /// Provider-synced tags from purple:provider_tags comment.
    pub provider_tags: Vec<String>,
    /// Whether a purple:provider_tags comment exists (distinguishes "never migrated" from "empty").
    pub has_provider_tags: bool,
    /// Cloud provider label from purple:provider comment (e.g. "do", "vultr").
    pub provider: Option<String>,
    /// Number of tunnel forwarding directives.
    pub tunnel_count: u16,
    /// Password source from purple:askpass comment (e.g. "keychain", "op://...", "pass:...").
    pub askpass: Option<String>,
    /// Vault SSH certificate signing role from purple:vault-ssh comment.
    pub vault_ssh: Option<String>,
    /// Optional Vault HTTP endpoint from purple:vault-addr comment. When
    /// set, purple passes it as `VAULT_ADDR` to the `vault` subprocess for
    /// this host's signing, overriding the parent shell. Empty = inherit env.
    pub vault_addr: Option<String>,
    /// CertificateFile directive value (e.g. "~/.ssh/my-cert.pub").
    pub certificate_file: String,
    /// Provider metadata from purple:meta comment (region, plan, etc.).
    pub provider_meta: Vec<(String, String)>,
    /// Unix timestamp when the host was marked stale (disappeared from provider sync).
    pub stale: Option<u64>,
}

impl Default for HostEntry {
    fn default() -> Self {
        Self {
            alias: String::new(),
            hostname: String::new(),
            user: String::new(),
            port: 22,
            identity_file: String::new(),
            proxy_jump: String::new(),
            source_file: None,
            tags: Vec::new(),
            provider_tags: Vec::new(),
            has_provider_tags: false,
            provider: None,
            tunnel_count: 0,
            askpass: None,
            vault_ssh: None,
            vault_addr: None,
            certificate_file: String::new(),
            provider_meta: Vec::new(),
            stale: None,
        }
    }
}

impl HostEntry {
    /// Build the SSH command string for this host.
    /// Includes `-F <config_path>` when the config is non-default so the alias
    /// resolves correctly when pasted into a terminal.
    /// Shell-quotes both the config path and alias to prevent injection.
    pub fn ssh_command(&self, config_path: &std::path::Path) -> String {
        let escaped = self.alias.replace('\'', "'\\''");
        let default = dirs::home_dir()
            .map(|h| h.join(".ssh/config"))
            .unwrap_or_default();
        if config_path == default {
            format!("ssh -- '{}'", escaped)
        } else {
            let config_escaped = config_path.display().to_string().replace('\'', "'\\''");
            format!("ssh -F '{}' -- '{}'", config_escaped, escaped)
        }
    }
}

/// Convenience view for pattern Host blocks in the TUI.
#[derive(Debug, Clone, Default)]
pub struct PatternEntry {
    pub pattern: String,
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub identity_file: String,
    pub proxy_jump: String,
    pub tags: Vec<String>,
    pub askpass: Option<String>,
    pub source_file: Option<PathBuf>,
    /// All non-comment directives as key-value pairs for display.
    pub directives: Vec<(String, String)>,
}

/// Inherited field hints from matching patterns. Each field is `Some((value,
/// source_pattern))` when a pattern provides that directive, `None` otherwise.
#[derive(Debug, Clone, Default)]
pub struct InheritedHints {
    pub proxy_jump: Option<(String, String)>,
    pub user: Option<(String, String)>,
    pub identity_file: Option<(String, String)>,
}

/// Returns true if the host pattern contains wildcards, character classes,
/// negation or whitespace-separated multi-patterns (*, ?, [], !, space/tab).
/// These are SSH match patterns, not concrete hosts.
pub fn is_host_pattern(pattern: &str) -> bool {
    pattern.contains('*')
        || pattern.contains('?')
        || pattern.contains('[')
        || pattern.starts_with('!')
        || pattern.contains(' ')
        || pattern.contains('\t')
}

/// Match a text string against an SSH host pattern.
/// Supports `*` (any sequence), `?` (single char), `[charset]` (character class),
/// `[!charset]`/`[^charset]` (negated class), `[a-z]` (ranges) and `!pattern` (negation).
pub fn ssh_pattern_match(pattern: &str, text: &str) -> bool {
    if let Some(rest) = pattern.strip_prefix('!') {
        return !match_glob(rest, text);
    }
    match_glob(pattern, text)
}

/// Core glob matcher without negation prefix handling.
/// Empty text only matches empty pattern.
fn match_glob(pattern: &str, text: &str) -> bool {
    if text.is_empty() {
        return pattern.is_empty();
    }
    if pattern.is_empty() {
        return false;
    }
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match(&pat, &txt)
}

/// Iterative glob matching with star-backtracking.
fn glob_match(pat: &[char], txt: &[char]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star: Option<(usize, usize)> = None; // (pattern_pos, text_pos)

    while ti < txt.len() {
        if pi < pat.len() && pat[pi] == '?' {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == '*' {
            star = Some((pi + 1, ti));
            pi += 1;
        } else if pi < pat.len() && pat[pi] == '[' {
            if let Some((matches, end)) = match_char_class(pat, pi, txt[ti]) {
                if matches {
                    pi = end;
                    ti += 1;
                } else if let Some((spi, sti)) = star {
                    let sti = sti + 1;
                    star = Some((spi, sti));
                    pi = spi;
                    ti = sti;
                } else {
                    return false;
                }
            } else if let Some((spi, sti)) = star {
                // Malformed class: backtrack
                let sti = sti + 1;
                star = Some((spi, sti));
                pi = spi;
                ti = sti;
            } else {
                return false;
            }
        } else if pi < pat.len() && pat[pi] == txt[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((spi, sti)) = star {
            let sti = sti + 1;
            star = Some((spi, sti));
            pi = spi;
            ti = sti;
        } else {
            return false;
        }
    }

    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }
    pi == pat.len()
}

/// Parse and match a `[...]` character class starting at `pat[start]`.
/// Returns `Some((matched, end_index))` where `end_index` is past `]`.
/// Returns `None` if no closing `]` is found.
fn match_char_class(pat: &[char], start: usize, ch: char) -> Option<(bool, usize)> {
    let mut i = start + 1;
    if i >= pat.len() {
        return None;
    }

    let negate = pat[i] == '!' || pat[i] == '^';
    if negate {
        i += 1;
    }

    let mut matched = false;
    while i < pat.len() && pat[i] != ']' {
        if i + 2 < pat.len() && pat[i + 1] == '-' && pat[i + 2] != ']' {
            let lo = pat[i];
            let hi = pat[i + 2];
            if ch >= lo && ch <= hi {
                matched = true;
            }
            i += 3;
        } else {
            matched |= pat[i] == ch;
            i += 1;
        }
    }

    if i >= pat.len() {
        return None;
    }

    let result = if negate { !matched } else { matched };
    Some((result, i + 1))
}

/// Check whether a `Host` pattern matches a given alias.
/// OpenSSH `Host` keyword matches only against the target alias typed on the
/// command line, never against the resolved HostName.
pub fn host_pattern_matches(host_pattern: &str, alias: &str) -> bool {
    let patterns: Vec<&str> = host_pattern.split_whitespace().collect();
    if patterns.is_empty() {
        return false;
    }

    let mut any_positive_match = false;
    for pat in &patterns {
        if let Some(neg) = pat.strip_prefix('!') {
            if match_glob(neg, alias) {
                return false;
            }
        } else if ssh_pattern_match(pat, alias) {
            any_positive_match = true;
        }
    }

    any_positive_match
}

/// Returns true if any hop in a (possibly comma-separated) ProxyJump value
/// matches the given alias. Strips optional `user@` prefix and `:port`
/// suffix from each hop before comparing. Handles IPv6 bracket notation
/// `[addr]:port`. Used to detect self-referencing loops.
pub fn proxy_jump_contains_self(proxy_jump: &str, alias: &str) -> bool {
    proxy_jump.split(',').any(|hop| {
        let h = hop.trim();
        // Strip optional user@ prefix (take everything after the first @).
        let h = h.split_once('@').map_or(h, |(_, host)| host);
        // Strip optional :port suffix. Handle [IPv6]:port bracket notation.
        let h = if let Some(bracketed) = h.strip_prefix('[') {
            bracketed.split_once(']').map_or(h, |(host, _)| host)
        } else {
            h.rsplit_once(':').map_or(h, |(host, _)| host)
        };
        h == alias
    })
}

/// Apply first-match-wins inheritance from a pattern to mutable field refs.
/// Only fills fields that are still empty. Self-referencing ProxyJump values
/// are assigned (SSH would do the same) so the UI can warn about the loop.
fn apply_first_match_fields(
    proxy_jump: &mut String,
    user: &mut String,
    identity_file: &mut String,
    p: &PatternEntry,
) {
    if proxy_jump.is_empty() && !p.proxy_jump.is_empty() {
        proxy_jump.clone_from(&p.proxy_jump);
    }
    if user.is_empty() && !p.user.is_empty() {
        user.clone_from(&p.user);
    }
    if identity_file.is_empty() && !p.identity_file.is_empty() {
        identity_file.clone_from(&p.identity_file);
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
    #[allow(dead_code)]
    fn pop_trailing_blanks(&mut self) -> Vec<Directive> {
        let end = self.content_end();
        self.directives.drain(end..).collect()
    }

    /// Ensure exactly one trailing blank line.
    #[allow(dead_code)]
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

    /// Extract provider-synced tags from purple:provider_tags comment.
    pub fn provider_tags(&self) -> Vec<String> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:provider_tags ") {
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

    /// Check if a purple:provider_tags comment exists (even if empty).
    /// Used to distinguish "never migrated" from "migrated with no tags".
    pub fn has_provider_tags_comment(&self) -> bool {
        self.directives.iter().any(|d| {
            d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:provider_tags" || t.starts_with("# purple:provider_tags ")
            }
        })
    }

    /// Extract provider info from purple:provider comment in directives.
    /// Returns (provider_name, server_id), e.g. ("digitalocean", "412345678").
    pub fn provider(&self) -> Option<(String, String)> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:provider ") {
                    if let Some((name, id)) = rest.split_once(':') {
                        return Some((name.trim().to_string(), id.trim().to_string()));
                    }
                }
            }
        }
        None
    }

    /// Set provider on a host block. Replaces existing purple:provider comment or adds one.
    pub fn set_provider(&mut self, provider_name: &str, server_id: &str) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && d.raw_line.trim().starts_with("# purple:provider "))
        });
        let pos = self.content_end();
        self.directives.insert(
            pos,
            Directive {
                key: String::new(),
                value: String::new(),
                raw_line: format!(
                    "{}# purple:provider {}:{}",
                    indent, provider_name, server_id
                ),
                is_non_directive: true,
            },
        );
    }

    /// Extract askpass source from purple:askpass comment in directives.
    pub fn askpass(&self) -> Option<String> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:askpass ") {
                    let val = rest.trim();
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
        None
    }

    /// Extract vault-ssh role from purple:vault-ssh comment.
    pub fn vault_ssh(&self) -> Option<String> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:vault-ssh ") {
                    let val = rest.trim();
                    if !val.is_empty() && crate::vault_ssh::is_valid_role(val) {
                        return Some(val.to_string());
                    }
                }
            }
        }
        None
    }

    /// Set vault-ssh role. Replaces existing comment or adds one. Empty string removes.
    pub fn set_vault_ssh(&mut self, role: &str) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:vault-ssh" || t.starts_with("# purple:vault-ssh ")
            })
        });
        if !role.is_empty() {
            let pos = self.content_end();
            self.directives.insert(
                pos,
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: format!("{}# purple:vault-ssh {}", indent, role),
                    is_non_directive: true,
                },
            );
        }
    }

    /// Extract the Vault SSH endpoint from a `# purple:vault-addr` comment.
    /// Returns None when the comment is absent, blank or contains an invalid
    /// URL value. Validation is intentionally minimal: we reject empty,
    /// whitespace-containing and control-character values but otherwise let
    /// the Vault CLI surface its own error on typos.
    pub fn vault_addr(&self) -> Option<String> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:vault-addr ") {
                    let val = rest.trim();
                    if !val.is_empty() && crate::vault_ssh::is_valid_vault_addr(val) {
                        return Some(val.to_string());
                    }
                }
            }
        }
        None
    }

    /// Set vault-addr endpoint. Replaces existing comment or adds one. Empty
    /// string removes. Caller is expected to have validated the URL upstream
    /// (e.g. via `is_valid_vault_addr`) — this function does not re-validate.
    pub fn set_vault_addr(&mut self, url: &str) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:vault-addr" || t.starts_with("# purple:vault-addr ")
            })
        });
        if !url.is_empty() {
            let pos = self.content_end();
            self.directives.insert(
                pos,
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: format!("{}# purple:vault-addr {}", indent, url),
                    is_non_directive: true,
                },
            );
        }
    }

    /// Set askpass source on a host block. Replaces existing purple:askpass comment or adds one.
    /// Pass an empty string to remove the comment.
    pub fn set_askpass(&mut self, source: &str) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:askpass" || t.starts_with("# purple:askpass ")
            })
        });
        if !source.is_empty() {
            let pos = self.content_end();
            self.directives.insert(
                pos,
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: format!("{}# purple:askpass {}", indent, source),
                    is_non_directive: true,
                },
            );
        }
    }

    /// Extract provider metadata from purple:meta comment in directives.
    /// Format: `# purple:meta key=value,key=value`
    pub fn meta(&self) -> Vec<(String, String)> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:meta ") {
                    return rest
                        .split(',')
                        .filter_map(|pair| {
                            let (k, v) = pair.split_once('=')?;
                            let k = k.trim();
                            let v = v.trim();
                            if k.is_empty() {
                                None
                            } else {
                                Some((k.to_string(), v.to_string()))
                            }
                        })
                        .collect();
                }
            }
        }
        Vec::new()
    }

    /// Set provider metadata on a host block. Replaces existing purple:meta comment or adds one.
    /// Pass an empty slice to remove the comment.
    pub fn set_meta(&mut self, meta: &[(String, String)]) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:meta" || t.starts_with("# purple:meta ")
            })
        });
        if !meta.is_empty() {
            let encoded: Vec<String> = meta
                .iter()
                .map(|(k, v)| {
                    let clean_k = Self::sanitize_tag(&k.replace([',', '='], ""));
                    let clean_v = Self::sanitize_tag(&v.replace(',', ""));
                    format!("{}={}", clean_k, clean_v)
                })
                .collect();
            let pos = self.content_end();
            self.directives.insert(
                pos,
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: format!("{}# purple:meta {}", indent, encoded.join(",")),
                    is_non_directive: true,
                },
            );
        }
    }

    /// Extract stale timestamp from purple:stale comment in directives.
    /// Returns `None` if absent or malformed.
    pub fn stale(&self) -> Option<u64> {
        for d in &self.directives {
            if d.is_non_directive {
                let trimmed = d.raw_line.trim();
                if let Some(rest) = trimmed.strip_prefix("# purple:stale ") {
                    return rest.trim().parse::<u64>().ok();
                }
            }
        }
        None
    }

    /// Mark a host block as stale with a unix timestamp.
    /// Replaces existing purple:stale comment or adds one.
    pub fn set_stale(&mut self, timestamp: u64) {
        let indent = self.detect_indent();
        self.clear_stale();
        let pos = self.content_end();
        self.directives.insert(
            pos,
            Directive {
                key: String::new(),
                value: String::new(),
                raw_line: format!("{}# purple:stale {}", indent, timestamp),
                is_non_directive: true,
            },
        );
    }

    /// Remove stale marking from a host block.
    pub fn clear_stale(&mut self) {
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:stale" || t.starts_with("# purple:stale ")
            })
        });
    }

    /// Sanitize a tag value: strip control characters, commas (delimiter),
    /// and Unicode format/bidi override characters. Truncate to 128 chars.
    fn sanitize_tag(tag: &str) -> String {
        tag.chars()
            .filter(|c| {
                !c.is_control()
                    && *c != ','
                    && !('\u{200B}'..='\u{200F}').contains(c) // zero-width, bidi marks
                    && !('\u{202A}'..='\u{202E}').contains(c) // bidi embedding/override
                    && !('\u{2066}'..='\u{2069}').contains(c) // bidi isolate
                    && *c != '\u{FEFF}' // BOM/zero-width no-break space
            })
            .take(128)
            .collect()
    }

    /// Set user tags on a host block. Replaces existing purple:tags comment or adds one.
    pub fn set_tags(&mut self, tags: &[String]) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:tags" || t.starts_with("# purple:tags ")
            })
        });
        let sanitized: Vec<String> = tags
            .iter()
            .map(|t| Self::sanitize_tag(t))
            .filter(|t| !t.is_empty())
            .collect();
        if !sanitized.is_empty() {
            let pos = self.content_end();
            self.directives.insert(
                pos,
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: format!("{}# purple:tags {}", indent, sanitized.join(",")),
                    is_non_directive: true,
                },
            );
        }
    }

    /// Set provider-synced tags. Replaces existing purple:provider_tags comment.
    /// Always writes the comment (even when empty) as a migration sentinel.
    pub fn set_provider_tags(&mut self, tags: &[String]) {
        let indent = self.detect_indent();
        self.directives.retain(|d| {
            !(d.is_non_directive && {
                let t = d.raw_line.trim();
                t == "# purple:provider_tags" || t.starts_with("# purple:provider_tags ")
            })
        });
        let sanitized: Vec<String> = tags
            .iter()
            .map(|t| Self::sanitize_tag(t))
            .filter(|t| !t.is_empty())
            .collect();
        let raw = if sanitized.is_empty() {
            format!("{}# purple:provider_tags", indent)
        } else {
            format!("{}# purple:provider_tags {}", indent, sanitized.join(","))
        };
        let pos = self.content_end();
        self.directives.insert(
            pos,
            Directive {
                key: String::new(),
                value: String::new(),
                raw_line: raw,
                is_non_directive: true,
            },
        );
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
            if d.key.eq_ignore_ascii_case("hostname") {
                entry.hostname = d.value.clone();
            } else if d.key.eq_ignore_ascii_case("user") {
                entry.user = d.value.clone();
            } else if d.key.eq_ignore_ascii_case("port") {
                entry.port = d.value.parse().unwrap_or(22);
            } else if d.key.eq_ignore_ascii_case("identityfile") {
                if entry.identity_file.is_empty() {
                    entry.identity_file = d.value.clone();
                }
            } else if d.key.eq_ignore_ascii_case("proxyjump") {
                entry.proxy_jump = d.value.clone();
            } else if d.key.eq_ignore_ascii_case("certificatefile")
                && entry.certificate_file.is_empty()
            {
                entry.certificate_file = d.value.clone();
            }
        }
        entry.tags = self.tags();
        entry.provider_tags = self.provider_tags();
        entry.has_provider_tags = self.has_provider_tags_comment();
        entry.provider = self.provider().map(|(name, _)| name);
        entry.tunnel_count = self.tunnel_count();
        entry.askpass = self.askpass();
        entry.vault_ssh = self.vault_ssh();
        entry.vault_addr = self.vault_addr();
        entry.provider_meta = self.meta();
        entry.stale = self.stale();
        entry
    }

    /// Extract a convenience PatternEntry view from this block.
    pub fn to_pattern_entry(&self) -> PatternEntry {
        let mut entry = PatternEntry {
            pattern: self.host_pattern.clone(),
            hostname: String::new(),
            user: String::new(),
            port: 22,
            identity_file: String::new(),
            proxy_jump: String::new(),
            tags: self.tags(),
            askpass: self.askpass(),
            source_file: None,
            directives: Vec::new(),
        };
        for d in &self.directives {
            if d.is_non_directive {
                continue;
            }
            match d.key.to_ascii_lowercase().as_str() {
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
            entry.directives.push((d.key.clone(), d.value.clone()));
        }
        entry
    }

    /// Count forwarding directives (LocalForward, RemoteForward, DynamicForward).
    pub fn tunnel_count(&self) -> u16 {
        let count = self
            .directives
            .iter()
            .filter(|d| {
                !d.is_non_directive
                    && (d.key.eq_ignore_ascii_case("localforward")
                        || d.key.eq_ignore_ascii_case("remoteforward")
                        || d.key.eq_ignore_ascii_case("dynamicforward"))
            })
            .count();
        count.min(u16::MAX as usize) as u16
    }

    /// Check if this block has any tunnel forwarding directives.
    #[allow(dead_code)]
    pub fn has_tunnels(&self) -> bool {
        self.directives.iter().any(|d| {
            !d.is_non_directive
                && (d.key.eq_ignore_ascii_case("localforward")
                    || d.key.eq_ignore_ascii_case("remoteforward")
                    || d.key.eq_ignore_ascii_case("dynamicforward"))
        })
    }

    /// Extract tunnel rules from forwarding directives.
    pub fn tunnel_directives(&self) -> Vec<crate::tunnel::TunnelRule> {
        self.directives
            .iter()
            .filter(|d| !d.is_non_directive)
            .filter_map(|d| crate::tunnel::TunnelRule::parse_value(&d.key, &d.value))
            .collect()
    }
}

impl SshConfigFile {
    /// Get all host entries as convenience views (including from Include files).
    /// Pattern-inherited directives (ProxyJump, User, IdentityFile) are merged
    /// using SSH-faithful alias-only matching so indicators like ↗ reflect what
    /// SSH will actually apply when connecting via `ssh <alias>`.
    pub fn host_entries(&self) -> Vec<HostEntry> {
        let mut entries = Vec::new();
        Self::collect_host_entries(&self.elements, &mut entries);
        self.apply_pattern_inheritance(&mut entries);
        entries
    }

    /// Get a single host entry by alias without pattern inheritance applied.
    /// Returns the raw directives from the host's own block only. Used by the
    /// edit form so inherited values can be shown as dimmed placeholders.
    pub fn raw_host_entry(&self, alias: &str) -> Option<HostEntry> {
        Self::find_raw_host_entry(&self.elements, alias)
    }

    fn find_raw_host_entry(elements: &[ConfigElement], alias: &str) -> Option<HostEntry> {
        for e in elements {
            match e {
                ConfigElement::HostBlock(block)
                    if !is_host_pattern(&block.host_pattern) && block.host_pattern == alias =>
                {
                    return Some(block.to_host_entry());
                }
                ConfigElement::Include(inc) => {
                    for file in &inc.resolved_files {
                        if let Some(mut found) = Self::find_raw_host_entry(&file.elements, alias) {
                            if found.source_file.is_none() {
                                found.source_file = Some(file.path.clone());
                            }
                            return Some(found);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Apply SSH first-match-wins pattern inheritance to host entries.
    /// Matches patterns against the alias only (SSH-faithful: `Host` patterns
    /// match the token typed on the command line, not the resolved `Hostname`).
    fn apply_pattern_inheritance(&self, entries: &mut [HostEntry]) {
        // Patterns are pre-collected once. Host entries never contain pattern
        // aliases — collect_host_entries skips is_host_pattern blocks.
        let all_patterns = self.pattern_entries();
        for entry in entries.iter_mut() {
            if !entry.proxy_jump.is_empty()
                && !entry.user.is_empty()
                && !entry.identity_file.is_empty()
            {
                continue;
            }
            for p in &all_patterns {
                if !host_pattern_matches(&p.pattern, &entry.alias) {
                    continue;
                }
                apply_first_match_fields(
                    &mut entry.proxy_jump,
                    &mut entry.user,
                    &mut entry.identity_file,
                    p,
                );
                if !entry.proxy_jump.is_empty()
                    && !entry.user.is_empty()
                    && !entry.identity_file.is_empty()
                {
                    break;
                }
            }
        }
    }

    /// Compute pattern-provided field hints for a host alias. Returns first-match
    /// values and their source patterns for ProxyJump, User and IdentityFile.
    /// These are returned regardless of whether the host has its own values for
    /// those fields. The caller (form rendering) decides visibility based on
    /// whether the field is empty. Matches by alias only (SSH-faithful).
    pub fn inherited_hints(&self, alias: &str) -> InheritedHints {
        let patterns = self.matching_patterns(alias);
        let mut hints = InheritedHints::default();
        for p in &patterns {
            if hints.proxy_jump.is_none() && !p.proxy_jump.is_empty() {
                hints.proxy_jump = Some((p.proxy_jump.clone(), p.pattern.clone()));
            }
            if hints.user.is_none() && !p.user.is_empty() {
                hints.user = Some((p.user.clone(), p.pattern.clone()));
            }
            if hints.identity_file.is_none() && !p.identity_file.is_empty() {
                hints.identity_file = Some((p.identity_file.clone(), p.pattern.clone()));
            }
            if hints.proxy_jump.is_some() && hints.user.is_some() && hints.identity_file.is_some() {
                break;
            }
        }
        hints
    }

    /// Get all pattern entries as convenience views (including from Include files).
    pub fn pattern_entries(&self) -> Vec<PatternEntry> {
        let mut entries = Vec::new();
        Self::collect_pattern_entries(&self.elements, &mut entries);
        entries
    }

    fn collect_pattern_entries(elements: &[ConfigElement], entries: &mut Vec<PatternEntry>) {
        for e in elements {
            match e {
                ConfigElement::HostBlock(block) => {
                    if !is_host_pattern(&block.host_pattern) {
                        continue;
                    }
                    entries.push(block.to_pattern_entry());
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        let start = entries.len();
                        Self::collect_pattern_entries(&file.elements, entries);
                        for entry in &mut entries[start..] {
                            if entry.source_file.is_none() {
                                entry.source_file = Some(file.path.clone());
                            }
                        }
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
    }

    /// Find all pattern blocks that match a given host alias and hostname.
    /// Returns entries in config order (first match first).
    pub fn matching_patterns(&self, alias: &str) -> Vec<PatternEntry> {
        let mut matches = Vec::new();
        Self::collect_matching_patterns(&self.elements, alias, &mut matches);
        matches
    }

    fn collect_matching_patterns(
        elements: &[ConfigElement],
        alias: &str,
        matches: &mut Vec<PatternEntry>,
    ) {
        for e in elements {
            match e {
                ConfigElement::HostBlock(block) => {
                    if !is_host_pattern(&block.host_pattern) {
                        continue;
                    }
                    if host_pattern_matches(&block.host_pattern, alias) {
                        matches.push(block.to_pattern_entry());
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        let start = matches.len();
                        Self::collect_matching_patterns(&file.elements, alias, matches);
                        for entry in &mut matches[start..] {
                            if entry.source_file.is_none() {
                                entry.source_file = Some(file.path.clone());
                            }
                        }
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
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
        let mut seen = std::collections::HashSet::new();
        let mut dirs = Vec::new();
        Self::collect_include_glob_dirs(&self.elements, config_dir, &mut seen, &mut dirs);
        dirs
    }

    fn collect_include_glob_dirs(
        elements: &[ConfigElement],
        config_dir: Option<&std::path::Path>,
        seen: &mut std::collections::HashSet<PathBuf>,
        dirs: &mut Vec<PathBuf>,
    ) {
        for e in elements {
            if let ConfigElement::Include(include) = e {
                // Split respecting quoted paths (same as resolve_include does)
                for single in Self::split_include_patterns(&include.pattern) {
                    let expanded = Self::expand_env_vars(&Self::expand_tilde(single));
                    let resolved = if expanded.starts_with('/') {
                        PathBuf::from(&expanded)
                    } else if let Some(dir) = config_dir {
                        dir.join(&expanded)
                    } else {
                        continue;
                    };
                    if let Some(parent) = resolved.parent() {
                        let parent = parent.to_path_buf();
                        if seen.insert(parent.clone()) {
                            dirs.push(parent);
                        }
                    }
                }
                // Recurse into resolved files
                for file in &include.resolved_files {
                    Self::collect_include_glob_dirs(&file.elements, file.path.parent(), seen, dirs);
                }
            }
        }
    }

    /// Remove `# purple:group <Name>` headers that have no corresponding
    /// provider hosts. Returns the number of headers removed.
    pub fn remove_all_orphaned_group_headers(&mut self) -> usize {
        // Collect all provider display names that have at least one host.
        let active_providers: std::collections::HashSet<String> = self
            .elements
            .iter()
            .filter_map(|e| {
                if let ConfigElement::HostBlock(block) = e {
                    block
                        .provider()
                        .map(|(name, _)| provider_group_display_name(&name).to_string())
                } else {
                    None
                }
            })
            .collect();

        let mut removed = 0;
        self.elements.retain(|e| {
            if let ConfigElement::GlobalLine(line) = e {
                if let Some(rest) = line.trim().strip_prefix("# purple:group ") {
                    if !active_providers.contains(rest.trim()) {
                        removed += 1;
                        return false;
                    }
                }
            }
            true
        });
        removed
    }

    /// Repair configs where `# purple:group` comments were absorbed into the
    /// preceding host block's directives instead of being stored as GlobalLines.
    /// Returns the number of blocks that were repaired.
    pub fn repair_absorbed_group_comments(&mut self) -> usize {
        let mut repaired = 0;
        let mut idx = 0;
        while idx < self.elements.len() {
            let needs_repair = if let ConfigElement::HostBlock(block) = &self.elements[idx] {
                block
                    .directives
                    .iter()
                    .any(|d| d.is_non_directive && d.raw_line.trim().starts_with("# purple:group "))
            } else {
                false
            };

            if !needs_repair {
                idx += 1;
                continue;
            }

            // Find the index of the first absorbed group comment in this block's directives.
            let block = if let ConfigElement::HostBlock(block) = &mut self.elements[idx] {
                block
            } else {
                unreachable!()
            };

            let group_idx = block
                .directives
                .iter()
                .position(|d| {
                    d.is_non_directive && d.raw_line.trim().starts_with("# purple:group ")
                })
                .unwrap();

            // Find where trailing blanks before the group comment start.
            let mut keep_end = group_idx;
            while keep_end > 0
                && block.directives[keep_end - 1].is_non_directive
                && block.directives[keep_end - 1].raw_line.trim().is_empty()
            {
                keep_end -= 1;
            }

            // Collect everything from keep_end onward as GlobalLines.
            let extracted: Vec<ConfigElement> = block
                .directives
                .drain(keep_end..)
                .map(|d| ConfigElement::GlobalLine(d.raw_line))
                .collect();

            // Insert extracted GlobalLines right after this HostBlock.
            let insert_at = idx + 1;
            for (i, elem) in extracted.into_iter().enumerate() {
                self.elements.insert(insert_at + i, elem);
            }

            repaired += 1;
            // Advance past the inserted elements.
            idx = insert_at;
            // Skip the inserted elements to continue scanning.
            while idx < self.elements.len() {
                if let ConfigElement::HostBlock(_) = &self.elements[idx] {
                    break;
                }
                idx += 1;
            }
        }
        repaired
    }

    /// Recursively collect host entries from a list of elements.
    fn collect_host_entries(elements: &[ConfigElement], entries: &mut Vec<HostEntry>) {
        for e in elements {
            match e {
                ConfigElement::HostBlock(block) => {
                    if is_host_pattern(&block.host_pattern) {
                        continue;
                    }
                    entries.push(block.to_host_entry());
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        let start = entries.len();
                        Self::collect_host_entries(&file.elements, entries);
                        for entry in &mut entries[start..] {
                            if entry.source_file.is_none() {
                                entry.source_file = Some(file.path.clone());
                            }
                        }
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
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

    /// Check if a host block with exactly this host_pattern exists (top-level only).
    /// Unlike `has_host` which splits multi-host patterns and checks individual parts,
    /// this matches the full `Host` line pattern string (e.g. "web-* db-*").
    /// Does not search Include files (patterns from includes are read-only).
    pub fn has_host_block(&self, pattern: &str) -> bool {
        self.elements
            .iter()
            .any(|e| matches!(e, ConfigElement::HostBlock(block) if block.host_pattern == pattern))
    }

    /// Check if a host alias is from an included file (read-only).
    /// Handles multi-pattern Host lines by splitting on whitespace.
    pub fn is_included_host(&self, alias: &str) -> bool {
        // Not in top-level elements → must be in an Include
        for e in &self.elements {
            match e {
                ConfigElement::HostBlock(block) => {
                    if block.host_pattern.split_whitespace().any(|p| p == alias) {
                        return false;
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
    /// Inserts before any trailing wildcard/pattern Host blocks (e.g. `Host *`)
    /// so that SSH "first match wins" semantics are preserved. If wildcards are
    /// only at the top of the file (acting as global defaults), appends at end.
    pub fn add_host(&mut self, entry: &HostEntry) {
        let block = Self::entry_to_block(entry);
        let insert_pos = self.find_trailing_pattern_start();

        if let Some(pos) = insert_pos {
            // Insert before the trailing pattern group, with blank separators
            let needs_blank_before = pos > 0
                && !matches!(
                    self.elements.get(pos - 1),
                    Some(ConfigElement::GlobalLine(line)) if line.trim().is_empty()
                );
            let mut idx = pos;
            if needs_blank_before {
                self.elements
                    .insert(idx, ConfigElement::GlobalLine(String::new()));
                idx += 1;
            }
            self.elements.insert(idx, ConfigElement::HostBlock(block));
            // Ensure a blank separator after the new block (before the wildcard group)
            let after = idx + 1;
            if after < self.elements.len()
                && !matches!(
                    self.elements.get(after),
                    Some(ConfigElement::GlobalLine(line)) if line.trim().is_empty()
                )
            {
                self.elements
                    .insert(after, ConfigElement::GlobalLine(String::new()));
            }
        } else {
            // No trailing patterns: append at end
            if !self.elements.is_empty() && !self.last_element_has_trailing_blank() {
                self.elements.push(ConfigElement::GlobalLine(String::new()));
            }
            self.elements.push(ConfigElement::HostBlock(block));
        }
    }

    /// Find the start of a trailing group of wildcard/pattern Host blocks.
    /// Scans backwards from the end, skipping GlobalLines (blanks/comments/Match).
    /// Returns `None` if no trailing patterns exist (or if ALL hosts are patterns,
    /// i.e. patterns start at position 0 — in that case we append at end).
    fn find_trailing_pattern_start(&self) -> Option<usize> {
        let mut first_pattern_pos = None;
        for i in (0..self.elements.len()).rev() {
            match &self.elements[i] {
                ConfigElement::HostBlock(block) => {
                    if is_host_pattern(&block.host_pattern) {
                        first_pattern_pos = Some(i);
                    } else {
                        // Found a concrete host: the trailing group starts after this
                        break;
                    }
                }
                ConfigElement::GlobalLine(_) => {
                    // Blank lines, comments, Match blocks between patterns: keep scanning
                    if first_pattern_pos.is_some() {
                        first_pattern_pos = Some(i);
                    }
                }
                ConfigElement::Include(_) => break,
            }
        }
        // Don't return position 0 — that means everything is patterns (or patterns at top)
        first_pattern_pos.filter(|&pos| pos > 0)
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
                    // Update host pattern (preserve raw_host_line when alias unchanged)
                    if entry.alias != block.host_pattern {
                        block.host_pattern = entry.alias.clone();
                        block.raw_host_line = format!("Host {}", entry.alias);
                    }

                    // Merge known directives (update existing, add missing, remove empty)
                    Self::upsert_directive(block, "HostName", &entry.hostname);
                    Self::upsert_directive(block, "User", &entry.user);
                    if entry.port != 22 {
                        Self::upsert_directive(block, "Port", &entry.port.to_string());
                    } else {
                        // Remove explicit Port 22 (it's the default)
                        block
                            .directives
                            .retain(|d| d.is_non_directive || !d.key.eq_ignore_ascii_case("port"));
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
                .retain(|d| d.is_non_directive || !d.key.eq_ignore_ascii_case(key));
            return;
        }
        let indent = block.detect_indent();
        for d in &mut block.directives {
            if !d.is_non_directive && d.key.eq_ignore_ascii_case(key) {
                // Only rebuild raw_line when value actually changed (preserves inline comments)
                if d.value != value {
                    d.value = value.to_string();
                    // Detect separator style from original raw_line and preserve it.
                    // Handles: "Key value", "Key=value", "Key = value", "Key =value"
                    // Only considers '=' as separator if it appears before any
                    // non-whitespace content (avoids matching '=' inside values
                    // like "IdentityFile ~/.ssh/id=prod").
                    let trimmed = d.raw_line.trim_start();
                    let after_key = &trimmed[d.key.len()..];
                    let sep = if after_key.trim_start().starts_with('=') {
                        let eq_pos = after_key.find('=').unwrap();
                        let after_eq = &after_key[eq_pos + 1..];
                        let trailing_ws = after_eq.len() - after_eq.trim_start().len();
                        after_key[..eq_pos + 1 + trailing_ws].to_string()
                    } else {
                        " ".to_string()
                    };
                    // Preserve inline comment from original raw_line (e.g. "# production")
                    let comment_suffix = Self::extract_inline_comment(&d.raw_line, &d.key);
                    d.raw_line = format!("{}{}{}{}{}", indent, d.key, sep, value, comment_suffix);
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

    /// Extract the inline comment suffix from a directive's raw line.
    /// Returns the trailing portion (e.g. " # production") or empty string.
    /// Respects double-quoted strings so that `#` inside quotes is not a comment.
    fn extract_inline_comment(raw_line: &str, key: &str) -> String {
        let trimmed = raw_line.trim_start();
        if trimmed.len() <= key.len() {
            return String::new();
        }
        // Skip past key and separator to reach the value portion
        let after_key = &trimmed[key.len()..];
        let rest = after_key.trim_start();
        let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
        // Scan for inline comment (# preceded by whitespace, outside quotes)
        let bytes = rest.as_bytes();
        let mut in_quote = false;
        for i in 0..bytes.len() {
            if bytes[i] == b'"' {
                in_quote = !in_quote;
            } else if !in_quote
                && bytes[i] == b'#'
                && i > 0
                && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t')
            {
                // Found comment start. The clean value ends before the whitespace preceding #.
                let clean_end = rest[..i].trim_end().len();
                return rest[clean_end..].to_string();
            }
        }
        String::new()
    }

    /// Set provider on a host block by alias.
    pub fn set_host_provider(&mut self, alias: &str, provider_name: &str, server_id: &str) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    block.set_provider(provider_name, server_id);
                    return;
                }
            }
        }
    }

    /// Find all hosts with a specific provider, returning (alias, server_id) pairs.
    /// Searches both top-level elements and Include files so that provider hosts
    /// in included configs are recognized during sync (prevents duplicate additions).
    pub fn find_hosts_by_provider(&self, provider_name: &str) -> Vec<(String, String)> {
        let mut results = Vec::new();
        Self::collect_provider_hosts(&self.elements, provider_name, &mut results);
        results
    }

    fn collect_provider_hosts(
        elements: &[ConfigElement],
        provider_name: &str,
        results: &mut Vec<(String, String)>,
    ) {
        for element in elements {
            match element {
                ConfigElement::HostBlock(block) => {
                    if let Some((name, id)) = block.provider() {
                        if name == provider_name {
                            results.push((block.host_pattern.clone(), id));
                        }
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        Self::collect_provider_hosts(&file.elements, provider_name, results);
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
    }

    /// Compare two directive values with whitespace normalization.
    /// Handles hand-edited configs with tabs or multiple spaces.
    fn values_match(a: &str, b: &str) -> bool {
        a.split_whitespace().eq(b.split_whitespace())
    }

    /// Add a forwarding directive to a host block.
    /// Inserts at `content_end()` (before trailing blanks), using detected indentation.
    /// Uses split_whitespace matching for multi-pattern Host lines.
    pub fn add_forward(&mut self, alias: &str, directive_key: &str, value: &str) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern.split_whitespace().any(|p| p == alias) {
                    let indent = block.detect_indent();
                    let pos = block.content_end();
                    block.directives.insert(
                        pos,
                        Directive {
                            key: directive_key.to_string(),
                            value: value.to_string(),
                            raw_line: format!("{}{} {}", indent, directive_key, value),
                            is_non_directive: false,
                        },
                    );
                    return;
                }
            }
        }
    }

    /// Remove a specific forwarding directive from a host block.
    /// Matches key (case-insensitive) and value (whitespace-normalized).
    /// Uses split_whitespace matching for multi-pattern Host lines.
    /// Returns true if a directive was actually removed.
    pub fn remove_forward(&mut self, alias: &str, directive_key: &str, value: &str) -> bool {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern.split_whitespace().any(|p| p == alias) {
                    if let Some(pos) = block.directives.iter().position(|d| {
                        !d.is_non_directive
                            && d.key.eq_ignore_ascii_case(directive_key)
                            && Self::values_match(&d.value, value)
                    }) {
                        block.directives.remove(pos);
                        return true;
                    }
                    return false;
                }
            }
        }
        false
    }

    /// Check if a host block has a specific forwarding directive.
    /// Uses whitespace-normalized value comparison and split_whitespace host matching.
    pub fn has_forward(&self, alias: &str, directive_key: &str, value: &str) -> bool {
        for element in &self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern.split_whitespace().any(|p| p == alias) {
                    return block.directives.iter().any(|d| {
                        !d.is_non_directive
                            && d.key.eq_ignore_ascii_case(directive_key)
                            && Self::values_match(&d.value, value)
                    });
                }
            }
        }
        false
    }

    /// Find tunnel directives for a host alias, searching all elements including
    /// Include files. Uses split_whitespace matching like has_host() for multi-pattern
    /// Host lines.
    pub fn find_tunnel_directives(&self, alias: &str) -> Vec<crate::tunnel::TunnelRule> {
        Self::find_tunnel_directives_in(&self.elements, alias)
    }

    fn find_tunnel_directives_in(
        elements: &[ConfigElement],
        alias: &str,
    ) -> Vec<crate::tunnel::TunnelRule> {
        for element in elements {
            match element {
                ConfigElement::HostBlock(block) => {
                    if block.host_pattern.split_whitespace().any(|p| p == alias) {
                        return block.tunnel_directives();
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        let rules = Self::find_tunnel_directives_in(&file.elements, alias);
                        if !rules.is_empty() {
                            return rules;
                        }
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
        Vec::new()
    }

    /// Generate a unique alias by appending -2, -3, etc. if the base alias is taken.
    pub fn deduplicate_alias(&self, base: &str) -> String {
        self.deduplicate_alias_excluding(base, None)
    }

    /// Generate a unique alias, optionally excluding one alias from collision detection.
    /// Used during rename so the host being renamed doesn't collide with itself.
    pub fn deduplicate_alias_excluding(&self, base: &str, exclude: Option<&str>) -> String {
        let is_taken = |alias: &str| {
            if exclude == Some(alias) {
                return false;
            }
            self.has_host(alias)
        };
        if !is_taken(base) {
            return base.to_string();
        }
        for n in 2..=9999 {
            let candidate = format!("{}-{}", base, n);
            if !is_taken(&candidate) {
                return candidate;
            }
        }
        // Practically unreachable: fall back to PID-based suffix
        format!("{}-{}", base, std::process::id())
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

    /// Set provider-synced tags on a host block by alias.
    pub fn set_host_provider_tags(&mut self, alias: &str, tags: &[String]) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    block.set_provider_tags(tags);
                    return;
                }
            }
        }
    }

    /// Set askpass source on a host block by alias.
    pub fn set_host_askpass(&mut self, alias: &str, source: &str) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    block.set_askpass(source);
                    return;
                }
            }
        }
    }

    /// Set vault-ssh role on a host block by alias.
    pub fn set_host_vault_ssh(&mut self, alias: &str, role: &str) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    block.set_vault_ssh(role);
                    return;
                }
            }
        }
    }

    /// Set or remove the Vault SSH endpoint comment on a host block by alias.
    /// Empty `url` removes the comment.
    ///
    /// Mirrors the safety invariants of `set_host_certificate_file`: wildcard
    /// aliases are refused to avoid accidentally applying a vault address to
    /// every host resolved through a pattern, and Match blocks are not
    /// touched (they live as inert `GlobalLines`). Returns `true` on a
    /// successful mutation, `false` when the alias is invalid or the block
    /// is not found.
    ///
    /// Callers that run asynchronously (e.g. form submit handlers that
    /// resolve the alias before writing) MUST check the return value so a
    /// silent config mutation failure is surfaced instead of pretending the
    /// vault address was wired up.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed or deleted hosts)"]
    pub fn set_host_vault_addr(&mut self, alias: &str, url: &str) -> bool {
        // Same guard as `set_host_certificate_file`: refuse empty aliases
        // and any SSH pattern shape. `is_host_pattern` already covers
        // wildcards, negation and whitespace-separated multi-host forms.
        if alias.is_empty() || is_host_pattern(alias) {
            return false;
        }
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    block.set_vault_addr(url);
                    return true;
                }
            }
        }
        false
    }

    /// Set or remove the CertificateFile directive on a host block by alias.
    /// Empty path removes the directive.
    /// Set the `CertificateFile` directive on the host block that matches
    /// `alias` exactly. Returns `true` if a matching block was found and
    /// updated, `false` if no top-level `HostBlock` matched (alias was
    /// renamed, deleted or lives only inside an `Include`d file).
    ///
    /// Callers that run asynchronously (e.g. the Vault SSH bulk-sign worker)
    /// MUST check the return value so a silent config mutation failure is
    /// surfaced to the user instead of pretending the cert was wired up.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed or deleted hosts)"]
    pub fn set_host_certificate_file(&mut self, alias: &str, path: &str) -> bool {
        // Defense in depth: refuse to mutate a host block when the requested
        // alias is empty or matches any SSH pattern shape (`*`, `?`, `[`,
        // leading `!`, or whitespace-separated multi-host form like
        // `Host web-* db-*`). Writing `CertificateFile` onto a pattern
        // block is almost never what a user intends and would affect every
        // host that resolves through that pattern. Reusing `is_host_pattern`
        // keeps this check in sync with the form-level pattern detection.
        if alias.is_empty() || is_host_pattern(alias) {
            return false;
        }
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    Self::upsert_directive(block, "CertificateFile", path);
                    return true;
                }
            }
        }
        false
    }

    /// Set provider metadata on a host block by alias.
    pub fn set_host_meta(&mut self, alias: &str, meta: &[(String, String)]) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    block.set_meta(meta);
                    return;
                }
            }
        }
    }

    /// Mark a host as stale by alias.
    pub fn set_host_stale(&mut self, alias: &str, timestamp: u64) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    block.set_stale(timestamp);
                    return;
                }
            }
        }
    }

    /// Clear stale marking from a host by alias.
    pub fn clear_host_stale(&mut self, alias: &str) {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if block.host_pattern == alias {
                    block.clear_stale();
                    return;
                }
            }
        }
    }

    /// Collect all stale hosts with their timestamps.
    pub fn stale_hosts(&self) -> Vec<(String, u64)> {
        let mut result = Vec::new();
        for element in &self.elements {
            if let ConfigElement::HostBlock(block) = element {
                if let Some(ts) = block.stale() {
                    result.push((block.host_pattern.clone(), ts));
                }
            }
        }
        result
    }

    /// Delete a host entry by alias.
    pub fn delete_host(&mut self, alias: &str) {
        // Before deletion, check if this host belongs to a provider so we can
        // clean up an orphaned group header afterwards.
        let provider_name = self.elements.iter().find_map(|e| {
            if let ConfigElement::HostBlock(b) = e {
                if b.host_pattern == alias {
                    return b.provider().map(|(name, _)| name);
                }
            }
            None
        });

        self.elements.retain(|e| match e {
            ConfigElement::HostBlock(block) => block.host_pattern != alias,
            _ => true,
        });

        // Remove orphaned group header if no hosts remain for the provider.
        if let Some(name) = provider_name {
            self.remove_orphaned_group_header(&name);
        }

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
    /// Does NOT collapse blank lines or remove group headers so the position
    /// stays valid for re-insertion via `insert_host_at()`.
    /// Orphaned group headers (if any) are cleaned up at next startup.
    pub fn delete_host_undoable(&mut self, alias: &str) -> Option<(ConfigElement, usize)> {
        let pos = self
            .elements
            .iter()
            .position(|e| matches!(e, ConfigElement::HostBlock(b) if b.host_pattern == alias))?;
        let element = self.elements.remove(pos);
        Some((element, pos))
    }

    /// Remove the `# purple:group <DisplayName>` GlobalLine for a provider
    /// if no remaining HostBlock has a `# purple:provider <name>:` directive.
    fn remove_orphaned_group_header(&mut self, provider_name: &str) {
        if self.find_hosts_by_provider(provider_name).is_empty() {
            let display = provider_group_display_name(provider_name);
            let header = format!("# purple:group {}", display);
            self.elements
                .retain(|e| !matches!(e, ConfigElement::GlobalLine(line) if *line == header));
        }
    }

    /// Insert a host block at a specific position (for undo).
    pub fn insert_host_at(&mut self, element: ConfigElement, position: usize) {
        let pos = position.min(self.elements.len());
        self.elements.insert(pos, element);
    }

    /// Find the position after the last HostBlock that belongs to a provider.
    /// Returns `None` if no hosts for this provider exist in the config.
    /// Used by the sync engine to insert new hosts adjacent to existing provider hosts.
    pub fn find_provider_insert_position(&self, provider_name: &str) -> Option<usize> {
        let mut last_pos = None;
        for (i, element) in self.elements.iter().enumerate() {
            if let ConfigElement::HostBlock(block) = element {
                if let Some((name, _)) = block.provider() {
                    if name == provider_name {
                        last_pos = Some(i);
                    }
                }
            }
        }
        // Return position after the last provider host
        last_pos.map(|p| p + 1)
    }

    /// Swap two host blocks in the config by alias. Returns true if swap was performed.
    #[allow(dead_code)]
    pub fn swap_hosts(&mut self, alias_a: &str, alias_b: &str) -> bool {
        let pos_a = self
            .elements
            .iter()
            .position(|e| matches!(e, ConfigElement::HostBlock(b) if b.host_pattern == alias_a));
        let pos_b = self
            .elements
            .iter()
            .position(|e| matches!(e, ConfigElement::HostBlock(b) if b.host_pattern == alias_b));
        if let (Some(a), Some(b)) = (pos_a, pos_b) {
            if a == b {
                return false;
            }
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
        // Defense-in-depth: callers must validate before reaching here.
        // Newlines in values would inject extra SSH config directives.
        debug_assert!(
            !entry.alias.contains('\n') && !entry.alias.contains('\r'),
            "entry_to_block: alias contains newline"
        );
        debug_assert!(
            !entry.hostname.contains('\n') && !entry.hostname.contains('\r'),
            "entry_to_block: hostname contains newline"
        );
        debug_assert!(
            !entry.user.contains('\n') && !entry.user.contains('\r'),
            "entry_to_block: user contains newline"
        );

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

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
