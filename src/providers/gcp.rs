use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost, map_ureq_error};

pub struct Gcp {
    pub zones: Vec<String>,
    pub project: String,
}

/// All GCP Compute Engine zones with display names.
/// Single source of truth. GCP_ZONE_GROUPS references slices of this array.
/// This list only affects the TUI zone picker. Unlisted zones are still synced
/// when no zone filter is configured (empty = all zones).
pub const GCP_ZONES: &[(&str, &str)] = &[
    // US Central (0..4)
    ("us-central1-a", "Iowa A"),
    ("us-central1-b", "Iowa B"),
    ("us-central1-c", "Iowa C"),
    ("us-central1-f", "Iowa F"),
    // US East (4..13)
    ("us-east1-b", "South Carolina B"),
    ("us-east1-c", "South Carolina C"),
    ("us-east1-d", "South Carolina D"),
    ("us-east4-a", "Virginia A"),
    ("us-east4-b", "Virginia B"),
    ("us-east4-c", "Virginia C"),
    ("us-east5-a", "Columbus A"),
    ("us-east5-b", "Columbus B"),
    ("us-east5-c", "Columbus C"),
    // US South (13..16)
    ("us-south1-a", "Dallas A"),
    ("us-south1-b", "Dallas B"),
    ("us-south1-c", "Dallas C"),
    // US West (16..28)
    ("us-west1-a", "Oregon A"),
    ("us-west1-b", "Oregon B"),
    ("us-west1-c", "Oregon C"),
    ("us-west2-a", "Los Angeles A"),
    ("us-west2-b", "Los Angeles B"),
    ("us-west2-c", "Los Angeles C"),
    ("us-west3-a", "Salt Lake City A"),
    ("us-west3-b", "Salt Lake City B"),
    ("us-west3-c", "Salt Lake City C"),
    ("us-west4-a", "Las Vegas A"),
    ("us-west4-b", "Las Vegas B"),
    ("us-west4-c", "Las Vegas C"),
    // North America (28..37)
    ("northamerica-northeast1-a", "Montreal A"),
    ("northamerica-northeast1-b", "Montreal B"),
    ("northamerica-northeast1-c", "Montreal C"),
    ("northamerica-northeast2-a", "Toronto A"),
    ("northamerica-northeast2-b", "Toronto B"),
    ("northamerica-northeast2-c", "Toronto C"),
    ("northamerica-south1-a", "Queretaro A"),
    ("northamerica-south1-b", "Queretaro B"),
    ("northamerica-south1-c", "Queretaro C"),
    // South America (37..43)
    ("southamerica-east1-a", "Sao Paulo A"),
    ("southamerica-east1-b", "Sao Paulo B"),
    ("southamerica-east1-c", "Sao Paulo C"),
    ("southamerica-west1-a", "Santiago A"),
    ("southamerica-west1-b", "Santiago B"),
    ("southamerica-west1-c", "Santiago C"),
    // Europe West (43..70)
    ("europe-west1-b", "Belgium B"),
    ("europe-west1-c", "Belgium C"),
    ("europe-west1-d", "Belgium D"),
    ("europe-west2-a", "London A"),
    ("europe-west2-b", "London B"),
    ("europe-west2-c", "London C"),
    ("europe-west3-a", "Frankfurt A"),
    ("europe-west3-b", "Frankfurt B"),
    ("europe-west3-c", "Frankfurt C"),
    ("europe-west4-a", "Netherlands A"),
    ("europe-west4-b", "Netherlands B"),
    ("europe-west4-c", "Netherlands C"),
    ("europe-west6-a", "Zurich A"),
    ("europe-west6-b", "Zurich B"),
    ("europe-west6-c", "Zurich C"),
    ("europe-west8-a", "Milan A"),
    ("europe-west8-b", "Milan B"),
    ("europe-west8-c", "Milan C"),
    ("europe-west9-a", "Paris A"),
    ("europe-west9-b", "Paris B"),
    ("europe-west9-c", "Paris C"),
    ("europe-west10-a", "Berlin A"),
    ("europe-west10-b", "Berlin B"),
    ("europe-west10-c", "Berlin C"),
    ("europe-west12-a", "Turin A"),
    ("europe-west12-b", "Turin B"),
    ("europe-west12-c", "Turin C"),
    // Europe Other (70..82)
    ("europe-north1-a", "Finland A"),
    ("europe-north1-b", "Finland B"),
    ("europe-north1-c", "Finland C"),
    ("europe-north2-a", "Stockholm A"),
    ("europe-north2-b", "Stockholm B"),
    ("europe-north2-c", "Stockholm C"),
    ("europe-central2-a", "Warsaw A"),
    ("europe-central2-b", "Warsaw B"),
    ("europe-central2-c", "Warsaw C"),
    ("europe-southwest1-a", "Madrid A"),
    ("europe-southwest1-b", "Madrid B"),
    ("europe-southwest1-c", "Madrid C"),
    // Asia East (82..88)
    ("asia-east1-a", "Taiwan A"),
    ("asia-east1-b", "Taiwan B"),
    ("asia-east1-c", "Taiwan C"),
    ("asia-east2-a", "Hong Kong A"),
    ("asia-east2-b", "Hong Kong B"),
    ("asia-east2-c", "Hong Kong C"),
    // Asia Northeast (88..97)
    ("asia-northeast1-a", "Tokyo A"),
    ("asia-northeast1-b", "Tokyo B"),
    ("asia-northeast1-c", "Tokyo C"),
    ("asia-northeast2-a", "Osaka A"),
    ("asia-northeast2-b", "Osaka B"),
    ("asia-northeast2-c", "Osaka C"),
    ("asia-northeast3-a", "Seoul A"),
    ("asia-northeast3-b", "Seoul B"),
    ("asia-northeast3-c", "Seoul C"),
    // Asia South (97..103)
    ("asia-south1-a", "Mumbai A"),
    ("asia-south1-b", "Mumbai B"),
    ("asia-south1-c", "Mumbai C"),
    ("asia-south2-a", "Delhi A"),
    ("asia-south2-b", "Delhi B"),
    ("asia-south2-c", "Delhi C"),
    // Asia Southeast (103..109)
    ("asia-southeast1-a", "Singapore A"),
    ("asia-southeast1-b", "Singapore B"),
    ("asia-southeast1-c", "Singapore C"),
    ("asia-southeast2-a", "Jakarta A"),
    ("asia-southeast2-b", "Jakarta B"),
    ("asia-southeast2-c", "Jakarta C"),
    // Australia (109..115)
    ("australia-southeast1-a", "Sydney A"),
    ("australia-southeast1-b", "Sydney B"),
    ("australia-southeast1-c", "Sydney C"),
    ("australia-southeast2-a", "Melbourne A"),
    ("australia-southeast2-b", "Melbourne B"),
    ("australia-southeast2-c", "Melbourne C"),
    // Middle East (115..124)
    ("me-west1-a", "Tel Aviv A"),
    ("me-west1-b", "Tel Aviv B"),
    ("me-west1-c", "Tel Aviv C"),
    ("me-central1-a", "Doha A"),
    ("me-central1-b", "Doha B"),
    ("me-central1-c", "Doha C"),
    ("me-central2-a", "Dammam A"),
    ("me-central2-b", "Dammam B"),
    ("me-central2-c", "Dammam C"),
    // Africa (124..127)
    ("africa-south1-a", "Johannesburg A"),
    ("africa-south1-b", "Johannesburg B"),
    ("africa-south1-c", "Johannesburg C"),
];

