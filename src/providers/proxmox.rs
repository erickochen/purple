use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use log::debug;
use serde::Deserialize;
use serde_json::Value;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct Proxmox {
    pub base_url: String,
    pub verify_tls: bool,
}

// --- Serde helpers ---

/// Deserialize a value that may be `null` or missing as `T::default()`.
/// `#[serde(default)]` only covers missing keys; this also handles explicit nulls.
fn null_to_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(|o| o.unwrap_or_default())
}

/// Deserialize a value that may be a string, integer or boolean into `Option<String>`.
/// Proxmox's Perl JSON serializer sometimes returns integer `1` instead of string `"1"`
/// for config values like `agent`.
fn lenient_string<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Option::<Value>::deserialize(d)? {
        Some(Value::String(s)) => Ok(Some(s)),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(Value::Bool(b)) => Ok(Some(if b { "1".to_string() } else { "0".to_string() })),
        _ => Ok(None),
    }
}

/// Deserialize a value that may be an integer, boolean or null as u8.
/// Handles `"template": true` (→ 1), `"template": 1`, `"template": null` (→ 0).
fn lenient_u8<'de, D>(d: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Option::<Value>::deserialize(d)? {
        Some(Value::Number(n)) => Ok(n.as_u64().unwrap_or(0) as u8),
        Some(Value::Bool(b)) => Ok(if b { 1 } else { 0 }),
        _ => Ok(0),
    }
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
    #[serde(default, deserialize_with = "null_to_default")]
    vmid: u64,
    #[serde(default, deserialize_with = "null_to_default")]
    name: String,
    #[serde(default, deserialize_with = "null_to_default")]
    node: String,
    #[serde(default, deserialize_with = "null_to_default")]
    status: String,
    #[serde(default, deserialize_with = "lenient_u8")]
    template: u8,
    #[serde(default)]
    tags: Option<String>,
    #[serde(default)]
    ip: Option<String>,
    #[serde(default)]
    maxcpu: Option<u64>,
    #[serde(default)]
    maxmem: Option<u64>,
}

#[derive(Deserialize, Default)]
struct VmConfig {
    #[serde(default, deserialize_with = "lenient_string")]
    agent: Option<String>,
    /// Catch-all for dynamic fields like ipconfig0-9, net0-9.
    #[serde(flatten)]
    extra: HashMap<String, Value>,
}

// Guest agent response is double-wrapped: {"data": {"result": [...]}}
// data or result may be null when the agent is starting up or unavailable.
#[derive(Deserialize)]
struct GuestAgentNetworkResponse {
    #[serde(default, deserialize_with = "null_to_default")]
    data: GuestAgentResult,
}

#[derive(Deserialize, Default)]
struct GuestAgentResult {
    #[serde(default, deserialize_with = "null_to_default")]
    result: Vec<GuestInterface>,
}

#[derive(Deserialize)]
struct GuestInterface {
    #[serde(default, deserialize_with = "null_to_default")]
    name: String,
    #[serde(default, deserialize_with = "null_to_default", rename = "ip-addresses")]
    ip_addresses: Vec<GuestIpAddress>,
}

#[derive(Deserialize)]
struct GuestIpAddress {
    #[serde(default, deserialize_with = "null_to_default", rename = "ip-address")]
    ip_address: String,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        rename = "ip-address-type"
    )]
    ip_address_type: String,
}

// Guest agent OS info response: {"data": {"result": {"pretty-name": "..."}}}
#[derive(Debug, Deserialize, Default)]
struct GuestOsInfoResult {
    #[serde(default, rename = "pretty-name")]
    pretty_name: String,
}

#[derive(Debug, Deserialize)]
struct GuestOsInfoData {
    #[serde(default, deserialize_with = "null_to_default")]
    result: GuestOsInfoResult,
}

