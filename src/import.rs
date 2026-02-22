use std::io::BufRead;
use std::path::Path;

use crate::quick_add;
use crate::ssh_config::model::{HostEntry, SshConfigFile};

/// Import hosts from a file with one `[user@]host[:port]` per line.
/// Returns (imported, skipped, parse_failures, read_errors).
pub fn import_from_file(
    config: &mut SshConfigFile,
    path: &Path,
    group: Option<&str>,
) -> Result<(usize, usize, usize, usize), String> {
    let file =
        std::fs::File::open(path).map_err(|e| format!("Can't open {}: {}", path.display(), e))?;
    let reader = std::io::BufReader::new(file);

    let mut read_errors = 0;
    let mut parse_failures = 0;
    let lines: Vec<String> = reader
        .lines()
        .filter_map(|r| match r {
            Ok(line) => Some(line),
            Err(_) => {
                read_errors += 1;
                None
            }
        })
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .collect();

    let mut entries = Vec::new();
    for line in &lines {
        let trimmed = line.trim();
        match quick_add::parse_target(trimmed) {
            Ok(parsed) => {
                let alias = parsed
                    .hostname
                    .split('.')
                    .next()
                    .unwrap_or(&parsed.hostname)
                    .to_string();
                entries.push(HostEntry {
                    alias,
                    hostname: parsed.hostname,
                    user: parsed.user,
                    port: parsed.port,
                    identity_file: String::new(),
                    proxy_jump: String::new(),
                    source_file: None,
                    tags: Vec::new(),
                    provider: None,
                });
            }
            Err(_) => {
                parse_failures += 1;
            }
        }
    }

    let (imported, skipped) = add_entries(config, &entries, group)?;
    Ok((imported, skipped, parse_failures, read_errors))
}

/// Import hosts from ~/.ssh/known_hosts.
/// Returns (imported, skipped, parse_failures, read_errors).
pub fn import_from_known_hosts(
    config: &mut SshConfigFile,
    group: Option<&str>,
) -> Result<(usize, usize, usize, usize), String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory.")?;
    let known_hosts_path = home.join(".ssh").join("known_hosts");

    if !known_hosts_path.exists() {
        return Err("~/.ssh/known_hosts not found.".to_string());
    }

    let file = std::fs::File::open(&known_hosts_path)
        .map_err(|e| format!("Can't open known_hosts: {}", e))?;
    let reader = std::io::BufReader::new(file);

    let mut read_errors = 0;
    let mut parse_failures = 0;
    let lines: Vec<String> = reader
        .lines()
        .filter_map(|r| match r {
            Ok(line) => Some(line),
            Err(_) => {
                read_errors += 1;
                None
            }
        })
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .collect();

    let mut entries = Vec::new();
    for line in &lines {
        match parse_known_hosts_line(line) {
            KnownHostResult::Parsed(entry) => entries.push(entry),
            KnownHostResult::Skipped => {} // Intentional skip (hashed, marker, IP-only, wildcard)
            KnownHostResult::Failed => parse_failures += 1,
        }
    }

    let (imported, skipped) = add_entries(config, &entries, group)?;
    Ok((imported, skipped, parse_failures, read_errors))
}

/// Result of parsing a known_hosts line.
enum KnownHostResult {
    /// Successfully parsed into a HostEntry.
    Parsed(HostEntry),
    /// Intentionally skipped (hashed, marker, IP-only, wildcard).
    Skipped,
    /// Failed to parse (malformed line).
    Failed,
}

/// Parse a single known_hosts line into a HostEntry.
fn parse_known_hosts_line(line: &str) -> KnownHostResult {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return KnownHostResult::Failed;
    }

    // Skip marker lines (@cert-authority, @revoked)
    if parts[0].starts_with('@') {
        return KnownHostResult::Skipped;
    }
    let host_part = parts[0];

    // Skip hashed entries (start with |)
    if host_part.starts_with('|') {
        return KnownHostResult::Skipped;
    }

    // Take the first host if comma-separated
    let host = host_part.split(',').next().unwrap_or(host_part);

    // Handle [host]:port format
    let (hostname, port) = if host.starts_with('[') {
        let Some(end) = host.find(']') else {
            return KnownHostResult::Failed;
        };
        let h = &host[1..end];
        let p = if host.len() > end + 2 && host.as_bytes()[end + 1] == b':' {
            host[end + 2..].parse::<u16>().unwrap_or(22)
        } else {
            22
        };
        (h.to_string(), p)
    } else {
        (host.to_string(), 22)
    };

    // Skip empty hostname
    if hostname.is_empty() {
        return KnownHostResult::Failed;
    }

    let alias = hostname
        .split('.')
        .next()
        .unwrap_or(&hostname)
        .to_string();

    // Skip bare IP aliases (IPv4: digits+dots, IPv6: hex+colons) and wildcard patterns
    if alias.chars().all(|c| c.is_ascii_hexdigit() || c == ':') {
        return KnownHostResult::Skipped;
    }
    if alias.contains('*') || alias.contains('?') {
        return KnownHostResult::Skipped;
    }

    KnownHostResult::Parsed(HostEntry {
        alias,
        hostname,
        user: String::new(),
        port,
        identity_file: String::new(),
        proxy_jump: String::new(),
        source_file: None,
        tags: Vec::new(),
        provider: None,
    })
}

