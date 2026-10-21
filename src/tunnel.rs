use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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
            self.bind_port.to_string()
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
    /// Monotonic start time. Used to render the UPTIME column in the
    /// tunnels-overview screen. `Instant` is monotonic, so wall-clock
    /// jumps (NTP, sleep/wake) cannot make uptime go backwards.
    pub started_at: Instant,
    /// Per-tunnel live counters fed by the stderr-parser worker.
    pub live: crate::tunnel_live::TunnelLiveState,
}

impl Drop for ActiveTunnel {
    fn drop(&mut self) {
        // Signal the stderr parser thread and join it. The parser
        // unblocks when ssh's stderr pipe closes, which happens once
        // the caller has killed the ssh child.
        self.live
            .parser_stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.live.parser_thread.take() {
            let _ = handle.join();
        }
    }
}

impl ActiveTunnel {
    /// Wrap a freshly-spawned ssh `Child` with live-state plumbing.
    /// Takes ownership of `child.stderr` and hands it off to a parser
    /// thread that emits `ChannelEvent`s on `parser_tx`. Throughput is
    /// derived in `TunnelState::poll` from the per-peer lsof samples,
    /// so no separate sampler thread is needed here.
    pub fn spawn(
        mut child: Child,
        alias: &str,
        parser_tx: std::sync::mpsc::Sender<crate::tunnel_live::ParserMessage>,
        watch: crate::tunnel_live::AuthWatch,
    ) -> Self {
        let started_at = Instant::now();
        let mut live = crate::tunnel_live::TunnelLiveState::new(started_at);
        if let Some(stderr) = child.stderr.take() {
            let flags = crate::tunnel_live::ParserFlags {
                stop: live.parser_stop.clone(),
                authenticated: live.authenticated.clone(),
                auth_refused: live.auth_refused.clone(),
                drained: live.drained.clone(),
            };
            let handle = crate::tunnel_live::spawn_parser_thread(
                stderr,
                alias.to_string(),
                parser_tx,
                live.stderr_buffer.clone(),
                flags,
                watch,
            );
            live.parser_thread = Some(handle);
        } else {
            live.drained
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        Self {
            child,
            started_at,
            live,
        }
    }
}

/// Format a tunnel uptime for the UPTIME column.
///
/// Bands:
/// - `< 60s`: `47s`
/// - `< 1h`: `12m 47s`
/// - `< 24h`: `2h 14m`
/// - `>= 24h`: `3d 4h`
pub fn format_uptime(elapsed: Duration) -> String {
    let total = elapsed.as_secs();
    if total < 60 {
        format!("{}s", total)
    } else if total < 3_600 {
        let m = total / 60;
        let s = total % 60;
        format!("{}m {}s", m, s)
    } else if total < 86_400 {
        let h = total / 3_600;
        let m = (total % 3_600) / 60;
        format!("{}h {}m", h, m)
    } else {
        let d = total / 86_400;
        let h = (total % 86_400) / 3_600;
        format!("{}d {}h", d, h)
    }
}

/// Build the tunnel command. Pure, so tests can inspect argv and env.
///
/// Without a password source and without a session password, `BatchMode=yes`
/// makes ssh fail at once on a password-only host, so the tunnel never sits
/// in "connecting" while ssh waits on a tty nobody can see. No
/// `StrictHostKeyChecking` override here: a user's own setting keeps working
/// for tunnels.
pub(crate) fn build_tunnel_command(
    alias: &str,
    config_path: &std::path::Path,
    askpass: Option<&str>,
    session_password: Option<&str>,
    bw_session: Option<&str>,
    security_key: &crate::security_key::SecurityKeyRun,
) -> Command {
    let mut cmd = Command::new("ssh");
    cmd.arg("-F")
        .arg(config_path)
        // `-v` enables debug1: lines on stderr. The per-tunnel parser
        // thread reads them to surface channel-open/-close events in
        // the LIVE and EVENTS detail cards.
        .arg("-v")
        .arg("-N")
        .arg("-o")
        .arg(crate::askpass_env::SINGLE_PASSWORD_PROMPT_OPT);
    if crate::askpass_env::needs_batch_mode(askpass, session_password) {
        cmd.arg("-o").arg(crate::askpass_env::BATCH_MODE_OPT);
    }
    cmd.arg("--")
        .arg(alias)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    crate::askpass_env::configure_auth(
        &mut cmd,
        alias,
        config_path,
        askpass,
        session_password,
        bw_session,
        true,
    );
    crate::security_key::configure(&mut cmd, security_key);
    cmd
}

/// Start an SSH tunnel process for the given host alias.
/// Uses `ssh -N` (no remote command). All configured forwards activate automatically.
/// Passes `-F <config_path>` so the alias resolves against the correct config file.
/// stderr is piped so poll_tunnels() can capture error messages on exit.
/// When `askpass` or `session_password` is Some, the askpass program is wired
/// up. Essential for tunnels since stdin is null and interactive password
/// entry is impossible.
pub fn start_tunnel(
    alias: &str,
    config_path: &std::path::Path,
    askpass: Option<&str>,
    session_password: Option<&str>,
    bw_session: Option<&str>,
    security_key: &crate::security_key::SecurityKeyRun,
) -> Result<Child> {
    let mut cmd = build_tunnel_command(
        alias,
        config_path,
        askpass,
        session_password,
        bw_session,
        security_key,
    );

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
        "[external] Tunnel SSH command: ssh -v -N -F {} -- {alias}",
        config_path.display()
    );

