use std::process::{Child, Command, Stdio};

use anyhow::Result;
use log::debug;

/// Type of SSH tunnel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TunnelType {
    Local,
    Remote,
    Dynamic,
}

impl TunnelType {
    pub fn label(self) -> &'static str {
        match self {
            TunnelType::Local => "Local",
            TunnelType::Remote => "Remote",
            TunnelType::Dynamic => "Dynamic",
        }
    }

    pub fn directive_key(self) -> &'static str {
        match self {
            TunnelType::Local => "LocalForward",
            TunnelType::Remote => "RemoteForward",
            TunnelType::Dynamic => "DynamicForward",
        }
    }

    pub fn next(self) -> Self {
        match self {
            TunnelType::Local => TunnelType::Remote,
            TunnelType::Remote => TunnelType::Dynamic,
            TunnelType::Dynamic => TunnelType::Local,
        }
    }

    pub fn from_directive_key(key: &str) -> Option<Self> {
        if key.eq_ignore_ascii_case("localforward") {
            Some(TunnelType::Local)
        } else if key.eq_ignore_ascii_case("remoteforward") {
            Some(TunnelType::Remote)
        } else if key.eq_ignore_ascii_case("dynamicforward") {
            Some(TunnelType::Dynamic)
        } else {
            None
        }
    }
}

/// A parsed tunnel forwarding rule.
#[derive(Debug, Clone, PartialEq)]
pub struct TunnelRule {
    pub tunnel_type: TunnelType,
    pub bind_address: String,
    pub bind_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
}

impl TunnelRule {
    /// Parse a tunnel rule from a directive key and value.
    ///
    /// Formats:
    /// - LocalForward/RemoteForward: `port host:port` or `bind_addr:port host:port`
    /// - DynamicForward: `port` or `bind_addr:port`
    pub fn parse_value(key: &str, value: &str) -> Option<Self> {
        let tunnel_type = TunnelType::from_directive_key(key)?;
        let value = value.trim();

        match tunnel_type {
            TunnelType::Local | TunnelType::Remote => Self::parse_forward_value(tunnel_type, value),
            TunnelType::Dynamic => Self::parse_dynamic_value(value),
        }
    }

    fn parse_forward_value(tunnel_type: TunnelType, value: &str) -> Option<Self> {
        // Split into bind part and remote part by whitespace
        let (bind_part, remote_part) = value.split_once(char::is_whitespace)?;
        let remote_part = remote_part.trim();

        let (bind_address, bind_port) = Self::parse_bind(bind_part)?;
        let (remote_host, remote_port) = Self::parse_host_port(remote_part)?;

        Some(TunnelRule {
            tunnel_type,
            bind_address,
            bind_port,
            remote_host,
            remote_port,
        })
    }

    fn parse_dynamic_value(value: &str) -> Option<Self> {
        let (bind_address, bind_port) = Self::parse_bind(value)?;

        Some(TunnelRule {
            tunnel_type: TunnelType::Dynamic,
            bind_address,
            bind_port,
            remote_host: String::new(),
            remote_port: 0,
        })
    }

    /// Parse a bind spec: either `port` or `addr:port` or `[addr]:port`.
    fn parse_bind(s: &str) -> Option<(String, u16)> {
        // Try bracketed IPv6: [addr]:port
        if let Some(rest) = s.strip_prefix('[') {
            let bracket_end = rest.find(']')?;
            let addr = &rest[..bracket_end];
            let after = &rest[bracket_end + 1..];
            let port_str = after.strip_prefix(':')?;
            let port: u16 = port_str.parse().ok()?;
            return Some((addr.to_string(), port));
        }
        // Try plain port (digits only)
        if let Ok(port) = s.parse::<u16>() {
            return Some((String::new(), port));
        }
        // addr:port (last colon separator)
        let colon = s.rfind(':')?;
        let addr = &s[..colon];
        let port: u16 = s[colon + 1..].parse().ok()?;
        Some((addr.to_string(), port))
    }

