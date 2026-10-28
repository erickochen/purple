use std::path::PathBuf;

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
    /// A Host block: the `Host <pattern>` line plus all indented directives.
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

/// Convenience view for the TUI, extracted from a HostBlock.
#[derive(Debug, Clone)]
pub struct HostEntry {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: u16,
    pub identity_file: String,
    pub proxy_jump: String,
    /// True when a `ProxyCommand` other than `none` is set. Such a host is
    /// reached through a helper process, so a direct TCP ping says nothing.
    pub has_proxy_command: bool,
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
    /// Provider config label from a 3-segment purple:provider marker
    /// (`provider:label:server_id`). None for legacy 2-segment markers.
    /// Used together with `provider` to resolve which labeled config a host
    /// belongs to in multi-config setups.
    pub provider_label: Option<String>,
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
    /// Last-synced provider username from purple:provider_user comment. Lets
    /// sync detect a manual per-host User override (user != marker) and
    /// propagate a changed provider user to hosts still on the old value.
    pub provider_user: Option<String>,
    /// Last-synced provider identity file from purple:provider_key comment.
    /// Same override-detection role as provider_user for IdentityFile.
    pub provider_key: Option<String>,
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
            has_proxy_command: false,
            source_file: None,
            tags: Vec::new(),
            provider_tags: Vec::new(),
            has_provider_tags: false,
            provider: None,
            provider_label: None,
            tunnel_count: 0,
            askpass: None,
            vault_ssh: None,
            vault_addr: None,
            certificate_file: String::new(),
            provider_meta: Vec::new(),
            stale: None,
            provider_user: None,
            provider_key: None,
        }
    }
}

impl HostEntry {
    /// Build the SSH command string for this host.
    /// Includes `-F <config_path>` when the config is non-default so the alias
    /// resolves correctly when pasted into a terminal.
    /// Shell-quotes both the config path and alias to prevent injection.
    pub fn ssh_command(
        &self,
        paths: Option<&crate::runtime::env::Paths>,
        config_path: &std::path::Path,
    ) -> String {
        self.ssh_command_with("ssh", paths, config_path)
    }