/// Zone group labels with start..end indices into GCP_ZONES.
pub const GCP_ZONE_GROUPS: &[(&str, usize, usize)] = &[
    ("US Central", 0, 4),
    ("US East", 4, 13),
    ("US South", 13, 16),
    ("US West", 16, 28),
    ("North America", 28, 37),
    ("South America", 37, 43),
    ("Europe West", 43, 70),
    ("Europe Other", 70, 82),
    ("Asia East", 82, 88),
    ("Asia Northeast", 88, 97),
    ("Asia South", 97, 103),
    ("Asia Southeast", 103, 109),
    ("Australia", 109, 115),
    ("Middle East", 115, 124),
    ("Africa", 124, 127),
];

// --- Serde response models ---

#[derive(Deserialize)]
struct AggregatedListResponse {
    #[serde(default)]
    items: std::collections::HashMap<String, InstancesScopedList>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct InstancesScopedList {
    #[serde(default)]
    instances: Vec<GcpInstance>,
}

#[derive(Deserialize)]
struct GcpInstance {
    id: String,
    name: String,
    #[serde(default)]
    status: String,
    #[serde(rename = "machineType", default)]
    machine_type: String,
    #[serde(rename = "networkInterfaces", default)]
    network_interfaces: Vec<NetworkInterface>,
    #[serde(default)]
    disks: Vec<Disk>,
    #[serde(default)]
    tags: Option<GcpTags>,
    #[serde(default)]
    labels: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    zone: String,
}

#[derive(Deserialize)]
struct NetworkInterface {
    #[serde(rename = "accessConfigs", default)]
    access_configs: Vec<AccessConfig>,
    #[serde(rename = "networkIP", default)]
    network_ip: String,
    #[serde(rename = "ipv6AccessConfigs", default)]
    ipv6_access_configs: Vec<Ipv6AccessConfig>,
}

#[derive(Deserialize)]
struct AccessConfig {
    #[serde(rename = "natIP", default)]
    nat_ip: String,
}

#[derive(Deserialize)]
struct Ipv6AccessConfig {
    #[serde(rename = "externalIpv6", default)]
    external_ipv6: String,
}

#[derive(Deserialize)]
struct Disk {
    #[serde(default)]
    licenses: Vec<String>,
}

#[derive(Deserialize)]
struct GcpTags {
    #[serde(default)]
    items: Vec<String>,
}

/// Extract the last segment of a URL path (e.g. ".../zones/us-central1-a" -> "us-central1-a").
fn last_url_segment(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or("")
}

/// Select the best IP for an instance.
/// Prefers external (natIP) > internal (networkIP) > external IPv6.
fn select_ip(instance: &GcpInstance) -> Option<String> {
    for ni in &instance.network_interfaces {
        for ac in &ni.access_configs {
            if !ac.nat_ip.is_empty() {
                return Some(ac.nat_ip.clone());
            }
        }
    }
    for ni in &instance.network_interfaces {
        if !ni.network_ip.is_empty() {
            return Some(ni.network_ip.clone());
        }
    }
    for ni in &instance.network_interfaces {
        for v6 in &ni.ipv6_access_configs {
            if !v6.external_ipv6.is_empty() {
                return Some(v6.external_ipv6.clone());
            }
        }
    }
    None
}

/// Build metadata key-value pairs for an instance.
fn build_metadata(instance: &GcpInstance) -> Vec<(String, String)> {
    let mut metadata = Vec::new();
    let zone = last_url_segment(&instance.zone);
    if !zone.is_empty() {
        metadata.push(("zone".to_string(), zone.to_string()));
    }
    let machine = last_url_segment(&instance.machine_type);
    if !machine.is_empty() {
        metadata.push(("machine".to_string(), machine.to_string()));
    }
    // OS from first disk's first license (e.g. "debian-11" from license URL)
    if let Some(disk) = instance.disks.first() {
        if let Some(license) = disk.licenses.first() {
            let os = last_url_segment(license);
            if !os.is_empty() {
                metadata.push(("os".to_string(), os.to_string()));
            }
        }
    }
    if !instance.status.is_empty() {
        metadata.push(("status".to_string(), instance.status.clone()));
    }
    metadata
}

/// Build tags from GCP tags and labels.
fn build_tags(instance: &GcpInstance) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some(ref t) = instance.tags {
        tags.extend(t.items.clone());
    }
    if let Some(ref labels) = instance.labels {
        for (k, v) in labels {
            if v.is_empty() {
                tags.push(k.clone());
            } else {
                tags.push(format!("{}:{}", k, v));
            }
        }
    }
    tags
}

