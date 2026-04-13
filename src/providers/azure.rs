use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct Azure {
    pub subscriptions: Vec<String>,
}

// --- VM response models ---

#[derive(Deserialize)]
#[cfg_attr(not(test), allow(dead_code))]
struct VmListResponse {
    #[serde(default)]
    value: Vec<VirtualMachine>,
    #[serde(rename = "nextLink")]
    next_link: Option<String>,
}

#[derive(Deserialize)]
struct VirtualMachine {
    name: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    tags: Option<HashMap<String, String>>,
    #[serde(default)]
    properties: VmProperties,
}

#[derive(Deserialize, Default)]
struct VmProperties {
    #[serde(rename = "vmId", default)]
    vm_id: String,
    #[serde(rename = "hardwareProfile")]
    hardware_profile: Option<HardwareProfile>,
    #[serde(rename = "storageProfile")]
    storage_profile: Option<StorageProfile>,
    #[serde(rename = "networkProfile")]
    network_profile: Option<NetworkProfile>,
    #[serde(rename = "instanceView")]
    instance_view: Option<InstanceView>,
}

#[derive(Deserialize)]
struct HardwareProfile {
    #[serde(rename = "vmSize")]
    vm_size: String,
}

#[derive(Deserialize)]
struct StorageProfile {
    #[serde(rename = "imageReference")]
    image_reference: Option<ImageReference>,
}

#[derive(Deserialize)]
struct ImageReference {
    offer: Option<String>,
    sku: Option<String>,
    #[allow(dead_code)]
    id: Option<String>,
}

#[derive(Deserialize)]
struct NetworkProfile {
    #[serde(rename = "networkInterfaces", default)]
    network_interfaces: Vec<NetworkInterfaceRef>,
}

#[derive(Deserialize)]
struct NetworkInterfaceRef {
    id: String,
    properties: Option<NicRefProperties>,
}

#[derive(Deserialize)]
struct NicRefProperties {
    primary: Option<bool>,
}

#[derive(Deserialize)]
struct InstanceView {
    #[serde(default)]
    statuses: Vec<InstanceViewStatus>,
}

#[derive(Deserialize)]
struct InstanceViewStatus {
    code: String,
}

// --- NIC response models ---

#[derive(Deserialize)]
#[cfg_attr(not(test), allow(dead_code))]
struct NicListResponse {
    #[serde(default)]
    value: Vec<Nic>,
    #[serde(rename = "nextLink")]
    #[allow(dead_code)]
    next_link: Option<String>,
}

#[derive(Deserialize)]
struct Nic {
    id: String,
    #[serde(default)]
    properties: NicProperties,
}

#[derive(Deserialize, Default)]
struct NicProperties {
    #[serde(rename = "ipConfigurations", default)]
    ip_configurations: Vec<IpConfiguration>,
}

#[derive(Deserialize)]
struct IpConfiguration {
    #[serde(default)]
    properties: IpConfigProperties,
}

#[derive(Deserialize, Default)]
struct IpConfigProperties {
    #[serde(rename = "privateIPAddress")]
    private_ip_address: Option<String>,
    #[serde(rename = "publicIPAddress")]
    public_ip_address: Option<PublicIpRef>,
    primary: Option<bool>,
}

#[derive(Deserialize)]
struct PublicIpRef {
    id: String,
}

// --- Public IP response models ---

#[derive(Deserialize)]
#[cfg_attr(not(test), allow(dead_code))]
struct PublicIpListResponse {
    #[serde(default)]
    value: Vec<PublicIp>,
    #[serde(rename = "nextLink")]
    #[allow(dead_code)]
    next_link: Option<String>,
}

#[derive(Deserialize)]
struct PublicIp {
    id: String,
    #[serde(default)]
    properties: PublicIpProperties,
}

#[derive(Deserialize, Default)]
struct PublicIpProperties {
    #[serde(rename = "ipAddress")]
    ip_address: Option<String>,
}

// --- Auth models ---

