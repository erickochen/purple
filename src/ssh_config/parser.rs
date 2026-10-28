use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::debug;

use super::model::{
    ConfigElement, Directive, HostBlock, IncludeDirective, IncludedFile, SshConfigFile,
};

const MAX_INCLUDE_DEPTH: usize = 16;

impl SshConfigFile {
    /// Parse an SSH config file from the given path. Convenience wrapper that
    /// captures the process environment once for `${VAR}` / `~` expansion.
    /// Production code threads its injected `Env` via
    /// [`SshConfigFile::parse_with_env`]; this shim keeps a path-only entry
    /// point for tests and external callers without scattering ambient reads.
    pub fn parse(path: &Path) -> Result<Self> {
        Self::parse_with_env(path, &crate::runtime::env::Env::from_process())
    }

    /// Parse with `${VAR}` expansion resolved from an injected environment
    /// instead of the process env, so tests control Include expansion without
    /// mutating `std::env`. Production `parse` delegates here with the real env.
    pub fn parse_with_env(path: &Path, env: &crate::runtime::env::Env) -> Result<Self> {
        Self::parse_with_depth(path, 0, &|n| env.var(n).map(str::to_string))
    }

    fn parse_with_depth(
        path: &Path,
        depth: usize,
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        // Track every config file we've already opened along the include chain.
        // OpenSSH's `readconf.c` aborts with "Too many recursive configuration
        // includes" the moment it detects a cycle; without this set, purple
        // would silently re-read the same file up to MAX_INCLUDE_DEPTH times,
        // producing N copies of every host (verified: A→B→A produces 17 host
        // duplicates). Insert the canonical top-level path before recursing
        // so Include directives that loop back to the main config are caught.
        let mut visited = std::collections::HashSet::new();
        if let Ok(canonical) = std::fs::canonicalize(path) {
            visited.insert(canonical);
        }
        Self::parse_with_depth_visited(path, depth, &mut visited, lookup)
    }

    fn parse_with_depth_visited(
        path: &Path,
        depth: usize,
        visited: &mut std::collections::HashSet<PathBuf>,
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        let content = if path.exists() {
            std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read SSH config at {}", path.display()))?
        } else {
            String::new()
        };

        // Strip UTF-8 BOM if present (Windows editors like Notepad add this).
        let (bom, content) = match content.strip_prefix('\u{FEFF}') {
            Some(stripped) => (true, stripped),
            None => (false, content.as_str()),
        };

        let crlf = detect_crlf_majority(content);
        let config_dir = Self::include_base_dir(path, lookup);
        let elements = Self::parse_content_with_includes(
            content,
            config_dir.as_deref(),
            depth,
            Some(path),
            Some(visited),
            lookup,
        );

        let host_count = elements
            .iter()
            .filter(|e| matches!(e, super::model::ConfigElement::HostBlock(_)))
            .count();
        debug!(
            "[config] SSH config loaded: {} ({} hosts)",
            path.display(),
            host_count
        );

        Ok(SshConfigFile {
            elements,
            path: path.to_path_buf(),
            crlf,
            bom,
        })
    }

    /// Base directory for relative Include paths across the whole include
    /// tree. OpenSSH uses `~/.ssh` for a user config, whichever file holds
    /// the Include, so a config kept elsewhere reads the same files ssh does.
    /// Without a home directory the config file's own directory is used.
    pub(crate) fn include_base_dir(
        config_path: &Path,
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Option<PathBuf> {
        lookup("HOME")
            .filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(".ssh"))
            .or_else(|| config_path.parent().map(Path::to_path_buf))
    }

    /// Create an SshConfigFile from raw content string (for demo/test use).
    /// Uses a synthetic path; the file is never read from or written to disk.
    pub fn from_content(content: &str, synthetic_path: PathBuf) -> Self {
        let elements = Self::parse_content_with_includes(
            content,
            None,
            MAX_INCLUDE_DEPTH,
            None,
            None,
            &|_: &str| None,
        );
        SshConfigFile {
            elements,
            path: synthetic_path,
            crlf: false,
            bom: false,
        }
    }

    /// Parse SSH config content from a string (without Include resolution).
    /// Used by tests to create SshConfigFile from inline strings.
    #[allow(dead_code)]
    pub fn parse_content(content: &str) -> Vec<ConfigElement> {
        Self::parse_content_with_includes(
            content,
            None,
            MAX_INCLUDE_DEPTH,
            None,
            None,
            &|_: &str| None,
        )
    }