    cmd.spawn()
        .map_err(|e| anyhow::anyhow!("Failed to start tunnel: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- format_uptime tests ---

    #[test]
    fn format_uptime_seconds_only() {
        assert_eq!(format_uptime(Duration::from_secs(0)), "0s");
        assert_eq!(format_uptime(Duration::from_secs(1)), "1s");
        assert_eq!(format_uptime(Duration::from_secs(47)), "47s");
        assert_eq!(format_uptime(Duration::from_secs(59)), "59s");
    }

    #[test]
    fn format_uptime_minutes_seconds() {
        assert_eq!(format_uptime(Duration::from_secs(60)), "1m 0s");
        assert_eq!(format_uptime(Duration::from_secs(60 + 47)), "1m 47s");
        assert_eq!(format_uptime(Duration::from_secs(12 * 60 + 47)), "12m 47s");
        assert_eq!(format_uptime(Duration::from_secs(59 * 60 + 59)), "59m 59s");
    }

    #[test]
    fn format_uptime_hours_minutes() {
        assert_eq!(format_uptime(Duration::from_secs(3_600)), "1h 0m");
        assert_eq!(
            format_uptime(Duration::from_secs(2 * 3_600 + 14 * 60)),
            "2h 14m"
        );
        // Seconds within the hours band must NOT leak into the output.
        assert_eq!(
            format_uptime(Duration::from_secs(2 * 3_600 + 14 * 60 + 30)),
            "2h 14m"
        );
        assert_eq!(
            format_uptime(Duration::from_secs(23 * 3_600 + 59 * 60)),
            "23h 59m"
        );
    }

    #[test]
    fn format_uptime_days_hours() {
        assert_eq!(format_uptime(Duration::from_secs(86_400)), "1d 0h");
        assert_eq!(
            format_uptime(Duration::from_secs(3 * 86_400 + 4 * 3_600)),
            "3d 4h"
        );
        // Minutes within the days band must NOT leak into the output.
        assert_eq!(
            format_uptime(Duration::from_secs(3 * 86_400 + 4 * 3_600 + 30 * 60)),
            "3d 4h"
        );
        assert_eq!(
            format_uptime(Duration::from_secs(365 * 86_400 + 12 * 3_600)),
            "365d 12h"
        );
    }

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
        // ":80" parses via rfind(':') as empty host + port 80. SSH would reject this
        // but the parser accepts it (validation happens at form/CLI level)
        let rule = TunnelRule::parse_value("LocalForward", "8080 :80").unwrap();
        assert_eq!(rule.remote_host, "");
        assert_eq!(rule.remote_port, 80);
    }

    #[test]
    fn parse_empty_brackets() {
        // "[]" produces empty address. SSH would reject, parser accepts
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
        // Bare (unbracketed) IPv6 via rfind(':'): remote_host="::1", remote_port=80
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
    // build_tunnel_command: argv and env of the ssh child
    // =========================================================================
    // `start_tunnel` spawns ssh; the builder is the part under test.

    fn tunnel_args(askpass: Option<&str>, session_password: Option<&str>) -> Vec<String> {
        build_tunnel_command(
            "web1",
            std::path::Path::new("/tmp/cfg"),
            askpass,
            session_password,
            None,
            &crate::security_key::NO_SECURITY_KEY,
        )
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
    }

    fn tunnel_env(askpass: Option<&str>, bw_session: Option<&str>) -> Vec<(String, String)> {
        build_tunnel_command(
            "web1",
            std::path::Path::new("/tmp/cfg"),
            askpass,
            None,
            bw_session,
            &crate::security_key::NO_SECURITY_KEY,
        )
        .get_envs()
        .filter_map(|(k, v)| {
            v.map(|val| {
                (
                    k.to_string_lossy().into_owned(),
                    val.to_string_lossy().into_owned(),
                )
            })
        })
        .collect()
    }

    fn has_opt(args: &[String], value: &str) -> bool {
        args.windows(2).any(|w| w[0] == "-o" && w[1] == value)
    }

    #[test]
    fn start_tunnel_without_a_source_still_wires_askpass() {
        // A tunnel is a background run with no terminal of its own. Even
        // with nothing to answer, the askpass program has to be wired: ssh
        // passes the environment to a ProxyJump hop but none of its `-o`
        // options, so a bastion asking for a password would otherwise
        // prompt on the terminal the TUI is drawing on.
        let env = tunnel_env(None, None);
        let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains(&"SSH_ASKPASS_REQUIRE"), "got: {names:?}");
        assert!(
            !names.contains(&crate::askpass_env::ONESHOT_SECRET_VAR),
            "there is no secret to carry: {names:?}"
        );
    }

    #[test]
    fn start_tunnel_askpass_some_wires_askpass_env() {
        let env = tunnel_env(Some("keychain"), None);
        let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        for expected in [
            "SSH_ASKPASS",
            "SSH_ASKPASS_REQUIRE",
            "PURPLE_ASKPASS_MODE",
            "PURPLE_HOST_ALIAS",
            "PURPLE_CONFIG_PATH",
        ] {
            assert!(names.contains(&expected), "missing {expected}: {names:?}");
        }
    }

    #[test]
    fn start_tunnel_forwards_bw_session_when_given() {
        let env = tunnel_env(Some("bw:item"), Some("tok"));
        assert!(env.contains(&("BW_SESSION".to_string(), "tok".to_string())));
    }

    #[test]
    fn start_tunnel_without_auth_sets_batch_mode() {
        // A password-only host without a source fails fast instead of
        // leaving the tunnel in "connecting" while ssh waits on a tty.
        let args = tunnel_args(None, None);
        assert!(has_opt(&args, "BatchMode=yes"), "got: {args:?}");
    }

    #[test]
    fn start_tunnel_with_source_omits_batch_mode() {
        let args = tunnel_args(Some("keychain"), None);
        assert!(!has_opt(&args, "BatchMode=yes"), "got: {args:?}");
    }

    #[test]
    fn start_tunnel_with_session_password_omits_batch_mode() {
        let args = tunnel_args(None, Some("hunter2"));
        assert!(!has_opt(&args, "BatchMode=yes"), "got: {args:?}");
        assert!(args.iter().all(|a| a != "hunter2"));
    }

    #[test]
    fn start_tunnel_always_limits_ssh_to_one_password_attempt() {
        for (askpass, session) in [(None, None), (Some("keychain"), None), (None, Some("pw"))] {
            let args = tunnel_args(askpass, session);
            assert!(has_opt(&args, "NumberOfPasswordPrompts=1"), "got: {args:?}");
        }
    }

    #[test]
    fn start_tunnel_keeps_user_strict_host_key_setting() {
        // Tunnels get no StrictHostKeyChecking override: the user's own
        // ssh config setting decides.
        let args = tunnel_args(None, None);
        assert!(!args.iter().any(|a| a.contains("StrictHostKeyChecking")));
    }

    #[test]
    fn start_tunnel_carries_security_key_pins_in_env_only() {
        let run = crate::security_key::SecurityKeyRun {
            pins: vec![crate::security_key::SecurityKeyPin {
                key_path: "/home/u/.ssh/id_sk".into(),
                pin: "4242".into(),
            }],
            flow: Some(crate::security_key::PinFlow::Tunnel),
        };
        let cmd = build_tunnel_command(
            "web1",
            std::path::Path::new("/tmp/cfg"),
            None,
            None,
            None,
            &run,
        );
        assert!(cmd.get_args().all(|a| a != "4242"));
        let envs: Vec<(String, String)> = cmd
            .get_envs()
            .filter_map(|(k, v)| {
                v.map(|v| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.to_string_lossy().into_owned(),
                    )
                })
            })
            .collect();
        assert!(envs.contains(&("PURPLE_ASKPASS_SK_PIN_0".into(), "4242".into())));
        assert!(envs.contains(&("PURPLE_ASKPASS_SK_FLOW".into(), "tunnel".into())));
    }

    #[test]
    fn start_tunnel_uses_dash_n_and_verbose_before_separator() {
        let args = tunnel_args(None, None);
        let sep = args.iter().position(|a| a == "--").expect("-- present");
        let n = args.iter().position(|a| a == "-N").expect("-N present");
        let v = args.iter().position(|a| a == "-v").expect("-v present");
        assert!(n < sep && v < sep, "got: {args:?}");
        assert_eq!(args.last().map(String::as_str), Some("web1"));
    }
}
