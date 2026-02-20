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

    let (hostname, port) = if let Some(colon_pos) = rest.rfind(':') {
        let port_str = &rest[colon_pos + 1..];
        // Only treat as port if the part after : is a valid number
        if let Ok(port) = port_str.parse::<u16>() {
            if port == 0 {
                return Err("Port 0? Bold choice, but no. Try 1-65535.".to_string());
            }
            (rest[..colon_pos].to_string(), port)
        } else {
            // Not a number, treat the whole thing as hostname (e.g. IPv6)
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

/// Check if a string looks like a smart-paste target (contains @ or :digit).
pub fn looks_like_target(s: &str) -> bool {
    if s.contains('@') {
        return true;
    }
    if let Some(colon_pos) = s.rfind(':') {
        let after = &s[colon_pos + 1..];
        return after.chars().all(|c| c.is_ascii_digit()) && !after.is_empty();
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
}
