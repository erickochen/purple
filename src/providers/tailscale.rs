use std::collections::HashMap;
use std::io::Read as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use log::debug;
use serde::Deserialize;

use base64::Engine as _;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error, strip_cidr};

pub struct Tailscale;

// =========================================================================
// CLI structs (`tailscale status --json` uses PascalCase)
// =========================================================================

#[derive(Deserialize)]
struct CliStatus {
    #[serde(rename = "Peer")]
    #[serde(default)]
    peer: HashMap<String, CliPeer>,
}

#[derive(Deserialize)]
struct CliPeer {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "HostName")]
    host_name: String,
    #[serde(rename = "TailscaleIPs")]
    #[serde(default)]
    tailscale_ips: Vec<String>,
    #[serde(rename = "OS")]
    #[serde(default)]
    os: String,
    #[serde(rename = "Online")]
    #[serde(default)]
    online: Option<bool>,
    #[serde(rename = "Tags")]
    #[serde(default)]
    tags: Vec<String>,
}

// =========================================================================
// API structs (camelCase)
// =========================================================================

#[derive(Deserialize)]
struct ApiResponse {
    devices: Vec<ApiDevice>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiDevice {
    node_id: String,
    hostname: String,
    name: String,
    #[serde(default)]
    addresses: Vec<String>,
    #[serde(default)]
    os: String,
    #[serde(default = "default_authorized")]
    authorized: bool,
    #[serde(default)]
    connected_to_control: bool,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    tags: Vec<String>,
}

/// Default for authorized field: true (most API devices are authorized;
/// missing field should not silently filter out devices).
fn default_authorized() -> bool {
    true
}

/// Deserialize a Vec that may be null in JSON (Tailscale API can return
/// `"tags": null` instead of omitting the field or using an empty array).
fn deserialize_null_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<Vec<String>>::deserialize(deserializer).map(|v| v.unwrap_or_default())
}

// =========================================================================
// Helpers
// =========================================================================

/// Select the best IP from a list of Tailscale addresses.
/// Prefers IPv4 (100.x) over IPv6 (fd7a:). Strips CIDR suffixes.
fn select_ip(ips: &[String]) -> Option<String> {
    // Prefer IPv4
    if let Some(ip) = ips.iter().find(|ip| ip.starts_with("100.")) {
        return Some(strip_cidr(ip).to_string());
    }
    // Fall back to first available
    ips.first().map(|ip| strip_cidr(ip).to_string())
}

/// Strip the `tag:` prefix from Tailscale tags.
fn strip_tag_prefix(tag: &str) -> String {
    tag.strip_prefix("tag:").unwrap_or(tag).to_string()
}

