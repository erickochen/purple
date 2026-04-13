use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use super::{Provider, ProviderError, ProviderHost};

pub struct Aws {
    pub regions: Vec<String>,
    pub profile: String,
}

/// All commonly available AWS regions with display names.
/// Single source of truth. AWS_REGION_GROUPS references slices of this array.
pub const AWS_REGIONS: &[(&str, &str)] = &[
    // Americas (0..8)
    ("us-east-1", "N. Virginia"),
    ("us-east-2", "Ohio"),
    ("us-west-1", "N. California"),
    ("us-west-2", "Oregon"),
    ("ca-central-1", "Canada Central"),
    ("ca-west-1", "Canada West"),
    ("mx-central-1", "Mexico Central"),
    ("sa-east-1", "Sao Paulo"),
    // Europe (8..16)
    ("eu-west-1", "Ireland"),
    ("eu-west-2", "London"),
    ("eu-west-3", "Paris"),
    ("eu-central-1", "Frankfurt"),
    ("eu-central-2", "Zurich"),
    ("eu-south-1", "Milan"),
    ("eu-south-2", "Spain"),
    ("eu-north-1", "Stockholm"),
    // Asia Pacific (16..30)
    ("ap-northeast-1", "Tokyo"),
    ("ap-northeast-2", "Seoul"),
    ("ap-northeast-3", "Osaka"),
    ("ap-southeast-1", "Singapore"),
    ("ap-southeast-2", "Sydney"),
    ("ap-southeast-3", "Jakarta"),
    ("ap-southeast-4", "Melbourne"),
    ("ap-southeast-5", "Malaysia"),
    ("ap-southeast-6", "New Zealand"),
    ("ap-southeast-7", "Thailand"),
    ("ap-east-1", "Hong Kong"),
    ("ap-east-2", "Taipei"),
    ("ap-south-1", "Mumbai"),
    ("ap-south-2", "Hyderabad"),
    // Middle East / Africa (30..34)
    ("me-south-1", "Bahrain"),
    ("me-central-1", "UAE"),
    ("il-central-1", "Tel Aviv"),
    ("af-south-1", "Cape Town"),
];

/// Region group labels with start..end indices into AWS_REGIONS.
pub const AWS_REGION_GROUPS: &[(&str, usize, usize)] = &[
    ("Americas", 0, 8),
    ("Europe", 8, 16),
    ("Asia Pacific", 16, 30),
    ("Middle East / Africa", 30, 34),
];

// --- Credentials ---

struct AwsCredentials {
    access_key: String,
    secret_key: String,
}

fn resolve_credentials(token: &str, profile: &str) -> Result<AwsCredentials, ProviderError> {
    // Profile takes priority: read from ~/.aws/credentials
    if !profile.is_empty() {
        return read_credentials_file(profile);
    }
    // Token field: ACCESS_KEY_ID:SECRET_ACCESS_KEY
    if let Some((ak, sk)) = token.split_once(':') {
        if !ak.is_empty() && !sk.is_empty() {
            return Ok(AwsCredentials {
                access_key: ak.to_string(),
                secret_key: sk.to_string(),
            });
        }
    }
    // Environment variables
    if let (Ok(ak), Ok(sk)) = (
        std::env::var("AWS_ACCESS_KEY_ID"),
        std::env::var("AWS_SECRET_ACCESS_KEY"),
    ) {
        if !ak.is_empty() && !sk.is_empty() {
            return Ok(AwsCredentials {
                access_key: ak,
                secret_key: sk,
            });
        }
    }
    Err(ProviderError::AuthFailed)
}

/// Parse AWS credentials from INI content (testable without filesystem).
fn parse_credentials(content: &str, profile: &str) -> Option<AwsCredentials> {
    let header = format!("[{}]", profile);
    let mut in_section = false;
    let mut access_key = String::new();
    let mut secret_key = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == header;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            match key.trim() {
                "aws_access_key_id" => access_key = value.trim().to_string(),
                "aws_secret_access_key" => secret_key = value.trim().to_string(),
                _ => {}
            }
        }
    }

    if access_key.is_empty() || secret_key.is_empty() {
        None
    } else {
        Some(AwsCredentials {
            access_key,
            secret_key,
        })
    }
}

