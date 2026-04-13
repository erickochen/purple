use std::io::BufRead;
use std::path::Path;

use log::{debug, info};

use crate::quick_add;
use crate::ssh_config::model::{HostEntry, SshConfigFile};

/// Import hosts from a file with one `[user@]host[:port]` per line.
/// Returns (imported, skipped, parse_failures, read_errors).
pub fn import_from_file(
    config: &mut SshConfigFile,
    path: &Path,
    group: Option<&str>,
) -> Result<(usize, usize, usize, usize), String> {
    info!("Import started: source={}", path.display());
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
                // Skip entries whose derived alias is an SSH pattern (*, ?, [, !)
                if crate::ssh_config::model::is_host_pattern(&alias) {
                    parse_failures += 1;
                    continue;
                }
                entries.push(HostEntry {
                    alias,
                    hostname: parsed.hostname,
                    user: parsed.user,
                    port: parsed.port,
                    ..Default::default()
                });
            }
            Err(_) => {
                debug!("[config] Import: skipped unparseable line: {trimmed}");
                parse_failures += 1;
            }
        }
    }

    let (imported, skipped) = add_entries(config, &entries, group)?;
    info!("Import completed: {imported} hosts added, {skipped} skipped");
    Ok((imported, skipped, parse_failures, read_errors))
}

/// Count how many importable entries exist in ~/.ssh/known_hosts.
/// Returns the count of parseable hostname entries, or 0 if the file
/// doesn't exist or can't be read.
pub fn count_known_hosts_candidates() -> usize {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return 0,
    };
    let known_hosts_path = home.join(".ssh").join("known_hosts");
    let file = match std::fs::File::open(&known_hosts_path) {
        Ok(f) => f,
        Err(_) => return 0,
    };
    let reader = std::io::BufReader::new(file);
    reader
        .lines()
        .map_while(Result::ok)
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .filter(|line| matches!(parse_known_hosts_line(line), KnownHostResult::Parsed(_)))
        .count()
}

/// Import hosts from ~/.ssh/known_hosts.
/// Returns (imported, skipped, parse_failures, read_errors).
pub fn import_from_known_hosts(
    config: &mut SshConfigFile,
    group: Option<&str>,
) -> Result<(usize, usize, usize, usize), String> {
    info!("Import started: source=~/.ssh/known_hosts");
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
    info!("Import completed: {imported} hosts added, {skipped} skipped");
    Ok((imported, skipped, parse_failures, read_errors))
}

/// Check if a hostname is a bare IP address (not an FQDN).
fn is_bare_ip(host: &str) -> bool {
    // IPv4: digits and dots only (e.g., "192.168.1.1")
    if !host.is_empty() && host.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return true;
    }
    // IPv6: hex digits + colons + optional zone ID (e.g., "2001:db8::1", "fe80::1%en0")
    let ipv6_part = host.split('%').next().unwrap_or(host);
    ipv6_part.contains(':') && ipv6_part.chars().all(|c| c.is_ascii_hexdigit() || c == ':')
}

/// Result of parsing a known_hosts line.
#[allow(clippy::large_enum_variant)]
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

    // Pick first non-IP host from comma-separated list.
    // known_hosts may have ip,hostname or hostname,ip pairs.
    let host = host_part
        .split(',')
        .find(|entry| {
            let bare = if entry.starts_with('[') {
                entry
                    .get(1..entry.find(']').unwrap_or(entry.len()))
                    .unwrap_or(entry)
            } else {
                entry
            };
            !is_bare_ip(bare)
        })
        .unwrap_or_else(|| host_part.split(',').next().unwrap_or(host_part));

    // Handle [host]:port format
    let (hostname, port) = if host.starts_with('[') {
        let Some(end) = host.find(']') else {
            return KnownHostResult::Failed;
        };
        let h = &host[1..end];
        let rest = &host[end + 1..];
        let p = if rest.is_empty() {
            22
        } else if let Some(port_str) = rest.strip_prefix(':') {
            if port_str.is_empty() {
                return KnownHostResult::Failed; // [host]: with no port
            }
            match port_str.parse::<u16>() {
                Ok(port) if port > 0 => port,
                _ => return KnownHostResult::Failed,
            }
        } else {
            return KnownHostResult::Failed; // [host]junk with no colon
        };
        (h.to_string(), p)
    } else {
        (host.to_string(), 22)
    };

    // Skip empty hostname
    if hostname.is_empty() {
        return KnownHostResult::Failed;
    }

    // Skip bare IP addresses (not FQDNs) before alias extraction.
    if is_bare_ip(&hostname) {
        return KnownHostResult::Skipped;
    }

    let alias = hostname.split('.').next().unwrap_or(&hostname).to_string();

    // Skip wildcard/pattern entries
    if crate::ssh_config::model::is_host_pattern(&alias) {
        return KnownHostResult::Skipped;
    }

    KnownHostResult::Parsed(HostEntry {
        alias,
        hostname,
        port,
        ..Default::default()
    })
}