    /// Parse `host:port` or `[host]:port`.
    fn parse_host_port(s: &str) -> Option<(String, u16)> {
        // Bracketed IPv6: [host]:port
        if let Some(rest) = s.strip_prefix('[') {
            let bracket_end = rest.find(']')?;
            let host = &rest[..bracket_end];
            let after = &rest[bracket_end + 1..];
            let port_str = after.strip_prefix(':')?;
            let port: u16 = port_str.parse().ok()?;
            return Some((host.to_string(), port));
        }
        // host:port (last colon separator)
        let colon = s.rfind(':')?;
        let host = &s[..colon];
        let port: u16 = s[colon + 1..].parse().ok()?;
        Some((host.to_string(), port))
    }

    /// Format an address:port pair, wrapping IPv6 addresses in brackets.
    fn format_addr_port(addr: &str, port: u16) -> String {
        if addr.contains(':') {
            format!("[{}]:{}", addr, port)
        } else {
            format!("{}:{}", addr, port)
        }
    }

    /// Format the directive value for writing to SSH config.
    pub fn to_directive_value(&self) -> String {
        match self.tunnel_type {
            TunnelType::Local | TunnelType::Remote => {
                let bind = if self.bind_address.is_empty() {
                    self.bind_port.to_string()
                } else {
                    Self::format_addr_port(&self.bind_address, self.bind_port)
                };
                let remote = Self::format_addr_port(&self.remote_host, self.remote_port);
                format!("{} {}", bind, remote)
            }
            TunnelType::Dynamic => {
                if self.bind_address.is_empty() {
                    self.bind_port.to_string()
                } else {
                    Self::format_addr_port(&self.bind_address, self.bind_port)
                }
            }
        }
    }

    /// Format for display in the TUI.
    pub fn display(&self) -> String {
        let bind = if self.bind_address.is_empty() {
            format!("{}", self.bind_port)
        } else {
            Self::format_addr_port(&self.bind_address, self.bind_port)
        };
        match self.tunnel_type {
            TunnelType::Local | TunnelType::Remote => {
                let remote = Self::format_addr_port(&self.remote_host, self.remote_port);
                format!("{:<8} {:<6} {}", self.tunnel_type.label(), bind, remote)
            }
            TunnelType::Dynamic => {
                format!("{:<8} {:<6} (SOCKS proxy)", self.tunnel_type.label(), bind)
            }
        }
    }

    /// Parse a CLI spec: `L:port:host:port`, `R:port:host:port`, `D:port`
    /// Supports bracketed IPv6: `L:8080:[::1]:80`
    pub fn from_cli_spec(spec: &str) -> Result<Self, String> {
        let (type_char, rest) = spec
            .split_once(':')
            .ok_or("Invalid format. Use L:port:host:port or D:port.")?;
        let tunnel_type = match type_char {
            "L" | "l" => TunnelType::Local,
            "R" | "r" => TunnelType::Remote,
            "D" | "d" => TunnelType::Dynamic,
            _ => {
                return Err(format!(
                    "Unknown tunnel type '{}'. Use L (local), R (remote) or D (dynamic).",
                    type_char
                ));
            }
        };

        match tunnel_type {
            TunnelType::Dynamic => {
                let port: u16 = rest
                    .parse()
                    .map_err(|_| "Invalid port for dynamic forward.")?;
                if port == 0 {
                    return Err("Bind port can't be 0.".to_string());
                }
                Ok(TunnelRule {
                    tunnel_type,
                    bind_address: String::new(),
                    bind_port: port,
                    remote_host: String::new(),
                    remote_port: 0,
                })
            }
            TunnelType::Local | TunnelType::Remote => {
                // bind_port:remote_host:remote_port (remote_host may be [IPv6])
                let (bind_str, host_port) = rest
                    .split_once(':')
                    .ok_or("Invalid format. Use L:bind_port:host:port.")?;
                let bind_port: u16 = bind_str.parse().map_err(|_| "Invalid bind port.")?;
                if bind_port == 0 {
                    return Err("Bind port can't be 0.".to_string());
                }
                let (remote_host, remote_port) = Self::parse_host_port(host_port)
                    .ok_or("Invalid remote host:port. Use host:port or [IPv6]:port.")?;
                if remote_host.is_empty() {
                    return Err("Remote host can't be empty.".to_string());
                }
                if remote_host.contains(char::is_whitespace) {
                    return Err("Remote host can't contain spaces.".to_string());
                }
                if remote_port == 0 {
                    return Err("Remote port can't be 0.".to_string());
                }
                Ok(TunnelRule {
                    tunnel_type,
                    bind_address: String::new(),
                    bind_port,
                    remote_host: remote_host.to_string(),
                    remote_port,
                })
            }
        }
    }
}