/// Service principal credentials. Supports two JSON formats:
/// - Azure CLI output (`az ad sp create-for-rbac`): `appId`, `password`, `tenant`
/// - Manual/portal format: `clientId`, `clientSecret`, `tenantId`
#[derive(Deserialize)]
struct ServicePrincipal {
    #[serde(alias = "tenantId", alias = "tenant")]
    tenant_id: String,
    #[serde(alias = "clientId", alias = "appId")]
    client_id: String,
    #[serde(alias = "clientSecret", alias = "password")]
    client_secret: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

/// Validate that a subscription ID is a valid UUID (8-4-4-4-12 hex chars).
pub fn is_valid_subscription_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected_lens = [8, 4, 4, 4, 12];
    parts
        .iter()
        .zip(expected_lens.iter())
        .all(|(part, &len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Detect whether a token string is a path to a service principal JSON file.
fn is_sp_file(token: &str) -> bool {
    token.to_ascii_lowercase().ends_with(".json")
}

/// Exchange service principal credentials for an access token.
fn resolve_sp_token(path: &str) -> Result<String, ProviderError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ProviderError::Http(format!("Failed to read SP file {}: {}", path, e)))?;
    let sp: ServicePrincipal = serde_json::from_str(&content)
        .map_err(|e| ProviderError::Http(format!(
            "Failed to parse SP file: {}. Expected JSON with appId/password/tenant (az CLI) or clientId/clientSecret/tenantId.", e
        )))?;

    let agent = super::http_agent();
    let url = format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
        sp.tenant_id
    );
    let mut resp = agent
        .post(&url)
        .send_form([
            ("grant_type", "client_credentials"),
            ("client_id", sp.client_id.as_str()),
            ("client_secret", sp.client_secret.as_str()),
            ("scope", "https://management.azure.com/.default"),
        ])
        .map_err(map_ureq_error)?;

    let token_resp: TokenResponse = resp
        .body_mut()
        .read_json()
        .map_err(|e| ProviderError::Parse(format!("Token response: {}", e)))?;

    Ok(token_resp.access_token)
}

/// Resolve token: if it's a path to a SP JSON file, exchange it for an access token.
/// Otherwise, use it as a raw access token. Strips "Bearer " prefix if present.
fn resolve_token(token: &str) -> Result<String, ProviderError> {
    if is_sp_file(token) {
        resolve_sp_token(token)
    } else {
        let t = token.strip_prefix("Bearer ").unwrap_or(token);
        if t.is_empty() {
            return Err(ProviderError::AuthFailed);
        }
        Ok(t.to_string())
    }
}

/// Select the best IP for a VM by looking up its primary NIC and IP configuration.
/// Priority: public IP > private IP > None.
fn select_ip(
    vm: &VirtualMachine,
    nic_map: &HashMap<String, &Nic>,
    public_ip_map: &HashMap<String, String>,
) -> Option<String> {
    let net_profile = vm.properties.network_profile.as_ref()?;
    if net_profile.network_interfaces.is_empty() {
        return None;
    }

    // Find primary NIC, fallback to first
    let nic_ref = net_profile
        .network_interfaces
        .iter()
        .find(|n| {
            n.properties
                .as_ref()
                .and_then(|p| p.primary)
                .unwrap_or(false)
        })
        .or_else(|| net_profile.network_interfaces.first())?;

    let nic_id_lower = nic_ref.id.to_ascii_lowercase();
    let nic = nic_map.get(&nic_id_lower)?;

    // Find primary IP config, fallback to first
    let ip_config = nic
        .properties
        .ip_configurations
        .iter()
        .find(|c| c.properties.primary.unwrap_or(false))
        .or_else(|| nic.properties.ip_configurations.first())?;

    // Try public IP first
    if let Some(ref pub_ref) = ip_config.properties.public_ip_address {
        let pub_id_lower = pub_ref.id.to_ascii_lowercase();
        if let Some(addr) = public_ip_map.get(&pub_id_lower) {
            if !addr.is_empty() {
                return Some(addr.clone());
            }
        }
    }

    // Fallback to private IP
    if let Some(ref private) = ip_config.properties.private_ip_address {
        if !private.is_empty() {
            return Some(private.clone());
        }
    }

    None
}

/// Extract power state from instanceView statuses.
fn extract_power_state(instance_view: &Option<InstanceView>) -> Option<String> {
    let iv = instance_view.as_ref()?;
    for status in &iv.statuses {
        if let Some(suffix) = status.code.strip_prefix("PowerState/") {
            return Some(suffix.to_string());
        }
    }
    None
}

