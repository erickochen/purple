use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde_json::Value;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct Proxmox {
    pub base_url: String,
    pub verify_tls: bool,
}

// --- Serde structs ---

#[derive(Deserialize)]
struct PveResponse<T> {
    data: T,
}

#[derive(Deserialize)]
struct ClusterResource {
    #[serde(rename = "type")]
    resource_type: String,
    #[serde(default)]
    vmid: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    node: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    template: u8,
    #[serde(default)]
    tags: Option<String>,
    #[serde(default)]
    ip: Option<String>,
}

#[derive(Deserialize, Default)]
struct VmConfig {
    #[serde(default)]
    agent: Option<String>,
    /// Catch-all for dynamic fields like ipconfig0-9, net0-9.
    #[serde(flatten)]
    extra: HashMap<String, Value>,
}

// Guest agent response is double-wrapped: {"data": {"result": [...]}}
#[derive(Deserialize)]
struct GuestAgentNetworkResponse {
    data: GuestAgentResult,
}

#[derive(Deserialize)]
struct GuestAgentResult {
    #[serde(default)]
    result: Vec<GuestInterface>,
}

#[derive(Deserialize)]
struct GuestInterface {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "ip-addresses")]
    ip_addresses: Vec<GuestIpAddress>,
}

#[derive(Deserialize)]
struct GuestIpAddress {
    #[serde(default, rename = "ip-address")]
    ip_address: String,
    #[serde(default, rename = "ip-address-type")]
    ip_address_type: String,
}

// LXC container interfaces from /lxc/{vmid}/interfaces
#[derive(Deserialize, Default)]
struct LxcInterface {
    #[serde(default)]
    name: String,
    // Legacy PVE format: inet/inet6 CIDR strings
    #[serde(default)]
    inet: Option<String>,
    #[serde(default)]
    inet6: Option<String>,
    // Newer PVE format: same ip-addresses array shape as QEMU guest agent
    #[serde(default, rename = "ip-addresses")]
    ip_addresses: Vec<GuestIpAddress>,
}

/// Outcome of resolving an IP for a single VM/container.
enum ResolveOutcome {
    /// Successfully resolved an IP address.
    Resolved(String),
    /// VM is stopped, cannot resolve runtime IP.
    Stopped,
    /// No IP could be determined (running but no static or agent IP).
    NoIp,
    /// API call failed (HTTP error, parse error).
    Failed,
    /// API call failed with 401/403 (authentication or permission error).
    AuthFailed,
}

// --- Helper functions ---

/// Build the PVE auth header value. Prepends "PVEAPIToken=" if not already present.
fn auth_header(token: &str) -> String {
    if token.starts_with("PVEAPIToken=") {
        token.to_string()
    } else {
        format!("PVEAPIToken={}", token)
    }
}

/// Normalize base URL: strip trailing slash and /api2/json suffix.
fn normalize_url(url: &str) -> String {
    let mut u = url.trim_end_matches('/').to_string();
    if u.ends_with("/api2/json") {
        u.truncate(u.len() - "/api2/json".len());
    }
    u
}

/// Parse a static IP from ipconfig0 value like "ip=10.0.0.1/24,gw=10.0.0.1".
/// Prefers IPv4 (ip=). Falls back to IPv6 (ip6=) if ip= is dhcp or absent.
/// Returns None if both are dhcp/auto or absent.
fn parse_ipconfig_ip(ipconfig: &str) -> Option<String> {
    let mut ipv6_candidate = None;
    for part in ipconfig.split(',') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("ip=") {
            if value.eq_ignore_ascii_case("dhcp") {
                continue;
            }
            return Some(super::strip_cidr(value).to_string());
        }
        if let Some(value) = part.strip_prefix("ip6=") {
            if value.eq_ignore_ascii_case("dhcp") || value.eq_ignore_ascii_case("auto") || value.eq_ignore_ascii_case("manual") {
                continue;
            }
            if ipv6_candidate.is_none() {
                ipv6_candidate = Some(super::strip_cidr(value).to_string());
            }
        }
    }
    ipv6_candidate
}

/// Parse a static IP from LXC net0 value like "name=eth0,bridge=vmbr0,ip=10.0.0.2/24,...".
/// Prefers IPv4 (ip=). Falls back to IPv6 (ip6=) if ip= is dhcp or absent.
fn parse_lxc_net_ip(net0: &str) -> Option<String> {
    let mut ipv6_candidate = None;
    for part in net0.split(',') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("ip=") {
            if value.eq_ignore_ascii_case("dhcp") {
                continue;
            }
            return Some(super::strip_cidr(value).to_string());
        }
        if let Some(value) = part.strip_prefix("ip6=") {
            if value.eq_ignore_ascii_case("dhcp") || value.eq_ignore_ascii_case("auto") || value.eq_ignore_ascii_case("manual") {
                continue;
            }
            if ipv6_candidate.is_none() {
                ipv6_candidate = Some(super::strip_cidr(value).to_string());
            }
        }
    }
    ipv6_candidate
}