/// Detect whether a token string is a path to a service account JSON key file.
/// Checks for .json extension (case-insensitive).
fn is_json_key_file(token: &str) -> bool {
    token.to_ascii_lowercase().ends_with(".json")
}

/// Service account key file fields we need.
#[derive(Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: String,
}

/// Create a JWT and exchange it for an access token via Google's OAuth2 endpoint.
fn resolve_service_account_token(path: &str) -> Result<String, ProviderError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ProviderError::Http(format!("Failed to read key file {}: {}", path, e)))?;
    let key: ServiceAccountKey = serde_json::from_str(&content)
        .map_err(|e| ProviderError::Http(format!("Failed to parse key file: {}", e)))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let header = r#"{"alg":"RS256","typ":"JWT"}"#;
    let claims = serde_json::json!({
        "iss": key.client_email,
        "scope": "https://www.googleapis.com/auth/compute.readonly",
        "aud": "https://oauth2.googleapis.com/token",
        "iat": now,
        "exp": now + 3600
    });
    let claims_str = claims.to_string();

    let header_b64 = URL_SAFE_NO_PAD.encode(header.as_bytes());
    let claims_b64 = URL_SAFE_NO_PAD.encode(claims_str.as_bytes());
    let signing_input = format!("{}.{}", header_b64, claims_b64);

    // Parse the PEM private key and sign with RSA-SHA256
    let der = rsa::pkcs8::DecodePrivateKey::from_pkcs8_pem(&key.private_key)
        .map_err(|e| ProviderError::Http(format!("Failed to parse private key: {}", e)))?;
    let signing_key = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(der);
    use rsa::signature::{SignatureEncoding, Signer};
    let signature = signing_key.sign(signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    let jwt = format!("{}.{}", signing_input, sig_b64);

    // Exchange JWT for access token
    let agent = super::http_agent();
    let mut resp = agent
        .post("https://oauth2.googleapis.com/token")
        .send_form([
            ("grant_type", "urn:ietf:params:oauth:grant_type:jwt-bearer"),
            ("assertion", jwt.as_str()),
        ])
        .map_err(map_ureq_error)?;

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let token_resp: TokenResponse = resp
        .body_mut()
        .read_json()
        .map_err(|e| ProviderError::Parse(format!("Token response: {}", e)))?;

    Ok(token_resp.access_token)
}

