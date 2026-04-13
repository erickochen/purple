use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer};
use serde::Deserialize;

use super::{Provider, ProviderError, ProviderHost};

/// Oracle Cloud Infrastructure provider configuration.
pub struct Oracle {
    pub regions: Vec<String>,
    pub compartment: String,
}

/// Parsed OCI API credentials.
#[derive(Debug)]
struct OciCredentials {
    tenancy: String,
    user: String,
    fingerprint: String,
    key_pem: String,
    region: String,
}

/// Parse an OCI config file and return credentials.
///
/// Only the `[DEFAULT]` profile is read (case-sensitive). The `key_pem`
/// field comes from the already-read key file content passed as
/// `key_content`.
fn parse_oci_config(content: &str, key_content: &str) -> Result<OciCredentials, ProviderError> {
    let mut in_default = false;
    let mut tenancy: Option<String> = None;
    let mut user: Option<String> = None;
    let mut fingerprint: Option<String> = None;
    let mut region: Option<String> = None;

    for raw_line in content.lines() {
        // Strip CRLF by stripping trailing \r after lines() removes \n
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let profile = &trimmed[1..trimmed.len() - 1];
            in_default = profile == "DEFAULT";
            continue;
        }

        if !in_default {
            continue;
        }

        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if let Some(eq) = trimmed.find('=') {
            let key = trimmed[..eq].trim();
            let val = trimmed[eq + 1..].trim().to_string();
            match key {
                "tenancy" => tenancy = Some(val),
                "user" => user = Some(val),
                "fingerprint" => fingerprint = Some(val),
                "region" => region = Some(val),
                _ => {}
            }
        }
    }

    let tenancy = tenancy
        .ok_or_else(|| ProviderError::Http("OCI config missing 'tenancy' in [DEFAULT]".into()))?;
    let user =
        user.ok_or_else(|| ProviderError::Http("OCI config missing 'user' in [DEFAULT]".into()))?;
    let fingerprint = fingerprint.ok_or_else(|| {
        ProviderError::Http("OCI config missing 'fingerprint' in [DEFAULT]".into())
    })?;
    let region = region.unwrap_or_default();

    Ok(OciCredentials {
        tenancy,
        user,
        fingerprint,
        key_pem: key_content.to_string(),
        region,
    })
}

/// Extract the `key_file` path from the `[DEFAULT]` profile of an OCI
/// config file.
fn extract_key_file(config_content: &str) -> Result<String, ProviderError> {
    let mut in_default = false;

    for raw_line in config_content.lines() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let profile = &trimmed[1..trimmed.len() - 1];
            in_default = profile == "DEFAULT";
            continue;
        }

        if !in_default || trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if let Some(eq) = trimmed.find('=') {
            let key = trimmed[..eq].trim();
            if key == "key_file" {
                return Ok(trimmed[eq + 1..].trim().to_string());
            }
        }
    }

    Err(ProviderError::Http(
        "OCI config missing 'key_file' in [DEFAULT]".into(),
    ))
}

/// Validate that an OCID string has a compartment or tenancy prefix.
fn validate_compartment(ocid: &str) -> Result<(), ProviderError> {
    if ocid.starts_with("ocid1.compartment.oc1..") || ocid.starts_with("ocid1.tenancy.oc1..") {
        Ok(())
    } else {
        Err(ProviderError::Http(format!(
            "Invalid compartment OCID: '{}'. Must start with 'ocid1.compartment.oc1..' or 'ocid1.tenancy.oc1..'",
            ocid
        )))
    }
}

// ---------------------------------------------------------------------------
// RFC 7231 date formatting
// ---------------------------------------------------------------------------

const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Format a Unix timestamp as an RFC 7231 date string.
///
/// Example: `Thu, 26 Mar 2026 12:00:00 GMT`
fn format_rfc7231(epoch_secs: u64) -> String {
    let d = super::epoch_to_date(epoch_secs);
    // Day of week: Jan 1 1970 was a Thursday (index 0 in WEEKDAYS)
    let weekday = WEEKDAYS[(d.epoch_days % 7) as usize];
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        weekday,
        d.day,
        MONTHS[(d.month - 1) as usize],
        d.year,
        d.hours,
        d.minutes,
        d.seconds,
    )
}