fn read_credentials_file(profile: &str) -> Result<AwsCredentials, ProviderError> {
    let path = dirs::home_dir()
        .ok_or(ProviderError::AuthFailed)?
        .join(".aws")
        .join("credentials");
    let content = std::fs::read_to_string(&path).map_err(|_| ProviderError::AuthFailed)?;
    parse_credentials(&content, profile).ok_or(ProviderError::AuthFailed)
}

// --- SigV4 signing ---

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn sha256_hash(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// RFC 3986 URI encoding (delegates to shared implementation).
fn uri_encode(s: &str) -> String {
    super::percent_encode(s)
}

/// Format epoch seconds as (timestamp, datestamp) for SigV4.
fn format_utc(epoch_secs: u64) -> (String, String) {
    let d = super::epoch_to_date(epoch_secs);
    let timestamp = format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        d.year, d.month, d.day, d.hours, d.minutes, d.seconds,
    );
    let datestamp = format!("{:04}{:02}{:02}", d.year, d.month, d.day);
    (timestamp, datestamp)
}

/// Build the SigV4 Authorization header value.
fn sign_request(
    creds: &AwsCredentials,
    region: &str,
    host: &str,
    query_string: &str,
    timestamp: &str,
    datestamp: &str,
) -> String {
    let payload_hash = hex_encode(&sha256_hash(b""));
    let canonical_headers = format!("host:{}\nx-amz-date:{}\n", host, timestamp);
    let signed_headers = "host;x-amz-date";

    let canonical_request = format!(
        "GET\n/\n{}\n{}\n{}\n{}",
        query_string, canonical_headers, signed_headers, payload_hash
    );

    let scope = format!("{}/{}/ec2/aws4_request", datestamp, region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        timestamp,
        scope,
        hex_encode(&sha256_hash(canonical_request.as_bytes())),
    );

    let k_date = hmac_sha256(
        format!("AWS4{}", creds.secret_key).as_bytes(),
        datestamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"ec2");
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex_encode(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        creds.access_key, scope, signed_headers, signature
    )
}

// --- XML response structs ---

/// Generic wrapper for AWS XML lists that use repeated `<item>` elements.
#[derive(serde::Deserialize, Debug)]
#[serde(bound(deserialize = "T: serde::Deserialize<'de>"))]
struct ItemList<T> {
    #[serde(rename = "item", default = "Vec::new")]
    item: Vec<T>,
}

impl<T> Default for ItemList<T> {
    fn default() -> Self {
        Self { item: Vec::new() }
    }
}

#[derive(serde::Deserialize, Debug)]
struct DescribeInstancesResponse {
    #[serde(rename = "reservationSet", default)]
    reservation_set: ItemList<Reservation>,
    #[serde(rename = "nextToken", default)]
    next_token: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
struct Reservation {
    #[serde(rename = "instancesSet", default)]
    instances_set: ItemList<Ec2Instance>,
}

#[derive(serde::Deserialize, Debug)]
struct Ec2Instance {
    #[serde(rename = "instanceId", default)]
    instance_id: String,
    #[serde(rename = "imageId", default)]
    image_id: String,
    #[serde(rename = "instanceState", default)]
    instance_state: InstanceState,
    #[serde(rename = "instanceType", default)]
    instance_type: String,
    #[serde(rename = "tagSet", default)]
    tag_set: ItemList<Ec2Tag>,
    #[serde(rename = "ipAddress", default)]
    ip_address: Option<String>,
    #[serde(rename = "privateIpAddress", default)]
    private_ip_address: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default)]
struct InstanceState {
    #[serde(default)]
    name: String,
}

#[derive(serde::Deserialize, Debug)]
struct Ec2Tag {
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: String,
}

#[derive(serde::Deserialize, Debug)]
struct DescribeImagesResponse {
    #[serde(rename = "imagesSet", default)]
    images_set: ItemList<ImageInfo>,
}

#[derive(serde::Deserialize, Debug)]
struct ImageInfo {
    #[serde(rename = "imageId", default)]
    image_id: String,
    #[serde(default)]
    name: String,
}