/// Resolve token: if it's a path to a JSON key file, exchange it for an access token.
/// Otherwise, use it as a raw access token.
fn resolve_token(token: &str) -> Result<String, ProviderError> {
    if is_json_key_file(token) {
        resolve_service_account_token(token)
    } else {
        Ok(token.to_string())
    }
}

/// Percent-encode a page token for use in a URL query parameter (delegates to shared implementation).
fn url_encode(s: &str) -> String {
    super::percent_encode(s)
}

impl Provider for Gcp {
    fn name(&self) -> &str {
        "gcp"
    }

    fn short_label(&self) -> &str {
        "gcp"
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
        if self.project.is_empty() {
            return Err(ProviderError::Http(
                "No GCP project configured. Set the Project ID in the provider settings."
                    .to_string(),
            ));
        }

        // Validate project ID format: lowercase letters, digits, hyphens, dots and colons
        // (dots and colons for domain-scoped projects like example.com:my-project)
        if !self
            .project
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | ':'))
        {
            return Err(ProviderError::Http(format!(
                "Invalid GCP project ID '{}'. Must contain only lowercase letters, digits, hyphens, dots and colons.",
                self.project
            )));
        }

        progress("Authenticating...");
        let access_token = resolve_token(token)?;

        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }

        let zone_filter: HashSet<&str> = self.zones.iter().map(|s| s.as_str()).collect();
        let agent = super::http_agent();
        let mut all_hosts = Vec::new();
        let mut page_token: Option<String> = None;

        for page in 0u32.. {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            // Safety guard: prevent infinite pagination loops
            if page > 500 {
                break;
            }

            let mut url = format!(
                "https://compute.googleapis.com/compute/v1/projects/{}/aggregated/instances?maxResults=500&returnPartialSuccess=true",
                self.project
            );
            if let Some(ref pt) = page_token {
                url.push_str(&format!("&pageToken={}", url_encode(pt)));
            }

            progress(&format!(
                "Fetching instances ({} so far)...",
                all_hosts.len()
            ));

            let mut response = match agent
                .get(&url)
                .header("Authorization", &format!("Bearer {}", access_token))
                .call()
            {
                Ok(r) => r,
                Err(e) => {
                    let err = map_ureq_error(e);
                    // If we already fetched some hosts, return a partial result
                    if !all_hosts.is_empty() {
                        let fetched = all_hosts.len();
                        progress(&format!("{} instances, page {} failed", fetched, page + 1));
                        return Err(ProviderError::PartialResult {
                            hosts: all_hosts,
                            failures: 1,
                            total: page as usize + 1,
                        });
                    }
                    return Err(err);
                }
            };

            let resp: AggregatedListResponse = match response.body_mut().read_json() {
                Ok(r) => r,
                Err(e) => {
                    if !all_hosts.is_empty() {
                        let fetched = all_hosts.len();
                        progress(&format!(
                            "{} instances, page {} failed to parse",
                            fetched,
                            page + 1
                        ));
                        return Err(ProviderError::PartialResult {
                            hosts: all_hosts,
                            failures: 1,
                            total: page as usize + 1,
                        });
                    }
                    return Err(ProviderError::Parse(format!("{}", e)));
                }
            };

            for (scope_key, scoped_list) in &resp.items {
                // scope_key is like "zones/us-central1-a"
                let zone = last_url_segment(scope_key);

                // Client-side zone filter (empty = all zones)
                if !zone_filter.is_empty() && !zone_filter.contains(zone) {
                    continue;
                }

                for instance in &scoped_list.instances {
                    if let Some(ip) = select_ip(instance) {
                        all_hosts.push(ProviderHost {
                            server_id: instance.id.clone(),
                            name: instance.name.clone(),
                            ip,
                            tags: build_tags(instance),
                            metadata: build_metadata(instance),
                        });
                    }
                }
            }

            match resp.next_page_token {
                Some(ref t) if !t.is_empty() => page_token = Some(t.clone()),
                _ => break,
            }
        }

        progress(&format!("{} instances", all_hosts.len()));
        Ok(all_hosts)
    }
}

#[cfg(test)]
#[path = "gcp_tests.rs"]
mod tests;