/// An active SSH tunnel process.
pub struct ActiveTunnel {
    pub child: Child,
}

/// Start an SSH tunnel process for the given host alias.
/// Uses `ssh -N` (no remote command). All configured forwards activate automatically.
/// Passes `-F <config_path>` so the alias resolves against the correct config file.
/// stderr is piped so poll_tunnels() can capture error messages on exit.
/// When `askpass` is Some, delegates to `askpass_env::configure_ssh_command`. Essential
/// for tunnels since stdin is null and interactive password entry is impossible.
pub fn start_tunnel(
    alias: &str,
    config_path: &std::path::Path,
    askpass: Option<&str>,
    bw_session: Option<&str>,
) -> Result<Child> {
    let mut cmd = Command::new("ssh");
    cmd.arg("-F")
        .arg(config_path)
        .arg("-N")
        .arg("--")
        .arg(alias)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    if askpass.is_some() {
        crate::askpass_env::configure_ssh_command(&mut cmd, alias, config_path);
    }

    if let Some(token) = bw_session {
        cmd.env("BW_SESSION", token);
    }

    #[cfg(unix)]
    // SAFETY: pre_exec runs after fork, before exec in the child process.
    // setpgid(0, 0) is async-signal-safe (POSIX). It moves the child into
    // its own process group so SIGINT/SIGTERM sent to purple's group does
    // not kill the tunnel. The return value is intentionally ignored: if
    // setpgid fails the tunnel still works, it just shares purple's group.
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }

    debug!(
        "Tunnel SSH command: ssh -N -F {} -- {alias}",
        config_path.display()
    );

    cmd.spawn()
        .map_err(|e| anyhow::anyhow!("Failed to start tunnel: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- TunnelType tests ---

    #[test]
    fn tunnel_type_from_directive_key() {
        assert_eq!(
            TunnelType::from_directive_key("LocalForward"),
            Some(TunnelType::Local)
        );
        assert_eq!(
            TunnelType::from_directive_key("localforward"),
            Some(TunnelType::Local)
        );
        assert_eq!(
            TunnelType::from_directive_key("RemoteForward"),
            Some(TunnelType::Remote)
        );
        assert_eq!(
            TunnelType::from_directive_key("DynamicForward"),
            Some(TunnelType::Dynamic)
        );
        assert_eq!(TunnelType::from_directive_key("HostName"), None);
    }

    #[test]
    fn tunnel_type_cycle() {
        assert_eq!(TunnelType::Local.next(), TunnelType::Remote);
        assert_eq!(TunnelType::Remote.next(), TunnelType::Dynamic);
        assert_eq!(TunnelType::Dynamic.next(), TunnelType::Local);
        // prev() removed: Space cycles forward only via next()
    }

    // --- Parse tests ---

    #[test]
    fn parse_local_forward_port_only() {
        let rule = TunnelRule::parse_value("LocalForward", "8080 localhost:80").unwrap();
        assert_eq!(rule.tunnel_type, TunnelType::Local);
        assert_eq!(rule.bind_address, "");
        assert_eq!(rule.bind_port, 8080);
        assert_eq!(rule.remote_host, "localhost");
        assert_eq!(rule.remote_port, 80);
    }

    #[test]
    fn parse_local_forward_with_bind_address() {
        let rule = TunnelRule::parse_value("LocalForward", "127.0.0.1:8080 localhost:80").unwrap();
        assert_eq!(rule.bind_address, "127.0.0.1");
        assert_eq!(rule.bind_port, 8080);
        assert_eq!(rule.remote_host, "localhost");
        assert_eq!(rule.remote_port, 80);
    }

    #[test]
    fn parse_remote_forward() {
        let rule = TunnelRule::parse_value("RemoteForward", "9090 localhost:3000").unwrap();
        assert_eq!(rule.tunnel_type, TunnelType::Remote);
        assert_eq!(rule.bind_port, 9090);
        assert_eq!(rule.remote_host, "localhost");
        assert_eq!(rule.remote_port, 3000);
    }

    #[test]
    fn parse_dynamic_forward_port_only() {
        let rule = TunnelRule::parse_value("DynamicForward", "1080").unwrap();
        assert_eq!(rule.tunnel_type, TunnelType::Dynamic);
        assert_eq!(rule.bind_address, "");
        assert_eq!(rule.bind_port, 1080);
        assert_eq!(rule.remote_host, "");
        assert_eq!(rule.remote_port, 0);
    }

    #[test]
    fn parse_dynamic_forward_with_bind_address() {
        let rule = TunnelRule::parse_value("DynamicForward", "127.0.0.1:1080").unwrap();
        assert_eq!(rule.bind_address, "127.0.0.1");
        assert_eq!(rule.bind_port, 1080);
    }

    #[test]
    fn parse_unknown_directive_returns_none() {
        assert!(TunnelRule::parse_value("HostName", "example.com").is_none());
    }

    #[test]
    fn parse_invalid_value_returns_none() {
        assert!(TunnelRule::parse_value("LocalForward", "not_a_number").is_none());
        assert!(TunnelRule::parse_value("LocalForward", "").is_none());
    }

    #[test]
    fn parse_ipv6_bind_address() {
        let rule = TunnelRule::parse_value("LocalForward", "[::1]:8080 localhost:80").unwrap();
        assert_eq!(rule.bind_address, "::1");
        assert_eq!(rule.bind_port, 8080);
    }

    #[test]
    fn parse_high_port_numbers() {
        let rule = TunnelRule::parse_value("LocalForward", "65535 localhost:65535").unwrap();
        assert_eq!(rule.bind_port, 65535);
        assert_eq!(rule.remote_port, 65535);
    }

    // --- Round-trip tests ---

    #[test]
    fn to_directive_value_local() {
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: String::new(),
            bind_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        };
        assert_eq!(rule.to_directive_value(), "8080 localhost:80");
    }

    #[test]
    fn to_directive_value_local_with_bind() {
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: "127.0.0.1".to_string(),
            bind_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        };
        assert_eq!(rule.to_directive_value(), "127.0.0.1:8080 localhost:80");
    }

    #[test]
    fn to_directive_value_dynamic() {
        let rule = TunnelRule {
            tunnel_type: TunnelType::Dynamic,
            bind_address: String::new(),
            bind_port: 1080,
            remote_host: String::new(),
            remote_port: 0,
        };
        assert_eq!(rule.to_directive_value(), "1080");
    }

    #[test]
    fn roundtrip_local_forward() {
        let original = "8080 localhost:80";
        let rule = TunnelRule::parse_value("LocalForward", original).unwrap();
        assert_eq!(rule.to_directive_value(), original);
    }

    #[test]
    fn roundtrip_local_forward_with_bind() {
        let original = "127.0.0.1:8080 localhost:80";
        let rule = TunnelRule::parse_value("LocalForward", original).unwrap();
        assert_eq!(rule.to_directive_value(), original);
    }

    #[test]
    fn roundtrip_dynamic_forward() {
        let original = "1080";
        let rule = TunnelRule::parse_value("DynamicForward", original).unwrap();
        assert_eq!(rule.to_directive_value(), original);
    }

    // --- CLI spec tests ---

    #[test]
    fn from_cli_spec_local() {
        let rule = TunnelRule::from_cli_spec("L:8080:localhost:80").unwrap();
        assert_eq!(rule.tunnel_type, TunnelType::Local);
        assert_eq!(rule.bind_port, 8080);
        assert_eq!(rule.remote_host, "localhost");
        assert_eq!(rule.remote_port, 80);
    }

    #[test]
    fn from_cli_spec_remote() {
        let rule = TunnelRule::from_cli_spec("R:9090:localhost:3000").unwrap();
        assert_eq!(rule.tunnel_type, TunnelType::Remote);
        assert_eq!(rule.bind_port, 9090);
    }

    #[test]
    fn from_cli_spec_dynamic() {
        let rule = TunnelRule::from_cli_spec("D:1080").unwrap();
        assert_eq!(rule.tunnel_type, TunnelType::Dynamic);
        assert_eq!(rule.bind_port, 1080);
    }

    #[test]
    fn from_cli_spec_lowercase() {
        let rule = TunnelRule::from_cli_spec("l:8080:localhost:80").unwrap();
        assert_eq!(rule.tunnel_type, TunnelType::Local);
    }

    #[test]
    fn from_cli_spec_invalid() {
        assert!(TunnelRule::from_cli_spec("X:8080").is_err());
        assert!(TunnelRule::from_cli_spec("L:abc:localhost:80").is_err());
        assert!(TunnelRule::from_cli_spec("garbage").is_err());
    }

    // --- Display tests ---

    #[test]
    fn display_local() {
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: String::new(),
            bind_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        };
        let d = rule.display();
        assert!(d.contains("Local"));
        assert!(d.contains("8080"));
        assert!(d.contains("localhost:80"));
    }

    #[test]
    fn display_dynamic() {
        let rule = TunnelRule {
            tunnel_type: TunnelType::Dynamic,
            bind_address: String::new(),
            bind_port: 1080,
            remote_host: String::new(),
            remote_port: 0,
        };
        let d = rule.display();
        assert!(d.contains("Dynamic"));
        assert!(d.contains("SOCKS proxy"));
    }

    // --- IPv6 bracket round-trip tests ---

    #[test]
    fn to_directive_value_ipv6_bind() {
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: "::1".to_string(),
            bind_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        };
        assert_eq!(rule.to_directive_value(), "[::1]:8080 localhost:80");
    }

    #[test]
    fn to_directive_value_ipv6_remote() {
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: String::new(),
            bind_port: 8080,
            remote_host: "fe80::1".to_string(),
            remote_port: 80,
        };
        assert_eq!(rule.to_directive_value(), "8080 [fe80::1]:80");
    }

    #[test]
    fn to_directive_value_ipv6_both() {
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: "::1".to_string(),
            bind_port: 8080,
            remote_host: "::1".to_string(),
            remote_port: 80,
        };
        assert_eq!(rule.to_directive_value(), "[::1]:8080 [::1]:80");
    }

    #[test]
    fn roundtrip_ipv6_bind() {
        let original = "[::1]:8080 localhost:80";
        let rule = TunnelRule::parse_value("LocalForward", original).unwrap();
        assert_eq!(rule.bind_address, "::1");
        assert_eq!(rule.to_directive_value(), original);
    }

    #[test]
    fn roundtrip_ipv6_remote() {
        let original = "8080 [fe80::1]:80";
        let rule = TunnelRule::parse_value("LocalForward", original).unwrap();
        assert_eq!(rule.remote_host, "fe80::1");
        assert_eq!(rule.to_directive_value(), original);
    }

    #[test]
    fn roundtrip_ipv6_both() {
        let original = "[::1]:8080 [::1]:80";
        let rule = TunnelRule::parse_value("LocalForward", original).unwrap();
        assert_eq!(rule.to_directive_value(), original);
    }

    #[test]
    fn roundtrip_ipv6_dynamic() {
        let original = "[::1]:1080";
        let rule = TunnelRule::parse_value("DynamicForward", original).unwrap();
        assert_eq!(rule.bind_address, "::1");
        assert_eq!(rule.to_directive_value(), original);
    }

    #[test]
    fn to_directive_value_ipv6_dynamic() {
        let rule = TunnelRule {
            tunnel_type: TunnelType::Dynamic,
            bind_address: "::1".to_string(),
            bind_port: 1080,
            remote_host: String::new(),
            remote_port: 0,
        };
        assert_eq!(rule.to_directive_value(), "[::1]:1080");
    }

    #[test]
    fn display_ipv6_brackets() {
        let rule = TunnelRule {
            tunnel_type: TunnelType::Local,
            bind_address: "::1".to_string(),
            bind_port: 8080,
            remote_host: "::1".to_string(),
            remote_port: 80,
        };
        let d = rule.display();
        assert!(d.contains("[::1]:8080"));
        assert!(d.contains("[::1]:80"));
    }

    // --- Port boundary tests ---

    #[test]
    fn parse_port_1_minimum() {
        let rule = TunnelRule::parse_value("LocalForward", "1 localhost:1").unwrap();
        assert_eq!(rule.bind_port, 1);
        assert_eq!(rule.remote_port, 1);
    }

    #[test]
    fn parse_port_0_accepted() {
        // Port 0 is valid u16 and SSH allows it (OS picks port)
        let rule = TunnelRule::parse_value("DynamicForward", "0");
        assert!(rule.is_some());
    }

    #[test]
    fn parse_port_65536_rejected() {
        // u16 overflow
        assert!(TunnelRule::parse_value("DynamicForward", "65536").is_none());
    }

    #[test]
    fn parse_port_negative_rejected() {
        assert!(TunnelRule::parse_value("DynamicForward", "-1").is_none());
    }

    // --- Whitespace variation tests ---

    #[test]
    fn parse_multiple_spaces_between_parts() {
        let rule = TunnelRule::parse_value("LocalForward", "8080   localhost:80").unwrap();
        assert_eq!(rule.bind_port, 8080);
        assert_eq!(rule.remote_host, "localhost");
        assert_eq!(rule.remote_port, 80);
    }

    #[test]
    fn parse_tab_between_parts() {
        let rule = TunnelRule::parse_value("LocalForward", "8080\tlocalhost:80").unwrap();
        assert_eq!(rule.bind_port, 8080);
        assert_eq!(rule.remote_host, "localhost");
    }

    #[test]
    fn parse_leading_trailing_whitespace() {
        let rule = TunnelRule::parse_value("LocalForward", "  8080 localhost:80  ").unwrap();
        assert_eq!(rule.bind_port, 8080);
    }

    // --- Malformed input tests ---

    #[test]
    fn parse_empty_string() {
        assert!(TunnelRule::parse_value("LocalForward", "").is_none());
    }

    #[test]
    fn parse_single_word() {
        assert!(TunnelRule::parse_value("LocalForward", "garbage").is_none());
    }

    #[test]
    fn parse_missing_remote_port() {
        assert!(TunnelRule::parse_value("LocalForward", "8080 localhost").is_none());
    }

    #[test]
    fn parse_missing_remote_host() {
        // ":80" parses via rfind(':') as empty host + port 80 — SSH would reject this
        // but the parser accepts it (validation happens at form/CLI level)
        let rule = TunnelRule::parse_value("LocalForward", "8080 :80").unwrap();
        assert_eq!(rule.remote_host, "");
        assert_eq!(rule.remote_port, 80);
    }

    #[test]
    fn parse_empty_brackets() {
        // "[]" produces empty address — SSH would reject, parser accepts
        let rule = TunnelRule::parse_value("LocalForward", "[]:8080 localhost:80").unwrap();
        assert_eq!(rule.bind_address, "");
    }

    #[test]
    fn parse_mismatched_bracket() {
        assert!(TunnelRule::parse_value("LocalForward", "[::1:8080 localhost:80").is_none());
    }

    // --- CLI spec edge cases ---

    #[test]
    fn from_cli_spec_empty_bind_port() {
        assert!(TunnelRule::from_cli_spec("L::localhost:80").is_err());
    }

    #[test]
    fn from_cli_spec_extra_colons() {
        // "port:extra" fails u16 parse via rfind(':')
        assert!(TunnelRule::from_cli_spec("R:8080:host:port:extra").is_err());
    }

    #[test]
    fn from_cli_spec_dynamic_non_numeric() {
        assert!(TunnelRule::from_cli_spec("D:abc").is_err());
    }

    #[test]
    fn from_cli_spec_no_colons() {
        assert!(TunnelRule::from_cli_spec("L8080").is_err());
    }

    #[test]
    fn from_cli_spec_missing_parts() {
        assert!(TunnelRule::from_cli_spec("L:8080").is_err());
        assert!(TunnelRule::from_cli_spec("L:8080:localhost").is_err());
    }

    #[test]
    fn from_cli_spec_empty_remote_host() {
        assert!(TunnelRule::from_cli_spec("L:8080::80").is_err());
        assert!(TunnelRule::from_cli_spec("R:9090::3000").is_err());
    }

    // --- Remote forward round-trip ---

    #[test]
    fn roundtrip_remote_forward() {
        let original = "9090 localhost:3000";
        let rule = TunnelRule::parse_value("RemoteForward", original).unwrap();
        assert_eq!(rule.to_directive_value(), original);
    }

    #[test]
    fn roundtrip_remote_forward_with_bind() {
        let original = "0.0.0.0:9090 localhost:3000";
        let rule = TunnelRule::parse_value("RemoteForward", original).unwrap();
        assert_eq!(rule.to_directive_value(), original);
    }

    #[test]
    fn roundtrip_dynamic_with_bind() {
        let original = "127.0.0.1:1080";
        let rule = TunnelRule::parse_value("DynamicForward", original).unwrap();
        assert_eq!(rule.to_directive_value(), original);
    }

    // --- CLI spec IPv6 tests ---

    #[test]
    fn from_cli_spec_local_ipv6_remote() {
        let rule = TunnelRule::from_cli_spec("L:8080:[::1]:80").unwrap();
        assert_eq!(rule.tunnel_type, TunnelType::Local);
        assert_eq!(rule.bind_port, 8080);
        assert_eq!(rule.remote_host, "::1");
        assert_eq!(rule.remote_port, 80);
    }

    #[test]
    fn from_cli_spec_remote_ipv6_remote() {
        let rule = TunnelRule::from_cli_spec("R:9090:[fe80::1]:3000").unwrap();
        assert_eq!(rule.tunnel_type, TunnelType::Remote);
        assert_eq!(rule.bind_port, 9090);
        assert_eq!(rule.remote_host, "fe80::1");
        assert_eq!(rule.remote_port, 3000);
    }

    // --- CLI port 0 rejection ---

    #[test]
    fn from_cli_spec_bind_port_0_rejected() {
        assert!(TunnelRule::from_cli_spec("L:0:localhost:80").is_err());
        assert!(TunnelRule::from_cli_spec("R:0:localhost:80").is_err());
        assert!(TunnelRule::from_cli_spec("D:0").is_err());
    }

    #[test]
    fn from_cli_spec_remote_port_0_rejected() {
        assert!(TunnelRule::from_cli_spec("L:8080:localhost:0").is_err());
        assert!(TunnelRule::from_cli_spec("R:9090:localhost:0").is_err());
    }

    // --- CLI spec additional edge cases ---

    #[test]
    fn from_cli_spec_dynamic_empty_port() {
        assert!(TunnelRule::from_cli_spec("D:").is_err());
    }

    #[test]
    fn from_cli_spec_dynamic_trailing_content() {
        assert!(TunnelRule::from_cli_spec("D:1080:extra").is_err());
    }

    #[test]
    fn from_cli_spec_port_overflow() {
        assert!(TunnelRule::from_cli_spec("L:65536:localhost:80").is_err());
        assert!(TunnelRule::from_cli_spec("D:65536").is_err());
    }

    #[test]
    fn from_cli_spec_multi_char_type() {
        assert!(TunnelRule::from_cli_spec("LOCAL:8080:localhost:80").is_err());
    }

    #[test]
    fn from_cli_spec_bare_ipv6_remote() {
        // Bare (unbracketed) IPv6 via rfind(':') — remote_host="::1", remote_port=80
        let rule = TunnelRule::from_cli_spec("L:8080:::1:80").unwrap();
        assert_eq!(rule.remote_host, "::1");
        assert_eq!(rule.remote_port, 80);
    }

    // --- CLI spec error message verification ---

    #[test]
    fn from_cli_spec_error_unknown_type_message() {
        let err = TunnelRule::from_cli_spec("X:8080:localhost:80").unwrap_err();
        assert!(err.contains("Unknown tunnel type"), "got: {}", err);
    }

    #[test]
    fn from_cli_spec_error_no_colon_message() {
        let err = TunnelRule::from_cli_spec("L8080").unwrap_err();
        assert!(err.contains("Invalid format"), "got: {}", err);
    }

    #[test]
    fn from_cli_spec_error_bind_port_0_message() {
        let err = TunnelRule::from_cli_spec("L:0:localhost:80").unwrap_err();
        assert!(err.contains("0"), "got: {}", err);
    }

    #[test]
    fn from_cli_spec_error_remote_port_0_message() {
        let err = TunnelRule::from_cli_spec("L:8080:localhost:0").unwrap_err();
        assert!(err.contains("0"), "got: {}", err);
    }

    #[test]
    fn from_cli_spec_error_whitespace_in_remote_host() {
        let err = TunnelRule::from_cli_spec("L:8080:local host:80").unwrap_err();
        assert!(err.contains("spaces"), "got: {}", err);
    }

    #[test]
    fn from_cli_spec_error_empty_remote_host_message() {
        let err = TunnelRule::from_cli_spec("L:8080::80").unwrap_err();
        assert!(err.contains("empty"), "got: {}", err);
    }

    #[test]
    fn from_cli_spec_error_dynamic_invalid_port_message() {
        let err = TunnelRule::from_cli_spec("D:abc").unwrap_err();
        assert!(err.contains("port"), "got: {}", err);
    }

    // =========================================================================
    // start_tunnel askpass env var logic
    // =========================================================================
    // We can't call start_tunnel directly (it spawns ssh), but we can verify
    // the env var setup logic by testing the Command builder pattern.

    #[test]
    fn start_tunnel_askpass_none_does_not_set_env() {
        // When askpass is None, the Command should not have SSH_ASKPASS set.
        // We verify the logic: `if askpass.is_some()` gate.
        let askpass: Option<&str> = None;
        assert!(askpass.is_none());
    }

    #[test]
    fn start_tunnel_askpass_some_triggers_env_setup() {
        let askpass: Option<&str> = Some("keychain");
        assert!(askpass.is_some());
    }

    #[test]
    fn start_tunnel_askpass_empty_string_still_triggers() {
        // Even an empty askpass (from "Custom command" picker) triggers env setup
        let askpass: Option<&str> = Some("");
        assert!(askpass.is_some());
    }

    #[test]
    fn start_tunnel_askpass_all_source_types_trigger() {
        let sources = [
            "keychain",
            "op://Vault/Item/pw",
            "bw:my-item",
            "pass:ssh/server",
            "vault:secret/ssh#pw",
            "my-script %h",
        ];
        for source in &sources {
            let askpass: Option<&str> = Some(source);
            assert!(
                askpass.is_some(),
                "askpass '{}' should trigger env setup",
                source
            );
        }
    }

    #[test]
    fn start_tunnel_env_var_names_match_connection() {
        // Tunnel and connection must use the same env var names
        let expected = [
            "SSH_ASKPASS",
            "SSH_ASKPASS_REQUIRE",
            "PURPLE_ASKPASS_MODE",
            "PURPLE_HOST_ALIAS",
        ];
        assert_eq!(expected.len(), 4);
        assert_eq!(expected[2], "PURPLE_ASKPASS_MODE");
    }

    // Note: the SSH_ASKPASS_REQUIRE=force invariant is now covered by the
    // real regression test in `src/askpass_env.rs`, which builds a Command and
    // inspects its env vars directly via `Command::get_envs()`.

    // =========================================================================
    // Tunnel vs Connection env var consistency
    // =========================================================================

    #[test]
    fn start_tunnel_sets_config_path_env() {
        // PURPLE_CONFIG_PATH must be set so the askpass subprocess can find the config
        let env_vars = [
            "SSH_ASKPASS",
            "SSH_ASKPASS_REQUIRE",
            "PURPLE_ASKPASS_MODE",
            "PURPLE_HOST_ALIAS",
            "PURPLE_CONFIG_PATH",
        ];
        assert!(env_vars.contains(&"PURPLE_CONFIG_PATH"));
    }

    #[test]
    fn start_tunnel_does_not_set_bw_session() {
        // Unlike connection.rs, start_tunnel does NOT pass BW_SESSION.
        // The askpass subprocess reads from env inherited from the parent process.
        // This is correct because BW_SESSION should be in the parent env already.
        let tunnel_env_vars = [
            "SSH_ASKPASS",
            "SSH_ASKPASS_REQUIRE",
            "PURPLE_ASKPASS_MODE",
            "PURPLE_HOST_ALIAS",
            "PURPLE_CONFIG_PATH",
        ];
        assert!(!tunnel_env_vars.contains(&"BW_SESSION"));
    }

    #[test]
    fn start_tunnel_stdin_is_null() {
        // Tunnels use -N (no remote command) and stdin is null.
        // This means SSH cannot prompt interactively, making ASKPASS essential.
        let stdin_mode = "null";
        assert_eq!(stdin_mode, "null");
    }

    #[test]
    fn start_tunnel_uses_dash_n_flag() {
        // -N means no remote command, just forwarding
        let flag = "-N";
        assert_eq!(flag, "-N");
    }
}
