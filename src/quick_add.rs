/// Parsed target from `user@hostname:port` format.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTarget {
    pub user: String,
    pub hostname: String,
    pub port: u16,
}

/// Parse a target string in the format `[user@]hostname[:port]`.
pub fn parse_target(target: &str) -> Result<ParsedTarget, String> {
    if target.is_empty() {
        return Err("Target can't be empty.".to_string());
    }

    let (user, rest) = if let Some(at_pos) = target.find('@') {
        let user = &target[..at_pos];
        if user.is_empty() {
            return Err("User part before @ can't be empty.".to_string());
        }
        (user.to_string(), &target[at_pos + 1..])
    } else {
        (String::new(), target)
    };

    let (hostname, port) = if rest.starts_with('[') {
        // Bracketed IPv6: [2001:db8::1]:port
        if let Some(bracket_end) = rest.find(']') {
            let host = &rest[1..bracket_end];
            let after = &rest[bracket_end + 1..];
            if let Some(port_str) = after.strip_prefix(':') {
                if let Ok(port) = port_str.parse::<u16>() {
                    if port == 0 {
                        return Err("Port 0? Bold choice, but no. Try 1-65535.".to_string());
                    }
                    (host.to_string(), port)
                } else {
                    return Err("Invalid port after bracketed host.".to_string());
                }
            } else if after.is_empty() {
                (host.to_string(), 22)
            } else {
                return Err("Unexpected text after closing bracket.".to_string());
            }
        } else {
            return Err("Missing closing bracket for IPv6 address.".to_string());
        }
    } else if let Some(colon_pos) = rest.rfind(':') {
        let port_str = &rest[colon_pos + 1..];
        let host_part = &rest[..colon_pos];
        // Only treat as host:port if the host part has no colons (not bare IPv6)
        if !host_part.contains(':') {
            if let Ok(port) = port_str.parse::<u16>() {
                if port == 0 {
                    return Err("Port 0? Bold choice, but no. Try 1-65535.".to_string());
                }
                (host_part.to_string(), port)
            } else {
                // Not a number after colon, treat as hostname
                (rest.to_string(), 22)
            }
        } else {
            // Multiple colons = bare IPv6 address, no port extraction
            (rest.to_string(), 22)
        }
    } else {
        (rest.to_string(), 22)
    };

    if hostname.is_empty() {
        return Err("Hostname can't be empty.".to_string());
    }

    Ok(ParsedTarget {
        user,
        hostname,
        port,
    })
}

/// Check if a string looks like a smart-paste target (contains @ or host:port).
pub fn looks_like_target(s: &str) -> bool {
    if s.contains('@') {
        return true;
    }
    // Bracketed IPv6 with port: [::1]:22
    if s.starts_with('[') {
        return true;
    }
    if let Some(colon_pos) = s.rfind(':') {
        let before = &s[..colon_pos];
        let after = &s[colon_pos + 1..];
        // Only match host:port if no colons in host part (avoids bare IPv6)
        return !before.contains(':')
            && !after.is_empty()
            && after.chars().all(|c| c.is_ascii_digit());
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_target() {
        let result = parse_target("admin@example.com:2222").unwrap();
        assert_eq!(result.user, "admin");
        assert_eq!(result.hostname, "example.com");
        assert_eq!(result.port, 2222);
    }

    #[test]
    fn test_user_and_host() {
        let result = parse_target("root@10.0.0.1").unwrap();
        assert_eq!(result.user, "root");
        assert_eq!(result.hostname, "10.0.0.1");
        assert_eq!(result.port, 22);
    }

    #[test]
    fn test_host_and_port() {
        let result = parse_target("example.com:8022").unwrap();
        assert_eq!(result.user, "");
        assert_eq!(result.hostname, "example.com");
        assert_eq!(result.port, 8022);
    }

    #[test]
    fn test_host_only() {
        let result = parse_target("example.com").unwrap();
        assert_eq!(result.user, "");
        assert_eq!(result.hostname, "example.com");
        assert_eq!(result.port, 22);
    }

    #[test]
    fn test_empty_target() {
        assert!(parse_target("").is_err());
    }

    #[test]
    fn test_empty_user() {
        assert!(parse_target("@example.com").is_err());
    }

    #[test]
    fn test_empty_hostname() {
        assert!(parse_target("user@").is_err());
    }

    #[test]
    fn test_port_zero() {
        assert!(parse_target("example.com:0").is_err());
    }

    #[test]
    fn test_looks_like_target_with_at() {
        assert!(looks_like_target("user@host"));
    }

    #[test]
    fn test_looks_like_target_with_port() {
        assert!(looks_like_target("host:22"));
    }

    #[test]
    fn test_looks_like_target_plain_host() {
        assert!(!looks_like_target("myserver"));
    }

    #[test]
    fn test_bare_ipv6() {
        let result = parse_target("2001:db8::1").unwrap();
        assert_eq!(result.hostname, "2001:db8::1");
        assert_eq!(result.port, 22);
    }

    #[test]
    fn test_bracketed_ipv6_with_port() {
        let result = parse_target("[2001:db8::1]:2222").unwrap();
        assert_eq!(result.hostname, "2001:db8::1");
        assert_eq!(result.port, 2222);
    }

    #[test]
    fn test_bracketed_ipv6_no_port() {
        let result = parse_target("[::1]").unwrap();
        assert_eq!(result.hostname, "::1");
        assert_eq!(result.port, 22);
    }

    #[test]
    fn test_user_at_ipv6() {
        let result = parse_target("root@2001:db8::1").unwrap();
        assert_eq!(result.user, "root");
        assert_eq!(result.hostname, "2001:db8::1");
        assert_eq!(result.port, 22);
    }

    #[test]
    fn test_looks_like_target_bare_ipv6() {
        // Bare IPv6 without @ should NOT look like a target (would be ambiguous)
        assert!(!looks_like_target("2001:db8::1"));
    }

    #[test]
    fn test_looks_like_target_bracketed_ipv6() {
        assert!(looks_like_target("[::1]:22"));
    }
}