/// Extract sorted string values for keys matching a prefix (e.g. "ipconfig" -> ipconfig0..9).
fn extract_numbered_values(extra: &HashMap<String, Value>, prefix: &str) -> Vec<String> {
    let mut entries: Vec<(u32, String)> = extra
        .iter()
        .filter_map(|(k, v)| {
            let suffix = k.strip_prefix(prefix)?;
            let n: u32 = suffix.parse().ok()?;
            let s = v.as_str()?.to_string();
            Some((n, s))
        })
        .collect();
    entries.sort_by_key(|(n, _)| *n);
    entries.into_iter().map(|(_, v)| v).collect()
}

/// Check if the QEMU agent is enabled. The agent field can be:
/// "1", "enabled=1", "1,fstrim_cloned_disks=1,type=virtio", etc.
fn is_agent_enabled(agent: Option<&str>) -> bool {
    let s = match agent {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    // First comma-separated token is the enable flag
    let first = s.split(',').next().unwrap_or("");
    if first == "1" {
        return true;
    }
    if let Some(val) = first.strip_prefix("enabled=") {
        return val == "1";
    }
    false
}

/// Parse PVE tags string. PVE 7 uses semicolons, PVE 8 uses commas.
/// Split on both for compatibility.
fn parse_pve_tags(tags: Option<&str>) -> Vec<String> {
    let s = match tags {
        Some(s) if !s.is_empty() => s,
        _ => return Vec::new(),
    };
    s.split([';', ','])
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Select the best IP from guest agent interfaces.
/// Skips loopback, link-local. Prefers IPv4.
fn select_guest_agent_ip(interfaces: &[GuestInterface]) -> Option<String> {
    let mut ipv4_candidate = None;
    let mut ipv6_candidate = None;

    for iface in interfaces {
        if iface.name == "lo" {
            continue;
        }
        for addr in &iface.ip_addresses {
            let ip = super::strip_cidr(&addr.ip_address);
            if ip.is_empty() {
                continue;
            }
            if addr.ip_address_type == "ipv4" {
                if ip.starts_with("169.254.") || ip.starts_with("127.") {
                    continue;
                }
                if ipv4_candidate.is_none() {
                    ipv4_candidate = Some(ip.to_string());
                }
            } else if addr.ip_address_type == "ipv6" {
                let ip_lc = ip.to_ascii_lowercase();
                if ip_lc.starts_with("fe80:") || ip_lc.starts_with("fe80%") || ip_lc == "::1" {
                    continue;
                }
                if ipv6_candidate.is_none() {
                    ipv6_candidate = Some(ip.to_string());
                }
            }
        }
    }

    ipv4_candidate.or(ipv6_candidate)
}

/// Select the best IP from LXC container interfaces.
/// Handles both the legacy inet/inet6 string format and the newer ip-addresses array format.
/// Skips loopback, link-local. Prefers IPv4.
fn select_lxc_interface_ip(interfaces: &[LxcInterface]) -> Option<String> {
    let mut ipv4_candidate = None;
    let mut ipv6_candidate = None;

    for iface in interfaces {
        if iface.name == "lo" {
            continue;
        }
        // Legacy format: inet/inet6 CIDR strings
        if let Some(ref inet) = iface.inet {
            let ip = super::strip_cidr(inet.split_whitespace().next().unwrap_or(inet));
            if !ip.is_empty() && !ip.starts_with("169.254.") && !ip.starts_with("127.") && ipv4_candidate.is_none() {
                ipv4_candidate = Some(ip.to_string());
            }
        }
        if let Some(ref inet6) = iface.inet6 {
            let ip = super::strip_cidr(inet6.split_whitespace().next().unwrap_or(inet6));
            let ip_lc = ip.to_ascii_lowercase();
            if !ip.is_empty()
                && !ip_lc.starts_with("fe80:") && !ip_lc.starts_with("fe80%")
                && ip_lc != "::1"
                && ipv6_candidate.is_none()
            {
                ipv6_candidate = Some(ip.to_string());
            }
        }
        // Newer format: ip-addresses array (same shape as QEMU guest agent response)
        for addr in &iface.ip_addresses {
            let ip = super::strip_cidr(&addr.ip_address);
            if ip.is_empty() {
                continue;
            }
            if addr.ip_address_type == "ipv4" {
                if ip.starts_with("169.254.") || ip.starts_with("127.") {
                    continue;
                }
                if ipv4_candidate.is_none() {
                    ipv4_candidate = Some(ip.to_string());
                }
            } else if addr.ip_address_type == "ipv6" {
                let ip_lc = ip.to_ascii_lowercase();
                if ip_lc.starts_with("fe80:") || ip_lc.starts_with("fe80%") || ip_lc == "::1" {
                    continue;
                }
                if ipv6_candidate.is_none() {
                    ipv6_candidate = Some(ip.to_string());
                }
            }
        }
    }

    ipv4_candidate.or(ipv6_candidate)
}

impl Proxmox {
    fn make_agent(&self) -> Result<ureq::Agent, ProviderError> {
        if self.verify_tls {
            Ok(super::http_agent())
        } else {
            super::http_agent_insecure()
        }
    }
}

impl Provider for Proxmox {
    fn name(&self) -> &str {
        "proxmox"
    }

    fn short_label(&self) -> &str {
        "pve"
    }

    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        self.fetch_hosts_with_progress(token, cancel, &|_| {})
    }

    fn fetch_hosts_with_progress(
        &self,
        token: &str,
        cancel: &AtomicBool,
        progress: &dyn Fn(&str),
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        let base = normalize_url(&self.base_url);
        if base.is_empty() {
            return Err(ProviderError::Http("No Proxmox URL configured.".to_string()));
        }
        if !base.to_ascii_lowercase().starts_with("https://") {
            return Err(ProviderError::Http(
                "Proxmox URL must use HTTPS. Update the URL in ~/.purple/providers.".to_string(),
            ));
        }

        let agent = self.make_agent()?;
        let auth = auth_header(token);

        // Phase 1: Fetch all cluster resources (unfiltered)
        progress("Fetching resources...");
        let url = format!("{}/api2/json/cluster/resources", base);
        let resp: PveResponse<Vec<ClusterResource>> = agent
            .get(&url)
            .set("Authorization", &auth)
            .call()
            .map_err(map_ureq_error)?
            .into_json()
            .map_err(|e| ProviderError::Parse(e.to_string()))?;

        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }

        // Filter for VMs and containers, skip templates
        let resources: Vec<&ClusterResource> = resp
            .data
            .iter()
            .filter(|r| (r.resource_type == "qemu" || r.resource_type == "lxc") && r.template == 0)
            .collect();

        let total = resources.len();
        progress(&format!("{} VMs/containers found.", total));

        // Phase 2: Resolve IPs for each resource
        let mut hosts = Vec::new();
        let mut fetch_failures = 0usize;
        let mut auth_failures = 0usize;
        let mut skipped_no_ip = 0usize;
        let mut skipped_stopped = 0usize;
        let mut resolved_count = 0usize;

        // N+1 API calls (one per VM). No rate limiting for v1. For very large clusters
        // (hundreds of VMs), consider adding a small delay between calls.
        for (i, resource) in resources.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            progress(&format!("Resolving IPs ({}/{})...", i + 1, total));

            // Use the IP from cluster/resources if available (free, no N+1 call).
            let cluster_ip = resource.ip.as_deref()
                .map(|ip| super::strip_cidr(ip).to_string())
                .filter(|ip| !ip.is_empty());
            let outcome = if let Some(ip) = cluster_ip {
                ResolveOutcome::Resolved(ip)
            } else if resource.resource_type == "qemu" {
                self.resolve_qemu_ip(&agent, &base, &auth, resource)
            } else {
                self.resolve_lxc_ip(&agent, &base, &auth, resource)
            };

            let ip = match outcome {
                ResolveOutcome::Resolved(ip) => {
                    resolved_count += 1;
                    ip
                }
                ResolveOutcome::Stopped => {
                    skipped_stopped += 1;
                    continue;
                }
                ResolveOutcome::NoIp => {
                    skipped_no_ip += 1;
                    continue;
                }
                ResolveOutcome::Failed => {
                    fetch_failures += 1;
                    continue;
                }
                ResolveOutcome::AuthFailed => {
                    fetch_failures += 1;
                    auth_failures += 1;
                    continue;
                }
            };

            // Build tags: PVE tags + resource type (dedup in case type already appears as a PVE tag)
            let mut tags = parse_pve_tags(resource.tags.as_deref());
            tags.push(resource.resource_type.clone());
            tags.sort();
            tags.dedup();

            hosts.push(ProviderHost {
                server_id: format!("{}:{}", resource.resource_type, resource.vmid),
                name: if resource.name.is_empty() {
                    format!("{}-{}", resource.resource_type, resource.vmid)
                } else {
                    resource.name.clone()
                },
                ip,
                tags,
            });
        }

        // Summary
        let mut parts = Vec::new();
        parts.push(format!("{} resolved", resolved_count));
        if skipped_no_ip > 0 {
            parts.push(format!("{} skipped (no IP)", skipped_no_ip));
        }
        if skipped_stopped > 0 {
            parts.push(format!("{} skipped (stopped)", skipped_stopped));
        }
        if fetch_failures > 0 {
            let label = if auth_failures == fetch_failures {
                format!("{} failed (authentication)", fetch_failures)
            } else if auth_failures > 0 {
                format!("{} failed ({} authentication)", fetch_failures, auth_failures)
            } else {
                format!("{} failed", fetch_failures)
            };
            parts.push(label);
        }
        progress(&parts.join(", "));

        if fetch_failures > 0 {
            if hosts.is_empty() {
                let msg = if auth_failures > 0 {
                    format!(
                        "Authentication failed for all {} VMs. Check your API token permissions.",
                        total
                    )
                } else {
                    format!("Failed to fetch details for all {} VMs", total)
                };
                return Err(ProviderError::Http(msg));
            }
            return Err(ProviderError::PartialResult {
                hosts,
                failures: fetch_failures,
                total,
            });
        }

        Ok(hosts)
    }
}

