use std::{collections::HashMap, sync::atomic::AtomicBool};

use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct NetBox {
    pub base_url: String,
    pub verify_tls: bool,
    /// Raw NetBox query parameters appended to both list endpoints.
    /// Empty means the default active-only filter.
    pub filter: String,
    /// Optional interface name whose assigned IP should be used instead of
    /// the device or VM primary IP.
    pub interface: String,
    /// Optional SSH port override applied to all synced hosts.
    pub ssh_port: Option<u16>,
}

/// v2 API tokens carry this prefix and authenticate with the Bearer scheme.
/// Classic tokens use NetBox's own Token scheme.
const V2_TOKEN_PREFIX: &str = "nbt_";

/// Filter applied when the user configured none. Keeps decommissioned and
/// planned objects out of the host list by default.
const DEFAULT_FILTER: &str = "status=active";

fn auth_header(token: &str) -> String {
    if token.starts_with(V2_TOKEN_PREFIX) {
        format!("Bearer {token}")
    } else {
        format!("Token {token}")
    }
}

/// Trim the configured filter and strip a leading `?` or `&` so it can be
/// appended to a query string that already carries limit and offset.
fn normalize_filter(filter: &str) -> String {
    let trimmed = filter.trim().trim_start_matches(['?', '&']);
    if trimmed.is_empty() {
        DEFAULT_FILTER.to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Deserialize)]
struct NetBoxListResponse {
    #[serde(default)]
    next: Option<String>,
    #[serde(default)]
    results: Vec<NetBoxObject>,
}

#[derive(Deserialize)]
struct NetBoxObject {
    id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<NetBoxStatus>,
    #[serde(default)]
    primary_ip4: Option<NetBoxIp>,
    #[serde(default)]
    primary_ip6: Option<NetBoxIp>,
    #[serde(default)]
    tags: Vec<NetBoxTag>,
    #[serde(default)]
    site: Option<NetBoxNamedRef>,
    /// `device_role` before NetBox 3.6, `role` since.
    #[serde(default, alias = "device_role")]
    role: Option<NetBoxNamedRef>,
    #[serde(default)]
    platform: Option<NetBoxNamedRef>,
    #[serde(default)]
    device_type: Option<NetBoxDeviceType>,
    #[serde(default)]
    cluster: Option<NetBoxNamedRef>,
}

#[derive(Deserialize)]
struct NetBoxIpListResponse {
    #[serde(default)]
    next: Option<String>,
    #[serde(default)]
    results: Vec<NetBoxAssignedIp>,
}

#[derive(Deserialize)]
struct NetBoxAssignedIp {
    #[serde(default)]
    address: String,
    #[serde(default)]
    assigned_object: Option<NetBoxAssignedObject>,
}

#[derive(Deserialize)]
struct NetBoxAssignedObject {
    #[serde(default)]
    name: String,
    #[serde(default)]
    device: Option<NetBoxAssignedOwner>,
    #[serde(default)]
    virtual_machine: Option<NetBoxAssignedOwner>,
}

#[derive(Deserialize)]
struct NetBoxAssignedOwner {
    id: u64,
}

#[derive(Deserialize)]
struct NetBoxStatus {
    #[serde(default)]
    value: String,
}

#[derive(Deserialize)]
struct NetBoxIp {
    #[serde(default)]
    address: String,
}

#[derive(Deserialize)]
struct NetBoxTag {
    #[serde(default)]
    slug: String,
}

#[derive(Deserialize)]
struct NetBoxNamedRef {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct NetBoxDeviceType {
    #[serde(default)]
    model: String,
}

/// Which list endpoint an object came from. Device and VM ids are
/// independent sequences in NetBox, so the server_id carries the kind
/// as a prefix to keep ownership markers collision-free.
#[derive(Clone, Copy)]
enum ObjectKind {
    Device,
    Vm,
}

fn map_object(obj: &NetBoxObject, kind: ObjectKind) -> Option<ProviderHost> {
    map_object_with_ip(obj, kind, None)
}

fn map_object_with_ip(
    obj: &NetBoxObject,
    kind: ObjectKind,
    interface_ip: Option<&str>,
) -> Option<ProviderHost> {
    let name = obj.name.as_deref().unwrap_or("").trim();
    if name.is_empty() {
        return None;
    }
    let ip = interface_ip
        .filter(|ip| !ip.is_empty())
        .map(|ip| super::strip_cidr(ip).to_string())
        .or_else(|| {
            obj.primary_ip4
                .as_ref()
                .filter(|ip| !ip.address.is_empty())
                .or_else(|| obj.primary_ip6.as_ref().filter(|ip| !ip.address.is_empty()))
                .map(|ip| super::strip_cidr(&ip.address).to_string())
        })?;

    let mut tags: Vec<String> = obj
        .tags
        .iter()
        .map(|t| t.slug.clone())
        .filter(|s| !s.is_empty())
        .collect();
    tags.sort();

    let mut metadata = super::ProviderMetadata::new();
    let named =
        |r: &Option<NetBoxNamedRef>| r.as_ref().map(|n| n.name.clone()).filter(|n| !n.is_empty());
    metadata.push_opt("location", named(&obj.site));
    metadata.push_opt("role", named(&obj.role));
    metadata.push_opt("os", named(&obj.platform));
    match kind {
        ObjectKind::Device => {
            metadata.push_opt(
                "type",
                obj.device_type
                    .as_ref()
                    .map(|d| d.model.clone())
                    .filter(|m| !m.is_empty()),
            );
        }
        ObjectKind::Vm => {
            metadata.push_opt("cluster", named(&obj.cluster));
        }
    }
    metadata.push_opt(
        "status",
        obj.status
            .as_ref()
            .map(|s| s.value.clone())
            .filter(|v| !v.is_empty()),
    );

    let prefix = match kind {
        ObjectKind::Device => "device",
        ObjectKind::Vm => "vm",
    };
    Some(ProviderHost {
        server_id: format!("{prefix}-{}", obj.id),
        name: name.to_string(),
        ip,
        tags,
        metadata: metadata.finish(),
        ..Default::default()
    })
}

/// Objects fetched per page. NetBox caps limit at 1000; 100 keeps individual
/// responses small while still covering large instances in few requests.
const PAGE_SIZE: u64 = 100;

const DEVICES_ENDPOINT: &str = "/api/dcim/devices/";
const VMS_ENDPOINT: &str = "/api/virtualization/virtual-machines/";

impl NetBox {
    fn make_agent(&self) -> Result<ureq::Agent, ProviderError> {
        if self.verify_tls {
            Ok(super::http_agent())
        } else {
            super::http_agent_insecure()
        }
    }

