use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use super::{Provider, ProviderError, ProviderHost};

pub struct Aws {
    pub regions: Vec<String>,
    pub profile: String,
    /// Whether synced hosts reach their instance through Session Manager.
    pub ssm: super::aws_ssm::SsmMode,
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

pub(super) struct AwsCredentials {
    pub(super) access_key: String,
    pub(super) secret_key: String,
    /// `aws_session_token` / `AWS_SESSION_TOKEN`. Present for temporary
    /// credentials (access key IDs starting with `ASIA`) issued by STS via
    /// AssumeRole, IAM Identity Center (SSO) or GetSessionToken. Must be sent
    /// as a signed `x-amz-security-token` header or AWS rejects the request.
    pub(super) session_token: Option<String>,
}

/// Credentials, or what still has to happen before there are any.
pub(super) enum CredentialSource {
    /// Usable as they are.
    Ready(AwsCredentials),
    /// A base key pair plus the roles to assume from it, innermost first.
    AssumeRole {
        base: AwsCredentials,
        roles: Vec<super::aws_profile::RoleStep>,
    },
}

/// Work out where this config's credentials come from, without making a
/// network call. A profile that assumes a role reports the chain instead of
/// following it, so the caller decides when to spend an STS request.
///
/// Order: a configured profile wins outright, then the token field, then the
/// environment. A profile is used on its own so a failure there never reads as
/// a token problem.
fn resolve_credentials(
    token: &str,
    profile: &str,
    env: &crate::runtime::env::Env,
) -> Result<CredentialSource, ProviderError> {
    if !profile.is_empty() {
        return resolve_profile(profile, env);
    }
    // Token field: ACCESS_KEY_ID:SECRET_ACCESS_KEY[:SESSION_TOKEN]
    if let Some((ak, rest)) = token.split_once(':') {
        let (sk, st) = match rest.split_once(':') {
            Some((sk, st)) if !st.is_empty() => (sk, Some(st.to_string())),
            Some((sk, _)) => (sk, None),
            None => (rest, None),
        };
        if !ak.is_empty() && !sk.is_empty() {
            return Ok(CredentialSource::Ready(AwsCredentials {
                access_key: ak.to_string(),
                secret_key: sk.to_string(),
                session_token: st,
            }));
        }
    }
    // Environment variables, from the injected snapshot.
    if let Some((ak, sk)) = env.aws_credentials()
        && !ak.is_empty()
        && !sk.is_empty()
    {
        return Ok(CredentialSource::Ready(AwsCredentials {
            access_key: ak.to_string(),
            secret_key: sk.to_string(),
            session_token: env.aws_session_token().map(str::to_string),
        }));
    }
    // A config saved without a token and without a profile is valid, so name
    // the three sources here rather than point at a token that was never set.
    if token.trim().is_empty() {
        return Err(ProviderError::Execute(
            crate::i18n::text(crate::messages::AWS_NO_CREDENTIALS).to_string(),
        ));
    }
    Err(ProviderError::AuthFailed)
}

/// Resolve one named profile out of `~/.aws/config` and `~/.aws/credentials`.
fn resolve_profile(
    profile: &str,
    env: &crate::runtime::env::Env,
) -> Result<CredentialSource, ProviderError> {
    let profiles = super::aws_profile::AwsProfiles::load(
        env.aws_config_file().as_deref(),
        env.aws_credentials_file().as_deref(),
    );
    let chain = profiles
        .resolve_chain(profile)
        .map_err(|e| ProviderError::Execute(chain_error_message(&e, &profiles)))?;
    let base = AwsCredentials {
        access_key: chain.base.access_key_id.clone(),
        secret_key: chain.base.secret_access_key.clone(),
        session_token: (!chain.base.session_token.is_empty())
            .then(|| chain.base.session_token.clone()),
    };
    if chain.roles.is_empty() {
        Ok(CredentialSource::Ready(base))
    } else {
        Ok(CredentialSource::AssumeRole {
            base,
            roles: chain.roles,
        })
    }
}

/// Picker suffix for a profile purple will refuse, naming the reason in the
/// few words a row has. The full sentence is `chain_error_message`.
pub(crate) fn chain_error_note(error: &super::aws_profile::ChainError) -> &'static str {
    use super::aws_profile::ChainError;
    match error {
        ChainError::SsoNotSupported(_) => crate::i18n::text(crate::messages::PROFILE_NOTE_SSO),
        ChainError::WebIdentity(_) => crate::i18n::text(crate::messages::PROFILE_NOTE_WEB_IDENTITY),
        ChainError::CredentialProcess(_) => {
            crate::i18n::text(crate::messages::PROFILE_NOTE_CREDENTIAL_PROCESS)
        }
        ChainError::CredentialSource(_) => {
            crate::i18n::text(crate::messages::PROFILE_NOTE_CREDENTIAL_SOURCE)
        }
        ChainError::MfaRequired(_) => crate::i18n::text(crate::messages::PROFILE_NOTE_MFA),
        ChainError::Loop(_) | ChainError::TooDeep(_) => {
            crate::i18n::text(crate::messages::PROFILE_NOTE_CHAIN)
        }
        ChainError::RoleWithoutSource(_) => {
            crate::i18n::text(crate::messages::PROFILE_NOTE_NO_SOURCE)
        }
        // Every row the picker draws is a profile that exists, so a missing
        // one can only be the profile a source_profile points at.
        ChainError::Missing(_) => crate::i18n::text(crate::messages::PROFILE_NOTE_MISSING_SOURCE),
        ChainError::FileUnreadable(_) => {
            crate::i18n::text(crate::messages::PROFILE_NOTE_UNREADABLE)
        }
        ChainError::NoKeys(_) | ChainError::PartialCredentials(..) => {
            crate::i18n::text(crate::messages::PROFILE_NOTE_NO_KEYS)
        }
    }
}