/// Build OS string from image reference: "{offer}-{sku}".
fn build_os_string(image_ref: &Option<ImageReference>) -> Option<String> {
    let img = image_ref.as_ref()?;
    let offer = img.offer.as_deref()?;
    let sku = img.sku.as_deref()?;
    if offer.is_empty() || sku.is_empty() {
        return None;
    }
    Some(format!("{}-{}", offer, sku))
}

/// Build metadata key-value pairs for a VM.
fn build_metadata(vm: &VirtualMachine) -> Vec<(String, String)> {
    let mut metadata = Vec::new();
    if !vm.location.is_empty() {
        metadata.push(("region".to_string(), vm.location.to_ascii_lowercase()));
    }
    if let Some(ref hw) = vm.properties.hardware_profile {
        if !hw.vm_size.is_empty() {
            metadata.push(("vm_size".to_string(), hw.vm_size.clone()));
        }
    }
    if let Some(ref sp) = vm.properties.storage_profile {
        if let Some(os) = build_os_string(&sp.image_reference) {
            metadata.push(("image".to_string(), os));
        }
    }
    if let Some(state) = extract_power_state(&vm.properties.instance_view) {
        metadata.push(("status".to_string(), state));
    }
    metadata
}

/// Build tags from Azure VM tags (key:value map).
fn build_tags(vm: &VirtualMachine) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some(ref vm_tags) = vm.tags {
        for (k, v) in vm_tags {
            if v.is_empty() {
                tags.push(k.clone());
            } else {
                tags.push(format!("{}:{}", k, v));
            }
        }
    }
    tags
}

/// Fetch a paginated Azure API list endpoint. Returns the deserialized items.
fn fetch_paginated<T: serde::de::DeserializeOwned>(
    agent: &ureq::Agent,
    initial_url: &str,
    access_token: &str,
    cancel: &AtomicBool,
    resource_name: &str,
    progress: &dyn Fn(&str),
) -> Result<Vec<T>, ProviderError> {
    // We need to deserialize a response that has `value: Vec<T>` and `nextLink: Option<String>`.
    // Since we can't use generics with serde easily, we'll use serde_json::Value.
    let mut all_items = Vec::new();
    let mut next_url: Option<String> = Some(initial_url.to_string());

    for page in 0u32.. {
        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }
        if page > 500 {
            break;
        }

        let url = match next_url.take() {
            Some(u) => u,
            None => break,
        };

        progress(&format!(
            "Fetching {} ({} so far)...",
            resource_name,
            all_items.len()
        ));

        let mut response = match agent
            .get(&url)
            .header("Authorization", &format!("Bearer {}", access_token))
            .call()
        {
            Ok(r) => r,
            Err(e) => {
                let err = map_ureq_error(e);
                // AuthFailed and RateLimited always propagate immediately
                if matches!(err, ProviderError::AuthFailed | ProviderError::RateLimited) {
                    return Err(err);
                }
                // On later pages, return what we have so far instead of losing it
                if !all_items.is_empty() {
                    break;
                }
                return Err(err);
            }
        };

        let body: serde_json::Value = match response.body_mut().read_json() {
            Ok(v) => v,
            Err(e) => {
                if !all_items.is_empty() {
                    break;
                }
                return Err(ProviderError::Parse(format!(
                    "{} response: {}",
                    resource_name, e
                )));
            }
        };

        if let Some(value_array) = body.get("value").and_then(|v| v.as_array()) {
            for item in value_array {
                match serde_json::from_value(item.clone()) {
                    Ok(parsed) => all_items.push(parsed),
                    Err(_) => continue, // skip malformed items
                }
            }
        }

        next_url = body
            .get("nextLink")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .filter(|s| s.starts_with("https://management.azure.com/"))
            .map(|s| s.to_string());
    }

    Ok(all_items)
}

impl Provider for Azure {
    fn name(&self) -> &str {
        "azure"
    }

