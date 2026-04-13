use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct Leaseweb;

// --- Dedicated servers (bareMetals) ---

#[derive(Deserialize)]
struct BareMetalListResponse {
    servers: Vec<BareMetalServer>,
    #[serde(rename = "_metadata")]
    metadata: PaginationMeta,
}

#[derive(Deserialize)]
struct BareMetalServer {
    id: String,
    #[serde(default)]
    reference: String,
    #[serde(rename = "networkInterfaces")]
    network_interfaces: BareMetalNetworkInterfaces,
    #[serde(default)]
    contract: Option<BareMetalContract>,
    #[serde(default)]
    location: Option<BareMetalLocation>,
    #[serde(default)]
    specs: Option<BareMetalSpecs>,
}

#[derive(Deserialize)]
struct BareMetalNetworkInterfaces {
    #[serde(default)]
    public: Option<BareMetalInterface>,
    #[serde(default)]
    internal: Option<BareMetalInterface>,
}

#[derive(Deserialize)]
struct BareMetalInterface {
    #[serde(default)]
    ip: String,
}

#[derive(Deserialize)]
struct BareMetalContract {
    #[serde(default, rename = "deliveryStatus")]
    delivery_status: String,
}

#[derive(Deserialize)]
struct BareMetalLocation {
    #[serde(default)]
    site: String,
}

#[derive(Deserialize)]
struct BareMetalSpecs {
    #[serde(default)]
    cpu: Option<BareMetalCpu>,
    #[serde(default)]
    ram: Option<BareMetalRam>,
}

#[derive(Deserialize)]
struct BareMetalCpu {
    #[serde(default)]
    quantity: u32,
    #[serde(default, rename = "type")]
    cpu_type: String,
}

#[derive(Deserialize)]
struct BareMetalRam {
    #[serde(default)]
    size: u32,
    #[serde(default)]
    unit: String,
}

// --- Public cloud instances ---

#[derive(Deserialize)]
struct CloudListResponse {
    instances: Vec<CloudInstance>,
    #[serde(rename = "_metadata")]
    metadata: PaginationMeta,
}

#[derive(Deserialize)]
struct CloudInstance {
    id: String,
    #[serde(default)]
    reference: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    region: String,
    #[serde(default, rename = "type")]
    instance_type: String,
    #[serde(default)]
    ips: Vec<CloudIp>,
    #[serde(default)]
    image: Option<CloudImage>,
}

#[derive(Deserialize)]
struct CloudIp {
    ip: String,
    #[serde(default)]
    version: u8,
    #[serde(default, rename = "networkType")]
    network_type: String,
}

#[derive(Deserialize)]
struct CloudImage {
    #[serde(default)]
    name: Option<String>,
}

// --- Shared ---

#[derive(Deserialize)]
#[allow(dead_code)]
struct PaginationMeta {
    #[serde(rename = "totalCount")]
    total_count: u64,
    limit: u64,
    offset: u64,
}

/// Select best IP from public cloud instance: public IPv4 > public IPv6 > private IPv4.
fn select_cloud_ip(ips: &[CloudIp]) -> Option<String> {
    ips.iter()
        .find(|ip| ip.network_type == "PUBLIC" && ip.version == 4)
        .or_else(|| {
            ips.iter()
                .find(|ip| ip.network_type == "PUBLIC" && ip.version == 6)
        })
        .or_else(|| {
            ips.iter()
                .find(|ip| ip.network_type == "INTERNAL" && ip.version == 4)
        })
        .map(|ip| super::strip_cidr(&ip.ip).to_string())
}

fn format_baremetal_specs(specs: &BareMetalSpecs) -> String {
    let mut parts = Vec::new();
    if let Some(ref cpu) = specs.cpu {
        if cpu.quantity > 0 && !cpu.cpu_type.is_empty() {
            parts.push(format!("{}x {}", cpu.quantity, cpu.cpu_type));
        }
    }
    if let Some(ref ram) = specs.ram {
        if ram.size > 0 {
            parts.push(format!("{}{}", ram.size, ram.unit));
        }
    }
    parts.join(", ")
}