// --- EC2 API ---

fn param(key: &str, value: &str) -> (String, String) {
    (key.to_string(), value.to_string())
}

/// Make a signed GET request to the EC2 API.
fn ec2_get(
    agent: &ureq::Agent,
    creds: &AwsCredentials,
    region: &str,
    params: Vec<(String, String)>,
) -> Result<String, ProviderError> {
    let host = format!("ec2.{}.amazonaws.com", region);
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (timestamp, datestamp) = format_utc(epoch);

    // Build sorted, URI-encoded query string (SigV4 requires sorted params)
    let mut sorted: Vec<(String, String)> = params
        .into_iter()
        .map(|(k, v)| (uri_encode(&k), uri_encode(&v)))
        .collect();
    sorted.sort();
    let query_string: String = sorted
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");

    let auth = sign_request(creds, region, &host, &query_string, &timestamp, &datestamp);
    let url = format!("https://{}/?{}", host, query_string);

    let mut resp = agent
        .get(&url)
        .header("Authorization", &auth)
        .header("x-amz-date", &timestamp)
        .call()
        .map_err(super::map_ureq_error)?;

    resp.body_mut()
        .read_to_string()
        .map_err(|e| ProviderError::Parse(e.to_string()))
}

/// Fetch all non-terminated instances in a region (handles pagination).
fn describe_instances(
    agent: &ureq::Agent,
    creds: &AwsCredentials,
    region: &str,
    cancel: &AtomicBool,
) -> Result<Vec<Ec2Instance>, ProviderError> {
    let mut all = Vec::new();
    let mut next_token: Option<String> = None;
    let mut page = 0usize;

    loop {
        page += 1;
        if page > 500 {
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }

        let mut params = vec![
            param("Action", "DescribeInstances"),
            param("Version", "2016-11-15"),
        ];
        if let Some(ref token) = next_token {
            params.push(param("NextToken", token));
        }

        let body = ec2_get(agent, creds, region, params)?;
        let resp: DescribeInstancesResponse = quick_xml::de::from_str(&body)
            .map_err(|e| ProviderError::Parse(format!("{}: {}", region, e)))?;

        for reservation in resp.reservation_set.item {
            for instance in reservation.instances_set.item {
                if instance.instance_state.name != "terminated"
                    && instance.instance_state.name != "shutting-down"
                {
                    all.push(instance);
                }
            }
        }

        match resp.next_token {
            Some(t) if !t.is_empty() => next_token = Some(t),
            _ => break,
        }
    }

    Ok(all)
}

/// Maximum AMI IDs per DescribeImages request to stay within AWS query limits.
const AMI_BATCH_SIZE: usize = 100;

/// Fetch AMI ID to name mapping (best effort, returns empty map on failure).
/// Batches requests to stay within AWS API limits.
fn fetch_image_names(
    agent: &ureq::Agent,
    creds: &AwsCredentials,
    region: &str,
    image_ids: &[String],
) -> Result<HashMap<String, String>, ProviderError> {
    if image_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut map = HashMap::new();
    for chunk in image_ids.chunks(AMI_BATCH_SIZE) {
        let mut params = vec![
            param("Action", "DescribeImages"),
            param("Version", "2016-11-15"),
        ];
        for (i, id) in chunk.iter().enumerate() {
            params.push(param(&format!("ImageId.{}", i + 1), id));
        }

        let body = ec2_get(agent, creds, region, params)?;
        let resp: DescribeImagesResponse = quick_xml::de::from_str(&body)
            .map_err(|e| ProviderError::Parse(format!("{}: {}", region, e)))?;

        for image in resp.images_set.item {
            if !image.name.is_empty() {
                map.insert(image.image_id, image.name);
            }
        }
    }
    Ok(map)
}

/// Extract Name tag value and user tags from an instance's tag set.
/// Filters out aws:* tags. Returns (name, tags) where tags are values only.
fn extract_tags(tag_set: &[Ec2Tag]) -> (String, Vec<String>) {
    let mut name = String::new();
    let mut tags = Vec::new();
    for tag in tag_set {
        if tag.key == "Name" {
            name = tag.value.clone();
        } else if !tag.key.starts_with("aws:") && !tag.value.is_empty() {
            tags.push(tag.value.clone());
        }
    }
    tags.sort();
    (name, tags)
}