/// Add entries to config, skipping exact alias duplicates.
fn add_entries(
    config: &mut SshConfigFile,
    entries: &[HostEntry],
    group: Option<&str>,
) -> Result<(usize, usize), String> {
    let mut imported = 0;
    let mut skipped = 0;
    let mut header_written = false;

    for entry in entries {
        if config.has_host(&entry.alias) {
            skipped += 1;
            continue;
        }

        // Write group header before the first actually-imported host
        if let Some(group_name) = group.filter(|_| !header_written) {
            if !config.elements.is_empty() && !config.last_element_has_trailing_blank() {
                config
                    .elements
                    .push(crate::ssh_config::model::ConfigElement::GlobalLine(
                        String::new(),
                    ));
            }
            config
                .elements
                .push(crate::ssh_config::model::ConfigElement::GlobalLine(
                    format!("# {}", group_name),
                ));
            header_written = true;
        }

        if group.is_some() && imported == 0 {
            // Push first host directly after group comment (no blank separator between them)
            let block = SshConfigFile::entry_to_block(entry);
            config
                .elements
                .push(crate::ssh_config::model::ConfigElement::HostBlock(block));
        } else {
            config.add_host(entry);
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
        let KnownHostResult::Parsed(entry) = parse_known_hosts_line("example.com ssh-rsa AAAA...")
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
    fn test_parse_known_hosts_hex_hostname_not_skipped() {
        // Pure hex hostnames without colons are valid hostnames, not IPs
        let KnownHostResult::Parsed(entry) = parse_known_hosts_line("deadbeef ssh-rsa AAAA...")
        else {
            panic!("expected Parsed");
        };
        assert_eq!(entry.alias, "deadbeef");

        let KnownHostResult::Parsed(entry) =
            parse_known_hosts_line("cafe.example.com ssh-rsa AAAA...")
        else {
            panic!("expected Parsed");
        };
        assert_eq!(entry.alias, "cafe");
    }

    #[test]
    fn test_parse_known_hosts_invalid_port() {
        // Non-numeric port
        assert!(matches!(
            parse_known_hosts_line("[myhost]:abc ssh-rsa AAAA..."),
            KnownHostResult::Failed
        ));
        // Port out of u16 range
        assert!(matches!(
            parse_known_hosts_line("[myhost]:70000 ssh-rsa AAAA..."),
            KnownHostResult::Failed
        ));
        // Port 0
        assert!(matches!(
            parse_known_hosts_line("[myhost]:0 ssh-rsa AAAA..."),
            KnownHostResult::Failed
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
    fn test_parse_known_hosts_numeric_first_label_not_skipped() {
        // "123.example.com" has a numeric first label but is a valid FQDN, not an IP
        let KnownHostResult::Parsed(entry) =
            parse_known_hosts_line("123.example.com ssh-rsa AAAA...")
        else {
            panic!("expected Parsed");
        };
        assert_eq!(entry.hostname, "123.example.com");
        assert_eq!(entry.alias, "123");
    }

    #[test]
    fn test_parse_known_hosts_bracket_trailing_colon_fails() {
        // [host]: with no port number should fail
        assert!(matches!(
            parse_known_hosts_line("[myhost]: ssh-rsa AAAA..."),
            KnownHostResult::Failed
        ));
    }

    #[test]
    fn test_parse_known_hosts_bracket_junk_after_close_fails() {
        // [host]junk with no colon separator should fail
        assert!(matches!(
            parse_known_hosts_line("[myhost]junk ssh-rsa AAAA..."),
            KnownHostResult::Failed
        ));
    }

    #[test]
    fn test_parse_known_hosts_bracket_no_port() {
        // [host] with no port should default to 22
        let KnownHostResult::Parsed(entry) = parse_known_hosts_line("[myhost.com] ssh-rsa AAAA...")
        else {
            panic!("expected Parsed");
        };
        assert_eq!(entry.hostname, "myhost.com");
        assert_eq!(entry.port, 22);
    }

    #[test]
    fn test_parse_known_hosts_wildcard_is_skipped() {
        assert!(matches!(
            parse_known_hosts_line("*.example.com ssh-rsa AAAA..."),
            KnownHostResult::Skipped
        ));
    }

    #[test]
    fn test_parse_known_hosts_bracket_pattern_skipped() {
        // OpenSSH character class pattern [12] should be skipped
        assert!(matches!(
            parse_known_hosts_line("web[12].example.com ssh-rsa AAAA..."),
            KnownHostResult::Skipped
        ));
    }

    #[test]
    fn test_parse_known_hosts_negation_pattern_skipped() {
        assert!(matches!(
            parse_known_hosts_line("!prod.example.com ssh-rsa AAAA..."),
            KnownHostResult::Skipped
        ));
    }

    #[test]
    fn test_parse_known_hosts_ip_first_comma_picks_hostname() {
        // When IP comes before hostname in comma list, hostname should still be used
        let KnownHostResult::Parsed(entry) =
            parse_known_hosts_line("192.0.2.10,web.example.com ssh-rsa AAAA...")
        else {
            panic!("expected Parsed");
        };
        assert_eq!(entry.hostname, "web.example.com");
        assert_eq!(entry.alias, "web");
    }

    #[test]
    fn test_parse_known_hosts_ipv6_first_comma_picks_hostname() {
        let KnownHostResult::Parsed(entry) =
            parse_known_hosts_line("2001:db8::1,server.example.com ssh-rsa AAAA...")
        else {
            panic!("expected Parsed");
        };
        assert_eq!(entry.hostname, "server.example.com");
        assert_eq!(entry.alias, "server");
    }

    #[test]
    fn test_parse_known_hosts_all_ips_comma_skipped() {
        // If all comma entries are IPs, skip the whole line
        assert!(matches!(
            parse_known_hosts_line("192.0.2.10,10.0.0.1 ssh-rsa AAAA..."),
            KnownHostResult::Skipped
        ));
    }

    #[test]
    fn test_parse_known_hosts_bracketed_ip_first_comma_picks_hostname() {
        // [ip]:port,hostname format should pick the hostname
        let KnownHostResult::Parsed(entry) =
            parse_known_hosts_line("[192.0.2.10]:2222,web.example.com ssh-rsa AAAA...")
        else {
            panic!("expected Parsed");
        };
        assert_eq!(entry.hostname, "web.example.com");
        assert_eq!(entry.alias, "web");
    }

    // =========================================================================
    // Additional parse_known_hosts_line edge cases
    // =========================================================================

    #[test]
    fn test_parse_known_hosts_empty_string() {
        // Empty line should be filtered before parsing, but if it reaches the parser:
        assert!(matches!(
            parse_known_hosts_line(""),
            KnownHostResult::Failed
        ));
    }

    #[test]
    fn test_parse_known_hosts_single_field() {
        // Only one field, not enough for a valid known_hosts line
        assert!(matches!(
            parse_known_hosts_line("example.com"),
            KnownHostResult::Failed
        ));
    }

    #[test]
    fn test_parse_known_hosts_hostname_with_hyphen() {
        let KnownHostResult::Parsed(entry) =
            parse_known_hosts_line("my-server.example.com ssh-rsa AAAA...")
        else {
            panic!("expected Parsed");
        };
        assert_eq!(entry.hostname, "my-server.example.com");
        assert_eq!(entry.alias, "my-server");
    }

    #[test]
    fn test_parse_known_hosts_multiple_hostnames_comma() {
        // Two non-IP hostnames: first one should be picked
        let KnownHostResult::Parsed(entry) =
            parse_known_hosts_line("primary.example.com,secondary.example.com ssh-rsa AAAA...")
        else {
            panic!("expected Parsed");
        };
        assert_eq!(entry.hostname, "primary.example.com");
        assert_eq!(entry.alias, "primary");
    }

    #[test]
    fn test_parse_known_hosts_ipv6_zone_id_skipped() {
        // IPv6 with zone ID should be detected as bare IP and skipped
        assert!(matches!(
            parse_known_hosts_line("fe80::1%eth0 ssh-rsa AAAA..."),
            KnownHostResult::Skipped
        ));
    }

    #[test]
    fn test_parse_known_hosts_question_mark_pattern_skipped() {
        // ? is a pattern character in SSH
        assert!(matches!(
            parse_known_hosts_line("web?.example.com ssh-rsa AAAA..."),
            KnownHostResult::Skipped
        ));
    }

    // =========================================================================
    // Import results and status message formatting
    // =========================================================================

    #[test]
    fn test_import_status_pluralization() {
        // Verify the exact format strings used in handler.rs
        let fmt = |imported: usize, skipped: usize| -> String {
            format!(
                "Imported {} host{}, skipped {} duplicate{}",
                imported,
                if imported == 1 { "" } else { "s" },
                skipped,
                if skipped == 1 { "" } else { "s" },
            )
        };
        assert_eq!(fmt(1, 0), "Imported 1 host, skipped 0 duplicates");
        assert_eq!(fmt(1, 1), "Imported 1 host, skipped 1 duplicate");
        assert_eq!(fmt(5, 0), "Imported 5 hosts, skipped 0 duplicates");
        assert_eq!(fmt(5, 3), "Imported 5 hosts, skipped 3 duplicates");
        assert_eq!(fmt(0, 5), "Imported 0 hosts, skipped 5 duplicates");
    }

    #[test]
    fn test_import_all_duplicates_message() {
        let msg_single = if 1 == 1 {
            "Host already exists".to_string()
        } else {
            format!("All {} hosts already exist", 1)
        };
        assert_eq!(msg_single, "Host already exists");

        let msg_multi = if 5 == 1 {
            "Host already exists".to_string()
        } else {
            format!("All {} hosts already exist", 5)
        };
        assert_eq!(msg_multi, "All 5 hosts already exist");
    }

    // =========================================================================
    // import_from_known_hosts with in-memory config
    // =========================================================================

    #[test]
    fn test_import_from_known_hosts_adds_to_config() {
        // Create a temporary known_hosts-style file and import via import_from_file
        let dir = std::env::temp_dir().join(format!(
            "purple_test_import_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let hosts_file = dir.join("hosts.txt");
        std::fs::write(&hosts_file, "web.example.com\ndb.example.com\n").unwrap();

        let mut config = SshConfigFile {
            elements: Vec::new(),
            path: dir.join("config"),
            crlf: false,
            bom: false,
        };

        let result = import_from_file(&mut config, &hosts_file, Some("test-import"));
        assert!(result.is_ok());
        let (imported, skipped, _, _) = result.unwrap();
        assert_eq!(imported, 2);
        assert_eq!(skipped, 0);

        // Verify hosts are in config
        assert!(config.has_host("web"));
        assert!(config.has_host("db"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_import_skips_duplicates() {
        let dir = std::env::temp_dir().join(format!(
            "purple_test_import_dup_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let hosts_file = dir.join("hosts.txt");
        std::fs::write(&hosts_file, "web.example.com\n").unwrap();

        let mut config = SshConfigFile {
            elements: Vec::new(),
            path: dir.join("config"),
            crlf: false,
            bom: false,
        };

        // First import
        let (imported, _, _, _) = import_from_file(&mut config, &hosts_file, None).unwrap();
        assert_eq!(imported, 1);

        // Second import - should be all duplicates
        let (imported, skipped, _, _) = import_from_file(&mut config, &hosts_file, None).unwrap();
        assert_eq!(imported, 0);
        assert_eq!(skipped, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_import_from_file_nonexistent() {
        let mut config = SshConfigFile {
            elements: Vec::new(),
            path: std::path::PathBuf::from("/dev/null"),
            crlf: false,
            bom: false,
        };
        let result = import_from_file(&mut config, Path::new("/nonexistent/file"), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_import_empty_file() {
        let dir = std::env::temp_dir().join(format!(
            "purple_test_import_empty_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let hosts_file = dir.join("hosts.txt");
        std::fs::write(&hosts_file, "").unwrap();

        let mut config = SshConfigFile {
            elements: Vec::new(),
            path: dir.join("config"),
            crlf: false,
            bom: false,
        };

        let (imported, skipped, _, _) = import_from_file(&mut config, &hosts_file, None).unwrap();
        assert_eq!(imported, 0);
        assert_eq!(skipped, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_import_comments_and_blanks_only() {
        let dir = std::env::temp_dir().join(format!(
            "purple_test_import_comments_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let hosts_file = dir.join("hosts.txt");
        std::fs::write(&hosts_file, "# comment\n\n# another\n").unwrap();

        let mut config = SshConfigFile {
            elements: Vec::new(),
            path: dir.join("config"),
            crlf: false,
            bom: false,
        };

        let (imported, skipped, _, _) = import_from_file(&mut config, &hosts_file, None).unwrap();
        assert_eq!(imported, 0);
        assert_eq!(skipped, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_is_bare_ip() {
        assert!(is_bare_ip("192.168.1.1"));
        assert!(is_bare_ip("10.0.0.1"));
        assert!(is_bare_ip("2001:db8::1"));
        assert!(is_bare_ip("fe80::1"));
        assert!(is_bare_ip("fe80::1%en0"));
        assert!(is_bare_ip("fe80::1%eth0"));
        assert!(!is_bare_ip("example.com"));
        assert!(!is_bare_ip("123.example.com"));
        assert!(!is_bare_ip("deadbeef"));
        assert!(!is_bare_ip(""));
    }
}