/// User-facing wording for a profile chain that did not resolve.
fn chain_error_message(
    error: &super::aws_profile::ChainError,
    profiles: &super::aws_profile::AwsProfiles,
) -> String {
    use super::aws_profile::ChainError;
    match error {
        ChainError::Missing(name) => crate::messages::aws_profile_not_found(
            name,
            &profiles.sources(),
            &profiles.names().join(", "),
        ),
        ChainError::NoKeys(name) => crate::messages::aws_profile_without_keys(name),
        ChainError::PartialCredentials(name, missing) => {
            crate::messages::aws_profile_partial_credentials(name, missing)
        }
        ChainError::RoleWithoutSource(name) => crate::messages::aws_role_without_source(name),
        ChainError::CredentialSource(name) => {
            crate::messages::aws_credential_source_unsupported(name)
        }
        ChainError::CredentialProcess(name) => {
            crate::messages::aws_credential_process_unsupported(name)
        }
        ChainError::WebIdentity(name) => crate::messages::aws_web_identity_unsupported(name),
        ChainError::SsoNotSupported(name) => crate::messages::aws_sso_unsupported(name),
        ChainError::MfaRequired(name) => crate::messages::aws_mfa_unsupported(name),
        ChainError::Loop(name) => crate::messages::aws_profile_loop(name, &profiles.sources()),
        ChainError::TooDeep(name) => {
            crate::messages::aws_profile_chain_too_deep(name, &profiles.sources())
        }
        ChainError::FileUnreadable(path) => crate::messages::aws_credentials_file_unreadable(path),
    }
}

// --- SigV4 signing ---

pub(super) fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub(super) fn sha256_hash(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    // INVARIANT: `Hmac::<Sha256>::new_from_slice` only fails when the MAC
    // implementation rejects the key length. HMAC-SHA256 accepts keys of any
    // length (RFC 2104 §2), so this branch is unreachable for Hmac<Sha256>.
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .expect("Hmac::<Sha256>::new_from_slice accepts any key length (RFC 2104)");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// RFC 3986 URI encoding (delegates to shared implementation).
pub(super) fn uri_encode(s: &str) -> String {
    super::percent_encode(s)
}

/// Format epoch seconds as (timestamp, datestamp) for SigV4.
pub(super) fn format_utc(epoch_secs: u64) -> (String, String) {
    let d = super::epoch_to_date(epoch_secs);
    let timestamp = format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        d.year, d.month, d.day, d.hours, d.minutes, d.seconds,
    );
    let datestamp = format!("{:04}{:02}{:02}", d.year, d.month, d.day);
    (timestamp, datestamp)
}