    /// Fetch hosts against an already-validated base URL. The trait entrypoint
    /// keeps the empty-URL and HTTPS gates; this seam holds the actual fetch so
    /// tests can drive both list endpoints against a mock server (which serves
    /// plain http).
    fn fetch_from(
        &self,
        base_url: &str,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        let agent = self.make_agent()?;
        let auth = auth_header(token);
        let base = base_url.trim_end_matches('/');
        let filter = normalize_filter(&self.filter);
        let interface = self.interface.trim();

        // Two list endpoints walked in sequence under one pagination
        // contract: physical devices first, then virtual machines.
        enum Stage {
            Devices,
            Vms,
        }
        let mut stage = Stage::Devices;
        let mut offset = 0u64;

        super::paginate(cancel, |_idx| {
            let (endpoint, kind) = match stage {
                Stage::Devices => (DEVICES_ENDPOINT, ObjectKind::Device),
                Stage::Vms => (VMS_ENDPOINT, ObjectKind::Vm),
            };
            let url = format!("{base}{endpoint}?limit={PAGE_SIZE}&offset={offset}&{filter}");
            let resp: NetBoxListResponse = agent
                .get(&url)
                .header("Authorization", &auth)
                .call()
                .map_err(map_ureq_error)?
                .body_mut()
                .read_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            let interface_ips = if interface.is_empty() {
                HashMap::new()
            } else {
                self.fetch_interface_ips(&agent, base, &auth, &resp.results, kind, interface)?
            };
            let mut hosts = Vec::new();
            for obj in &resp.results {
                let host = if interface.is_empty() {
                    map_object(obj, kind)
                } else {
                    interface_ips
                        .get(&obj.id)
                        .and_then(|ip| map_object_with_ip(obj, kind, Some(ip)))
                };
                if let Some(mut host) = host {
                    host.port = self.ssh_port;
                    hosts.push(host);
                }
            }

            let more = if resp.next.is_some() {
                offset += PAGE_SIZE;
                true
            } else {
                match stage {
                    Stage::Devices => {
                        stage = Stage::Vms;
                        offset = 0;
                        true
                    }
                    Stage::Vms => false,
                }
            };
            Ok(super::PageResult { hosts, more })
        })
    }

    fn fetch_interface_ips(
        &self,
        agent: &ureq::Agent,
        base: &str,
        auth: &str,
        objects: &[NetBoxObject],
        kind: ObjectKind,
        interface: &str,
    ) -> Result<HashMap<u64, String>, ProviderError> {
        if objects.is_empty() {
            return Ok(HashMap::new());
        }
        let owner_filter = match kind {
            ObjectKind::Device => "device_id",
            ObjectKind::Vm => "virtual_machine_id",
        };
        let owner_query = objects
            .iter()
            .map(|obj| format!("{owner_filter}={}", obj.id))
            .collect::<Vec<_>>()
            .join("&");
        let mut interface_ips = HashMap::new();
        let mut offset = 0u64;
        loop {
            let url = format!(
                "{base}/api/ipam/ip-addresses/?limit={PAGE_SIZE}&offset={offset}&{owner_query}"
            );
            let resp: NetBoxIpListResponse = agent
                .get(&url)
                .header("Authorization", auth)
                .call()
                .map_err(map_ureq_error)?
                .body_mut()
                .read_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            for ip in &resp.results {
                let Some(assigned) = ip.assigned_object.as_ref() else {
                    continue;
                };
                if assigned.name != interface || ip.address.is_empty() {
                    continue;
                }
                let object_id = match kind {
                    ObjectKind::Device => assigned.device.as_ref().map(|owner| owner.id),
                    ObjectKind::Vm => assigned.virtual_machine.as_ref().map(|owner| owner.id),
                };
                if let Some(object_id) = object_id {
                    interface_ips
                        .entry(object_id)
                        .or_insert_with(|| ip.address.clone());
                }
            }
            if resp.next.is_none() {
                return Ok(interface_ips);
            }
            offset += PAGE_SIZE;
        }
    }
}

impl Provider for NetBox {
    fn name(&self) -> &str {
        "netbox"
    }

    fn short_label(&self) -> &str {
        "nb"
    }

    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
        _env: &crate::runtime::env::Env,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        let base = self.base_url.trim();
        if base.is_empty() {
            return Err(ProviderError::Http("No NetBox URL configured.".to_string()));
        }
        if !base.to_ascii_lowercase().starts_with("https://") {
            return Err(ProviderError::Http(
                "NetBox URL must use HTTPS. Update the URL in ~/.purple/providers.".to_string(),
            ));
        }
        self.fetch_from(base, token, cancel)
    }
}

#[cfg(test)]
#[path = "netbox_tests.rs"]
mod tests;