/// Add entries to config, deduplicating against existing hosts.
/// Entries with conflicting aliases get auto-suffixed (-2, -3, etc.).
fn add_entries(
    config: &mut SshConfigFile,
    entries: &[HostEntry],
    group: Option<&str>,
) -> Result<(usize, usize), String> {
    let mut imported = 0;
    let mut skipped = 0;
    let mut first_in_group = group.is_some();

    // Add group comment header if specified
    if let Some(group_name) = group {
        if !entries.is_empty() {
            // Blank separator before group comment (only if config isn't empty/already blank)
            if !config.elements.is_empty() && !config.last_element_has_trailing_blank() {
                config.elements.push(
                    crate::ssh_config::model::ConfigElement::GlobalLine(String::new()),
                );
            }
            config.elements.push(
                crate::ssh_config::model::ConfigElement::GlobalLine(format!("# {}", group_name)),
            );
        }
    }

    for entry in entries {
        let alias = config.deduplicate_alias(&entry.alias);
        if alias == entry.alias && config.has_host(&alias) {
            skipped += 1;
            continue;
        }
        let mut deduped = entry.clone();
        deduped.alias = alias;
        if first_in_group {
            // Push first host directly after group comment (no blank separator between them)
            let block = SshConfigFile::entry_to_block(&deduped);
            config.elements.push(
                crate::ssh_config::model::ConfigElement::HostBlock(block),
            );
            first_in_group = false;
        } else {
            config.add_host(&deduped);
        }
        imported += 1;
    }

    Ok((imported, skipped))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_known_hosts_simple() {
        let KnownHostResult::Parsed(entry) =
            parse_known_hosts_line("example.com ssh-rsa AAAA...")
        else {
            panic!("expected Parsed");
        };
        assert_eq!(entry.hostname, "example.com");
        assert_eq!(entry.alias, "example");
        assert_eq!(entry.port, 22);
    }

    #[test]
    fn test_parse_known_hosts_with_port() {
        let KnownHostResult::Parsed(entry) =
            parse_known_hosts_line("[myhost.com]:2222 ssh-ed25519 AAAA...")
        else {
            panic!("expected Parsed");
        };
        assert_eq!(entry.hostname, "myhost.com");
        assert_eq!(entry.alias, "myhost");
        assert_eq!(entry.port, 2222);
    }

    #[test]
    fn test_parse_known_hosts_hashed() {
        assert!(matches!(
            parse_known_hosts_line("|1|abc=|def= ssh-rsa AAAA..."),
            KnownHostResult::Skipped
        ));
    }

    #[test]
    fn test_parse_known_hosts_ip_only() {
        assert!(matches!(
            parse_known_hosts_line("192.168.1.1 ssh-rsa AAAA..."),
            KnownHostResult::Skipped
        ));
    }

    #[test]
    fn test_parse_known_hosts_ipv6_skipped() {
        // Bare IPv6 addresses should be skipped (hex digits + colons)
        assert!(matches!(
            parse_known_hosts_line("2001:db8::1 ssh-rsa AAAA..."),
            KnownHostResult::Skipped
        ));
        assert!(matches!(
            parse_known_hosts_line("fe80::1 ssh-ed25519 AAAA..."),
            KnownHostResult::Skipped
        ));
    }

    #[test]
    fn test_parse_known_hosts_comma_separated() {
        let KnownHostResult::Parsed(entry) =
            parse_known_hosts_line("myserver.com,192.168.1.1 ssh-ed25519 AAAA...")
        else {
            panic!("expected Parsed");
        };
        assert_eq!(entry.hostname, "myserver.com");
        assert_eq!(entry.alias, "myserver");
    }

    #[test]
    fn test_parse_known_hosts_malformed_is_failure() {
        // Too few fields = parse failure
        assert!(matches!(
            parse_known_hosts_line("onlyhost ssh-rsa"),
            KnownHostResult::Failed
        ));
        // Unclosed bracket = parse failure
        assert!(matches!(
            parse_known_hosts_line("[broken ssh-rsa AAAA..."),
            KnownHostResult::Failed
        ));
    }

    #[test]
    fn test_parse_known_hosts_marker_is_skipped() {
        assert!(matches!(
            parse_known_hosts_line("@cert-authority *.example.com ssh-rsa AAAA..."),
            KnownHostResult::Skipped
        ));
        assert!(matches!(
            parse_known_hosts_line("@revoked host.com ssh-rsa AAAA..."),
            KnownHostResult::Skipped
        ));
    }

    #[test]
    fn test_parse_known_hosts_wildcard_is_skipped() {
        assert!(matches!(
            parse_known_hosts_line("*.example.com ssh-rsa AAAA..."),
            KnownHostResult::Skipped
        ));
    }
}
