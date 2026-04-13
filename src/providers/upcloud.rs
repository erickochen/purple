use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct UpCloud;

#[derive(Deserialize)]
struct ServerListResponse {
    servers: ServerListWrapper,
}

#[derive(Deserialize)]
struct ServerListWrapper {
    server: Vec<ServerSummary>,
}

#[derive(Deserialize)]
struct ServerSummary {
    uuid: String,
    title: String,
    hostname: String,
    #[serde(default)]
    tags: TagWrapper,
    #[serde(default)]
    labels: LabelWrapper,
    #[serde(default)]
    zone: String,
    #[serde(default)]
    plan: String,
    #[serde(default)]
    state: String,
}

#[derive(Deserialize, Default)]
struct TagWrapper {
    #[serde(default)]
    tag: Vec<String>,
}

#[derive(Deserialize, Default)]
struct LabelWrapper {
    #[serde(default)]
    label: Vec<Label>,
}

#[derive(Deserialize)]
struct Label {
    key: String,
    value: String,
}

#[derive(Deserialize)]
struct ServerDetailResponse {
    server: ServerDetail,
}

#[derive(Deserialize)]
struct ServerDetail {
    #[serde(default)]
    networking: Networking,
    #[serde(default)]
    storage_devices: Option<StorageDevices>,
}

#[derive(Deserialize)]
struct StorageDevices {
    #[serde(default)]
    storage_device: Vec<StorageDevice>,
}

#[derive(Deserialize)]
struct StorageDevice {
    #[serde(default)]
    storage_title: String,
    /// "1" for boot disk, "0" otherwise.
    #[serde(default)]
    boot_disk: String,
}

#[derive(Deserialize, Default)]
struct Networking {
    #[serde(default)]
    interfaces: InterfacesWrapper,
}

#[derive(Deserialize, Default)]
struct InterfacesWrapper {
    #[serde(default)]
    interface: Vec<NetworkInterface>,
}

#[derive(Deserialize)]
struct NetworkInterface {
    #[serde(default)]
    ip_addresses: IpAddressesWrapper,
    #[serde(rename = "type")]
    iface_type: String,
}

#[derive(Deserialize, Default)]
struct IpAddressesWrapper {
    #[serde(default)]
    ip_address: Vec<IpAddress>,
}

#[derive(Deserialize)]
struct IpAddress {
    address: String,
    family: String,
}

/// Collect all IP addresses from networking interfaces, filtered by interface type.
fn collect_ips<'a>(interfaces: &'a [NetworkInterface], iface_type: &str) -> Vec<&'a IpAddress> {
    interfaces
        .iter()
        .filter(|iface| iface.iface_type == iface_type)
        .flat_map(|iface| &iface.ip_addresses.ip_address)
        .collect()
}

/// Select the best public IP address from networking interfaces.
/// Priority: public IPv4 > public IPv6. Skips utility/private interfaces.
/// Filters out placeholder IPs (0.0.0.0, ::) from provisioning servers.
fn select_ip(interfaces: &[NetworkInterface]) -> Option<String> {
    let public_ips = collect_ips(interfaces, "public");
    // Public IPv4 (skip placeholder)
    if let Some(ip) = public_ips
        .iter()
        .find(|a| a.family == "IPv4" && a.address != "0.0.0.0")
    {
        return Some(ip.address.clone());
    }
    // Public IPv6 (skip placeholder)
    public_ips
        .iter()
        .find(|a| a.family == "IPv6" && a.address != "::")
        .map(|ip| ip.address.clone())
}

impl Provider for UpCloud {
    fn name(&self) -> &str {
        "upcloud"
    }

    fn short_label(&self) -> &str {
        "uc"
    }

    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        let mut all_servers: Vec<ServerSummary> = Vec::new();
        let limit = 100;
        let mut offset = 0u64;
        let agent = super::http_agent();
        let mut pages = 0u64;

        // Phase 1: Paginate server list
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            let url = format!(
                "https://api.upcloud.com/1.3/server?limit={}&offset={}",
                limit, offset
            );
            let resp: ServerListResponse = agent
                .get(&url)
                .header("Authorization", &format!("Bearer {}", token))
                .call()
                .map_err(map_ureq_error)?
                .body_mut()
                .read_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            let count = resp.servers.server.len();
            all_servers.extend(resp.servers.server);

            if count < limit {
                break;
            }
            offset += limit as u64;
            pages += 1;
            if pages >= 500 {
                break;
            }
        }

        // Phase 2: Fetch detail for each server to get IPs via networking.interfaces.
        // Auth/rate-limit errors abort immediately. Other per-server failures are counted
        // and reported as an error to prevent --remove acting on incomplete data.
        let mut all_hosts = Vec::new();
        let mut fetch_failures = 0usize;
        for server in &all_servers {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            let url = format!("https://api.upcloud.com/1.3/server/{}", server.uuid);
            let detail: ServerDetailResponse = match agent
                .get(&url)
                .header("Authorization", &format!("Bearer {}", token))
                .call()
            {
                Ok(mut resp) => match resp.body_mut().read_json() {
                    Ok(d) => d,
                    Err(_) => {
                        fetch_failures += 1;
                        continue;
                    }
                },
                Err(ureq::Error::StatusCode(401 | 403)) => {
                    return Err(ProviderError::AuthFailed);
                }
                Err(ureq::Error::StatusCode(429)) => {
                    return Err(ProviderError::RateLimited);
                }
                Err(_) => {
                    fetch_failures += 1;
                    continue;
                }
            };

            let ip = match select_ip(&detail.server.networking.interfaces.interface) {
                Some(ip) => super::strip_cidr(&ip).to_string(),
                None => continue,
            };

            // Server name: title if non-empty, otherwise hostname
            let name = if server.title.is_empty() {
                server.hostname.clone()
            } else {
                server.title.clone()
            };

            // Tags: UpCloud tags (lowercased) + labels as key=value, sorted
            let mut tags: Vec<String> = server.tags.tag.iter().map(|t| t.to_lowercase()).collect();
            for label in &server.labels.label {
                if label.value.is_empty() {
                    tags.push(label.key.clone());
                } else {
                    tags.push(format!("{}={}", label.key, label.value));
                }
            }
            tags.sort();

            let mut metadata = Vec::new();
            if !server.zone.is_empty() {
                metadata.push(("zone".to_string(), server.zone.clone()));
            }
            if !server.plan.is_empty() {
                metadata.push(("plan".to_string(), server.plan.clone()));
            }
            if let Some(ref sd) = detail.server.storage_devices {
                // Prefer boot disk, fall back to first device
                let boot = sd
                    .storage_device
                    .iter()
                    .find(|d| d.boot_disk == "1")
                    .or_else(|| sd.storage_device.first());
                if let Some(disk) = boot {
                    if !disk.storage_title.is_empty() {
                        metadata.push(("image".to_string(), disk.storage_title.clone()));
                    }
                }
            }
            if !server.state.is_empty() {
                metadata.push(("status".to_string(), server.state.clone()));
            }
            all_hosts.push(ProviderHost {
                server_id: server.uuid.clone(),
                name,
                ip,
                tags,
                metadata,
            });
        }

        if fetch_failures > 0 {
            let total = all_servers.len();
            if all_hosts.is_empty() {
                return Err(ProviderError::Http(format!(
                    "Failed to fetch details for all {} servers",
                    total
                )));
            }
            return Err(ProviderError::PartialResult {
                hosts: all_hosts,
                failures: fetch_failures,
                total,
            });
        }

        Ok(all_hosts)
    }
}

#[cfg(test)]
#[path = "upcloud_tests.rs"]
mod tests;