    fn short_label(&self) -> &str {
        "az"
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
        if self.subscriptions.is_empty() {
            return Err(ProviderError::Http(
                "No Azure subscriptions configured. Set at least one subscription ID.".to_string(),
            ));
        }

        // Validate subscription ID format (UUID: 8-4-4-4-12 hex chars)
        for sub in &self.subscriptions {
            if !is_valid_subscription_id(sub) {
                return Err(ProviderError::Http(format!(
                    "Invalid subscription ID '{}'. Expected UUID format (e.g. 12345678-1234-1234-1234-123456789012).",
                    sub
                )));
            }
        }

        progress("Authenticating...");
        let access_token = resolve_token(token)?;

        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }

        let agent = super::http_agent();
        let mut all_hosts = Vec::new();
        let mut failures = 0usize;
        let total = self.subscriptions.len();

        for (i, sub) in self.subscriptions.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            progress(&format!("Subscription {}/{} ({})...", i + 1, total, sub));

            match self.fetch_subscription(&agent, &access_token, sub, cancel, progress) {
                Ok(hosts) => all_hosts.extend(hosts),
                Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
                Err(ProviderError::AuthFailed) => return Err(ProviderError::AuthFailed),
                Err(ProviderError::RateLimited) => return Err(ProviderError::RateLimited),
                Err(_) => {
                    failures += 1;
                }
            }
        }

        if failures > 0 && !all_hosts.is_empty() {
            return Err(ProviderError::PartialResult {
                hosts: all_hosts,
                failures,
                total,
            });
        }
        if failures > 0 && all_hosts.is_empty() {
            return Err(ProviderError::Http(format!(
                "All {} subscription(s) failed.",
                total
            )));
        }

        progress(&format!("{} VMs", all_hosts.len()));
        Ok(all_hosts)
    }
}

impl Azure {
    fn fetch_subscription(
        &self,
        agent: &ureq::Agent,
        access_token: &str,
        subscription_id: &str,
        cancel: &AtomicBool,
        progress: &dyn Fn(&str),
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        // 1. Fetch all VMs (with instanceView expanded for power state)
        let vm_url = format!(
            "https://management.azure.com/subscriptions/{}/providers/Microsoft.Compute/virtualMachines?api-version=2024-07-01&$expand=instanceView",
            subscription_id
        );
        let vms: Vec<VirtualMachine> =
            fetch_paginated(agent, &vm_url, access_token, cancel, "VMs", progress)?;

        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }

        // 2. Fetch all NICs
        let nic_url = format!(
            "https://management.azure.com/subscriptions/{}/providers/Microsoft.Network/networkInterfaces?api-version=2024-05-01",
            subscription_id
        );
        let nics: Vec<Nic> =
            fetch_paginated(agent, &nic_url, access_token, cancel, "NICs", progress)?;

        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }

        // 3. Fetch all public IPs
        let pip_url = format!(
            "https://management.azure.com/subscriptions/{}/providers/Microsoft.Network/publicIPAddresses?api-version=2024-05-01",
            subscription_id
        );
        let public_ips: Vec<PublicIp> = fetch_paginated(
            agent,
            &pip_url,
            access_token,
            cancel,
            "public IPs",
            progress,
        )?;

        // Build lookup maps (case-insensitive Azure resource IDs)
        let nic_map: HashMap<String, &Nic> = nics
            .iter()
            .map(|n| (n.id.to_ascii_lowercase(), n))
            .collect();

        let public_ip_map: HashMap<String, String> = public_ips
            .iter()
            .filter_map(|p| {
                p.properties
                    .ip_address
                    .as_ref()
                    .map(|addr| (p.id.to_ascii_lowercase(), addr.clone()))
            })
            .collect();

        // 4. Join: VM -> NIC -> public IP
        let mut hosts = Vec::new();
        for vm in &vms {
            // Skip VMs with empty vm_id (would collide in sync engine)
            if vm.properties.vm_id.is_empty() {
                continue;
            }
            if let Some(ip) = select_ip(vm, &nic_map, &public_ip_map) {
                hosts.push(ProviderHost {
                    server_id: vm.properties.vm_id.clone(),
                    name: vm.name.clone(),
                    ip,
                    tags: build_tags(vm),
                    metadata: build_metadata(vm),
                });
            }
        }

        Ok(hosts)
    }
}

#[cfg(test)]
#[path = "azure_tests.rs"]
mod tests;