// --- Provider trait ---

impl Provider for Aws {
    fn name(&self) -> &str {
        "aws"
    }

    fn short_label(&self) -> &str {
        "aws"
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
        if self.regions.is_empty() {
            return Err(ProviderError::Http(
                "No AWS regions configured. Add regions in the provider settings.".to_string(),
            ));
        }

        let valid_codes: HashSet<&str> = AWS_REGIONS.iter().map(|(c, _)| *c).collect();
        for region in &self.regions {
            if !valid_codes.contains(region.as_str()) {
                return Err(ProviderError::Http(format!(
                    "Unknown AWS region '{}'. Check your provider settings.",
                    region
                )));
            }
        }

        let creds = resolve_credentials(token, &self.profile)?;
        let agent = super::http_agent();
        let total_regions = self.regions.len();
        let mut all_hosts = Vec::new();
        let mut failed_regions = 0usize;

        for (i, region) in self.regions.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            progress(&format!(
                "Fetching {} ({}/{})...",
                region,
                i + 1,
                total_regions
            ));

            let instances = match describe_instances(&agent, &creds, region, cancel) {
                Ok(instances) => instances,
                Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
                Err(ProviderError::AuthFailed) => return Err(ProviderError::AuthFailed),
                Err(ProviderError::RateLimited) => return Err(ProviderError::RateLimited),
                Err(_) => {
                    failed_regions += 1;
                    continue;
                }
            };

            // Collect unique AMI IDs for OS metadata lookup
            let ami_ids: Vec<String> = {
                let mut set = HashSet::new();
                for inst in &instances {
                    if !inst.image_id.is_empty() {
                        set.insert(inst.image_id.clone());
                    }
                }
                set.into_iter().collect()
            };

            // Fetch AMI names (best effort)
            let ami_names = if !ami_ids.is_empty() {
                progress(&format!("Resolving AMIs for {}...", region));
                fetch_image_names(&agent, &creds, region, &ami_ids).unwrap_or_default()
            } else {
                HashMap::new()
            };

            for instance in instances {
                let ip = match instance.ip_address {
                    Some(ref ip) if !ip.is_empty() => ip.clone(),
                    _ => match instance.private_ip_address {
                        Some(ref ip) if !ip.is_empty() => ip.clone(),
                        _ => continue,
                    },
                };

                let (name, tags) = extract_tags(&instance.tag_set.item);
                let name = if name.is_empty() {
                    instance.instance_id.clone()
                } else {
                    name
                };

                let mut metadata = Vec::new();
                metadata.push(("region".to_string(), region.clone()));
                if !instance.instance_type.is_empty() {
                    metadata.push(("instance".to_string(), instance.instance_type.clone()));
                }
                if let Some(os_name) = ami_names.get(&instance.image_id) {
                    metadata.push(("os".to_string(), os_name.clone()));
                }
                if !instance.instance_state.name.is_empty() {
                    metadata.push(("status".to_string(), instance.instance_state.name.clone()));
                }

                all_hosts.push(ProviderHost {
                    server_id: instance.instance_id,
                    name,
                    ip,
                    tags,
                    metadata,
                });
            }
        }

        // Summary
        let mut parts = vec![format!("{} instances", all_hosts.len())];
        if failed_regions > 0 {
            parts.push(format!(
                "{} of {} regions failed",
                failed_regions, total_regions
            ));
        }
        progress(&parts.join(", "));

        if failed_regions > 0 {
            if all_hosts.is_empty() {
                return Err(ProviderError::Http(format!(
                    "All {} regions failed. Check your credentials and region configuration.",
                    total_regions,
                )));
            }
            return Err(ProviderError::PartialResult {
                hosts: all_hosts,
                failures: failed_regions,
                total: total_regions,
            });
        }

        Ok(all_hosts)
    }
}

#[cfg(test)]
#[path = "aws_tests.rs"]
mod tests;