/// One request to sign. The three AWS services purple talks to differ in
/// method, service name, payload and which headers are covered, so they are
/// inputs rather than constants: EC2 and STS are GET with an empty body over
/// the Query API, while Systems Manager is a JSON POST that must also sign
/// `content-type` and `x-amz-target`.
pub(super) struct SigV4Request<'a> {
    pub(super) method: &'a str,
    pub(super) service: &'a str,
    pub(super) host: &'a str,
    pub(super) query_string: &'a str,
    pub(super) payload: &'a [u8],
    /// Extra headers to cover, as lowercase name and value. Sorted in with the
    /// rest, so a caller passes them in any order.
    pub(super) extra_headers: &'a [(&'a str, &'a str)],
}

/// Build the SigV4 Authorization header value.
pub(super) fn sign_request(
    creds: &AwsCredentials,
    region: &str,
    req: &SigV4Request<'_>,
    timestamp: &str,
    datestamp: &str,
) -> String {
    let payload_hash = hex_encode(&sha256_hash(req.payload));

    // Canonical headers are sorted by lowercase header name. With temporary
    // credentials `x-amz-security-token` joins the set and sorts after
    // `x-amz-date`; for a JSON POST `content-type` sorts before `host`.
    let mut headers: Vec<(&str, &str)> = vec![("host", req.host), ("x-amz-date", timestamp)];
    if let Some(token) = &creds.session_token {
        headers.push(("x-amz-security-token", token));
    }
    headers.extend_from_slice(req.extra_headers);
    headers.sort_by(|a, b| a.0.cmp(b.0));

    let canonical_headers: String = headers
        .iter()
        .map(|(name, value)| format!("{}:{}\n", name, value))
        .collect();
    let signed_headers = headers
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{}\n/\n{}\n{}\n{}\n{}",
        req.method, req.query_string, canonical_headers, signed_headers, payload_hash
    );

    let scope = format!("{}/{}/{}/aws4_request", datestamp, region, req.service);
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
    let k_service = hmac_sha256(&k_region, req.service.as_bytes());
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

/// SigV4 service name for the EC2 Query API.
const EC2_SERVICE: &str = "ec2";

/// EC2 Query API version pinned by the DescribeInstances and DescribeImages
/// calls below.
const EC2_API_VERSION: &str = "2016-11-15";

fn param(key: &str, value: &str) -> (String, String) {
    (key.to_string(), value.to_string())
}

/// Make a signed GET request to the EC2 API.
fn ec2_get(
    agent: &ureq::Agent,
    creds: &AwsCredentials,
    region: &str,
    endpoint: &str,
    params: Vec<(String, String)>,
) -> Result<String, ProviderError> {
    // Host used for SigV4 signing and the request URL. Derived from the
    // injected endpoint so tests can point the signed request at a mock; the
    // authority is everything after the scheme (e.g. "ec2.us-east-1.amazonaws.com"
    // or "127.0.0.1:1234").
    let host = endpoint
        .split_once("://")
        .map(|(_, authority)| authority)
        .unwrap_or(endpoint)
        .to_string();
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

    let auth = sign_request(
        creds,
        region,
        &SigV4Request {
            method: "GET",
            service: EC2_SERVICE,
            host: &host,
            query_string: &query_string,
            payload: b"",
            extra_headers: &[],
        },
        &timestamp,
        &datestamp,
    );
    let url = format!("{}/?{}", endpoint, query_string);

    let mut req = agent
        .get(&url)
        .header("Authorization", &auth)
        .header("x-amz-date", &timestamp);
    if let Some(token) = &creds.session_token {
        req = req.header("x-amz-security-token", token);
    }
    let mut resp = req.call().map_err(super::map_ureq_error)?;

    resp.body_mut()
        .read_to_string()
        .map_err(|e| ProviderError::Parse(e.to_string()))
}

