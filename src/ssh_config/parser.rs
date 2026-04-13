use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::debug;

use super::model::{
    ConfigElement, Directive, HostBlock, IncludeDirective, IncludedFile, SshConfigFile,
};

const MAX_INCLUDE_DEPTH: usize = 16;

impl SshConfigFile {
    /// Parse an SSH config file from the given path.
    /// Preserves all formatting, comments, and unknown directives for round-trip fidelity.
    pub fn parse(path: &Path) -> Result<Self> {
        Self::parse_with_depth(path, 0)
    }

    fn parse_with_depth(path: &Path, depth: usize) -> Result<Self> {
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

        let crlf = content.contains("\r\n");
        let config_dir = path.parent().map(|p| p.to_path_buf());
        let elements =
            Self::parse_content_with_includes(content, config_dir.as_deref(), depth, Some(path));

        let host_count = elements
            .iter()
            .filter(|e| matches!(e, super::model::ConfigElement::HostBlock(_)))
            .count();
        debug!(
            "SSH config loaded: {} ({} hosts)",
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

    /// Create an SshConfigFile from raw content string (for demo/test use).
    /// Uses a synthetic path; the file is never read from or written to disk.
    pub fn from_content(content: &str, synthetic_path: PathBuf) -> Self {
        let elements = Self::parse_content_with_includes(content, None, MAX_INCLUDE_DEPTH, None);
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
        Self::parse_content_with_includes(content, None, MAX_INCLUDE_DEPTH, None)
    }

    /// Parse SSH config content, optionally resolving Include directives.
    fn parse_content_with_includes(
        content: &str,
        config_dir: Option<&Path>,
        depth: usize,
        config_path: Option<&Path>,
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

            // Check for Include directive.
            // An indented Include inside a Host block is preserved as a directive
            // (not a top-level Include). A non-indented Include flushes the block.
            let is_indented = line.starts_with(' ') || line.starts_with('\t');
            if !(current_block.is_some() && is_indented) {
                if let Some(pattern) = Self::parse_include_line(trimmed) {
                    if let Some(block) = current_block.take() {
                        elements.push(ConfigElement::HostBlock(block));
                    }
                    let resolved = if depth < MAX_INCLUDE_DEPTH {
                        Self::resolve_include(pattern, config_dir, depth)
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
            }

            // Non-indented Match line = block boundary (flush current Host block).
            // Match blocks are stored as GlobalLines (inert, never edited/deleted).
            if !is_indented && Self::is_match_line(trimmed) {
                if let Some(block) = current_block.take() {
                    elements.push(ConfigElement::HostBlock(block));
                }
                elements.push(ConfigElement::GlobalLine(line.to_string()));
                continue;
            }

            // Non-indented purple:group comment = block boundary (visual separator
            // between provider groups, written as GlobalLine by the sync engine).
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
    fn resolve_include(
        pattern: &str,
        config_dir: Option<&Path>,
        depth: usize,
    ) -> Vec<IncludedFile> {
        let mut files = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for single in Self::split_include_patterns(pattern) {
            let expanded = Self::expand_env_vars(&Self::expand_tilde(single));

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
                        match std::fs::read_to_string(&path) {
                            Ok(content) => {
                                // Strip UTF-8 BOM if present (same as main config)
                                let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
                                let elements = Self::parse_content_with_includes(
                                    content,
                                    path.parent(),
                                    depth + 1,
                                    Some(&path),
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

    /// Expand ~ to the home directory.
    pub(crate) fn expand_tilde(pattern: &str) -> String {
        if let Some(rest) = pattern.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                return format!("{}/{}", home.display(), rest);
            }
        }
        pattern.to_string()
    }

    /// Expand `${VAR}` environment variable references (matches OpenSSH behavior).
    /// Unknown variables are preserved as-is so that SSH itself can report the error.
    pub(crate) fn expand_env_vars(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '$' {
                if let Some(&(_, '{')) = chars.peek() {
                    chars.next(); // consume '{'
                    if let Some(close) = s[i + 2..].find('}') {
                        let var_name = &s[i + 2..i + 2 + close];
                        if let Ok(val) = std::env::var(var_name) {
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
            }
            result.push(c);
        }
        result
    }

    /// Check if a line is a "Host <pattern>" line.
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

    /// Check if a line is a "Match ..." line (block boundary).
    fn is_match_line(trimmed: &str) -> bool {
        let mut parts = trimmed.splitn(2, [' ', '\t']);
        let keyword = parts.next().unwrap_or("");
        keyword.eq_ignore_ascii_case("match")
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

        Some((key.to_string(), value.to_string()))
    }
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
    use std::path::PathBuf;

    fn parse_str(content: &str) -> SshConfigFile {
        SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: PathBuf::from("/tmp/test_config"),
            crlf: content.contains("\r\n"),
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
        let default_path = dirs::home_dir().unwrap().join(".ssh/config");
        assert_eq!(entry.ssh_command(&default_path), "ssh -- 'myserver'");
        let custom_path = PathBuf::from("/tmp/my_config");
        assert_eq!(
            entry.ssh_command(&custom_path),
            "ssh -F '/tmp/my_config' -- 'myserver'"
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
        // SAFETY: test-only, single-threaded context
        unsafe { std::env::set_var("_PURPLE_TEST_VAR", "/custom/path") };
        let result = SshConfigFile::expand_env_vars("${_PURPLE_TEST_VAR}/.ssh/config");
        assert_eq!(result, "/custom/path/.ssh/config");
        unsafe { std::env::remove_var("_PURPLE_TEST_VAR") };
    }

    #[test]
    fn test_expand_env_vars_multiple() {
        // SAFETY: test-only, single-threaded context
        unsafe { std::env::set_var("_PURPLE_TEST_A", "hello") };
        unsafe { std::env::set_var("_PURPLE_TEST_B", "world") };
        let result = SshConfigFile::expand_env_vars("${_PURPLE_TEST_A}/${_PURPLE_TEST_B}");
        assert_eq!(result, "hello/world");
        unsafe { std::env::remove_var("_PURPLE_TEST_A") };
        unsafe { std::env::remove_var("_PURPLE_TEST_B") };
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
}