// ---------------------------------------------------------------------------
// RSA private key parsing
// ---------------------------------------------------------------------------

/// Parse a PEM-encoded RSA private key (PKCS#1 or PKCS#8).
fn parse_private_key(pem: &str) -> Result<rsa::RsaPrivateKey, ProviderError> {
    if pem.contains("ENCRYPTED") {
        return Err(ProviderError::Http(
            "OCI private key is encrypted. Please provide an unencrypted key.".into(),
        ));
    }

    // Try PKCS#1 first, then PKCS#8
    if let Ok(key) = rsa::RsaPrivateKey::from_pkcs1_pem(pem) {
        return Ok(key);
    }

    rsa::RsaPrivateKey::from_pkcs8_pem(pem)
        .map_err(|e| ProviderError::Http(format!("Failed to parse OCI private key: {}", e)))
}

// ---------------------------------------------------------------------------
// HTTP request signing
// ---------------------------------------------------------------------------

/// Build the OCI `Authorization` header value for a GET request.
///
/// Signs `date`, `(request-target)` and `host` headers using RSA-SHA256.
/// The caller must parse the RSA private key once and pass it in to avoid
/// re-parsing on every request.
fn sign_request(
    creds: &OciCredentials,
    rsa_key: &rsa::RsaPrivateKey,
    date: &str,
    host: &str,
    path_and_query: &str,
) -> Result<String, ProviderError> {
    let signing_string = format!(
        "date: {}\n(request-target): get {}\nhost: {}",
        date, path_and_query, host
    );

    let signing_key = rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(rsa_key.clone());
    let signature = signing_key.sign(signing_string.as_bytes());
    let sig_b64 = STANDARD.encode(signature.to_bytes());

    let key_id = format!("{}/{}/{}", creds.tenancy, creds.user, creds.fingerprint);
    Ok(format!(
        "Signature version=\"1\",keyId=\"{}\",algorithm=\"rsa-sha256\",headers=\"date (request-target) host\",signature=\"{}\"",
        key_id, sig_b64
    ))
}

// ---------------------------------------------------------------------------
// JSON response models
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct OciCompartment {
    id: String,
    #[serde(rename = "lifecycleState")]
    lifecycle_state: String,
}

#[derive(Deserialize)]
struct OciInstance {
    id: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "lifecycleState")]
    lifecycle_state: String,
    shape: String,
    #[serde(rename = "imageId")]
    image_id: Option<String>,
    #[serde(rename = "freeformTags")]
    freeform_tags: Option<std::collections::HashMap<String, String>>,
}

#[derive(Deserialize)]
struct OciVnicAttachment {
    #[serde(rename = "instanceId")]
    instance_id: String,
    #[serde(rename = "vnicId")]
    vnic_id: Option<String>,
    #[serde(rename = "lifecycleState")]
    lifecycle_state: String,
    #[serde(rename = "isPrimary")]
    is_primary: Option<bool>,
}

#[derive(Deserialize)]
struct OciVnic {
    #[serde(rename = "publicIp")]
    public_ip: Option<String>,
    #[serde(rename = "privateIp")]
    private_ip: Option<String>,
}