impl Proxmox {
    fn resolve_qemu_ip(
        &self,
        agent: &ureq::Agent,
        base: &str,
        auth: &str,
        resource: &ClusterResource,
    ) -> ResolveOutcome {
        // Step 1: Get VM config for ipconfig0
        let config_url = format!(
            "{}/api2/json/nodes/{}/qemu/{}/config",
            base, resource.node, resource.vmid
        );
        let config: VmConfig = match agent
            .get(&config_url)
            .set("Authorization", auth)
            .call()
        {
            Ok(resp) => match resp.into_json::<PveResponse<VmConfig>>() {
                Ok(r) => r.data,
                Err(_) => return ResolveOutcome::Failed,
            },
            Err(ureq::Error::Status(401, _) | ureq::Error::Status(403, _)) => {
                return ResolveOutcome::AuthFailed;
            }
            Err(_) => return ResolveOutcome::Failed,
        };

        // Try static IP from ipconfig0..9
        for ipconfig in extract_numbered_values(&config.extra, "ipconfig") {
            if let Some(ip) = parse_ipconfig_ip(&ipconfig) {
                return ResolveOutcome::Resolved(ip);
            }
        }

        // Step 2: Try guest agent if VM is running and agent is enabled
        if resource.status != "running" {
            return ResolveOutcome::Stopped;
        }

        if !is_agent_enabled(config.agent.as_deref()) {
            return ResolveOutcome::NoIp;
        }

        let agent_url = format!(
            "{}/api2/json/nodes/{}/qemu/{}/agent/network-get-interfaces",
            base, resource.node, resource.vmid
        );
        match agent.get(&agent_url).set("Authorization", auth).call() {
            Ok(resp) => {
                match resp.into_json::<GuestAgentNetworkResponse>() {
                    Ok(ga) => match select_guest_agent_ip(&ga.data.result) {
                        Some(ip) => ResolveOutcome::Resolved(ip),
                        None => ResolveOutcome::NoIp,
                    },
                    Err(_) => ResolveOutcome::Failed,
                }
            }
            Err(ureq::Error::Status(500, _))
            | Err(ureq::Error::Status(501, _)) => {
                // Agent not responding or not supported
                ResolveOutcome::NoIp
            }
            Err(ureq::Error::Status(401, _) | ureq::Error::Status(403, _)) => {
                ResolveOutcome::AuthFailed
            }
            Err(_) => {
                // Network errors, timeouts, etc.
                ResolveOutcome::Failed
            }
        }
    }