/// Fetch all non-terminated instances in a region (handles pagination).
fn describe_instances(
    agent: &ureq::Agent,
    creds: &AwsCredentials,
    region: &str,
    endpoint: &str,
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
            param("Version", EC2_API_VERSION),
        ];
        if let Some(ref token) = next_token {
            params.push(param("NextToken", token));
        }

        let body = ec2_get(agent, creds, region, endpoint, params)?;
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
    endpoint: &str,
    image_ids: &[String],
) -> Result<HashMap<String, String>, ProviderError> {
    if image_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut map = HashMap::new();
    for chunk in image_ids.chunks(AMI_BATCH_SIZE) {
        let mut params = vec![
            param("Action", "DescribeImages"),
            param("Version", EC2_API_VERSION),
        ];
        for (i, id) in chunk.iter().enumerate() {
            params.push(param(&format!("ImageId.{}", i + 1), id));
        }

        let body = ec2_get(agent, creds, region, endpoint, params)?;
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

/// Per-region API hosts for one fetch. Production resolves the real AWS
/// endpoints; tests point both at a mock server so the whole signed pipeline
/// runs end to end.
struct Endpoints<'a> {
    ec2: &'a dyn Fn(&str) -> String,
    ssm: &'a dyn Fn(&str) -> String,
    sts: &'a dyn Fn(&str) -> String,
}

impl Aws {
    /// Real EC2 endpoint for a region. Overridable via `fetch_with_endpoint`
    /// so tests can point the signed request at a mock server.
    fn region_endpoint(region: &str) -> String {
        format!("https://ec2.{}.amazonaws.com", region)
    }

    /// Per-region fetch pipeline against caller-supplied endpoints. Production
    /// resolves the real EC2 host per region; tests pass a closure returning a
    /// mock URL so SigV4 signing, DescribeInstances + DescribeImages, XML
    /// deserialize and `ProviderHost` mapping all run end to end.
    fn fetch_with_endpoint(
        &self,
        endpoints: &Endpoints<'_>,
        token: &str,
        cancel: &AtomicBool,
        env: &crate::runtime::env::Env,
        progress: &dyn Fn(&str),
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        let resolve_endpoint = endpoints.ec2;
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

        if self.ssm.is_enabled()
            && !self.profile.is_empty()
            && !super::aws_ssm::is_safe_profile_name(&self.profile)
        {
            return Err(ProviderError::Execute(
                crate::messages::aws_ssm_profile_unsafe(&self.profile),
            ));
        }

        let agent = super::http_agent();
        // The STS region is the first configured one: it is validated above,
        // and pinning it keeps every assume-role call on one regional endpoint
        // instead of the global one.
        let sts_region = self.regions[0].clone();
        let creds = match resolve_credentials(token, &self.profile, env)? {
            CredentialSource::Ready(creds) => creds,
            CredentialSource::AssumeRole { base, roles } => {
                progress(&crate::messages::aws_assuming_roles(roles.len()));
                super::aws_sts::assume_chain_with_endpoint(
                    &agent,
                    base,
                    &roles,
                    &sts_region,
                    cancel,
                    endpoints.sts,
                )?
            }
        };
        let total_regions = self.regions.len();
        let mut all_hosts = Vec::new();
        let mut failed_regions = 0usize;
        // The first region's own reason, kept for the summary error. Without
        // it a missing IAM action reads as a credential problem.
        let mut first_failure: Option<String> = None;

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

            // `auto` asks Systems Manager which nodes can take a session;
            // `always` skips the lookup for a caller allowed to open a session
            // but not to list nodes. A failed lookup fails the region rather
            // than silently leaving every host on its IP address.
            let ssm_nodes = match self.ssm {
                super::aws_ssm::SsmMode::Auto => {
                    progress(&crate::messages::aws_ssm_checking(region));
                    match super::aws_ssm::online_nodes_with_endpoint(
                        &agent,
                        &creds,
                        region,
                        cancel,
                        &(endpoints.ssm)(region),
                    ) {
                        Ok(nodes) => nodes,
                        Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
                        Err(e) => {
                            log::warn!("[external] aws ssm: {} lookup failed: {}", region, e);
                            let reason =
                                crate::messages::aws_ssm_lookup_failed(region, &e.to_string());
                            progress(&reason);
                            first_failure.get_or_insert(reason);
                            failed_regions += 1;
                            continue;
                        }
                    }
                }
                _ => HashSet::new(),
            };

            let endpoint = resolve_endpoint(region);
            let instances = match describe_instances(&agent, &creds, region, &endpoint, cancel) {
                Ok(instances) => instances,
                Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
                Err(ProviderError::AuthFailed) => return Err(ProviderError::AuthFailed),
                Err(ProviderError::RateLimited) => return Err(ProviderError::RateLimited),
                Err(e) => {
                    log::warn!("[external] aws ec2: {} listing failed: {}", region, e);
                    first_failure.get_or_insert_with(|| {
                        crate::messages::aws_region_failed(region, &e.to_string())
                    });
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
                fetch_image_names(&agent, &creds, region, &endpoint, &ami_ids).unwrap_or_default()
            } else {
                HashMap::new()
            };

            for instance in instances {
                // Session Manager reaches an instance by ID over a tunnel the
                // node opens outbound, so an instance with no address at all
                // is still reachable.
                let via_ssm = match self.ssm {
                    super::aws_ssm::SsmMode::Off => false,
                    super::aws_ssm::SsmMode::Always => true,
                    super::aws_ssm::SsmMode::Auto => ssm_nodes.contains(&instance.instance_id),
                };

                // With Session Manager the HostName is the instance ID, which
                // is what the proxy command's `%h` passes to `--target`.
                //
                // An instance with neither routing reports an empty address
                // rather than being dropped from the result. Empty means "this
                // exists but purple cannot reach it", which keeps it out of
                // the stale and `--remove` paths; dropping it would let a
                // running instance be deleted from the user's config, and an
                // instance that was reachable over Session Manager a moment
                // ago lands here the instant the mode is turned off.
                let ip = if via_ssm {
                    instance.instance_id.clone()
                } else {
                    match instance.ip_address {
                        Some(ref ip) if !ip.is_empty() => ip.clone(),
                        _ => match instance.private_ip_address {
                            Some(ref ip) if !ip.is_empty() => ip.clone(),
                            _ => String::new(),
                        },
                    }
                };

                let (directives, retract_directives) = if via_ssm {
                    (
                        vec![(
                            "ProxyCommand".to_string(),
                            super::aws_ssm::proxy_command(&self.profile, region),
                        )],
                        Vec::new(),
                    )
                } else if ip.is_empty() {
                    // Nothing to fall back to, so nothing is withdrawn: the
                    // proxy command is the only thing still reaching this
                    // host. Sync skips a host with no address before it reads
                    // either list, so this is what keeps the intent true if
                    // that ever changes.
                    (Vec::new(), Vec::new())
                } else {
                    // Withdraw a proxy command purple generated here, so
                    // turning Session Manager off puts the host back on its
                    // address instead of leaving a dead command behind. The
                    // profile segment is the one part the config can change
                    // between syncs, so the rule reads the line's shape rather
                    // than one exact value; a command the user wrote
                    // themselves, which opens with the same line AWS
                    // publishes, is not that shape and stays.
                    (
                        Vec::new(),
                        vec![super::RetractDirective {
                            key: "ProxyCommand".to_string(),
                            owns: super::aws_ssm::is_generated_proxy_command,
                            context: region.clone(),
                        }],
                    )
                };

                let (name, tags) = extract_tags(&instance.tag_set.item);
                let name = if name.is_empty() {
                    instance.instance_id.clone()
                } else {
                    name
                };

                let mut metadata = super::ProviderMetadata::new();
                metadata.push("region", region.clone());
                if !instance.instance_type.is_empty() {
                    metadata.push("instance", instance.instance_type.clone());
                }
                if let Some(os_name) = ami_names.get(&instance.image_id) {
                    metadata.push("os", os_name.clone());
                }
                if !instance.instance_state.name.is_empty() {
                    metadata.push("status", instance.instance_state.name.clone());
                }
                if via_ssm {
                    metadata.push("via", "Session Manager");
                }

                all_hosts.push(ProviderHost {
                    server_id: instance.instance_id,
                    name,
                    ip,
                    tags,
                    metadata: metadata.finish(),
                    directives,
                    retract_directives,
                    ..Default::default()
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
                return Err(ProviderError::Http(
                    crate::messages::aws_no_instances_after_failures(
                        failed_regions,
                        total_regions,
                        first_failure.as_deref(),
                    ),
                ));
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
        env: &crate::runtime::env::Env,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        self.fetch_hosts_with_progress(token, cancel, env, &|_| {})
    }

    fn fetch_hosts_with_progress(
        &self,
        token: &str,
        cancel: &AtomicBool,
        env: &crate::runtime::env::Env,
        progress: &dyn Fn(&str),
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        self.fetch_with_endpoint(
            &Endpoints {
                ec2: &Self::region_endpoint,
                ssm: &super::aws_ssm::region_endpoint,
                sts: &super::aws_sts::region_endpoint,
            },
            token,
            cancel,
            env,
            progress,
        )
    }
}

#[cfg(test)]
#[path = "aws_tests.rs"]
mod tests;