#[derive(Deserialize)]
struct OciImage {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

// ureq 3.x does not expose the response body on StatusCode errors, so we
// cannot parse OCI error JSON from failed responses. Other providers in this
// codebase handle errors the same way (status code only). Kept for future use
// if ureq adds body-on-error support.
#[derive(Deserialize)]
#[allow(dead_code)]
struct OciErrorBody {
    code: Option<String>,
    message: Option<String>,
}

// ---------------------------------------------------------------------------
// IP selection, VNIC mapping and helpers
// ---------------------------------------------------------------------------

fn select_ip(vnic: &OciVnic) -> String {
    if let Some(ip) = &vnic.public_ip {
        if !ip.is_empty() {
            return ip.clone();
        }
    }
    if let Some(ip) = &vnic.private_ip {
        if !ip.is_empty() {
            return ip.clone();
        }
    }
    String::new()
}

fn select_vnic_for_instance(
    attachments: &[OciVnicAttachment],
    instance_id: &str,
) -> Option<String> {
    let matching: Vec<_> = attachments
        .iter()
        .filter(|a| a.instance_id == instance_id && a.lifecycle_state == "ATTACHED")
        .collect();
    if let Some(primary) = matching.iter().find(|a| a.is_primary == Some(true)) {
        return primary.vnic_id.clone();
    }
    matching.first().and_then(|a| a.vnic_id.clone())
}

fn extract_tags(freeform_tags: &Option<std::collections::HashMap<String, String>>) -> Vec<String> {
    match freeform_tags {
        Some(tags) => {
            let mut result: Vec<String> = tags
                .iter()
                .map(|(k, v)| {
                    if v.is_empty() {
                        k.clone()
                    } else {
                        format!("{}:{}", k, v)
                    }
                })
                .collect();
            result.sort();
            result
        }
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Region constants
// ---------------------------------------------------------------------------

pub const OCI_REGIONS: &[(&str, &str)] = &[
    // Americas (0..12)
    ("us-ashburn-1", "Ashburn"),
    ("us-phoenix-1", "Phoenix"),
    ("us-sanjose-1", "San Jose"),
    ("us-chicago-1", "Chicago"),
    ("ca-toronto-1", "Toronto"),
    ("ca-montreal-1", "Montreal"),
    ("br-saopaulo-1", "Sao Paulo"),
    ("br-vinhedo-1", "Vinhedo"),
    ("mx-queretaro-1", "Queretaro"),
    ("mx-monterrey-1", "Monterrey"),
    ("cl-santiago-1", "Santiago"),
    ("co-bogota-1", "Bogota"),
    // EMEA (12..29)
    ("eu-amsterdam-1", "Amsterdam"),
    ("eu-frankfurt-1", "Frankfurt"),
    ("eu-zurich-1", "Zurich"),
    ("eu-stockholm-1", "Stockholm"),
    ("eu-marseille-1", "Marseille"),
    ("eu-milan-1", "Milan"),
    ("eu-paris-1", "Paris"),
    ("eu-madrid-1", "Madrid"),
    ("eu-jovanovac-1", "Jovanovac"),
    ("uk-london-1", "London"),
    ("uk-cardiff-1", "Cardiff"),
    ("me-jeddah-1", "Jeddah"),
    ("me-abudhabi-1", "Abu Dhabi"),
    ("me-dubai-1", "Dubai"),
    ("me-riyadh-1", "Riyadh"),
    ("af-johannesburg-1", "Johannesburg"),
    ("il-jerusalem-1", "Jerusalem"),
    // Asia Pacific (29..38)
    ("ap-tokyo-1", "Tokyo"),
    ("ap-osaka-1", "Osaka"),
    ("ap-seoul-1", "Seoul"),
    ("ap-chuncheon-1", "Chuncheon"),
    ("ap-singapore-1", "Singapore"),
    ("ap-sydney-1", "Sydney"),
    ("ap-melbourne-1", "Melbourne"),
    ("ap-mumbai-1", "Mumbai"),
    ("ap-hyderabad-1", "Hyderabad"),
];

pub const OCI_REGION_GROUPS: &[(&str, usize, usize)] = &[
    ("Americas", 0, 12),
    ("EMEA", 12, 29),
    ("Asia Pacific", 29, 38),
];

// ---------------------------------------------------------------------------
// Provider trait implementation
// ---------------------------------------------------------------------------

impl Provider for Oracle {
    fn name(&self) -> &str {
        "oracle"
    }

    fn short_label(&self) -> &str {
        "oci"
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
        if self.compartment.is_empty() {
            return Err(ProviderError::Http(
                "No compartment configured. Run: purple provider add oracle --token ~/.oci/config --compartment <OCID>".to_string(),
            ));
        }
        validate_compartment(&self.compartment)?;

        let config_content = std::fs::read_to_string(token).map_err(|e| {
            ProviderError::Http(format!("Cannot read OCI config file '{}': {}", token, e))
        })?;
        let key_file = extract_key_file(&config_content)?;
        let expanded = if key_file.starts_with("~/") {
            if let Some(home) = dirs::home_dir() {
                format!("{}{}", home.display(), &key_file[1..])
            } else {
                key_file.clone()
            }
        } else {
            key_file.clone()
        };
        let key_content = std::fs::read_to_string(&expanded).map_err(|e| {
            ProviderError::Http(format!("Cannot read OCI private key '{}': {}", expanded, e))
        })?;
        let creds = parse_oci_config(&config_content, &key_content)?;
        let rsa_key = parse_private_key(&creds.key_pem)?;

        let regions: Vec<String> = if self.regions.is_empty() {
            if creds.region.is_empty() {
                return Err(ProviderError::Http(
                    "No regions configured and OCI config has no default region".to_string(),
                ));
            }
            vec![creds.region.clone()]
        } else {
            self.regions.clone()
        };

        let mut all_hosts = Vec::new();
        let mut region_failures = 0usize;
        let total_regions = regions.len();
        for region in &regions {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }
            progress(&format!("Syncing {} ...", region));
            match self.fetch_region(&creds, &rsa_key, region, cancel, progress) {
                Ok(mut hosts) => all_hosts.append(&mut hosts),
                Err(ProviderError::AuthFailed) => return Err(ProviderError::AuthFailed),
                Err(ProviderError::RateLimited) => return Err(ProviderError::RateLimited),
                Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
                Err(ProviderError::PartialResult {
                    hosts: mut partial, ..
                }) => {
                    all_hosts.append(&mut partial);
                    region_failures += 1;
                }
                Err(_) => {
                    region_failures += 1;
                }
            }
        }
        if region_failures > 0 {
            if all_hosts.is_empty() {
                return Err(ProviderError::Http(format!(
                    "Failed to sync all {} region(s)",
                    total_regions
                )));
            }
            return Err(ProviderError::PartialResult {
                hosts: all_hosts,
                failures: region_failures,
                total: total_regions,
            });
        }
        Ok(all_hosts)
    }
}

impl Oracle {
    /// Perform a signed GET request against the OCI API.
    fn signed_get(
        &self,
        creds: &OciCredentials,
        rsa_key: &rsa::RsaPrivateKey,
        agent: &ureq::Agent,
        host: &str,
        url: &str,
    ) -> Result<ureq::http::Response<ureq::Body>, ProviderError> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let date = format_rfc7231(now);

        // Extract path+query from URL (everything after the host part)
        let path_and_query = if let Some(pos) = url.find(host) {
            &url[pos + host.len()..]
        } else {
            // Fallback: strip scheme + host
            url.splitn(4, '/').nth(3).map_or("/", |p| {
                // We need the leading slash
                &url[url.len() - p.len() - 1..]
            })
        };

        let auth = sign_request(creds, rsa_key, &date, host, path_and_query)?;

        agent
            .get(url)
            .header("date", &date)
            .header("Authorization", &auth)
            .call()
            .map_err(|e| match e {
                ureq::Error::StatusCode(401 | 403) => ProviderError::AuthFailed,
                ureq::Error::StatusCode(429) => ProviderError::RateLimited,
                ureq::Error::StatusCode(code) => ProviderError::Http(format!("HTTP {}", code)),
                other => super::map_ureq_error(other),
            })
    }

    /// List active sub-compartments (Identity API supports compartmentIdInSubtree).
    fn list_compartments(
        &self,
        creds: &OciCredentials,
        rsa_key: &rsa::RsaPrivateKey,
        agent: &ureq::Agent,
        region: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<String>, ProviderError> {
        let host = format!("identity.{}.oraclecloud.com", region);
        let compartment_encoded = urlencoding_encode(&self.compartment);

        let mut compartment_ids = vec![self.compartment.clone()];
        let mut next_page: Option<String> = None;
        for _ in 0..500 {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            let url = match &next_page {
                Some(page) => format!(
                    "https://{}/20160918/compartments?compartmentId={}&compartmentIdInSubtree=true&lifecycleState=ACTIVE&limit=100&page={}",
                    host,
                    compartment_encoded,
                    urlencoding_encode(page)
                ),
                None => format!(
                    "https://{}/20160918/compartments?compartmentId={}&compartmentIdInSubtree=true&lifecycleState=ACTIVE&limit=100",
                    host, compartment_encoded
                ),
            };

            let mut resp = self.signed_get(creds, rsa_key, agent, &host, &url)?;

            let opc_next = resp
                .headers()
                .get("opc-next-page")
                .and_then(|v| v.to_str().ok())
                .filter(|s| !s.is_empty())
                .map(String::from);

            let items: Vec<OciCompartment> = resp
                .body_mut()
                .read_json()
                .map_err(|e| ProviderError::Parse(e.to_string()))?;

            compartment_ids.extend(
                items
                    .into_iter()
                    .filter(|c| c.lifecycle_state == "ACTIVE")
                    .map(|c| c.id),
            );

            match opc_next {
                Some(p) => next_page = Some(p),
                None => break,
            }
        }
        Ok(compartment_ids)
    }

    fn fetch_region(
        &self,
        creds: &OciCredentials,
        rsa_key: &rsa::RsaPrivateKey,
        region: &str,
        cancel: &AtomicBool,
        progress: &dyn Fn(&str),
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        let agent = super::http_agent();
        let host = format!("iaas.{}.oraclecloud.com", region);

        // Step 0: Discover all compartments (root + sub-compartments)
        progress("Listing compartments...");
        let compartment_ids = self.list_compartments(creds, rsa_key, &agent, region, cancel)?;
        let total_compartments = compartment_ids.len();

        // Step 1: List instances across all compartments (paginated per compartment)
        let mut instances: Vec<OciInstance> = Vec::new();
        for (ci, comp_id) in compartment_ids.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }
            if total_compartments > 1 {
                progress(&format!(
                    "Listing instances ({}/{} compartments)...",
                    ci + 1,
                    total_compartments
                ));
            } else {
                progress("Listing instances...");
            }
            let compartment_encoded = urlencoding_encode(comp_id);
            let mut next_page: Option<String> = None;
            for _ in 0..500 {
                if cancel.load(Ordering::Relaxed) {
                    return Err(ProviderError::Cancelled);
                }

                let url = match &next_page {
                    Some(page) => format!(
                        "https://{}/20160918/instances?compartmentId={}&limit=100&page={}",
                        host,
                        compartment_encoded,
                        urlencoding_encode(page)
                    ),
                    None => format!(
                        "https://{}/20160918/instances?compartmentId={}&limit=100",
                        host, compartment_encoded
                    ),
                };

                let mut resp = self.signed_get(creds, rsa_key, &agent, &host, &url)?;

                let opc_next = resp
                    .headers()
                    .get("opc-next-page")
                    .and_then(|v| v.to_str().ok())
                    .filter(|s| !s.is_empty())
                    .map(String::from);

                let page_items: Vec<OciInstance> = resp
                    .body_mut()
                    .read_json()
                    .map_err(|e| ProviderError::Parse(e.to_string()))?;

                instances.extend(
                    page_items
                        .into_iter()
                        .filter(|i| i.lifecycle_state != "TERMINATED"),
                );

                match opc_next {
                    Some(p) => next_page = Some(p),
                    None => break,
                }
            }
        }

        // Step 2: List VNIC attachments across all compartments (paginated per compartment)
        progress("Listing VNIC attachments...");
        let mut attachments: Vec<OciVnicAttachment> = Vec::new();
        for comp_id in &compartment_ids {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }
            let compartment_encoded = urlencoding_encode(comp_id);
            let mut next_page: Option<String> = None;
            for _ in 0..500 {
                if cancel.load(Ordering::Relaxed) {
                    return Err(ProviderError::Cancelled);
                }

                let url = match &next_page {
                    Some(page) => format!(
                        "https://{}/20160918/vnicAttachments?compartmentId={}&limit=100&page={}",
                        host,
                        compartment_encoded,
                        urlencoding_encode(page)
                    ),
                    None => format!(
                        "https://{}/20160918/vnicAttachments?compartmentId={}&limit=100",
                        host, compartment_encoded
                    ),
                };

                let mut resp = self.signed_get(creds, rsa_key, &agent, &host, &url)?;

                let opc_next = resp
                    .headers()
                    .get("opc-next-page")
                    .and_then(|v| v.to_str().ok())
                    .filter(|s| !s.is_empty())
                    .map(String::from);

                let page_items: Vec<OciVnicAttachment> = resp
                    .body_mut()
                    .read_json()
                    .map_err(|e| ProviderError::Parse(e.to_string()))?;

                attachments.extend(page_items);

                match opc_next {
                    Some(p) => next_page = Some(p),
                    None => break,
                }
            }
        }