// LXC container interfaces from /lxc/{vmid}/interfaces
#[derive(Deserialize, Default)]
struct LxcInterface {
    #[serde(default, deserialize_with = "null_to_default")]
    name: String,
    // Legacy PVE format: inet/inet6 CIDR strings
    #[serde(default)]
    inet: Option<String>,
    #[serde(default)]
    inet6: Option<String>,
    // Newer PVE format: same ip-addresses array shape as QEMU guest agent
    #[serde(default, deserialize_with = "null_to_default", rename = "ip-addresses")]
    ip_addresses: Vec<GuestIpAddress>,
}

/// Outcome of resolving an IP for a single VM/container.
#[derive(Debug)]
enum ResolveOutcome {
    /// Successfully resolved an IP address (ip, optional ostype).
    Resolved(String, Option<String>),
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

/// Map QEMU ostype codes to human-readable names.
/// Values and descriptions from qm.conf(5): l24, l26, other, solaris,
/// w2k, w2k3, w2k8, win7, win8, win10, win11, wvista, wxp.
fn map_qemu_ostype(ostype: &str) -> &str {
    match ostype {
        "l26" => "Linux 2.6-6.x",
        "l24" => "Linux 2.4",
        "win11" => "Windows 11/2022/2025",
        "win10" => "Windows 10/2016/2019",
        "win8" => "Windows 8/2012/2012r2",
        "win7" => "Windows 7",
        "wvista" => "Windows Vista",
        "w2k8" => "Windows Server 2008",
        "w2k3" => "Windows Server 2003",
        "wxp" => "Windows XP",
        "w2k" => "Windows 2000",
        "solaris" => "Solaris",
        "other" => "Other",
        other => other,
    }
}

/// Extract ostype from a VmConfig's extra fields.
fn extract_ostype(config: &VmConfig) -> Option<String> {
    config
        .extra
        .get("ostype")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Try to get the real OS name from the QEMU guest agent.
/// Returns the pretty-name (e.g. "Debian GNU/Linux 13 (trixie)") or None.
fn fetch_guest_os_info(
    agent: &ureq::Agent,
    base: &str,
    auth: &str,
    node: &str,
    vmid: u64,
) -> Option<String> {
    let url = format!(
        "{}/api2/json/nodes/{}/qemu/{}/agent/get-osinfo",
        base, node, vmid
    );
    let mut resp = match agent.get(&url).header("Authorization", auth).call() {
        Ok(r) => r,
        Err(e) => {
            debug!("[external] Proxmox guest OS info fetch failed for {url}: {e}");
            return None;
        }
    };
    let info: PveResponse<GuestOsInfoData> = match resp.body_mut().read_json() {
        Ok(i) => i,
        Err(e) => {
            debug!("[external] Proxmox guest OS info parse failed: {e}");
            return None;
        }
    };
    let name = info.data.result.pretty_name;
    if name.is_empty() { None } else { Some(name) }
}

/// Format CPU/memory as a compact plan string (e.g. "2c/4GiB").
fn format_plan(maxcpu: Option<u64>, maxmem: Option<u64>) -> Option<String> {
    let format_mem = |mem: u64| -> String {
        let gib = mem / 1_073_741_824;
        if gib > 0 {
            format!("{}GiB", gib)
        } else {
            let mib = mem / 1_048_576;
            format!("{}MiB", mib)
        }
    };
    match (maxcpu, maxmem) {
        (Some(cpu), Some(mem)) if cpu > 0 && mem > 0 => {
            Some(format!("{}c/{}", cpu, format_mem(mem)))
        }
        (Some(cpu), _) if cpu > 0 => Some(format!("{}c", cpu)),
        (_, Some(mem)) if mem > 0 => Some(format_mem(mem)),
        _ => None,
    }
}

/// Build the PVE auth header value. Prepends "PVEAPIToken=" if not already present.
fn auth_header(token: &str) -> String {
    if token.starts_with("PVEAPIToken=") {
        token.to_string()
    } else {
        format!("PVEAPIToken={}", token)
    }
}

/// Normalize base URL: trim whitespace, strip trailing slash and /api2/json suffix.
fn normalize_url(url: &str) -> String {
    let mut u = url.trim().trim_end_matches('/').to_string();
    if u.ends_with("/api2/json") {
        u.truncate(u.len() - "/api2/json".len());
    }
    u
}

/// Returns true if the IP is a loopback or link-local address that should not be
/// used as an SSH hostname.
fn is_unusable_ip(ip: &str) -> bool {
    if ip.is_empty() {
        return true;
    }
    // IPv4 loopback (127.0.0.0/8) and link-local (169.254.0.0/16)
    if ip.starts_with("127.") || ip.starts_with("169.254.") {
        return true;
    }
    // IPv6 loopback and link-local
    let ip_lc = ip.to_ascii_lowercase();
    ip_lc == "::1" || ip_lc.starts_with("fe80:") || ip_lc.starts_with("fe80%")
}

/// Parse a static IP from ipconfig0 value like "ip=10.0.0.1/24,gw=10.0.0.1".
/// Prefers IPv4 (ip=). Falls back to IPv6 (ip6=) if ip= is dhcp or absent.
/// Returns None if both are dhcp/auto or absent.
fn parse_ipconfig_ip(ipconfig: &str) -> Option<String> {
    let mut ipv6_candidate = None;
    for part in ipconfig.split(',') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("ip=") {
            if value.is_empty()
                || value.eq_ignore_ascii_case("dhcp")
                || value.eq_ignore_ascii_case("manual")
            {
                continue;
            }
            return Some(super::strip_cidr(value).to_string());
        }
        if let Some(value) = part.strip_prefix("ip6=") {
            if value.is_empty()
                || value.eq_ignore_ascii_case("dhcp")
                || value.eq_ignore_ascii_case("auto")
                || value.eq_ignore_ascii_case("manual")
            {
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
            if value.is_empty()
                || value.eq_ignore_ascii_case("dhcp")
                || value.eq_ignore_ascii_case("manual")
            {
                continue;
            }
            return Some(super::strip_cidr(value).to_string());
        }
        if let Some(value) = part.strip_prefix("ip6=") {
            if value.is_empty()
                || value.eq_ignore_ascii_case("dhcp")
                || value.eq_ignore_ascii_case("auto")
                || value.eq_ignore_ascii_case("manual")
            {
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

/// Parse PVE tags string. PVE uses semicolons (PVE 7), commas (PVE 8) or spaces
/// as tag separators. Split on all three for compatibility.
fn parse_pve_tags(tags: Option<&str>) -> Vec<String> {
    let s = match tags {
        Some(s) if !s.is_empty() => s,
        _ => return Vec::new(),
    };
    s.split([';', ',', ' '])
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
            if !ip.is_empty()
                && !ip.starts_with("169.254.")
                && !ip.starts_with("127.")
                && ipv4_candidate.is_none()
            {
                ipv4_candidate = Some(ip.to_string());
            }
        }
        if let Some(ref inet6) = iface.inet6 {
            let ip = super::strip_cidr(inet6.split_whitespace().next().unwrap_or(inet6));
            let ip_lc = ip.to_ascii_lowercase();
            if !ip.is_empty()
                && !ip_lc.starts_with("fe80:")
                && !ip_lc.starts_with("fe80%")
                && ip_lc != "::1"
                && ipv6_candidate.is_none()
            {
                ipv6_candidate = Some(ip.to_string());
            }
        }
        // Newer format: ip-addresses array.
        // LXC uses "inet"/"inet6" for ip-address-type (unlike QEMU guest agent
        // which uses "ipv4"/"ipv6"), so we accept both variants.
        for addr in &iface.ip_addresses {
            let ip = super::strip_cidr(&addr.ip_address);
            if ip.is_empty() {
                continue;
            }
            let t = addr.ip_address_type.as_str();
            if t == "ipv4" || t == "inet" {
                if ip.starts_with("169.254.") || ip.starts_with("127.") {
                    continue;
                }
                if ipv4_candidate.is_none() {
                    ipv4_candidate = Some(ip.to_string());
                }
            } else if t == "ipv6" || t == "inet6" {
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
            return Err(ProviderError::Http(
                "No Proxmox URL configured.".to_string(),
            ));
        }
        if !base.to_ascii_lowercase().starts_with("https://") {
            return Err(ProviderError::Http(
                "Proxmox URL must use HTTPS. Update the URL in ~/.purple/providers.".to_string(),
            ));
        }

        let agent = self.make_agent()?;
        let auth = auth_header(token);

        // Phase 1: Fetch VM/container resources (type=vm returns both qemu and lxc)
        progress("Fetching resources...");
        let url = format!("{}/api2/json/cluster/resources?type=vm", base);
        let resp: PveResponse<Vec<ClusterResource>> = agent
            .get(&url)
            .header("Authorization", &auth)
            .call()
            .map_err(map_ureq_error)?
            .body_mut()
            .read_json()
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
            let cluster_ip = resource
                .ip
                .as_deref()
                .map(|ip| super::strip_cidr(ip).to_string())
                .filter(|ip| !is_unusable_ip(ip));
            let outcome = if let Some(ip) = cluster_ip {
                // Cluster IP available; still fetch config for ostype
                let ostype = self.fetch_ostype(&agent, &base, &auth, resource);
                ResolveOutcome::Resolved(ip, ostype)
            } else if resource.resource_type == "qemu" {
                self.resolve_qemu_ip(&agent, &base, &auth, resource)
            } else {
                self.resolve_lxc_ip(&agent, &base, &auth, resource)
            };

            let (ip, ostype) = match outcome {
                ResolveOutcome::Resolved(ip, ostype) => {
                    resolved_count += 1;
                    (ip, ostype)
                }
                ResolveOutcome::Stopped => {
                    // Include stopped VMs with empty IP so they stay in
                    // remote_ids and don't get marked stale. The sync engine
                    // skips config updates for empty IPs.
                    skipped_stopped += 1;
                    (String::new(), None)
                }
                ResolveOutcome::NoIp => {
                    skipped_no_ip += 1;
                    (String::new(), None)
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

            // Build tags from PVE tags (resource type is already in metadata)
            let mut tags = parse_pve_tags(resource.tags.as_deref());
            tags.sort();
            tags.dedup();

            let mut metadata = Vec::new();
            if !resource.node.is_empty() {
                metadata.push(("node".to_string(), resource.node.clone()));
            }
            if !resource.resource_type.is_empty() {
                metadata.push(("type".to_string(), resource.resource_type.clone()));
            }
            if let Some(plan) = format_plan(resource.maxcpu, resource.maxmem) {
                metadata.push(("specs".to_string(), plan));
            }
            if let Some(os) = ostype {
                let label = if resource.resource_type == "qemu" {
                    map_qemu_ostype(&os).to_string()
                } else {
                    os
                };
                metadata.push(("os".to_string(), label));
            }
            if !resource.status.is_empty() {
                metadata.push(("status".to_string(), resource.status.clone()));
            }

            hosts.push(ProviderHost {
                server_id: format!("{}:{}", resource.resource_type, resource.vmid),
                name: if resource.name.is_empty() {
                    format!("{}-{}", resource.resource_type, resource.vmid)
                } else {
                    resource.name.clone()
                },
                ip,
                tags,
                metadata,
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
                format!(
                    "{} failed ({} authentication)",
                    fetch_failures, auth_failures
                )
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
    /// Fetch ostype for a VM/container that already has an IP from the cluster API.
    /// Tries guest agent get-osinfo for QEMU VMs, falls back to config ostype.
    fn fetch_ostype(
        &self,
        agent: &ureq::Agent,
        base: &str,
        auth: &str,
        resource: &ClusterResource,
    ) -> Option<String> {
        let api_type = if resource.resource_type == "qemu" {
            "qemu"
        } else {
            "lxc"
        };
        let config_url = format!(
            "{}/api2/json/nodes/{}/{}/{}/config",
            base, resource.node, api_type, resource.vmid
        );
        let config: VmConfig = match agent.get(&config_url).header("Authorization", auth).call() {
            Ok(mut resp) => match resp.body_mut().read_json::<PveResponse<VmConfig>>() {
                Ok(r) => r.data,
                Err(e) => {
                    debug!("[external] Proxmox VM config parse failed for {config_url}: {e}");
                    return None;
                }
            },
            Err(e) => {
                debug!("[external] Proxmox VM config fetch failed for {config_url}: {e}");
                return None;
            }
        };

        // For running QEMU VMs with guest agent, try get-osinfo first
        if resource.resource_type == "qemu"
            && resource.status == "running"
            && is_agent_enabled(config.agent.as_deref())
        {
            if let Some(os) = fetch_guest_os_info(agent, base, auth, &resource.node, resource.vmid)
            {
                return Some(os);
            }
        }

        extract_ostype(&config)
    }

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
        let config: VmConfig = match agent.get(&config_url).header("Authorization", auth).call() {
            Ok(mut resp) => match resp.body_mut().read_json::<PveResponse<VmConfig>>() {
                Ok(r) => r.data,
                Err(_) => return ResolveOutcome::Failed,
            },
            Err(ureq::Error::StatusCode(401 | 403)) => {
                return ResolveOutcome::AuthFailed;
            }
            Err(_) => return ResolveOutcome::Failed,
        };

        let ostype = extract_ostype(&config);

        // Try guest agent OS info for a better OS label
        let ostype = if resource.status == "running" && is_agent_enabled(config.agent.as_deref()) {
            fetch_guest_os_info(agent, base, auth, &resource.node, resource.vmid).or(ostype)
        } else {
            ostype
        };

        // Try static IP from ipconfig0..9
        for ipconfig in extract_numbered_values(&config.extra, "ipconfig") {
            if let Some(ip) = parse_ipconfig_ip(&ipconfig) {
                return ResolveOutcome::Resolved(ip, ostype);
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
        match agent.get(&agent_url).header("Authorization", auth).call() {
            Ok(mut resp) => match resp.body_mut().read_json::<GuestAgentNetworkResponse>() {
                Ok(ga) => match select_guest_agent_ip(&ga.data.result) {
                    Some(ip) => ResolveOutcome::Resolved(ip, ostype),
                    None => ResolveOutcome::NoIp,
                },
                Err(_) => ResolveOutcome::Failed,
            },
            Err(ureq::Error::StatusCode(500 | 501)) => {
                // Agent not responding or not supported
                ResolveOutcome::NoIp
            }
            Err(ureq::Error::StatusCode(401 | 403)) => ResolveOutcome::AuthFailed,
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
        let config: VmConfig = match agent.get(&config_url).header("Authorization", auth).call() {
            Ok(mut resp) => match resp.body_mut().read_json::<PveResponse<VmConfig>>() {
                Ok(r) => r.data,
                Err(_) => return ResolveOutcome::Failed,
            },
            Err(ureq::Error::StatusCode(401 | 403)) => {
                return ResolveOutcome::AuthFailed;
            }
            Err(_) => return ResolveOutcome::Failed,
        };

        let ostype = extract_ostype(&config);

        // Try static IP from net0..9
        for net in extract_numbered_values(&config.extra, "net") {
            if let Some(ip) = parse_lxc_net_ip(&net) {
                return ResolveOutcome::Resolved(ip, ostype);
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
        match agent.get(&iface_url).header("Authorization", auth).call() {
            Ok(mut resp) => match resp
                .body_mut()
                .read_json::<PveResponse<Vec<LxcInterface>>>()
            {
                Ok(r) => match select_lxc_interface_ip(&r.data) {
                    Some(ip) => ResolveOutcome::Resolved(ip, ostype),
                    None => ResolveOutcome::NoIp,
                },
                Err(_) => ResolveOutcome::Failed,
            },
            Err(ureq::Error::StatusCode(401 | 403)) => ResolveOutcome::AuthFailed,
            Err(ureq::Error::StatusCode(500 | 404 | 501)) => {
                // 500: container restarting or PVE hiccup
                // 404/501: endpoint may not exist on older PVE
                ResolveOutcome::NoIp
            }
            Err(_) => ResolveOutcome::Failed,
        }
    }
}

#[cfg(test)]
#[path = "proxmox_tests.rs"]
mod tests;