    /// Same as `ssh_command`, with `program` (already shell-safe, e.g. `kitten ssh`)
    /// in place of the leading `ssh`.
    pub fn ssh_command_with(
        &self,
        program: &str,
        paths: Option<&crate::runtime::env::Paths>,
        config_path: &std::path::Path,
    ) -> String {
        let escaped = self.alias.replace('\'', "'\\''");
        let default = paths
            .map(|p| p.ssh_dir().join("config"))
            .unwrap_or_default();
        if config_path == default {
            format!("{program} -- '{escaped}'")
        } else {
            let config_escaped = config_path.display().to_string().replace('\'', "'\\''");
            format!("{program} -F '{config_escaped}' -- '{escaped}'")
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

use super::pattern::apply_first_match_fields;
/// Returns true if the host pattern contains wildcards, character classes,
/// negation or whitespace-separated multi-patterns (*, ?, [], !, space/tab).
/// These are SSH match patterns, not concrete hosts.
// Pattern-matching lives in `ssh_config::pattern`. These re-exports preserve
// the old `ssh_config::model::*` import paths used across the codebase and in
// the model_tests file mounted below.
#[allow(unused_imports)]
pub use super::pattern::{
    host_pattern_matches, is_host_pattern, proxy_jump_contains_self, ssh_pattern_match,
};

/// True if a `CertificateFile` directive value points at purple's managed
/// certificate directory. Recognises both tilde-prefixed and absolute paths
/// (`~/.purple/certs/...`, `/home/user/.purple/certs/...`,
/// `$HOME/.purple/certs/...`). Used by `set_host_certificate_file` so
/// user-set custom CertificateFile entries are preserved across vault
/// sign / unsign cycles.
pub(super) fn is_purple_managed_cert_value(value: &str) -> bool {
    let trimmed = value.trim();
    // Strip surrounding double quotes; OpenSSH treats `"~/.purple/..."` and
    // `~/.purple/...` as equivalent.
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(trimmed);
    unquoted.contains(".purple/certs/")
}
// Re-exported so the test file mounted below keeps working.
#[allow(unused_imports)]
pub(super) use super::repair::provider_group_display_name;

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
        // aliases: collect_host_entries skips is_host_pattern blocks.
        let all_patterns = self.pattern_entries();
        // Every ProxyCommand in the file, in the order ssh reads them. A
        // ProxyCommand reaches a machine of its own, so whichever block
        // states one first decides whether this host has a bastion in front
        // of it. The host's own block is in there beside the patterns,
        // because ssh gives it no precedence: an opt-out written as
        // `ProxyCommand none` only takes effect above the pattern it opts
        // out of.
        let mut proxy_commands = Vec::new();
        Self::collect_proxy_commands(&self.elements, &mut proxy_commands);
        for entry in entries.iter_mut() {
            entry.has_proxy_command = proxy_commands
                .iter()
                .find(|(pattern, _)| host_pattern_matches(pattern, &entry.alias))
                .map(|(_, reaches_a_machine)| *reaches_a_machine)
                .unwrap_or(false);
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

    /// Collect every `ProxyCommand` declaration in the file, in reading
    /// order: the block's host pattern paired with whether its value
    /// reaches a machine of its own. `none` is the opt-out spelling and
    /// pairs with false. Concrete blocks and patterns go in the same list
    /// because ssh reads them the same way, taking the first value it
    /// obtains for the keyword.
    fn collect_proxy_commands(elements: &[ConfigElement], out: &mut Vec<(String, bool)>) {
        for e in elements {
            match e {
                ConfigElement::HostBlock(block) => {
                    if let Some(d) = block
                        .directives
                        .iter()
                        .find(|d| d.key.eq_ignore_ascii_case("ProxyCommand"))
                    {
                        out.push((
                            block.host_pattern.clone(),
                            !d.value.trim().eq_ignore_ascii_case("none"),
                        ));
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        Self::collect_proxy_commands(&file.elements, out);
                    }
                }
                _ => {}
            }
        }
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
    /// Convenience wrapper that captures the process environment once for
    /// `${VAR}` resolution; production paths call `include_glob_dirs_with`
    /// with an injected lookup so they stay free of scattered ambient reads.
    pub fn include_glob_dirs(&self) -> Vec<PathBuf> {
        let env = crate::runtime::env::Env::from_process();
        self.include_glob_dirs_with(&|n| env.var(n).map(str::to_string))
    }

    /// Like `include_glob_dirs` but resolves `${VAR}` from an injected lookup
    /// instead of the process env, so tests control expansion deterministically.
    pub fn include_glob_dirs_with(&self, lookup: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
        let config_dir = Self::include_base_dir(&self.path, lookup);
        let mut seen = std::collections::HashSet::new();
        let mut dirs = Vec::new();
        Self::collect_include_glob_dirs(
            &self.elements,
            config_dir.as_deref(),
            &mut seen,
            &mut dirs,
            lookup,
        );
        dirs
    }

    fn collect_include_glob_dirs(
        elements: &[ConfigElement],
        config_dir: Option<&std::path::Path>,
        seen: &mut std::collections::HashSet<PathBuf>,
        dirs: &mut Vec<PathBuf>,
        lookup: &dyn Fn(&str) -> Option<String>,
    ) {
        for e in elements {
            if let ConfigElement::Include(include) = e {
                // Split respecting quoted paths (same as resolve_include does)
                for single in Self::split_include_patterns(&include.pattern) {
                    let expanded =
                        Self::expand_env_vars_with(&Self::expand_tilde(single, lookup), lookup);
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
                // Nested Includes share the same base, like the parser.
                for file in &include.resolved_files {
                    Self::collect_include_glob_dirs(&file.elements, config_dir, seen, dirs, lookup);
                }
            }
        }
    }

    /// Remove `# purple:group <Name>` headers that have no corresponding
    /// provider hosts. Returns the number of headers removed.
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
                    if pattern_contains_token(&block.host_pattern, alias) {
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

    /// Return the sibling aliases that share a `Host` block with `alias`.
    ///
    /// An empty vector means `alias` lives in its own single-alias block (or
    /// is not present). A non-empty vector lists the other tokens in the
    /// block in source order, so the UI can render indicators like `+N` or
    /// spell the aliases out in a confirm dialog before a destructive
    /// action. Does not recurse into `Include`d files: those are read-only
    /// and their hosts cannot be edited from purple anyway.
    pub fn siblings_of(&self, alias: &str) -> Vec<String> {
        if alias.is_empty() {
            return Vec::new();
        }
        self.elements
            .iter()
            .find_map(|el| match el {
                ConfigElement::HostBlock(b) => {
                    // Full-pattern match means the caller is acting on the
                    // whole block (e.g. pattern browser delete of
                    // `web-01 web-01.prod`). All tokens are the target, so
                    // there are no "siblings" to preserve.
                    if b.host_pattern == alias {
                        return Some(Vec::new());
                    }
                    let tokens: Vec<String> = b
                        .host_pattern
                        .split_whitespace()
                        .map(String::from)
                        .collect();
                    if tokens.iter().any(|t| t == alias) {
                        Some(tokens.into_iter().filter(|t| t != alias).collect())
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Find a mutable top-level `HostBlock` whose `host_pattern` contains
    /// `alias` as one of its whitespace-separated tokens.
    ///
    /// Mirrors the matching used by read-path helpers like `has_host` and
    /// `find_tunnel_directives`, so that any host visible in the TUI is also
    /// addressable from write paths (`update_host`, `delete_host`,
    /// `set_host_*`). Prior to this helper, writers compared the full
    /// `host_pattern` for exact equality, which silently no-op'd on
    /// multi-alias blocks like `Host web-01 web-01.prod 10.0.1.5` and
    /// resulted in on-disk drift between the in-memory view and the config
    /// file.
    ///
    /// Does not recurse into `Include`d files: those are read-only.
    ///
    /// A block matches when either (a) its full `host_pattern` equals
    /// `alias` (used by the pattern browser for blocks like `web-* db-*`
    /// or `web-01 web-01.prod` whose full pattern is the caller's key) or
    /// (b) `alias` appears as one of the whitespace-separated tokens (used
    /// by the host list for multi-alias blocks). The full-pattern match is
    /// tried first so callers that pass a pattern string do not
    /// accidentally trigger the token-strip path.
    fn find_host_block_mut(&mut self, alias: &str) -> Option<&mut HostBlock> {
        if alias.is_empty() {
            return None;
        }
        self.elements.iter_mut().find_map(|el| match el {
            ConfigElement::HostBlock(b)
                if b.host_pattern == alias || pattern_contains_token(&b.host_pattern, alias) =>
            {
                Some(b)
            }
            _ => None,
        })
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
                    if pattern_contains_token(&block.host_pattern, alias) {
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
    /// i.e. patterns start at position 0, in which case we append at end).
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
        // Don't return position 0: that means everything is patterns (or patterns at top)
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
    ///
    /// Alias matching uses whitespace-tokenized equality, so a host visible
    /// under a multi-alias block like `Host web-01 web-01.prod` is reachable
    /// from any of its aliases. Directives are shared across all tokens in
    /// the block (per SSH semantics): updating `User` on `web-01.prod`
    /// therefore also affects `web-01`.
    ///
    /// On rename of a multi-alias block only the matching token is replaced
    /// in the `Host` line; sibling aliases are preserved verbatim.
    pub fn update_host(&mut self, old_alias: &str, entry: &HostEntry) {
        let Some(block) = self.find_host_block_mut(old_alias) else {
            return;
        };

        if entry.alias != old_alias {
            // Sanitise the new alias before it flows into `raw_host_line`.
            // A malicious provider response with `\n` in the alias would
            // otherwise inject extra Host blocks into the user's config.
            // entry_to_block already sanitises the add-host path; this
            // mirrors it for the rename path.
            let safe_alias = HostBlock::sanitize_raw_line_value(&entry.alias);
            // Full-pattern match (pattern browser rename) replaces the whole
            // `host_pattern` verbatim. Token match (host list rename on a
            // multi-alias block) replaces only the selected token so
            // siblings survive. Single-alias blocks are covered by the
            // token path because `tokens == [old_alias]`.
            let is_full_pattern_match = block.host_pattern == old_alias;
            let new_pattern: String = if is_full_pattern_match {
                safe_alias.to_string()
            } else {
                block
                    .host_pattern
                    .split_whitespace()
                    .map(|t| {
                        if t == old_alias {
                            safe_alias.as_ref()
                        } else {
                            t
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            block.host_pattern = new_pattern.clone();
            block.raw_host_line = rebuild_host_line(&block.raw_host_line, &new_pattern);
        }

        // Merge known directives (update existing, add missing, remove empty)
        Self::upsert_directive(block, "HostName", &entry.hostname);
        Self::upsert_directive(block, "User", &entry.user);
        if entry.port != 22 {
            Self::upsert_directive(block, "Port", &entry.port.to_string());
        } else {
            // Port 22 is the SSH default: drop the explicit directive so
            // the rendered block stays minimal. Route through
            // `upsert_directive` with an empty value so the first-only
            // semantics match every other key here; a separate `retain`
            // would diverge from the cumulative-directive invariant.
            Self::upsert_directive(block, "Port", "");
        }
        Self::upsert_directive(block, "IdentityFile", &entry.identity_file);
        Self::upsert_directive(block, "ProxyJump", &entry.proxy_jump);
    }

    /// Update a directive in-place, add it if missing, or remove it if value is empty.
    ///
    /// When `value` is empty only the FIRST matching directive is removed.
    /// OpenSSH treats some directives (`IdentityFile`, `CertificateFile`,
    /// `LocalForward`, etc.) as cumulative: a host with three `IdentityFile`
    /// lines is intentionally multi-key. Wiping all matching directives on
    /// an empty form field would silently delete the user's other keys.
    /// The form only edits the first occurrence (see `to_host_entry` which
    /// reads `if entry.identity_file.is_empty()`), so the symmetric remove
    /// only-first behaviour keeps the per-form-field invariant intact:
    /// "what the user sees in the field is what the field controls".
    fn upsert_directive(block: &mut HostBlock, key: &str, value: &str) {
        Self::upsert_directive_impl(block, key, value, true);
    }

    /// `upsert_directive` for directives that take the rest of the line as a
    /// command (`ProxyCommand` and friends): the value is written verbatim,
    /// since ssh hands it to the shell as-is and quoting it whole would break it.
    fn upsert_directive_raw(block: &mut HostBlock, key: &str, value: &str) {
        Self::upsert_directive_impl(block, key, value, false);
    }

    fn upsert_directive_impl(block: &mut HostBlock, key: &str, value: &str, quote: bool) {
        // Defence in depth: sanitise the value before interpolation. The
        // provider-sync update path passes `remote.ip` directly to
        // `update_host` -&gt; `upsert_directive`, so a self-hosted provider
        // with TLS verification disabled (Proxmox, OCI) could supply a
        // hostname containing `\n  ProxyCommand evil` and inject a real
        // directive. `entry_to_block` (the add-host path) sanitises at
        // construction; mirroring it here closes the symmetric edit path.
        let value_owned = HostBlock::sanitize_raw_line_value(value);
        let value = value_owned.as_ref();
        if value.is_empty() {
            if let Some(pos) = block
                .directives
                .iter()
                .position(|d| !d.is_non_directive && d.key.eq_ignore_ascii_case(key))
            {
                block.directives.remove(pos);
            }
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
                    let rendered = if quote {
                        HostBlock::render_value(value)
                    } else {
                        std::borrow::Cow::Borrowed(value)
                    };
                    d.raw_line =
                        format!("{}{}{}{}{}", indent, d.key, sep, rendered, comment_suffix);
                }
                return;
            }
        }
        // Not found, so insert before trailing blanks
        let pos = block.content_end();
        let rendered = if quote {
            HostBlock::render_value(value)
        } else {
            std::borrow::Cow::Borrowed(value)
        };
        block.directives.insert(
            pos,
            Directive {
                key: key.to_string(),
                value: value.to_string(),
                raw_line: format!("{}{} {}", indent, key, rendered),
                is_non_directive: false,
            },
        );
    }

    /// Directives whose value is a command line ssh passes to the shell whole.
    fn takes_command_line(key: &str) -> bool {
        [
            "ProxyCommand",
            "LocalCommand",
            "RemoteCommand",
            "KnownHostsCommand",
        ]
        .iter()
        .any(|k| k.eq_ignore_ascii_case(key))
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

    /// Set provider on a host block by alias using a full ProviderConfigId.
    /// Emits a 3-segment marker when the id has a label, 2-segment otherwise.
    ///
    /// Refuses pattern aliases and multi-alias blocks: claiming a sibling
    /// alias as provider-owned cascades into stale-marking and bulk-purge,
    /// which would silently delete the user's hand-curated entries.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed, deleted or shared-block hosts)"]
    pub fn set_host_provider_id(
        &mut self,
        alias: &str,
        id: &crate::providers::config::ProviderConfigId,
        server_id: &str,
    ) -> bool {
        if alias.is_empty() || is_host_pattern(alias) {
            return false;
        }
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        if is_host_pattern(&block.host_pattern) {
            return false;
        }
        block.set_provider_id(id, server_id);
        true
    }

    /// Rewrite every 2-segment legacy marker for `provider_name` to a
    /// 3-segment marker keyed to `(provider_name, label)`. Used by the
    /// lazy-migration flow so existing hosts of a now-labeled config stay
    /// owned (and don't get re-claimed or stale-marked) on the next sync.
    ///
    /// Only top-level host blocks are rewritten; Include files are read-only
    /// per the project's invariant. Returns the count of host blocks touched.
    pub fn rewrite_legacy_markers_to_label(&mut self, provider_name: &str, label: &str) -> usize {
        let new_id = crate::providers::config::ProviderConfigId::labeled(provider_name, label);
        let mut rewritten = 0usize;
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element {
                let Some((id, server_id)) = block.provider_id() else {
                    continue;
                };
                if id.provider == provider_name && id.label.is_none() {
                    block.set_provider_id(&new_id, &server_id);
                    rewritten += 1;
                }
            }
        }
        rewritten
    }

    /// Find all hosts with a specific provider, returning (alias, server_id) pairs.
    /// Searches both top-level elements and Include files so that provider hosts
    /// in included configs are recognized during sync (prevents duplicate additions).
    pub fn find_hosts_by_provider(&self, provider_name: &str) -> Vec<(String, String)> {
        let mut results = Vec::new();
        Self::collect_provider_hosts(&self.elements, provider_name, &mut results);
        results
    }

    /// Find hosts owned by an exact `ProviderConfigId`. Used during multi-config sync
    /// so two labeled configs of the same provider don't claim each other's hosts.
    /// Legacy 2-segment markers match a bare id (label=None) for backward compatibility.
    pub fn find_hosts_by_id(
        &self,
        id: &crate::providers::config::ProviderConfigId,
    ) -> Vec<(String, String)> {
        let mut results = Vec::new();
        Self::collect_provider_hosts_by_id(&self.elements, id, &mut results);
        results
    }

    /// Like `find_hosts_by_provider`, but returns the FULL server_id from the
    /// raw marker (everything after the first colon), without trying to
    /// interpret the middle segment as a label. Used by sync of BARE configs
    /// so server_ids containing colons (Proxmox `qemu:300`) are matched
    /// against the API response one-to-one instead of being mis-parsed as
    /// labeled markers.
    pub fn find_hosts_by_provider_raw(&self, provider_name: &str) -> Vec<(String, String)> {
        let mut results = Vec::new();
        Self::collect_provider_hosts_raw(&self.elements, provider_name, &mut results);
        results
    }

    fn collect_provider_hosts_raw(
        elements: &[ConfigElement],
        provider_name: &str,
        results: &mut Vec<(String, String)>,
    ) {
        for element in elements {
            match element {
                ConfigElement::HostBlock(block) => {
                    if let Some((name, server_id)) = block.provider_raw()
                        && name == provider_name
                    {
                        results.push((block.host_pattern.clone(), server_id));
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        Self::collect_provider_hosts_raw(&file.elements, provider_name, results);
                    }
                }
                ConfigElement::GlobalLine(_) => {}
            }
        }
    }

    fn collect_provider_hosts(
        elements: &[ConfigElement],
        provider_name: &str,
        results: &mut Vec<(String, String)>,
    ) {
        for element in elements {
            match element {
                ConfigElement::HostBlock(block) => {
                    if let Some((name, id)) = block.provider()
                        && name == provider_name
                    {
                        results.push((block.host_pattern.clone(), id));
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

    fn collect_provider_hosts_by_id(
        elements: &[ConfigElement],
        id: &crate::providers::config::ProviderConfigId,
        results: &mut Vec<(String, String)>,
    ) {
        for element in elements {
            match element {
                ConfigElement::HostBlock(block) => {
                    if let Some((host_id, server_id)) = block.provider_id()
                        && &host_id == id
                    {
                        results.push((block.host_pattern.clone(), server_id));
                    }
                }
                ConfigElement::Include(include) => {
                    for file in &include.resolved_files {
                        Self::collect_provider_hosts_by_id(&file.elements, id, results);
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
            if let ConfigElement::HostBlock(block) = element
                && pattern_contains_token(&block.host_pattern, alias)
            {
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

    /// Remove a specific forwarding directive from a host block.
    /// Matches key (case-insensitive) and value (whitespace-normalized).
    /// Uses split_whitespace matching for multi-pattern Host lines.
    /// Returns true if a directive was actually removed.
    pub fn remove_forward(&mut self, alias: &str, directive_key: &str, value: &str) -> bool {
        for element in &mut self.elements {
            if let ConfigElement::HostBlock(block) = element
                && pattern_contains_token(&block.host_pattern, alias)
            {
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
        false
    }

    /// Check if a host block has a specific forwarding directive.
    /// Uses whitespace-normalized value comparison and split_whitespace host matching.
    pub fn has_forward(&self, alias: &str, directive_key: &str, value: &str) -> bool {
        for element in &self.elements {
            if let ConfigElement::HostBlock(block) = element
                && pattern_contains_token(&block.host_pattern, alias)
            {
                return block.directives.iter().any(|d| {
                    !d.is_non_directive
                        && d.key.eq_ignore_ascii_case(directive_key)
                        && Self::values_match(&d.value, value)
                });
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
                    if pattern_contains_token(&block.host_pattern, alias) {
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
    ///
    /// Refuses pattern aliases and multi-alias blocks symmetric with the
    /// vault/certificate setters: a tag on a shared block silently applies to
    /// every sibling alias, which is rarely the user's intent.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed, deleted or shared-block hosts)"]
    pub fn set_host_tags(&mut self, alias: &str, tags: &[String]) -> bool {
        if alias.is_empty() || is_host_pattern(alias) {
            return false;
        }
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        if is_host_pattern(&block.host_pattern) {
            return false;
        }
        block.set_tags(tags);
        true
    }

    /// Set provider-synced tags on a host block by alias.
    ///
    /// Same multi-alias and pattern refusal as the other purple-marker
    /// setters. Provider tags drive sync decisions, so a wrong-block mutation
    /// can cascade into delete/stale.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed, deleted or shared-block hosts)"]
    pub fn set_host_provider_tags(&mut self, alias: &str, tags: &[String]) -> bool {
        if alias.is_empty() || is_host_pattern(alias) {
            return false;
        }
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        if is_host_pattern(&block.host_pattern) {
            return false;
        }
        block.set_provider_tags(tags);
        true
    }

    /// Record the last-synced provider username on a host block by alias.
    /// Empty value removes the marker. Same shared-block and pattern refusal
    /// as the other purple-marker setters: a wrong-block mutation would
    /// mis-track override state and let sync clobber a manual User edit.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed, deleted or shared-block hosts)"]
    pub fn set_host_provider_user(&mut self, alias: &str, user: &str) -> bool {
        if alias.is_empty() || is_host_pattern(alias) {
            return false;
        }
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        if is_host_pattern(&block.host_pattern) {
            return false;
        }
        block.set_provider_user(user);
        true
    }

    /// Record the last-synced provider identity file on a host block by alias.
    /// Empty value removes the marker. Same safety invariants as
    /// `set_host_provider_user`.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed, deleted or shared-block hosts)"]
    pub fn set_host_provider_key(&mut self, alias: &str, key: &str) -> bool {
        if alias.is_empty() || is_host_pattern(alias) {
            return false;
        }
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        if is_host_pattern(&block.host_pattern) {
            return false;
        }
        block.set_provider_key(key);
        true
    }

    /// Set askpass source on a host block by alias.
    ///
    /// Askpass is an authentication credential source; applying it to a
    /// sibling alias in a shared block would route the wrong credential.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed, deleted or shared-block hosts)"]
    pub fn set_host_askpass(&mut self, alias: &str, source: &str) -> bool {
        if alias.is_empty() || is_host_pattern(alias) {
            return false;
        }
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        if is_host_pattern(&block.host_pattern) {
            return false;
        }
        block.set_askpass(source);
        true
    }

    /// Set or remove the Vault SSH role comment on a host block by alias.
    /// Empty `role` removes the comment.
    ///
    /// Mirrors the safety invariants of `set_host_certificate_file` and
    /// `set_host_vault_addr`: wildcard aliases are refused so a `Host *.prod`
    /// pattern can never have a Vault role silently assigned to every host
    /// it resolves, and multi-alias blocks (`Host web-01 web-01.prod`) are
    /// refused so the role is never applied to sibling aliases the user did
    /// not authorise. Returns `true` on a successful mutation, `false` when
    /// the alias is invalid, missing, or lives in an Include file.
    ///
    /// Callers that run asynchronously (form submit handlers, sync workers)
    /// MUST check the return value so a silent config mutation failure is
    /// surfaced instead of pretending the role was wired up.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed, deleted or shared-block hosts)"]
    pub fn set_host_vault_ssh(&mut self, alias: &str, role: &str) -> bool {
        if alias.is_empty() || is_host_pattern(alias) {
            return false;
        }
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        if is_host_pattern(&block.host_pattern) {
            return false;
        }
        block.set_vault_ssh(role);
        true
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
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        // Defense in depth: refuse to mutate a block that is itself a
        // pattern or a multi-alias block (ExactAliasOnly policy). Writing a
        // vault endpoint onto such a block would apply to every sibling
        // alias and every host resolving through the pattern, which is
        // almost certainly not what the caller intends.
        if is_host_pattern(&block.host_pattern) {
            return false;
        }
        block.set_vault_addr(url);
        true
    }

    /// Set or remove the CertificateFile directive on a host block by alias.
    /// Empty path removes the directive.
    /// Set the `CertificateFile` directive on the host block that matches
    /// `alias` exactly. Returns `true` if a matching block was found and
    /// updated, `false` if no top-level `HostBlock` matched (alias was
    /// renamed, deleted or lives only inside an `Include`d file).
    ///
    /// Only touches `CertificateFile` directives that are purple-managed
    /// (path contains `.purple/certs/`). User-set custom `CertificateFile`
    /// entries (e.g. a corporate or personal cert at `~/.ssh/corp-cert.pub`)
    /// are never modified or removed: empty-path clears only the purple
    /// managed line; non-empty path updates the purple-managed line in
    /// place or inserts a new one if absent. A host with both a corporate
    /// cert and a Vault-signed cert ends up with both lines present, in
    /// OpenSSH's documented cumulative semantics.
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
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        // Additionally refuse when the matched block is itself a pattern or
        // multi-alias block (ExactAliasOnly policy). The input `alias` may
        // be a plain token yet resolve into a block like `Host web-01
        // web-01.prod`, where writing `CertificateFile` would silently
        // affect sibling aliases.
        if is_host_pattern(&block.host_pattern) {
            return false;
        }

        // Find the existing purple-managed CertificateFile entry, if any.
        let purple_pos = block.directives.iter().position(|d| {
            !d.is_non_directive
                && d.key.eq_ignore_ascii_case("CertificateFile")
                && is_purple_managed_cert_value(&d.value)
        });

        if path.is_empty() {
            if let Some(pos) = purple_pos {
                block.directives.remove(pos);
            }
            return true;
        }

        let sanitized = HostBlock::sanitize_raw_line_value(path);
        let indent = block.detect_indent();
        if let Some(pos) = purple_pos {
            let d = &mut block.directives[pos];
            if d.value != sanitized.as_ref() {
                d.value = sanitized.to_string();
                // Preserve separator style + inline comment in the same way
                // upsert_directive does for the single-line case.
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
                let comment_suffix = Self::extract_inline_comment(&d.raw_line, &d.key);
                let rendered = HostBlock::render_value(&sanitized);
                d.raw_line = format!("{}{}{}{}{}", indent, d.key, sep, rendered, comment_suffix);
            }
        } else if is_purple_managed_cert_value(sanitized.as_ref()) {
            // Defensive gate: only insert a NEW CertificateFile line when
            // the caller's path is itself purple-managed. The rollback flow
            // in `app/hosts.rs` may pass `old_entry.certificate_file` which
            // could be a user-set custom path; inserting it here would
            // duplicate a user-managed entry. A non-purple-managed path
            // with no existing purple-managed line is a no-op.
            let pos = block.content_end();
            block.directives.insert(
                pos,
                Directive {
                    key: "CertificateFile".to_string(),
                    value: sanitized.to_string(),
                    raw_line: format!(
                        "{}CertificateFile {}",
                        indent,
                        HostBlock::render_value(&sanitized)
                    ),
                    is_non_directive: false,
                },
            );
        }
        true
    }

    /// Set provider metadata on a host block by alias.
    ///
    /// Refuses pattern aliases and multi-alias blocks; same rationale as the
    /// other `# purple:*` setters.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed, deleted or shared-block hosts)"]
    pub fn set_host_meta(&mut self, alias: &str, meta: &[(String, String)]) -> bool {
        if alias.is_empty() || is_host_pattern(alias) {
            return false;
        }
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        if is_host_pattern(&block.host_pattern) {
            return false;
        }
        block.set_meta(meta);
        true
    }

    /// First value of `key` on the host block for `alias`. None when the
    /// host or the directive is missing. Top-level blocks only.
    pub fn host_directive(&self, alias: &str, key: &str) -> Option<String> {
        if alias.is_empty() {
            return None;
        }
        self.elements.iter().find_map(|el| match el {
            ConfigElement::HostBlock(b)
                if b.host_pattern == alias || pattern_contains_token(&b.host_pattern, alias) =>
            {
                b.directives
                    .iter()
                    .find(|d| !d.is_non_directive && d.key.eq_ignore_ascii_case(key))
                    .map(|d| d.value.clone())
            }
            _ => None,
        })
    }

    /// Set the first `key` directive on the host block for `alias`, adding it
    /// when missing and removing it when `value` is empty. Refuses pattern and
    /// multi-alias blocks like the other provider-managed setters.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed, deleted or shared-block hosts)"]
    pub fn set_host_directive(&mut self, alias: &str, key: &str, value: &str) -> bool {
        if alias.is_empty() || is_host_pattern(alias) || key.trim().is_empty() {
            return false;
        }
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        if is_host_pattern(&block.host_pattern) {
            return false;
        }
        let key = key.trim();
        if Self::takes_command_line(key) {
            Self::upsert_directive_raw(block, key, value);
        } else {
            Self::upsert_directive(block, key, value);
        }
        true
    }

    /// Mark a host as stale by alias.
    ///
    /// Stale markers drive the `X` purge flow which deletes the full block,
    /// so a wrong-block mutation here cascades into data loss for a sibling
    /// alias the user added by hand. Refuse pattern and multi-alias blocks.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed, deleted or shared-block hosts)"]
    pub fn set_host_stale(&mut self, alias: &str, timestamp: u64) -> bool {
        if alias.is_empty() || is_host_pattern(alias) {
            return false;
        }
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        if is_host_pattern(&block.host_pattern) {
            return false;
        }
        block.set_stale(timestamp);
        true
    }

    /// Clear stale marking from a host by alias.
    ///
    /// Symmetric guard with `set_host_stale`. Clearing on a shared block is
    /// benign but the asymmetry would be confusing; reject for consistency.
    #[must_use = "check the return value to detect silently-skipped mutations (renamed, deleted or shared-block hosts)"]
    pub fn clear_host_stale(&mut self, alias: &str) -> bool {
        if alias.is_empty() || is_host_pattern(alias) {
            return false;
        }
        let Some(block) = self.find_host_block_mut(alias) else {
            return false;
        };
        if is_host_pattern(&block.host_pattern) {
            return false;
        }
        block.clear_stale();
        true
    }

    /// Collect all stale hosts with their timestamps.
    pub fn stale_hosts(&self) -> Vec<(String, u64)> {
        let mut result = Vec::new();
        for element in &self.elements {
            if let ConfigElement::HostBlock(block) = element
                && let Some(ts) = block.stale()
            {
                result.push((block.host_pattern.clone(), ts));
            }
        }
        result
    }

    /// Delete a host entry by alias.
    ///
    /// For a single-alias block this removes the whole block (and cleans up
    /// any orphaned `# purple:group` header left behind). For a multi-alias
    /// block like `Host web-01 web-01.prod 10.0.1.5` only the matching
    /// alias token is stripped from the `Host` line; sibling aliases and
    /// all directives are preserved so that `delete_host("web-01.prod")`
    /// does not silently wipe configuration for `web-01` and `10.0.1.5`.
    ///
    /// Callers that want to remove the entire block regardless of sibling
    /// aliases should surface an explicit confirmation in the UI and then
    /// delete each sibling alias in turn.
    pub fn delete_host(&mut self, alias: &str) {
        // Two matching modes:
        //   - Full-pattern match: block.host_pattern == alias. Removes the
        //     entire block (plus duplicates). Used by the pattern browser,
        //     where `alias` is a full pattern string like `web-* db-*` or
        //     `web-01 web-01.prod`.
        //   - Token match: alias appears as one of the whitespace-separated
        //     tokens. Strips just that token from a multi-alias block and
        //     removes single-alias blocks outright. Used by the host list.
        // Full-pattern is checked first so pattern-browser deletes never
        // degenerate into partial token strips.
        let has_full_match = self
            .elements
            .iter()
            .any(|e| matches!(e, ConfigElement::HostBlock(b) if b.host_pattern == alias));

        // Capture the provider for orphaned-group cleanup before mutation.
        let provider_name = self.elements.iter().find_map(|e| match e {
            ConfigElement::HostBlock(b)
                if (has_full_match && b.host_pattern == alias)
                    || (!has_full_match && pattern_contains_token(&b.host_pattern, alias)) =>
            {
                b.provider().map(|(name, _)| name)
            }
            _ => None,
        });

        if has_full_match {
            // Harvest trailing comments (column-0 `#` lines or section
            // headers) from each block we're about to delete, so they
            // survive the delete and re-attach to whatever follows.
            // Skip `# purple:*` metadata: that's bookkeeping owned by the
            // block being removed.
            let mut salvaged_comments: Vec<String> = Vec::new();
            for el in &mut self.elements {
                if let ConfigElement::HostBlock(block) = el
                    && block.host_pattern == alias
                {
                    let drain_from = {
                        let mut idx = block.directives.len();
                        while idx > 0 {
                            let d = &block.directives[idx - 1];
                            let is_user_comment = d.is_non_directive
                                && (d.raw_line.trim().is_empty()
                                    || (d.raw_line.trim().starts_with('#')
                                        && !d.raw_line.trim().starts_with("# purple:")));
                            if !is_user_comment {
                                break;
                            }
                            idx -= 1;
                        }
                        idx
                    };
                    for d in block.directives.drain(drain_from..) {
                        if !d.raw_line.trim().is_empty() {
                            salvaged_comments.push(d.raw_line);
                        }
                    }
                }
            }
            // Remove every block whose full host_pattern equals the input
            // (duplicate-block invariant preserved, matches pre-refactor).
            self.elements.retain(|e| match e {
                ConfigElement::HostBlock(block) => block.host_pattern != alias,
                _ => true,
            });
            // Re-emit salvaged comments as GlobalLines just before the next
            // remaining HostBlock, so a section-header lands above what
            // follows rather than vanishing with the preceding host.
            if !salvaged_comments.is_empty() {
                let next_host = self
                    .elements
                    .iter()
                    .position(|e| matches!(e, ConfigElement::HostBlock(_)));
                let insert_pos = next_host.unwrap_or(self.elements.len());
                for (offset, raw) in salvaged_comments.into_iter().enumerate() {
                    self.elements
                        .insert(insert_pos + offset, ConfigElement::GlobalLine(raw));
                }
            }
        }
        // Always run the token-strip pass too. A config can contain BOTH a
        // full-pattern block (`Host web-01`) AND a sibling block that carries
        // the same alias as one token of a multi-alias pattern (`Host web-01
        // staging`). Without this second pass, `delete_host("web-01")` would
        // remove the first block, leave the second untouched, and `ssh web-01`
        // would silently re-route to staging's HostName. The strip is a no-op
        // when no token-only sibling exists.
        for el in &mut self.elements {
            if let ConfigElement::HostBlock(block) = el {
                let tokens: Vec<&str> = block.host_pattern.split_whitespace().collect();
                if tokens.len() > 1 && tokens.contains(&alias) {
                    let new_pattern = tokens
                        .iter()
                        .filter(|t| **t != alias)
                        .copied()
                        .collect::<Vec<_>>()
                        .join(" ");
                    block.host_pattern = new_pattern.clone();
                    block.raw_host_line = rebuild_host_line(&block.raw_host_line, &new_pattern);
                }
            }
        }
        self.elements.retain(|e| match e {
            ConfigElement::HostBlock(block) => {
                let mut tokens = block.host_pattern.split_whitespace();
                !matches!(
                    (tokens.next(), tokens.next()),
                    (Some(first), None) if first == alias
                )
            }
            _ => true,
        });

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
    ///
    /// For multi-alias blocks this returns `None`: undoable-delete of a
    /// single alias out of a shared `Host` line cannot be round-tripped via
    /// `insert_host_at` because sibling aliases would be lost. Callers
    /// should fall back to `delete_host` in that case (which strips only
    /// the requested token).
    pub fn delete_host_undoable(&mut self, alias: &str) -> Option<(ConfigElement, usize)> {
        // Two-mode match mirroring `delete_host`: full-pattern first (for
        // pattern-browser deletes where `alias` is the whole pattern
        // string), then token match. Undoable delete is only safe when
        // removing the entire block; token-strip on a multi-alias block is
        // therefore refused (returns `None`) because re-inserting the
        // whole element would not reverse a token strip.
        let full_pos = self
            .elements
            .iter()
            .position(|e| matches!(e, ConfigElement::HostBlock(b) if b.host_pattern == alias));
        let pos = if let Some(p) = full_pos {
            p
        } else {
            let token_pos = self.elements.iter().position(|e| match e {
                ConfigElement::HostBlock(b) => pattern_contains_token(&b.host_pattern, alias),
                _ => false,
            })?;
            if let ConfigElement::HostBlock(b) = &self.elements[token_pos]
                && b.host_pattern.split_whitespace().count() > 1
            {
                return None;
            }
            token_pos
        };
        let element = self.elements.remove(pos);
        Some((element, pos))
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
            if let ConfigElement::HostBlock(block) = element
                && let Some((name, _)) = block.provider()
                && name == provider_name
            {
                last_pos = Some(i);
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
            if second < self.elements.len() - 1
                && let ConfigElement::HostBlock(block) = &mut self.elements[second]
            {
                block.ensure_trailing_blank();
            }

            return true;
        }
        false
    }

    /// Convert a HostEntry into a new HostBlock with clean formatting.
    ///
    /// Every value that ends up inside a `raw_line` is routed through
    /// `HostBlock::sanitize_raw_line_value`. A `\n` or `\r` in `alias`,
    /// `hostname`, `user`, `identity_file` or `proxy_jump` would otherwise
    /// split the rendered line and inject extra SSH config directives, for
    /// example a provider API returning `name = "evil\n  ProxyJump bad"`
    /// would land as a real ProxyJump directive in the user's config. The
    /// previous `debug_assert!` guards were stripped from release builds,
    /// so the sanitiser is the only release-mode defence.
    pub(crate) fn entry_to_block(entry: &HostEntry) -> HostBlock {
        let alias = HostBlock::sanitize_raw_line_value(&entry.alias);
        let hostname = HostBlock::sanitize_raw_line_value(&entry.hostname);
        let user = HostBlock::sanitize_raw_line_value(&entry.user);
        let identity_file = HostBlock::sanitize_raw_line_value(&entry.identity_file);
        let proxy_jump = HostBlock::sanitize_raw_line_value(&entry.proxy_jump);

        let mut directives = Vec::new();

        if !hostname.is_empty() {
            directives.push(Directive {
                key: "HostName".to_string(),
                value: hostname.to_string(),
                raw_line: format!("  HostName {}", HostBlock::render_value(&hostname)),
                is_non_directive: false,
            });
        }
        if !user.is_empty() {
            directives.push(Directive {
                key: "User".to_string(),
                value: user.to_string(),
                raw_line: format!("  User {}", HostBlock::render_value(&user)),
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
        if !identity_file.is_empty() {
            directives.push(Directive {
                key: "IdentityFile".to_string(),
                value: identity_file.to_string(),
                raw_line: format!("  IdentityFile {}", HostBlock::render_value(&identity_file)),
                is_non_directive: false,
            });
        }
        if !proxy_jump.is_empty() {
            directives.push(Directive {
                key: "ProxyJump".to_string(),
                value: proxy_jump.to_string(),
                raw_line: format!("  ProxyJump {}", HostBlock::render_value(&proxy_jump)),
                is_non_directive: false,
            });
        }

        HostBlock {
            host_pattern: alias.to_string(),
            raw_host_line: format!("Host {}", alias),
            directives,
        }
    }
}

/// Check whether `host_pattern` contains `alias` as one of its
/// whitespace-separated tokens, with quote-stripping. OpenSSH accepts
/// `Host "alpha"` as `Host alpha`; without quote-stripping the stored pattern
/// `"alpha"` (with literal quote characters) would never match the typed
/// alias `alpha`, leaving the block unreachable to the mutation API.
pub(super) fn pattern_contains_token(host_pattern: &str, alias: &str) -> bool {
    host_pattern.split_whitespace().any(|t| {
        let unquoted = if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
            &t[1..t.len() - 1]
        } else {
            t
        };
        unquoted == alias
    })
}

/// Rebuild a `Host` line with a new pattern, preserving the original line's
/// keyword form (`Host` vs `HOST`, with or without `=`), separator (space vs
/// tab) and trailing inline comment. Used by delete-token and rename paths
/// so that an unrelated edit on a multi-alias block never silently drops the
/// inline comment or tab style the user typed.
///
/// Falls back to `format!("Host {}", new_pattern)` when the original line
/// is too short or malformed to deconstruct.
pub(super) fn rebuild_host_line(original: &str, new_pattern: &str) -> String {
    // Find the position of the inline comment (if any). Inline comments on
    // SSH config lines start with a `#` preceded by whitespace, OUTSIDE any
    // quoted string. This mirrors `strip_inline_comment` in parser.rs.
    let (body, suffix) = {
        let bytes = original.as_bytes();
        let mut in_quote = false;
        let mut comment_start: Option<usize> = None;
        for i in 0..bytes.len() {
            if bytes[i] == b'"' {
                in_quote = !in_quote;
            } else if !in_quote
                && bytes[i] == b'#'
                && i > 0
                && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t')
            {
                comment_start = Some(i - 1); // include the leading whitespace
                break;
            }
        }
        match comment_start {
            Some(idx) => (
                original[..idx].trim_end_matches([' ', '\t']),
                &original[idx..],
            ),
            None => (original.trim_end_matches([' ', '\t']), ""),
        }
    };

    // Split body into keyword + separator + (existing pattern, which we drop).
    // Accept tab or space and optional `=`, matching parse_host_line.
    let bytes = body.as_bytes();
    if bytes.len() < 5 || !bytes[..4].eq_ignore_ascii_case(b"host") {
        return format!("Host {}", new_pattern);
    }
    let sep = bytes[4];
    if !sep.is_ascii_whitespace() && sep != b'=' {
        return format!("Host {}", new_pattern);
    }

    // Preserve the original keyword casing (`Host` vs `HOST` vs `host`).
    let keyword = &body[..4];

    // Capture the original separator span between keyword and pattern so a
    // tab-separated `Host\tweb-01` stays tab-separated and `Host=foo` stays
    // equals-separated.
    let after_keyword = &body[4..];
    let pattern_start = after_keyword
        .char_indices()
        .find(|(_, c)| !c.is_whitespace() && *c != '=')
        .map(|(i, _)| i)
        .unwrap_or(after_keyword.len());
    let separator = &after_keyword[..pattern_start];

    format!("{}{}{}{}", keyword, separator, new_pattern, suffix)
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