        // Step 3: Resolve images (N+1 per unique imageId)
        let unique_image_ids: Vec<String> = {
            let mut ids: Vec<String> = instances
                .iter()
                .filter_map(|i| i.image_id.clone())
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        let total_images = unique_image_ids.len();
        let mut image_names: HashMap<String, String> = HashMap::new();
        for (n, image_id) in unique_image_ids.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }
            progress(&format!("Resolving images ({}/{})...", n + 1, total_images));

            let url = format!("https://{}/20160918/images/{}", host, image_id);
            match self.signed_get(creds, rsa_key, &agent, &host, &url) {
                Ok(mut resp) => {
                    if let Ok(img) = resp.body_mut().read_json::<OciImage>() {
                        if let Some(name) = img.display_name {
                            image_names.insert(image_id.clone(), name);
                        }
                    }
                }
                Err(ProviderError::AuthFailed) => return Err(ProviderError::AuthFailed),
                Err(ProviderError::RateLimited) => return Err(ProviderError::RateLimited),
                Err(_) => {} // Non-fatal: skip silently
            }
        }

        // Step 4: Get VNIC + build hosts (N+1 per VNIC for RUNNING instances)
        let total_instances = instances.len();
        let mut hosts: Vec<ProviderHost> = Vec::new();
        let mut fetch_failures = 0usize;
        for (n, instance) in instances.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }
            progress(&format!("Fetching IPs ({}/{})...", n + 1, total_instances));

            let ip = if instance.lifecycle_state == "RUNNING" {
                match select_vnic_for_instance(&attachments, &instance.id) {
                    Some(vnic_id) => {
                        let url = format!("https://{}/20160918/vnics/{}", host, vnic_id);
                        match self.signed_get(creds, rsa_key, &agent, &host, &url) {
                            Ok(mut resp) => match resp.body_mut().read_json::<OciVnic>() {
                                Ok(vnic) => {
                                    let raw = select_ip(&vnic);
                                    super::strip_cidr(&raw).to_string()
                                }
                                Err(_) => {
                                    fetch_failures += 1;
                                    String::new()
                                }
                            },
                            Err(ProviderError::AuthFailed) => {
                                return Err(ProviderError::AuthFailed);
                            }
                            Err(ProviderError::RateLimited) => {
                                return Err(ProviderError::RateLimited);
                            }
                            Err(ProviderError::Http(ref msg)) if msg == "HTTP 404" => {
                                // 404: race condition, silent skip
                                String::new()
                            }
                            Err(_) => {
                                fetch_failures += 1;
                                String::new()
                            }
                        }
                    }
                    None => String::new(),
                }
            } else {
                String::new()
            };

            let os_name = instance
                .image_id
                .as_ref()
                .and_then(|id| image_names.get(id))
                .cloned()
                .unwrap_or_default();

            let mut metadata = Vec::new();
            metadata.push(("region".to_string(), region.to_string()));
            metadata.push(("shape".to_string(), instance.shape.clone()));
            if !os_name.is_empty() {
                metadata.push(("os".to_string(), os_name));
            }
            metadata.push(("status".to_string(), instance.lifecycle_state.clone()));

            hosts.push(ProviderHost {
                server_id: instance.id.clone(),
                name: instance.display_name.clone(),
                ip,
                tags: extract_tags(&instance.freeform_tags),
                metadata,
            });
        }

        if fetch_failures > 0 {
            if hosts.is_empty() {
                return Err(ProviderError::Http(format!(
                    "Failed to fetch details for all {} instances",
                    total_instances
                )));
            }
            return Err(ProviderError::PartialResult {
                hosts,
                failures: fetch_failures,
                total: total_instances,
            });
        }

        Ok(hosts)
    }
}

/// Minimal percent-encoding for query parameter values (delegates to shared implementation).
fn urlencoding_encode(input: &str) -> String {
    super::percent_encode(input)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "oracle_tests.rs"]
mod tests;
