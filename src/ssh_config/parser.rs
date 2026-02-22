use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::model::{
    ConfigElement, Directive, HostBlock, IncludeDirective, IncludedFile, SshConfigFile,
};

const MAX_INCLUDE_DEPTH: usize = 5;

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

        let crlf = content.contains("\r\n");
        let config_dir = path.parent().map(|p| p.to_path_buf());
        let elements = Self::parse_content_with_includes(&content, config_dir.as_deref(), depth);

        Ok(SshConfigFile {
            elements,
            path: path.to_path_buf(),
            crlf,
        })
    }

    /// Parse SSH config content from a string (without Include resolution).
    /// Used by tests to create SshConfigFile from inline strings.
    #[allow(dead_code)]
    pub fn parse_content(content: &str) -> Vec<ConfigElement> {
        Self::parse_content_with_includes(content, None, MAX_INCLUDE_DEPTH)
    }

    /// Parse SSH config content, optionally resolving Include directives.
    fn parse_content_with_includes(
        content: &str,
        config_dir: Option<&Path>,
        depth: usize,
    ) -> Vec<ConfigElement> {
        let mut elements = Vec::new();
        let mut current_block: Option<HostBlock> = None;

        for line in content.lines() {
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
    /// Handles both space and tab between keyword and value (SSH allows either).
    fn parse_include_line(trimmed: &str) -> Option<&str> {
        let bytes = trimmed.as_bytes();
        // "include" is 7 ASCII bytes; byte 7 must be ASCII whitespace (space or tab)
        if bytes.len() > 8
            && bytes[..7].eq_ignore_ascii_case(b"include")
            && bytes[7].is_ascii_whitespace()
        {
            // byte 8 is safe to slice at: bytes 0-7 are ASCII, so byte 8 is a char boundary
            let pattern = trimmed[8..].trim();
            if !pattern.is_empty() {
                return Some(pattern);
            }
        }
        None
    }

    /// Resolve an Include pattern to a list of included files.
    fn resolve_include(
        pattern: &str,
        config_dir: Option<&Path>,
        depth: usize,
    ) -> Vec<IncludedFile> {
        let expanded = Self::expand_tilde(pattern);

        // If relative path, resolve against config dir
        let glob_pattern = if expanded.starts_with('/') {
            expanded
        } else if let Some(dir) = config_dir {
            dir.join(&expanded).to_string_lossy().to_string()
        } else {
            return Vec::new();
        };

        let mut files = Vec::new();
        if let Ok(paths) = glob::glob(&glob_pattern) {
            let mut matched: Vec<PathBuf> = paths.filter_map(|p| p.ok()).collect();
            matched.sort();
            for path in matched {
                if path.is_file() {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let elements = Self::parse_content_with_includes(
                            &content,
                            path.parent(),
                            depth + 1,
                        );
                        files.push(IncludedFile {
                            path: path.clone(),
                            elements,
                        });
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

    /// Check if a line is a "Host <pattern>" line.
    /// Returns the pattern if it is.
    /// Handles both space and tab between keyword and value (SSH allows either).
    fn parse_host_line(trimmed: &str) -> Option<String> {
        // Split on first space or tab to isolate the keyword
        let mut parts = trimmed.splitn(2, [' ', '\t']);
        let keyword = parts.next()?;
        if !keyword.eq_ignore_ascii_case("host") {
            return None;
        }
        // "hostname" splits as keyword="hostname" which fails the check above
        let pattern = parts.next()?.trim().to_string();
        if !pattern.is_empty() {
            return Some(pattern);
        }
        None
    }

    /// Parse a "Key Value" directive line.
    fn parse_directive(trimmed: &str) -> Option<(String, String)> {
        // SSH config format: Key Value (space-separated) or Key=Value
        let (key, value) = if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim();
            let value = trimmed[eq_pos + 1..].trim();
            (key, value)
        } else {
            let mut parts = trimmed.splitn(2, char::is_whitespace);
            let key = parts.next()?;
            let value = parts.next().unwrap_or("").trim();
            (key, value)
        };

        if key.is_empty() {
            return None;
        }

        // Strip inline comments (# preceded by whitespace) from parsed value.
        // Don't strip from raw_line — that preserves round-trip fidelity.
        let value = if let Some(pos) = value.find(" #").or_else(|| value.find("\t#")) {
            value[..pos].trim_end()
        } else {
            value
        };

        Some((key.to_string(), value.to_string()))
    }
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
        }
    }

    #[test]
    fn test_empty_config() {
        let config = parse_str("");
        assert!(config.host_entries().is_empty());
    }

    #[test]
    fn test_basic_host() {
        let config = parse_str(
            "Host myserver\n  HostName 192.168.1.10\n  User admin\n  Port 2222\n",
        );
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
        assert!(matches!(&config.elements[0], ConfigElement::GlobalLine(s) if s == "# Global comment"));
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
        assert!(matches!(&config.elements[0], ConfigElement::Include(inc) if inc.raw_line == "Include config.d/*"));
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
        let entry = HostEntry {
            alias: "myserver".to_string(),
            hostname: "10.0.0.1".to_string(),
            ..Default::default()
        };
        assert_eq!(entry.ssh_command(), "ssh myserver");
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
        assert!(matches!(&config.elements[0], ConfigElement::Include(inc) if inc.pattern == "config.d/*"));
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
}