impl Provider for Leaseweb {
    fn name(&self) -> &str {
        "leaseweb"
    }

    fn short_label(&self) -> &str {
        "lsw"
    }

    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        let agent = super::http_agent();
        let mut all_hosts = Vec::new();

        // Fetch dedicated servers
        let mut offset = 0u64;
        let limit = 50u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }
            let url = format!(
                "https://api.leaseweb.com/bareMetals/v2/servers?limit={}&offset={}",
                limit, offset
            );
            let resp: BareMetalListResponse = agent
                .get(&url)
                .header("X-Lsw-Auth", token)
                .call()
                .map_err(map_ureq_error)?
                .body_mut()
                .read_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            for server in &resp.servers {
                let ip = server
                    .network_interfaces
                    .public
                    .as_ref()
                    .map(|iface| super::strip_cidr(&iface.ip).to_string())
                    .or_else(|| {
                        server
                            .network_interfaces
                            .internal
                            .as_ref()
                            .map(|iface| super::strip_cidr(&iface.ip).to_string())
                    });
                if let Some(ip) = ip {
                    if !ip.is_empty() {
                        let mut metadata = Vec::with_capacity(4);
                        if let Some(ref loc) = server.location {
                            if !loc.site.is_empty() {
                                metadata.push(("location".to_string(), loc.site.clone()));
                            }
                        }
                        if let Some(ref specs) = server.specs {
                            let spec_str = format_baremetal_specs(specs);
                            if !spec_str.is_empty() {
                                metadata.push(("specs".to_string(), spec_str));
                            }
                        }
                        if let Some(ref contract) = server.contract {
                            if !contract.delivery_status.is_empty() {
                                metadata
                                    .push(("status".to_string(), contract.delivery_status.clone()));
                            }
                        }
                        let name = if server.reference.is_empty() {
                            server.id.clone()
                        } else {
                            server.reference.clone()
                        };
                        all_hosts.push(ProviderHost {
                            server_id: format!("bm-{}", server.id),
                            name,
                            ip,
                            tags: Vec::new(),
                            metadata,
                        });
                    }
                }
            }

            if offset + limit >= resp.metadata.total_count {
                break;
            }
            offset += limit;
            // Safety guard: prevent infinite pagination loops
            if offset / limit >= 500 {
                break;
            }
        }

        // Fetch public cloud instances
        offset = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }
            let url = format!(
                "https://api.leaseweb.com/publicCloud/v1/instances?limit={}&offset={}",
                limit, offset
            );
            let resp: CloudListResponse = agent
                .get(&url)
                .header("X-Lsw-Auth", token)
                .call()
                .map_err(map_ureq_error)?
                .body_mut()
                .read_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            for instance in &resp.instances {
                if let Some(ip) = select_cloud_ip(&instance.ips) {
                    let mut metadata = Vec::with_capacity(4);
                    if !instance.region.is_empty() {
                        metadata.push(("region".to_string(), instance.region.clone()));
                    }
                    if !instance.instance_type.is_empty() {
                        metadata.push(("type".to_string(), instance.instance_type.clone()));
                    }
                    if let Some(ref image) = instance.image {
                        if let Some(ref name) = image.name {
                            if !name.is_empty() {
                                metadata.push(("image".to_string(), name.clone()));
                            }
                        }
                    }
                    if !instance.state.is_empty() {
                        metadata.push(("status".to_string(), instance.state.clone()));
                    }
                    let name = if instance.reference.is_empty() {
                        instance.id.clone()
                    } else {
                        instance.reference.clone()
                    };
                    all_hosts.push(ProviderHost {
                        server_id: format!("cloud-{}", instance.id),
                        name,
                        ip,
                        tags: Vec::new(),
                        metadata,
                    });
                }
            }

            if offset + limit >= resp.metadata.total_count {
                break;
            }
            offset += limit;
            // Safety guard: prevent infinite pagination loops
            if offset / limit >= 500 {
                break;
            }
        }

        Ok(all_hosts)
    }
}

#[cfg(test)]
#[path = "leaseweb_tests.rs"]
mod tests;