    fn resolve_lxc_ip(
        &self,
        agent: &ureq::Agent,
        base: &str,
        auth: &str,
        resource: &ClusterResource,
    ) -> ResolveOutcome {
        // Step 1: Get container config for net0
        let config_url = format!(
            "{}/api2/json/nodes/{}/lxc/{}/config",
            base, resource.node, resource.vmid
        );
        let config: VmConfig = match agent
            .get(&config_url)
            .set("Authorization", auth)
            .call()
        {
            Ok(resp) => match resp.into_json::<PveResponse<VmConfig>>() {
                Ok(r) => r.data,
                Err(_) => return ResolveOutcome::Failed,
            },
            Err(ureq::Error::Status(401, _) | ureq::Error::Status(403, _)) => {
                return ResolveOutcome::AuthFailed;
            }
            Err(_) => return ResolveOutcome::Failed,
        };

        // Try static IP from net0..9
        for net in extract_numbered_values(&config.extra, "net") {
            if let Some(ip) = parse_lxc_net_ip(&net) {
                return ResolveOutcome::Resolved(ip);
            }
        }

        // Step 2: Try runtime interfaces if container is running
        if resource.status != "running" {
            return ResolveOutcome::Stopped;
        }

        let iface_url = format!(
            "{}/api2/json/nodes/{}/lxc/{}/interfaces",
            base, resource.node, resource.vmid
        );
        match agent.get(&iface_url).set("Authorization", auth).call() {
            Ok(resp) => {
                match resp.into_json::<PveResponse<Vec<LxcInterface>>>() {
                    Ok(r) => match select_lxc_interface_ip(&r.data) {
                        Some(ip) => ResolveOutcome::Resolved(ip),
                        None => ResolveOutcome::NoIp,
                    },
                    Err(_) => ResolveOutcome::Failed,
                }
            }
            Err(ureq::Error::Status(401, _) | ureq::Error::Status(403, _)) => {
                ResolveOutcome::AuthFailed
            }
            Err(ureq::Error::Status(404, _))
            | Err(ureq::Error::Status(501, _)) => {
                // Endpoint may not exist on older PVE
                ResolveOutcome::NoIp
            }
            Err(_) => ResolveOutcome::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Serde tests ---

    #[test]
    fn test_parse_cluster_resources() {
        let json = r#"{"data": [
            {"type": "qemu", "vmid": 100, "name": "web-1", "node": "pve1", "status": "running", "template": 0, "tags": "prod;web"},
            {"type": "lxc", "vmid": 200, "name": "dns-1", "node": "pve1", "status": "running", "template": 0},
            {"type": "qemu", "vmid": 999, "name": "template", "node": "pve1", "status": "stopped", "template": 1},
            {"type": "storage", "id": "local", "node": "pve1", "status": "available"}
        ]}"#;
        let resp: PveResponse<Vec<ClusterResource>> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 4);
        let vms: Vec<_> = resp.data.iter()
            .filter(|r| (r.resource_type == "qemu" || r.resource_type == "lxc") && r.template == 0)
            .collect();
        assert_eq!(vms.len(), 2);
        assert_eq!(vms[0].vmid, 100);
        assert_eq!(vms[1].vmid, 200);
    }

    #[test]
    fn test_cluster_resource_ip_field() {
        let json = r#"{"data": [
            {"type": "qemu", "vmid": 100, "name": "web-1", "node": "pve1", "status": "running", "template": 0, "ip": "10.0.0.5"},
            {"type": "lxc",  "vmid": 200, "name": "dns-1", "node": "pve1", "status": "running", "template": 0}
        ]}"#;
        let resp: PveResponse<Vec<ClusterResource>> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data[0].ip.as_deref(), Some("10.0.0.5"));
        assert_eq!(resp.data[1].ip, None);
    }

    #[test]
    fn test_parse_guest_agent_response_double_wrapped() {
        let json = r#"{"data": {"result": [
            {"name": "lo", "ip-addresses": [{"ip-address": "127.0.0.1", "ip-address-type": "ipv4"}]},
            {"name": "eth0", "ip-addresses": [
                {"ip-address": "10.0.0.5", "ip-address-type": "ipv4"},
                {"ip-address": "fe80::1", "ip-address-type": "ipv6"}
            ]}
        ]}}"#;
        let resp: GuestAgentNetworkResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.result.len(), 2);
        assert_eq!(resp.data.result[1].ip_addresses[0].ip_address, "10.0.0.5");
    }

    #[test]
    fn test_parse_lxc_interfaces() {
        let json = r#"{"data": [
            {"name": "lo", "inet": "127.0.0.1/8", "inet6": "::1/128"},
            {"name": "eth0", "inet": "10.0.0.10/24", "inet6": "fd00::10/64"}
        ]}"#;
        let resp: PveResponse<Vec<LxcInterface>> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.data[1].inet.as_deref(), Some("10.0.0.10/24"));
    }

    // --- extract_numbered_values tests ---

    #[test]
    fn test_extract_numbered_values_sorted() {
        let mut extra = HashMap::new();
        extra.insert("ipconfig2".into(), Value::String("ip=10.0.2.1/24".into()));
        extra.insert("ipconfig0".into(), Value::String("ip=dhcp".into()));
        extra.insert("ipconfig1".into(), Value::String("ip=10.0.1.1/24".into()));
        extra.insert("agent".into(), Value::String("1".into()));
        let values = extract_numbered_values(&extra, "ipconfig");
        assert_eq!(values, vec!["ip=dhcp", "ip=10.0.1.1/24", "ip=10.0.2.1/24"]);
    }

    #[test]
    fn test_extract_numbered_values_skips_non_string() {
        let mut extra = HashMap::new();
        extra.insert("net0".into(), Value::String("name=eth0,ip=10.0.0.1/24".into()));
        extra.insert("net1".into(), Value::Number(serde_json::Number::from(42)));
        let values = extract_numbered_values(&extra, "net");
        assert_eq!(values, vec!["name=eth0,ip=10.0.0.1/24"]);
    }

    #[test]
    fn test_vmconfig_flatten_deserialization() {
        let json = r#"{"agent": "1", "ipconfig0": "ip=dhcp", "ipconfig1": "ip=10.0.1.1/24", "net0": "name=eth0,bridge=vmbr0,ip=dhcp", "cores": 4}"#;
        let config: VmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agent, Some("1".to_string()));
        let ipconfigs = extract_numbered_values(&config.extra, "ipconfig");
        assert_eq!(ipconfigs, vec!["ip=dhcp", "ip=10.0.1.1/24"]);
        let nets = extract_numbered_values(&config.extra, "net");
        assert_eq!(nets, vec!["name=eth0,bridge=vmbr0,ip=dhcp"]);
    }

    #[test]
    fn test_multi_nic_ipconfig_fallback() {
        // ipconfig0 is DHCP, ipconfig1 has static IP
        let mut extra = HashMap::new();
        extra.insert("ipconfig0".into(), Value::String("ip=dhcp".into()));
        extra.insert("ipconfig1".into(), Value::String("ip=10.0.1.5/24".into()));
        let mut result = None;
        for ipconfig in extract_numbered_values(&extra, "ipconfig") {
            if let Some(ip) = parse_ipconfig_ip(&ipconfig) {
                result = Some(ip);
                break;
            }
        }
        assert_eq!(result, Some("10.0.1.5".to_string()));
    }

    // --- parse_ipconfig_ip tests ---

    #[test]
    fn test_parse_ipconfig_static() {
        assert_eq!(parse_ipconfig_ip("ip=10.0.0.1/24,gw=10.0.0.1"), Some("10.0.0.1".to_string()));
    }

    #[test]
    fn test_parse_ipconfig_dhcp() {
        assert_eq!(parse_ipconfig_ip("ip=dhcp"), None);
    }

    #[test]
    fn test_parse_ipconfig_ip6_only() {
        assert_eq!(
            parse_ipconfig_ip("ip6=2001:db8::1/64,gw6=2001:db8::ffff"),
            Some("2001:db8::1".to_string())
        );
    }

    #[test]
    fn test_parse_ipconfig_dhcp_with_ip6_static() {
        assert_eq!(
            parse_ipconfig_ip("ip=dhcp,ip6=fd00::1/64"),
            Some("fd00::1".to_string())
        );
    }

    #[test]
    fn test_parse_ipconfig_ip6_dhcp() {
        assert_eq!(parse_ipconfig_ip("ip6=dhcp"), None);
    }

    #[test]
    fn test_parse_ipconfig_ip6_auto() {
        assert_eq!(parse_ipconfig_ip("ip6=auto"), None);
    }

    #[test]
    fn test_parse_ipconfig_ipv4_preferred_over_ipv6() {
        assert_eq!(
            parse_ipconfig_ip("ip=10.0.0.1/24,ip6=2001:db8::1/64"),
            Some("10.0.0.1".to_string())
        );
    }

    #[test]
    fn test_parse_ipconfig_both_dhcp() {
        assert_eq!(parse_ipconfig_ip("ip=dhcp,ip6=dhcp"), None);
    }

    #[test]
    fn test_parse_ipconfig_no_ip_key() {
        assert_eq!(parse_ipconfig_ip("gw=10.0.0.1"), None);
    }

    #[test]
    fn test_parse_ipconfig_ipv6() {
        assert_eq!(
            parse_ipconfig_ip("ip=2001:db8::1/64,gw=2001:db8::ffff"),
            Some("2001:db8::1".to_string())
        );
    }

    // --- parse_lxc_net_ip tests ---

    #[test]
    fn test_parse_lxc_net_static() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip=10.0.0.2/24,gw=10.0.0.1"),
            Some("10.0.0.2".to_string())
        );
    }

    #[test]
    fn test_parse_lxc_net_dhcp() {
        assert_eq!(parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip=dhcp"), None);
    }

    #[test]
    fn test_parse_lxc_net_ip6_only() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip6=fd00::2/64"),
            Some("fd00::2".to_string())
        );
    }

    #[test]
    fn test_parse_lxc_net_dhcp_with_ip6_static() {
        assert_eq!(
            parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip=dhcp,ip6=fd00::2/64"),
            Some("fd00::2".to_string())
        );
    }

    #[test]
    fn test_parse_lxc_net_ip6_auto() {
        assert_eq!(parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip6=auto"), None);
    }

    #[test]
    fn test_parse_lxc_net_ip6_manual() {
        assert_eq!(parse_lxc_net_ip("name=eth0,bridge=vmbr0,ip6=manual"), None);
    }

    #[test]
    fn test_parse_ipconfig_ip6_manual() {
        assert_eq!(parse_ipconfig_ip("ip6=manual"), None);
    }

    #[test]
    fn test_parse_ipconfig_dhcp_and_ip6_manual() {
        assert_eq!(parse_ipconfig_ip("ip=dhcp,ip6=manual"), None);
    }

    // --- is_agent_enabled tests ---

    #[test]
    fn test_agent_enabled_simple() {
        assert!(is_agent_enabled(Some("1")));
    }

    #[test]
    fn test_agent_disabled_simple() {
        assert!(!is_agent_enabled(Some("0")));
    }

    #[test]
    fn test_agent_enabled_explicit() {
        assert!(is_agent_enabled(Some("enabled=1")));
    }

    #[test]
    fn test_agent_enabled_with_options() {
        assert!(is_agent_enabled(Some("1,fstrim_cloned_disks=1,type=virtio")));
    }

    #[test]
    fn test_agent_disabled_explicit() {
        assert!(!is_agent_enabled(Some("enabled=0")));
    }

    #[test]
    fn test_agent_none() {
        assert!(!is_agent_enabled(None));
    }

    #[test]
    fn test_agent_empty() {
        assert!(!is_agent_enabled(Some("")));
    }

    // --- parse_pve_tags tests ---

    #[test]
    fn test_tags_semicolons() {
        assert_eq!(parse_pve_tags(Some("prod;web;us-east")), vec!["prod", "web", "us-east"]);
    }

    #[test]
    fn test_tags_commas() {
        assert_eq!(parse_pve_tags(Some("prod,web,us-east")), vec!["prod", "web", "us-east"]);
    }

    #[test]
    fn test_tags_mixed() {
        assert_eq!(parse_pve_tags(Some("prod;web,us-east")), vec!["prod", "web", "us-east"]);
    }

    #[test]
    fn test_tags_empty() {
        assert!(parse_pve_tags(None).is_empty());
        assert!(parse_pve_tags(Some("")).is_empty());
    }

    #[test]
    fn test_tags_whitespace() {
        assert_eq!(parse_pve_tags(Some(" prod ; web ")), vec!["prod", "web"]);
    }

    #[test]
    fn test_tags_lowercased() {
        assert_eq!(parse_pve_tags(Some("PROD;Web")), vec!["prod", "web"]);
    }

    // --- auth_header tests ---

    #[test]
    fn test_auth_header_without_prefix() {
        assert_eq!(auth_header("user@pam!tok=secret"), "PVEAPIToken=user@pam!tok=secret");
    }

    #[test]
    fn test_auth_header_with_prefix() {
        assert_eq!(
            auth_header("PVEAPIToken=user@pam!tok=secret"),
            "PVEAPIToken=user@pam!tok=secret"
        );
    }

    // --- normalize_url tests ---

    #[test]
    fn test_normalize_url_trailing_slash() {
        assert_eq!(normalize_url("https://pve:8006/"), "https://pve:8006");
    }

    #[test]
    fn test_normalize_url_api_suffix() {
        assert_eq!(
            normalize_url("https://pve:8006/api2/json"),
            "https://pve:8006"
        );
    }

    #[test]
    fn test_normalize_url_bare() {
        assert_eq!(normalize_url("https://pve:8006"), "https://pve:8006");
    }

    #[test]
    fn test_normalize_url_api_suffix_trailing_slash() {
        assert_eq!(
            normalize_url("https://pve:8006/api2/json/"),
            "https://pve:8006"
        );
    }

    // --- select_guest_agent_ip tests ---

    #[test]
    fn test_guest_agent_ipv4_preferred() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "2001:db8::1".into(), ip_address_type: "ipv6".into() },
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_guest_agent_skips_loopback() {
        let interfaces = vec![
            GuestInterface {
                name: "lo".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.0.0.1".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_skips_link_local() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "169.254.1.1".into(), ip_address_type: "ipv4".into() },
                    GuestIpAddress { ip_address: "fe80::1".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_skips_link_local_uppercase() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "FE80::1".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_ipv6_fallback() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "2001:db8::1".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), Some("2001:db8::1".to_string()));
    }

    // --- select_lxc_interface_ip tests ---

    #[test]
    fn test_lxc_inet_preferred() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: Some("10.0.0.10/24".into()), inet6: Some("fd00::10/64".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.10".to_string()));
    }

    #[test]
    fn test_lxc_inet6_fallback() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: None, inet6: Some("fd00::10/64".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("fd00::10".to_string()));
    }

    #[test]
    fn test_lxc_skips_loopback() {
        let interfaces = vec![
            LxcInterface { name: "lo".into(), inet: Some("127.0.0.1/8".into()), inet6: None, ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_skips_link_local_ipv6_colon() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: None, inet6: Some("fe80::1/64".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_skips_link_local_ipv6_zone_id() {
        // fe80%eth0 zone-id format must be filtered the same way as guest agent
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: None, inet6: Some("fe80%eth0/64".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_skips_link_local_ipv6_zone_id_uppercase() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: None, inet6: Some("FE80%eth0/64".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    // --- server_id format ---

    #[test]
    fn test_server_id_format() {
        let resource = ClusterResource {
            resource_type: "qemu".into(),
            vmid: 100,
            name: "web-1".into(),
            node: "pve1".into(),
            status: "running".into(),
            template: 0,
            tags: None,
            ip: None,
        };
        assert_eq!(format!("{}:{}", resource.resource_type, resource.vmid), "qemu:100");
    }

    // --- resource type tag injection ---

    #[test]
    fn test_resource_type_tag_added() {
        let mut tags = parse_pve_tags(Some("prod;web"));
        tags.push("qemu".to_string());
        tags.sort();
        tags.dedup();
        assert_eq!(tags, vec!["prod", "qemu", "web"]);
    }

    #[test]
    fn test_resource_type_tag_no_duplicate_when_pve_tag_matches() {
        // VM with PVE tag "qemu" must not produce ["prod", "qemu", "qemu"]
        let mut tags = parse_pve_tags(Some("prod;qemu"));
        tags.push("qemu".to_string());
        tags.sort();
        tags.dedup();
        assert_eq!(tags, vec!["prod", "qemu"]);
    }

    #[test]
    fn test_lxc_resource_type_tag_no_duplicate() {
        let mut tags = parse_pve_tags(Some("lxc;db"));
        tags.push("lxc".to_string());
        tags.sort();
        tags.dedup();
        assert_eq!(tags, vec!["db", "lxc"]);
    }

    // --- template filtering ---

    #[test]
    fn test_template_filtered() {
        let resources = [
            ClusterResource {
                resource_type: "qemu".into(), vmid: 100, name: "vm".into(),
                node: "n".into(), status: "running".into(), template: 0, tags: None, ip: None,
            },
            ClusterResource {
                resource_type: "qemu".into(), vmid: 999, name: "tmpl".into(),
                node: "n".into(), status: "stopped".into(), template: 1, tags: None, ip: None,
            },
        ];
        let filtered: Vec<_> = resources.iter()
            .filter(|r| r.template == 0)
            .collect();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].vmid, 100);
    }

    // --- loopback IP filtering ---

    #[test]
    fn test_guest_agent_skips_loopback_ip_on_non_lo_iface() {
        // 127.x.x.x on a non-lo interface must still be skipped
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.0.0.1".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_skips_loopback_range() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.1.2.3".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_skips_ipv6_loopback() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "::1".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_guest_agent_loopback_then_real_ip() {
        // loopback on non-lo must not prevent picking real IP from another interface
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.0.0.1".into(), ip_address_type: "ipv4".into() },
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_lxc_skips_loopback_ip_on_non_lo_iface() {
        // 127.x.x.x on a non-lo interface must still be skipped
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: Some("127.0.0.1/8".into()), inet6: None, ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_skips_ipv6_loopback() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet: None, inet6: Some("::1/128".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    // --- LxcInterface ip-addresses format (fix 1) ---

    #[test]
    fn test_lxc_ip_addresses_format_ipv4() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "ipv4".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_lxc_ip_addresses_format_skips_loopback() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "127.0.0.1".into(), ip_address_type: "ipv4".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_ip_addresses_format_skips_link_local() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "fe80::1".into(), ip_address_type: "ipv6".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_ip_addresses_format_ipv4_preferred_over_ipv6() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "2001:db8::1".into(), ip_address_type: "ipv6".into() },
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "ipv4".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_lxc_inet_takes_precedence_over_ip_addresses() {
        // If both formats present, inet wins (encountered first in code)
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                inet: Some("192.168.1.1/24".into()),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "10.0.0.5".into(), ip_address_type: "ipv4".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("192.168.1.1".to_string()));
    }

    // --- strip_cidr in guest agent (fix 4) ---

    #[test]
    fn test_guest_agent_strips_cidr_ipv4() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "10.0.0.5/24".into(), ip_address_type: "ipv4".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn test_guest_agent_strips_cidr_ipv6() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "2001:db8::1/64".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), Some("2001:db8::1".to_string()));
    }

    // --- Fe80 mixed-case filtering (fix 3) ---

    #[test]
    fn test_guest_agent_skips_mixed_case_link_local() {
        let interfaces = vec![
            GuestInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "Fe80::1".into(), ip_address_type: "ipv6".into() },
                ],
            },
        ];
        assert_eq!(select_guest_agent_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_skips_mixed_case_link_local_inet6() {
        let interfaces = vec![
            LxcInterface { name: "eth0".into(), inet6: Some("Fe80::1/64".into()), ..Default::default() },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), None);
    }

    #[test]
    fn test_lxc_ip_addresses_strips_cidr() {
        let interfaces = vec![
            LxcInterface {
                name: "eth0".into(),
                ip_addresses: vec![
                    GuestIpAddress { ip_address: "10.0.0.5/24".into(), ip_address_type: "ipv4".into() },
                ],
                ..Default::default()
            },
        ];
        assert_eq!(select_lxc_interface_ip(&interfaces), Some("10.0.0.5".to_string()));
    }

    // --- name fallback ---

    #[test]
    fn test_name_fallback_when_empty() {
        let resource = ClusterResource {
            resource_type: "lxc".into(), vmid: 200, name: String::new(),
            node: "n".into(), status: "running".into(), template: 0, tags: None, ip: None,
        };
        let name = if resource.name.is_empty() {
            format!("{}-{}", resource.resource_type, resource.vmid)
        } else {
            resource.name.clone()
        };
        assert_eq!(name, "lxc-200");
    }
}