/// Find the tailscale binary. Checks PATH first, then macOS app bundle.
fn find_tailscale_binary() -> Result<PathBuf, ProviderError> {
    // Check PATH via shell builtin (more portable than `which`)
    let found = std::process::Command::new("sh")
        .args(["-c", "command -v tailscale"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    if let Ok(output) = found {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    // macOS app bundle fallback (the CLI binary inside the GUI app)
    let macos_path = PathBuf::from("/Applications/Tailscale.app/Contents/MacOS/Tailscale");
    if macos_path.exists() {
        return Ok(macos_path);
    }

    Err(ProviderError::Execute(
        "Tailscale CLI not found. Install from https://tailscale.com/download or add it to PATH."
            .to_string(),
    ))
}

// =========================================================================
// Provider impl
// =========================================================================

impl Provider for Tailscale {
    fn name(&self) -> &str {
        "tailscale"
    }

    fn short_label(&self) -> &str {
        "ts"
    }

    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        if token.is_empty() {
            self.fetch_from_cli(cancel)
        } else {
            self.fetch_from_api(token, cancel)
        }
    }
}

impl Tailscale {
    fn fetch_from_cli(&self, cancel: &AtomicBool) -> Result<Vec<ProviderHost>, ProviderError> {
        let binary = find_tailscale_binary()?;

        let mut child = std::process::Command::new(&binary)
            .args(["status", "--json"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ProviderError::Execute(format!("Failed to run tailscale: {}", e)))?;

        // Read stdout in a background thread to avoid pipe deadlock.
        // If the child produces more output than the OS pipe buffer (~64KB),
        // it blocks until the parent reads. We must read concurrently.
        let stdout_pipe = child.stdout.take();
        let stdout_handle = std::thread::spawn(move || -> Result<String, String> {
            match stdout_pipe {
                Some(mut pipe) => {
                    let mut buf = String::new();
                    pipe.read_to_string(&mut buf)
                        .map_err(|e| format!("Failed to read tailscale stdout: {}", e))?;
                    Ok(buf)
                }
                None => Err("No stdout from tailscale".to_string()),
            }
        });

        let start = Instant::now();
        let timeout = Duration::from_secs(30);

        let exit_err: Option<ProviderError> = loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                break Some(ProviderError::Cancelled);
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        let stderr = child
                            .stderr
                            .take()
                            .map(|mut s| {
                                let mut buf = String::new();
                                if let Err(e) = s.read_to_string(&mut buf) {
                                    debug!("[external] Failed to read tailscale stderr: {e}");
                                }
                                buf
                            })
                            .unwrap_or_default();
                        break Some(ProviderError::Execute(format!(
                            "tailscale status failed: {}",
                            stderr.trim()
                        )));
                    }
                    break None;
                }
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        break Some(ProviderError::Execute(
                            "Tailscale CLI timed out after 30s.".to_string(),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Some(ProviderError::Execute(format!(
                        "Failed to wait for tailscale: {}",
                        e
                    )));
                }
            }
        };

        // Always join the stdout reader thread to prevent thread leaks.
        // All error paths above call kill()+wait() so the pipe is closed
        // and the thread will receive EOF promptly.
        let stdout_result = stdout_handle.join();

        if let Some(err) = exit_err {
            return Err(err);
        }

        let stdout_data = stdout_result
            .map_err(|_| ProviderError::Parse("stdout reader thread panicked".to_string()))?
            .map_err(ProviderError::Parse)?;

        let status: CliStatus = serde_json::from_str(&stdout_data).map_err(|e| {
            ProviderError::Parse(format!("Failed to parse tailscale output: {}", e))
        })?;

        Self::hosts_from_cli(status)
    }

    fn hosts_from_cli(status: CliStatus) -> Result<Vec<ProviderHost>, ProviderError> {
        let mut hosts = Vec::new();

        // Sort by peer key for deterministic output (HashMap iteration is random)
        let mut peers: Vec<_> = status.peer.into_iter().collect();
        peers.sort_by(|a, b| a.0.cmp(&b.0));

        for (_key, peer) in peers {
            let ip = match select_ip(&peer.tailscale_ips) {
                Some(ip) => ip,
                None => continue,
            };

            let tags: Vec<String> = peer.tags.iter().map(|t| strip_tag_prefix(t)).collect();

            let status_str = match peer.online {
                Some(true) => "online",
                Some(false) => "offline",
                None => "unknown",
            };

            let mut metadata = Vec::new();
            if !peer.os.is_empty() {
                metadata.push(("os".to_string(), peer.os.clone()));
            }
            metadata.push(("status".to_string(), status_str.to_string()));

            hosts.push(ProviderHost {
                server_id: peer.id,
                name: peer.host_name,
                ip,
                tags,
                metadata,
            });
        }

        Ok(hosts)
    }

    fn fetch_from_api(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        // Validate token prefix
        if token.starts_with("tskey-auth-") {
            return Err(ProviderError::Execute(
                "This is a device auth key, not an API key. Use a key starting with tskey-api-."
                    .to_string(),
            ));
        }

        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }

        let agent = super::http_agent();

        // Tailscale API keys (tskey-api-*) use HTTP Basic auth (key as username,
        // empty password). OAuth access tokens use Bearer auth.
        let auth_header = if token.starts_with("tskey-") {
            let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{}:", token));
            format!("Basic {}", encoded)
        } else {
            format!("Bearer {}", token)
        };

        let resp: ApiResponse = agent
            .get("https://api.tailscale.com/api/v2/tailnet/-/devices?fields=all")
            .header("Authorization", &auth_header)
            .call()
            .map_err(map_ureq_error)?
            .body_mut()
            .read_json()
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        Self::hosts_from_api(resp)
    }

    fn hosts_from_api(resp: ApiResponse) -> Result<Vec<ProviderHost>, ProviderError> {
        let mut hosts = Vec::new();

        for device in resp.devices {
            // Skip unauthorized devices
            if !device.authorized {
                continue;
            }

            let ip = match select_ip(&device.addresses) {
                Some(ip) => ip,
                None => continue,
            };

            // Use hostname, or strip FQDN from name if hostname is empty
            let name = if device.hostname.is_empty() {
                device
                    .name
                    .split('.')
                    .next()
                    .unwrap_or(&device.name)
                    .to_string()
            } else {
                device.hostname.clone()
            };

            let tags: Vec<String> = device.tags.iter().map(|t| strip_tag_prefix(t)).collect();

            let mut metadata = Vec::new();
            if !device.os.is_empty() {
                metadata.push(("os".to_string(), device.os.clone()));
            }
            let status_str = if device.connected_to_control {
                "online"
            } else {
                "offline"
            };
            metadata.push(("status".to_string(), status_str.to_string()));

            hosts.push(ProviderHost {
                server_id: device.node_id,
                name,
                ip,
                tags,
                metadata,
            });
        }

        Ok(hosts)
    }
}

#[cfg(test)]
#[path = "tailscale_tests.rs"]
mod tests;