    /// Parse SSH config content, optionally resolving Include directives.
    /// `visited` carries the canonical paths of files already opened so an
    /// Include cycle aborts cleanly. `None` disables cycle tracking and is
    /// used by the demo/test entry points that pass `depth = MAX_INCLUDE_DEPTH`
    /// (which gates Include resolution off anyway).
    fn parse_content_with_includes(
        content: &str,
        config_dir: Option<&Path>,
        depth: usize,
        config_path: Option<&Path>,
        mut visited: Option<&mut std::collections::HashSet<PathBuf>>,
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<ConfigElement> {
        let mut elements = Vec::new();
        let mut current_block: Option<HostBlock> = None;

        for (line_idx, raw_line) in content.lines().enumerate() {
            let line_num = line_idx + 1;
            // Strip trailing \r characters that may be left when a file mixes
            // line endings or contains lone \r (old Mac style). Rust's
            // str::lines() splits on \n and strips \r from \r\n pairs, but
            // lone \r (not followed by \n) stays in the line. Stripping
            // prevents stale \r from leaking into raw_line and breaking
            // round-trip idempotency.
            let line = raw_line.trim_end_matches('\r');
            let trimmed = line.trim();

            // OpenSSH's readconf.c treats keywords as significant regardless
            // of leading whitespace; indentation is purely cosmetic. We follow
            // the same rule: Include and Match always start a new section, even
            // when indented inside a previous Host block.
            if let Some(pattern) = Self::parse_include_line(trimmed) {
                if let Some(block) = current_block.take() {
                    elements.push(ConfigElement::HostBlock(block));
                }
                let resolved = if depth < MAX_INCLUDE_DEPTH {
                    Self::resolve_include(
                        pattern,
                        config_dir,
                        depth,
                        visited.as_deref_mut(),
                        lookup,
                    )
                } else {
                    Vec::new()
                };
                elements.push(ConfigElement::Include(IncludeDirective {
                    raw_line: line.to_string(),
                    pattern: pattern.to_string(),
                    resolved_files: resolved,
                }));
                continue;
            }

            // Match line = block boundary (flush current Host block).
            // Match blocks are stored as GlobalLines (inert, never edited/deleted).
            // Recognizes `Match foo`, `Match=foo`, `Match = foo`, with optional
            // leading whitespace.
            if Self::is_match_line(trimmed) {
                if let Some(block) = current_block.take() {
                    elements.push(ConfigElement::HostBlock(block));
                }
                elements.push(ConfigElement::GlobalLine(line.to_string()));
                continue;
            }

            // Non-indented purple:group comment = block boundary (visual separator
            // between provider groups, written as GlobalLine by the sync engine).
            let is_indented = line.starts_with(' ') || line.starts_with('\t');
            if !is_indented && trimmed.starts_with("# purple:group ") {
                if let Some(block) = current_block.take() {
                    elements.push(ConfigElement::HostBlock(block));
                }
                elements.push(ConfigElement::GlobalLine(line.to_string()));
                continue;
            }

            // Check if this line starts a new Host block
            if let Some(pattern) = Self::parse_host_line(trimmed) {
                // Flush the previous block if any
                if let Some(block) = current_block.take() {
                    elements.push(ConfigElement::HostBlock(block));
                }
                current_block = Some(HostBlock {
                    host_pattern: pattern,
                    raw_host_line: line.to_string(),
                    directives: Vec::new(),
                });
                continue;
            }

            // If we're inside a Host block, add this line as a directive
            if let Some(ref mut block) = current_block {
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    // Comment or blank line inside a host block
                    block.directives.push(Directive {
                        key: String::new(),
                        value: String::new(),
                        raw_line: line.to_string(),
                        is_non_directive: true,
                    });
                } else if let Some((key, value)) = Self::parse_directive(trimmed) {
                    block.directives.push(Directive {
                        key,
                        value,
                        raw_line: line.to_string(),
                        is_non_directive: false,
                    });
                } else {
                    // Unrecognized line format — preserve verbatim
                    if let Some(p) = config_path {
                        debug!(
                            "[config] SSH config: unrecognized line {} in {}",
                            line_num,
                            p.display()
                        );
                    }
                    block.directives.push(Directive {
                        key: String::new(),
                        value: String::new(),
                        raw_line: line.to_string(),
                        is_non_directive: true,
                    });
                }
            } else {
                // Global line (before any Host block)
                elements.push(ConfigElement::GlobalLine(line.to_string()));
            }
        }

        // Flush the last block
        if let Some(block) = current_block {
            elements.push(ConfigElement::HostBlock(block));
        }

        elements
    }

    /// Parse an Include directive line. Returns the pattern if it matches.
    /// Handles space, tab and `=` between keyword and value (SSH allows all three).
    /// Matches OpenSSH behavior: skip whitespace, optional `=`, more whitespace.
    fn parse_include_line(trimmed: &str) -> Option<&str> {
        let bytes = trimmed.as_bytes();
        // "include" is 7 ASCII bytes; byte 7 must be whitespace or '='
        if bytes.len() > 7 && bytes[..7].eq_ignore_ascii_case(b"include") {
            let sep = bytes[7];
            if sep.is_ascii_whitespace() || sep == b'=' {
                // Skip whitespace, optional '=', and more whitespace after keyword.
                // All bytes 0..7 are ASCII so byte 7 onward is a valid slice point.
                let rest = trimmed[7..].trim_start();
                let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
                if !rest.is_empty() {
                    return Some(rest);
                }
            }
        }
        None
    }

    /// Split Include patterns respecting double-quoted paths.
    /// OpenSSH supports `Include "path with spaces" other_path`.
    pub(crate) fn split_include_patterns(pattern: &str) -> Vec<&str> {
        let mut result = Vec::new();
        let mut chars = pattern.char_indices().peekable();
        while let Some(&(i, c)) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
                continue;
            }
            if c == '"' {
                chars.next(); // skip opening quote
                let start = i + 1;
                let mut end = pattern.len();
                for (j, ch) in chars.by_ref() {
                    if ch == '"' {
                        end = j;
                        break;
                    }
                }
                let token = &pattern[start..end];
                if !token.is_empty() {
                    result.push(token);
                }
            } else {
                let start = i;
                let mut end = pattern.len();
                for (j, ch) in chars.by_ref() {
                    if ch.is_whitespace() {
                        end = j;
                        break;
                    }
                }
                result.push(&pattern[start..end]);
            }
        }
        result
    }

    /// Resolve an Include pattern to a list of included files.
    /// Supports multiple space-separated patterns on one line (SSH spec).
    /// Handles quoted paths for paths containing spaces.
    ///
    /// `visited` carries the canonical path of every file already opened up
    /// the include chain. When a candidate file's canonical path is already
    /// present, the file is skipped with a warning and its branch is pruned.
    /// This breaks cycles (A→B→A, A→B→C→A) cleanly instead of letting them
    /// fan out exponentially until MAX_INCLUDE_DEPTH bottoms out.
    fn resolve_include(
        pattern: &str,
        config_dir: Option<&Path>,
        depth: usize,
        mut visited: Option<&mut std::collections::HashSet<PathBuf>>,
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<IncludedFile> {
        let mut files = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for single in Self::split_include_patterns(pattern) {
            let expanded = Self::expand_env_vars_with(&Self::expand_tilde(single, lookup), lookup);

            // If relative path, resolve against config dir
            let glob_pattern = if expanded.starts_with('/') {
                expanded
            } else if let Some(dir) = config_dir {
                dir.join(&expanded).to_string_lossy().to_string()
            } else {
                continue;
            };

            if let Ok(paths) = glob::glob(&glob_pattern) {
                let mut matched: Vec<PathBuf> = paths.filter_map(|p| p.ok()).collect();
                matched.sort();
                for path in matched {
                    if path.is_file() && seen.insert(path.clone()) {
                        // Cycle detection: compare canonical paths so a
                        // symlink and its target are treated as the same
                        // file. Falls back to the lexical path when
                        // canonicalize fails (e.g. permission denied).
                        let canonical =
                            std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                        if let Some(ref mut v) = visited
                            && !v.insert(canonical)
                        {
                            log::warn!(
                                "[config] Include cycle detected, skipping {} (already visited up the chain)",
                                path.display()
                            );
                            continue;
                        }
                        match std::fs::read_to_string(&path) {
                            Ok(content) => {
                                // Strip UTF-8 BOM if present (same as main config)
                                let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
                                // Pass `config_dir` (the user-config base for
                                // the entire include tree) through to nested
                                // parses, NOT `path.parent()`. OpenSSH resolves
                                // every relative Include against the original
                                // base, not against the includer's directory.
                                let elements = Self::parse_content_with_includes(
                                    content,
                                    config_dir,
                                    depth + 1,
                                    Some(&path),
                                    visited.as_deref_mut(),
                                    lookup,
                                );
                                files.push(IncludedFile {
                                    path: path.clone(),
                                    elements,
                                });
                            }
                            Err(e) => {
                                log::warn!(
                                    "[config] Could not read Include file {}: {}",
                                    path.display(),
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }
        files
    }

    /// Expand `~` to the home directory, resolved from the injected `lookup`
    /// (`$HOME`) so expansion stays free of ambient `dirs::home_dir()`. On the
    /// supported Unix platforms `$HOME` is the same source `dirs` reads.
    pub(crate) fn expand_tilde(pattern: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
        if let Some(rest) = pattern.strip_prefix("~/")
            && let Some(home) = lookup("HOME")
        {
            return format!("{}/{}", home, rest);
        }
        pattern.to_string()
    }

    /// Expand `${VAR}` references using the real process environment. Test-only
    /// convenience; production paths build the lookup at the edge and call
    /// `expand_env_vars_with` so expansion stays injectable.
    #[cfg(test)]
    pub(crate) fn expand_env_vars(s: &str) -> String {
        Self::expand_env_vars_with(s, &|n| std::env::var(n).ok())
    }

    /// Expand `${VAR}` references via an injected lookup (matches OpenSSH behavior).
    /// Unknown variables are preserved as-is so that SSH itself can report the error.
    /// Tests pass a closure instead of mutating the process environment.
    pub(crate) fn expand_env_vars_with(s: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '$'
                && let Some(&(_, '{')) = chars.peek()
            {
                chars.next(); // consume '{'
                if let Some(close) = s[i + 2..].find('}') {
                    let var_name = &s[i + 2..i + 2 + close];
                    if let Some(val) = lookup(var_name) {
                        result.push_str(&val);
                    } else {
                        // Preserve unknown vars as-is
                        result.push_str(&s[i..i + 2 + close + 1]);
                    }
                    // Advance past the closing '}'
                    while let Some(&(j, _)) = chars.peek() {
                        if j <= i + 2 + close {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    continue;
                }
                // No closing '}' — preserve literally
                result.push('$');
                result.push('{');
                continue;
            }
            result.push(c);
        }
        result
    }

    /// Check if a line is a `Host <pattern>` line.
    /// Returns the pattern if it is.
    /// Handles space, tab and `=` between keyword and value (SSH allows all three).
    /// Matches OpenSSH behavior: skip whitespace, optional `=`, more whitespace.
    /// Strips inline comments (`# ...` preceded by whitespace) from the pattern.
    fn parse_host_line(trimmed: &str) -> Option<String> {
        let bytes = trimmed.as_bytes();
        // "host" is 4 ASCII bytes; byte 4 must be whitespace or '='
        if bytes.len() > 4 && bytes[..4].eq_ignore_ascii_case(b"host") {
            let sep = bytes[4];
            if sep.is_ascii_whitespace() || sep == b'=' {
                // Reject "hostname", "hostkey" etc: after "host" + separator,
                // the keyword must end. If sep is alphanumeric, it's a different keyword.
                // Skip whitespace, optional '=', and more whitespace after keyword.
                let rest = trimmed[4..].trim_start();
                let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
                let pattern = strip_inline_comment(rest).to_string();
                if !pattern.is_empty() {
                    return Some(pattern);
                }
            }
        }
        None
    }

    /// Check if a line is a `Match ...` line (block boundary).
    /// Recognizes `Match foo`, `Match=foo` and `Match = foo` per OpenSSH's
    /// strdelim() rule that any keyword can use `=` as separator.
    fn is_match_line(trimmed: &str) -> bool {
        let bytes = trimmed.as_bytes();
        // "match" is 5 ASCII bytes; byte 5 must be whitespace or '=' (or absent
        // for the bare `Match` keyword, which OpenSSH itself doesn't accept,
        // so we require at least the separator).
        if bytes.len() > 5 && bytes[..5].eq_ignore_ascii_case(b"match") {
            let sep = bytes[5];
            return sep.is_ascii_whitespace() || sep == b'=';
        }
        false
    }

    /// Parse a "Key Value" directive line.
    /// Matches OpenSSH behavior: keyword ends at first whitespace or `=`.
    /// An `=` in the value portion (e.g. `IdentityFile ~/.ssh/id=prod`) is
    /// NOT treated as a separator.
    fn parse_directive(trimmed: &str) -> Option<(String, String)> {
        // Find end of keyword: first whitespace or '='
        let key_end = trimmed.find(|c: char| c.is_whitespace() || c == '=')?;
        let key = &trimmed[..key_end];
        if key.is_empty() {
            return None;
        }

        // Skip whitespace, optional '=', and more whitespace after the keyword
        let rest = trimmed[key_end..].trim_start();
        let rest = rest.strip_prefix('=').unwrap_or(rest);
        let value = rest.trim_start();

        // Strip inline comments (# preceded by whitespace) from parsed value,
        // but only outside quoted strings. Raw_line is untouched for round-trip fidelity.
        let value = strip_inline_comment(value);
        // Unwrap a single fully-quoted token into its logical value, so the
        // form and change-detection see the real path rather than the quoted
        // form. Symmetric with the writer's render_value. Raw_line keeps the
        // quotes for round-trip fidelity.
        let value = strip_surrounding_quotes(value);

        Some((key.to_string(), value.to_string()))
    }
}

/// Strip one wrapping pair of double quotes from a value that is wholly a
/// single quoted token, unescaping `\\` and `\"` inside. OpenSSH uses `"..."`
/// to carry an argument containing spaces; the logical value is the unwrapped,
/// unescaped content. Inverse of `HostBlock::render_value`. A value with an
/// UNescaped interior quote (a command line such as `ssh -W "%h:%p" gw`) is
/// not a single quoted token and is left untouched.
fn strip_surrounding_quotes(value: &str) -> std::borrow::Cow<'_, str> {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return std::borrow::Cow::Borrowed(value);
    }
    let interior = &value[1..value.len() - 1];
    let mut out = String::with_capacity(interior.len());
    let mut chars = interior.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                // Unknown escape: keep the backslash and the following char
                // verbatim so no information is lost.
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            // A bare, unescaped quote inside means this is not a single quoted
            // token (e.g. two adjacent quoted args); leave the whole value as-is.
            '"' => return std::borrow::Cow::Borrowed(value),
            _ => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Detect CRLF only when the MAJORITY of lines use `\r\n`. A single
/// Windows-edited line in an otherwise LF file must not flip the entire
/// output to CRLF on the next save; that would produce a wholesale diff
/// against version control for an unrelated edit.
pub fn detect_crlf_majority(content: &str) -> bool {
    let mut crlf_lines = 0usize;
    let mut lf_lines = 0usize;
    for line in content.split('\n') {
        if line.ends_with('\r') {
            crlf_lines += 1;
        } else if !line.is_empty() {
            lf_lines += 1;
        }
    }
    crlf_lines > lf_lines
}

/// Strip an inline comment (`# ...` preceded by whitespace) from a parsed value,
/// respecting double-quoted strings.
fn strip_inline_comment(value: &str) -> &str {
    let bytes = value.as_bytes();
    let mut in_quote = false;
    for i in 0..bytes.len() {
        if bytes[i] == b'"' {
            in_quote = !in_quote;
        } else if !in_quote
            && bytes[i] == b'#'
            && i > 0
            && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t')
        {
            return value[..i].trim_end();
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use std::path::PathBuf;

    fn parse_str(content: &str) -> SshConfigFile {
        SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: tempfile::tempdir()
                .expect("tempdir")
                .keep()
                .join("test_config"),
            crlf: crate::ssh_config::parser::detect_crlf_majority(content),
            bom: false,
        }
    }

    #[test]
    fn test_empty_config() {
        let config = parse_str("");
        assert!(config.host_entries().is_empty());
    }

    #[test]
    fn test_basic_host() {
        let config =
            parse_str("Host myserver\n  HostName 192.168.1.10\n  User admin\n  Port 2222\n");
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "myserver");
        assert_eq!(entries[0].hostname, "192.168.1.10");
        assert_eq!(entries[0].user, "admin");
        assert_eq!(entries[0].port, 2222);
    }

    #[test]
    fn test_multiple_hosts() {
        let content = "\
Host alpha
  HostName alpha.example.com
  User deploy

Host beta
  HostName beta.example.com
  User root
  Port 22022
";
        let config = parse_str(content);
        let entries = config.host_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].alias, "alpha");
        assert_eq!(entries[1].alias, "beta");
        assert_eq!(entries[1].port, 22022);
    }

    #[test]
    fn test_wildcard_host_filtered() {
        let content = "\
Host *
  ServerAliveInterval 60

Host myserver
  HostName 10.0.0.1
";
        let config = parse_str(content);
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "myserver");
    }

    #[test]
    fn test_comments_preserved() {
        let content = "\
# Global comment
Host myserver
  # This is a comment
  HostName 10.0.0.1
  User admin
";
        let config = parse_str(content);
        // Check that the global comment is preserved
        assert!(
            matches!(&config.elements[0], ConfigElement::GlobalLine(s) if s == "# Global comment")
        );
        // Check that the host block has the comment directive
        if let ConfigElement::HostBlock(block) = &config.elements[1] {
            assert!(block.directives[0].is_non_directive);
            assert_eq!(block.directives[0].raw_line, "  # This is a comment");
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn proxy_command_sets_has_proxy_command_unless_none() {
        let content = "Host a\n  ProxyCommand tsh proxy ssh %r@%h:%p\n\n\
                       Host b\n  proxycommand none\n\n\
                       Host c\n  HostName 10.0.0.1\n\n\
                       Host d\n  ProxyCommand none\n  ProxyCommand nc %h %p\n";
        let config = parse_str(content);
        let entries = config.host_entries();
        let by_alias = |alias: &str| entries.iter().find(|e| e.alias == alias).unwrap();
        assert!(by_alias("a").has_proxy_command);
        assert!(!by_alias("b").has_proxy_command);
        assert!(!by_alias("c").has_proxy_command);
        // First value wins, as ssh_config(5) prescribes.
        assert!(!by_alias("d").has_proxy_command);
    }

    #[test]
    fn test_identity_file_and_proxy_jump() {
        let content = "\
Host bastion
  HostName bastion.example.com
  User admin
  IdentityFile ~/.ssh/id_ed25519
  ProxyJump gateway
";
        let config = parse_str(content);
        let entries = config.host_entries();
        assert_eq!(entries[0].identity_file, "~/.ssh/id_ed25519");
        assert_eq!(entries[0].proxy_jump, "gateway");
    }

    #[test]
    fn test_unknown_directives_preserved() {
        let content = "\
Host myserver
  HostName 10.0.0.1
  ForwardAgent yes
  LocalForward 8080 localhost:80
";
        let config = parse_str(content);
        if let ConfigElement::HostBlock(block) = &config.elements[0] {
            assert_eq!(block.directives.len(), 3);
            assert_eq!(block.directives[1].key, "ForwardAgent");
            assert_eq!(block.directives[1].value, "yes");
            assert_eq!(block.directives[2].key, "LocalForward");
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn test_include_directive_parsed() {
        let content = "\
Include config.d/*

Host myserver
  HostName 10.0.0.1
";
        let config = parse_str(content);
        // parse_content uses no config_dir, so Include resolves to no files
        assert!(
            matches!(&config.elements[0], ConfigElement::Include(inc) if inc.raw_line == "Include config.d/*")
        );
        // Blank line becomes a GlobalLine between Include and HostBlock
        assert!(matches!(&config.elements[1], ConfigElement::GlobalLine(s) if s.is_empty()));
        assert!(matches!(&config.elements[2], ConfigElement::HostBlock(_)));
    }

    #[test]
    fn test_include_round_trip() {
        let content = "\
Include ~/.ssh/config.d/*

Host myserver
  HostName 10.0.0.1
";
        let config = parse_str(content);
        assert_eq!(config.serialize(), content);
    }

    #[test]
    fn test_ssh_command() {
        use crate::ssh_config::model::HostEntry;
        use std::path::PathBuf;
        let entry = HostEntry {
            alias: "myserver".to_string(),
            hostname: "10.0.0.1".to_string(),
            ..Default::default()
        };
        let paths = crate::runtime::env::Paths::new("/home/testuser");
        let default_path = paths.ssh_dir().join("config");
        assert_eq!(
            entry.ssh_command(Some(&paths), &default_path),
            "ssh -- 'myserver'"
        );
        let custom_path = PathBuf::from("/tmp/my_config");
        assert_eq!(
            entry.ssh_command(Some(&paths), &custom_path),
            "ssh -F '/tmp/my_config' -- 'myserver'"
        );
        assert_eq!(
            entry.ssh_command_with("kitten ssh", Some(&paths), &default_path),
            "kitten ssh -- 'myserver'"
        );
        assert_eq!(
            entry.ssh_command_with("kitten ssh", Some(&paths), &custom_path),
            "kitten ssh -F '/tmp/my_config' -- 'myserver'"
        );
    }

    #[test]
    fn test_unicode_comment_no_panic() {
        // "# abcdeé" has byte 8 mid-character (é starts at byte 7, is 2 bytes)
        // This must not panic in parse_include_line
        let content = "# abcde\u{00e9} test\n\nHost myserver\n  HostName 10.0.0.1\n";
        let config = parse_str(content);
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "myserver");
    }

    #[test]
    fn test_unicode_multibyte_line_no_panic() {
        // Three 3-byte CJK characters: byte 8 falls mid-character
        let content = "# \u{3042}\u{3042}\u{3042}xyz\n\nHost myserver\n  HostName 10.0.0.1\n";
        let config = parse_str(content);
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_host_with_tab_separator() {
        let content = "Host\tmyserver\n  HostName 10.0.0.1\n";
        let config = parse_str(content);
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "myserver");
    }

    #[test]
    fn test_include_with_tab_separator() {
        let content = "Include\tconfig.d/*\n\nHost myserver\n  HostName 10.0.0.1\n";
        let config = parse_str(content);
        assert!(
            matches!(&config.elements[0], ConfigElement::Include(inc) if inc.pattern == "config.d/*")
        );
    }

    #[test]
    fn test_include_with_equals_separator() {
        let content = "Include=config.d/*\n\nHost myserver\n  HostName 10.0.0.1\n";
        let config = parse_str(content);
        assert!(
            matches!(&config.elements[0], ConfigElement::Include(inc) if inc.pattern == "config.d/*")
        );
    }

    #[test]
    fn test_include_with_space_equals_separator() {
        let content = "Include =config.d/*\n\nHost myserver\n  HostName 10.0.0.1\n";
        let config = parse_str(content);
        assert!(
            matches!(&config.elements[0], ConfigElement::Include(inc) if inc.pattern == "config.d/*")
        );
    }

    #[test]
    fn test_include_with_space_equals_space_separator() {
        let content = "Include = config.d/*\n\nHost myserver\n  HostName 10.0.0.1\n";
        let config = parse_str(content);
        assert!(
            matches!(&config.elements[0], ConfigElement::Include(inc) if inc.pattern == "config.d/*")
        );
    }

    #[test]
    fn test_hostname_not_confused_with_host() {
        // "HostName" should not be parsed as a Host line
        let content = "Host myserver\n  HostName example.com\n";
        let config = parse_str(content);
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hostname, "example.com");
    }

    #[test]
    fn test_equals_in_value_not_treated_as_separator() {
        let content = "Host myserver\n  IdentityFile ~/.ssh/id=prod\n";
        let config = parse_str(content);
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].identity_file, "~/.ssh/id=prod");
    }

    #[test]
    fn test_equals_syntax_key_value() {
        let content = "Host myserver\n  HostName=10.0.0.1\n  User = admin\n";
        let config = parse_str(content);
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hostname, "10.0.0.1");
        assert_eq!(entries[0].user, "admin");
    }

    #[test]
    fn test_inline_comment_inside_quotes_preserved() {
        let content = "Host myserver\n  ProxyCommand ssh -W \"%h #test\" gateway\n";
        let config = parse_str(content);
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        // The value should preserve the # inside quotes
        if let ConfigElement::HostBlock(block) = &config.elements[0] {
            let proxy_cmd = block
                .directives
                .iter()
                .find(|d| d.key == "ProxyCommand")
                .unwrap();
            assert_eq!(proxy_cmd.value, "ssh -W \"%h #test\" gateway");
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn test_inline_comment_outside_quotes_stripped() {
        let content = "Host myserver\n  HostName 10.0.0.1 # production\n";
        let config = parse_str(content);
        let entries = config.host_entries();
        assert_eq!(entries[0].hostname, "10.0.0.1");
    }

    #[test]
    fn test_surrounding_quotes_stripped_from_single_token() {
        // A value that is one fully-quoted token (OpenSSH's way to carry
        // spaces) parses to the logical, unquoted value so the form and the
        // change-detection see the real path, not the quoted form.
        let config = parse_str("Host h\n  IdentityFile \"~/my key/id\"\n");
        if let ConfigElement::HostBlock(block) = &config.elements[0] {
            let d = block
                .directives
                .iter()
                .find(|d| d.key == "IdentityFile")
                .expect("IdentityFile directive");
            assert_eq!(d.value, "~/my key/id");
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn test_internal_quotes_not_stripped() {
        // A value whose quotes are internal (a command line) keeps them: only
        // a value that is wholly wrapped in one quote pair is unwrapped.
        let config = parse_str("Host h\n  ProxyCommand ssh -W \"%h:%p\" gw\n");
        if let ConfigElement::HostBlock(block) = &config.elements[0] {
            let d = block
                .directives
                .iter()
                .find(|d| d.key == "ProxyCommand")
                .expect("ProxyCommand directive");
            assert_eq!(d.value, "ssh -W \"%h:%p\" gw");
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn test_host_inline_comment_stripped() {
        let content = "Host alpha # this is a comment\n  HostName 10.0.0.1\n";
        let config = parse_str(content);
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "alpha");
        // Raw line is preserved for round-trip fidelity
        if let ConfigElement::HostBlock(block) = &config.elements[0] {
            assert_eq!(block.raw_host_line, "Host alpha # this is a comment");
            assert_eq!(block.host_pattern, "alpha");
        } else {
            panic!("Expected HostBlock");
        }
    }

    #[test]
    fn test_match_block_is_global_line() {
        let content = "\
Host myserver
  HostName 10.0.0.1

Match host *.example.com
  ForwardAgent yes
";
        let config = parse_str(content);
        // Match line should flush the Host block and become a GlobalLine
        let host_count = config
            .elements
            .iter()
            .filter(|e| matches!(e, ConfigElement::HostBlock(_)))
            .count();
        assert_eq!(host_count, 1);
        // Match line itself
        assert!(
            config.elements.iter().any(
                |e| matches!(e, ConfigElement::GlobalLine(s) if s == "Match host *.example.com")
            )
        );
        // Indented lines after Match (no current_block) become GlobalLines
        assert!(
            config
                .elements
                .iter()
                .any(|e| matches!(e, ConfigElement::GlobalLine(s) if s.contains("ForwardAgent")))
        );
    }

    #[test]
    fn test_match_block_survives_host_deletion() {
        let content = "\
Host myserver
  HostName 10.0.0.1

Match host *.example.com
  ForwardAgent yes

Host other
  HostName 10.0.0.2
";
        let mut config = parse_str(content);
        config.delete_host("myserver");
        let output = config.serialize();
        assert!(output.contains("Match host *.example.com"));
        assert!(output.contains("ForwardAgent yes"));
        assert!(output.contains("Host other"));
        assert!(!output.contains("Host myserver"));
    }

    #[test]
    fn test_match_block_round_trip() {
        let content = "\
Host myserver
  HostName 10.0.0.1

Match host *.example.com
  ForwardAgent yes
";
        let config = parse_str(content);
        assert_eq!(config.serialize(), content);
    }

    #[test]
    fn test_match_at_start_of_file() {
        let content = "\
Match all
  ServerAliveInterval 60

Host myserver
  HostName 10.0.0.1
";
        let config = parse_str(content);
        assert!(matches!(&config.elements[0], ConfigElement::GlobalLine(s) if s == "Match all"));
        assert!(
            matches!(&config.elements[1], ConfigElement::GlobalLine(s) if s.contains("ServerAliveInterval"))
        );
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "myserver");
    }

    #[test]
    fn test_host_equals_syntax() {
        let config = parse_str("Host=foo\n  HostName 10.0.0.1\n");
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "foo");
    }

    #[test]
    fn test_host_space_equals_syntax() {
        let config = parse_str("Host =foo\n  HostName 10.0.0.1\n");
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "foo");
    }

    #[test]
    fn test_host_equals_space_syntax() {
        let config = parse_str("Host= foo\n  HostName 10.0.0.1\n");
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "foo");
    }

    #[test]
    fn test_host_space_equals_space_syntax() {
        let config = parse_str("Host = foo\n  HostName 10.0.0.1\n");
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "foo");
    }

    #[test]
    fn test_host_equals_case_insensitive() {
        let config = parse_str("HOST=foo\n  HostName 10.0.0.1\n");
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "foo");
    }

    #[test]
    fn test_hostname_equals_not_parsed_as_host() {
        // "HostName=example.com" must NOT be parsed as a Host line
        let config = parse_str("Host myserver\n  HostName=example.com\n");
        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "myserver");
        assert_eq!(entries[0].hostname, "example.com");
    }

    #[test]
    fn test_host_multi_pattern_with_inline_comment() {
        // Multi-pattern host with inline comment: "prod staging # servers"
        // The comment should be stripped, but "prod staging" is still multi-pattern
        // and gets filtered by host_entries()
        let content = "Host prod staging # servers\n  HostName 10.0.0.1\n";
        let config = parse_str(content);
        if let ConfigElement::HostBlock(block) = &config.elements[0] {
            assert_eq!(block.host_pattern, "prod staging");
        } else {
            panic!("Expected HostBlock");
        }
        // Multi-pattern hosts are filtered out of host_entries
        assert_eq!(config.host_entries().len(), 0);
    }

    #[test]
    fn test_expand_env_vars_basic() {
        let result =
            SshConfigFile::expand_env_vars_with("${_PURPLE_TEST_VAR}/.ssh/config", &|name| {
                match name {
                    "_PURPLE_TEST_VAR" => Some("/custom/path".to_string()),
                    _ => None,
                }
            });
        assert_eq!(result, "/custom/path/.ssh/config");
    }

    #[test]
    fn test_expand_env_vars_multiple() {
        let result =
            SshConfigFile::expand_env_vars_with("${_PURPLE_TEST_A}/${_PURPLE_TEST_B}", &|name| {
                match name {
                    "_PURPLE_TEST_A" => Some("hello".to_string()),
                    "_PURPLE_TEST_B" => Some("world".to_string()),
                    _ => None,
                }
            });
        assert_eq!(result, "hello/world");
    }

    #[test]
    fn test_expand_env_vars_unknown_preserved() {
        let result = SshConfigFile::expand_env_vars("${_PURPLE_NONEXISTENT_VAR}/path");
        assert_eq!(result, "${_PURPLE_NONEXISTENT_VAR}/path");
    }

    #[test]
    fn test_expand_env_vars_no_vars() {
        let result = SshConfigFile::expand_env_vars("~/.ssh/config.d/*");
        assert_eq!(result, "~/.ssh/config.d/*");
    }

    #[test]
    fn test_expand_env_vars_unclosed_brace() {
        let result = SshConfigFile::expand_env_vars("${UNCLOSED/path");
        assert_eq!(result, "${UNCLOSED/path");
    }

    #[test]
    fn test_expand_env_vars_dollar_without_brace() {
        let result = SshConfigFile::expand_env_vars("$HOME/.ssh/config");
        // Only ${VAR} syntax should be expanded, not bare $VAR
        assert_eq!(result, "$HOME/.ssh/config");
    }

    #[test]
    fn test_max_include_depth_matches_openssh() {
        assert_eq!(MAX_INCLUDE_DEPTH, 16);
    }

    #[test]
    fn test_split_include_patterns_single_unquoted() {
        let result = SshConfigFile::split_include_patterns("config.d/*");
        assert_eq!(result, vec!["config.d/*"]);
    }

    #[test]
    fn test_split_include_patterns_quoted_with_spaces() {
        let result = SshConfigFile::split_include_patterns("\"/path/with spaces/config\"");
        assert_eq!(result, vec!["/path/with spaces/config"]);
    }

    #[test]
    fn test_split_include_patterns_mixed() {
        let result =
            SshConfigFile::split_include_patterns("\"/path/with spaces/*\" ~/.ssh/config.d/*");
        assert_eq!(result, vec!["/path/with spaces/*", "~/.ssh/config.d/*"]);
    }

    #[test]
    fn test_split_include_patterns_quoted_no_spaces() {
        let result = SshConfigFile::split_include_patterns("\"config.d/*\"");
        assert_eq!(result, vec!["config.d/*"]);
    }

    #[test]
    fn test_split_include_patterns_multiple_unquoted() {
        let result = SshConfigFile::split_include_patterns("~/.ssh/conf.d/* /etc/ssh/config.d/*");
        assert_eq!(result, vec!["~/.ssh/conf.d/*", "/etc/ssh/config.d/*"]);
    }

    // Relative Include paths resolve against `~/.ssh` the way ssh does,
    // also for a config kept outside it.

    struct IncludeFixture {
        _dir: tempfile::TempDir,
        home: PathBuf,
        elsewhere: PathBuf,
    }

    impl IncludeFixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let home = dir.path().join("home");
            let elsewhere = dir.path().join("elsewhere");
            std::fs::create_dir_all(home.join(".ssh")).expect("ssh dir");
            std::fs::create_dir_all(&elsewhere).expect("elsewhere dir");
            Self {
                _dir: dir,
                home,
                elsewhere,
            }
        }

        fn write(&self, path: &std::path::Path, content: &str) {
            std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
            std::fs::write(path, content).expect("write fixture");
        }

        fn env(&self) -> crate::runtime::env::Env {
            crate::runtime::env::Env::for_test(&self.home)
                .with_var("HOME", self.home.to_string_lossy().to_string())
        }

        fn aliases(config: &SshConfigFile) -> Vec<String> {
            config.host_entries().into_iter().map(|h| h.alias).collect()
        }
    }

    #[test]
    fn relative_include_resolves_from_home_ssh_for_a_config_elsewhere() {
        let f = IncludeFixture::new();
        f.write(
            &f.home.join(".ssh/extra"),
            "Host from-ssh-dir\n  HostName 10.0.0.1\n",
        );
        f.write(
            &f.elsewhere.join("extra"),
            "Host from-config-dir\n  HostName 10.0.0.2\n",
        );
        let config_path = f.elsewhere.join("config");
        f.write(&config_path, "Include extra\n");

        let config = SshConfigFile::parse_with_env(&config_path, &f.env()).expect("parse");
        assert_eq!(IncludeFixture::aliases(&config), vec!["from-ssh-dir"]);
    }

    #[test]
    fn nested_relative_include_keeps_the_home_ssh_base() {
        let f = IncludeFixture::new();
        f.write(&f.home.join(".ssh/conf.d/first"), "Include second\n");
        f.write(
            &f.home.join(".ssh/second"),
            "Host nested-base\n  HostName 10.0.0.3\n",
        );
        f.write(
            &f.home.join(".ssh/conf.d/second"),
            "Host nested-includer-dir\n  HostName 10.0.0.4\n",
        );
        let config_path = f.elsewhere.join("config");
        f.write(&config_path, "Include conf.d/first\n");

        let config = SshConfigFile::parse_with_env(&config_path, &f.env()).expect("parse");
        assert_eq!(IncludeFixture::aliases(&config), vec!["nested-base"]);
    }

    // For the default ~/.ssh/config the base stays its own directory, also
    // with a trailing slash on HOME.
    #[test]
    fn default_config_include_base_is_its_own_directory() {
        let f = IncludeFixture::new();
        let default = f.home.join(".ssh").join("config");
        let expected = default.parent().map(std::path::Path::to_path_buf);
        let env = f.env();
        let lookup = |n: &str| env.var(n).map(str::to_string);
        assert_eq!(SshConfigFile::include_base_dir(&default, &lookup), expected);
        let with_slash = format!("{}/", f.home.display());
        let lookup_slash = |n: &str| (n == "HOME").then(|| with_slash.clone());
        assert_eq!(
            SshConfigFile::include_base_dir(&default, &lookup_slash),
            expected
        );
    }

    #[test]
    fn relative_include_without_home_uses_the_config_directory() {
        let f = IncludeFixture::new();
        f.write(
            &f.elsewhere.join("extra"),
            "Host from-config-dir\n  HostName 10.0.0.2\n",
        );
        let config_path = f.elsewhere.join("config");
        f.write(&config_path, "Include extra\n");
        let no_home = crate::runtime::env::Env::for_test(&f.home);

        let config = SshConfigFile::parse_with_env(&config_path, &no_home).expect("parse");
        assert_eq!(IncludeFixture::aliases(&config), vec!["from-config-dir"]);
    }

    #[test]
    fn include_glob_dirs_follow_the_home_ssh_base() {
        let f = IncludeFixture::new();
        f.write(&f.home.join(".ssh/conf.d/a"), "Include more/*\n");
        let config_path = f.elsewhere.join("config");
        f.write(&config_path, "Include conf.d/*\n");
        let env = f.env();

        let config = SshConfigFile::parse_with_env(&config_path, &env).expect("parse");
        let dirs = config.include_glob_dirs_with(&|n| env.var(n).map(str::to_string));
        assert_eq!(
            dirs,
            vec![f.home.join(".ssh/conf.d"), f.home.join(".ssh/more")]
        );
    }
}
