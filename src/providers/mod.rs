pub mod aws;
pub mod aws_profile;
pub mod aws_ssm;
pub mod aws_sts;
pub mod azure;
pub mod config;
mod digitalocean;
pub mod gcp;
mod hetzner;
mod i3d;
pub mod kind;
mod leaseweb;
mod linode;
mod netbox;
pub mod oracle;
pub mod ovh;
mod proxmox;
pub mod scaleway;
pub mod sync;
mod tailscale;
mod teleport;
mod transip;
mod upcloud;
mod vultr;

pub use kind::ProviderKind;

use std::sync::atomic::{AtomicBool, Ordering};

use log::{debug, error, warn};
use thiserror::Error;

/// A host discovered from a cloud provider API.
#[derive(Debug, Clone, Default)]
pub struct ProviderHost {
    /// Provider-assigned server ID.
    pub server_id: String,
    /// Server name/label.
    pub name: String,
    /// Public IP address (IPv4 or IPv6) or a hostname the transport resolves.
    pub ip: String,
    /// Provider tags/labels.
    pub tags: Vec<String>,
    /// Provider metadata (region, plan, etc.) as key-value pairs.
    pub metadata: Vec<(String, String)>,
    /// SSH port when the provider knows it differs from 22.
    pub port: Option<u16>,
    /// Extra ssh_config directives the provider manages on this host, such as
    /// a `ProxyCommand`. Written on add and kept current on later syncs.
    pub directives: Vec<(String, String)>,
    /// Directives the provider once wrote here and now wants gone.
    pub retract_directives: Vec<RetractDirective>,
}

/// A directive the provider once wrote on a host and now withdraws.
///
/// A key the same host also lists in `directives` is written rather than
/// withdrawn: the provider wants that value, so the rule is skipped.
///
/// The provider decides whether the value on disk is one of its own, because
/// only it knows the shape it writes. A transport's command usually follows
/// the vendor's published line, so its opening is exactly what a user who set
/// the same thing up by hand would have typed; recognizing one by its opening
/// would delete the other. A value the provider does not claim is left alone,
/// so the worst case is a stale line the user can see and delete, never a line
/// silently removed.
#[derive(Debug, Clone)]
pub struct RetractDirective {
    /// The directive keyword, e.g. `ProxyCommand`.
    pub key: String,
    /// Whether `value` is one this provider wrote. `context` carries whatever
    /// the decision needs, which for AWS is the region. A plain function
    /// pointer rather than a closure, so `ProviderHost` stays `Clone`.
    pub owns: fn(value: &str, context: &str) -> bool,
    pub context: String,
}

impl RetractDirective {
    /// Whether this rule claims the value currently on the host.
    pub fn claims(&self, value: &str) -> bool {
        (self.owns)(value, &self.context)
    }
}

impl ProviderHost {
    /// Create a ProviderHost with no metadata.
    #[allow(dead_code)]
    pub fn new(server_id: String, name: String, ip: String, tags: Vec<String>) -> Self {
        Self {
            server_id,
            name,
            ip,
            tags,
            ..Default::default()
        }
    }
}

/// Errors from provider API calls.
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("Failed to parse response: {0}")]
    Parse(String),
    #[error("Authentication failed. Check your API token.")]
    AuthFailed,
    #[error("Rate limited. Try again in a moment.")]
    RateLimited,
    #[error("{0}")]
    Execute(String),
    #[error("Cancelled.")]
    Cancelled,
    /// Some hosts were fetched but others failed. The caller should use the
    /// hosts but suppress destructive operations like --remove.
    #[error("Partial result: {failures} of {total} failed")]
    PartialResult {
        hosts: Vec<ProviderHost>,
        failures: usize,
        total: usize,
    },
}

/// Trait implemented by each cloud provider.
///
/// The trait deliberately stays narrow: every provider implements
/// `fetch_hosts_cancellable` itself rather than overriding a uniform
/// fetch loop. Variation is too wide for a single shape. Linode and
/// DigitalOcean use `?page=N&per_page=N`; GCP uses `?maxResults=N`
/// plus a `pageToken` cursor across an aggregated multi-zone listing;
/// Azure paginates across multiple subscriptions; AWS uses SigV4 plus
/// the EC2 query API; Tailscale mixes Basic and Bearer auth depending
/// on key prefix; Oracle signs each request with a per-tenancy RSA
/// key.
///
/// What is shared lives in this module as free helpers so every
/// provider opts in to the parts that apply:
///   - `bearer_auth`: `Authorization: Bearer <token>` formatter.
///   - `ProviderMetadata`: typed builder for `ProviderHost::metadata`.
///   - `paginate`: cancellable page-walk with `MAX_PAGES` guard.
///   - `http_agent` / `http_agent_insecure`: ureq agent setup.
///   - `map_ureq_error`: HTTP error to `ProviderError` mapping
///     (401 to `AuthFailed`, 429 to `RateLimited`, etc).
///   - `strip_cidr`, `percent_encode`, `epoch_to_date`: string and
///     URL helpers.
///
/// Adding a new provider: implement `fetch_hosts_cancellable` and
/// reach for the helpers above instead of reimplementing them. Do
/// not invent provider-specific copies of these primitives; that is
/// where past parity bugs have hidden.
pub trait Provider {
    /// Full provider name (e.g. "digitalocean").
    fn name(&self) -> &str;
    /// Short label for aliases (e.g. "do").
    fn short_label(&self) -> &str;
    /// Fetch hosts with cancellation support. `env` carries the resolved
    /// process environment (home directory, credential env vars) so the few
    /// providers that read AWS credentials or expand `~` in key paths take
    /// them from the injected snapshot instead of ambient `std::env` /
    /// `dirs::home_dir`. Most providers ignore it.
    #[allow(dead_code)]
    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
        env: &crate::runtime::env::Env,
    ) -> Result<Vec<ProviderHost>, ProviderError>;
    /// Fetch all servers from the provider API.
    #[allow(dead_code)]
    fn fetch_hosts(
        &self,
        token: &str,
        env: &crate::runtime::env::Env,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        self.fetch_hosts_cancellable(token, &AtomicBool::new(false), env)
    }
    /// Fetch hosts with progress reporting. Default delegates to fetch_hosts_cancellable.
    #[allow(dead_code)]
    fn fetch_hosts_with_progress(
        &self,
        token: &str,
        cancel: &AtomicBool,
        env: &crate::runtime::env::Env,
        _progress: &dyn Fn(&str),
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        self.fetch_hosts_cancellable(token, cancel, env)
    }
}

/// Parse a comma-separated provider config field into a list of trimmed,
/// non-empty entries. Used for regions/zones/subscriptions.
fn parse_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Format an `Authorization: Bearer <token>` header value. Centralised so
/// the literal lives in one place across the 9-plus providers that use
/// bearer auth; a typo in any of them would silently break sync.
pub(crate) fn bearer_auth(token: &str) -> String {
    format!("Bearer {token}")
}

/// Builder for `ProviderHost::metadata`. Replaces the
/// `metadata.push(("region".to_string(), value.clone()))` boilerplate
/// that repeated across every provider with a typed builder so callers
/// only spell the value once and never clone strings by hand.
///
/// Conventions: keys are spelled as `&'static str` literals on each
/// call so a typo is a compile error rather than a silent runtime
/// mismatch downstream. The canonical key set (`region`, `status`,
/// `plan`, `image`, `os`, `location`, `type`, `zone`, `size`, `instance`,
/// `project`, `compartment`, `tenancy`) lives in the trait docs.
#[derive(Debug, Default)]
pub(crate) struct ProviderMetadata(Vec<(String, String)>);

impl ProviderMetadata {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    /// Append `(key, value)` and return the builder for chaining.
    /// `value: impl Into<String>` accepts both `String` and `&str`, so
    /// callers do not have to `.to_string()` their fields by hand.
    pub(crate) fn push(&mut self, key: &'static str, value: impl Into<String>) -> &mut Self {
        self.0.push((key.to_string(), value.into()));
        self
    }

    /// Append `(key, value)` only when `value` is `Some`. Saves the
    /// repeated `if let Some(v) = ... { metadata.push(...) }` wrapper.
    pub(crate) fn push_opt<S: Into<String>>(
        &mut self,
        key: &'static str,
        value: Option<S>,
    ) -> &mut Self {
        if let Some(v) = value {
            self.0.push((key.to_string(), v.into()));
        }
        self
    }

    /// Consume the builder, yielding the canonical `Vec<(String,
    /// String)>` shape that `ProviderHost::metadata` expects.
    pub(crate) fn finish(self) -> Vec<(String, String)> {
        self.0
    }
}

/// Factory for a provider implementation from an optional config section.
/// `None` yields a default-constructed instance; `Some(section)` wires the
/// section's fields into the provider struct.
type ProviderBuild = fn(Option<&config::ProviderSection>) -> Box<dyn Provider>;

/// Static registry entry describing one provider. Adding a provider means
/// adding exactly one `ProviderDescriptor` to `PROVIDERS` below.
pub struct ProviderDescriptor {
    /// Slug used in config files and aliases.
    pub name: &'static str,
    /// Human-readable name shown in the UI.
    pub display: &'static str,
    /// Builder. Must not allocate or fail.
    pub build: ProviderBuild,
}

/// Single source of truth for the provider registry. Adding a new provider
/// means one entry here plus the provider module itself.
pub const PROVIDERS: &[ProviderDescriptor] = &[
    ProviderDescriptor {
        name: "digitalocean",
        display: "DigitalOcean",
        build: |_| Box::new(digitalocean::DigitalOcean),
    },
    ProviderDescriptor {
        name: "vultr",
        display: "Vultr",
        build: |_| Box::new(vultr::Vultr),
    },
    ProviderDescriptor {
        name: "linode",
        display: "Linode",
        build: |_| Box::new(linode::Linode),
    },
    ProviderDescriptor {
        name: "hetzner",
        display: "Hetzner",
        build: |_| Box::new(hetzner::Hetzner),
    },
    ProviderDescriptor {
        name: "upcloud",
        display: "UpCloud",
        build: |_| Box::new(upcloud::UpCloud),
    },
    ProviderDescriptor {
        name: "proxmox",
        display: "Proxmox VE",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(proxmox::Proxmox {
                base_url: s.url,
                verify_tls: s.verify_tls,
            })
        },
    },
    ProviderDescriptor {
        name: "aws",
        display: "AWS EC2",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(aws::Aws {
                regions: parse_csv(&s.regions),
                profile: s.profile,
                ssm: s.ssm,
            })
        },
    },
    ProviderDescriptor {
        name: "scaleway",
        display: "Scaleway",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(scaleway::Scaleway {
                zones: parse_csv(&s.regions),
            })
        },
    },
    ProviderDescriptor {
        name: "gcp",
        display: "GCP",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(gcp::Gcp {
                zones: parse_csv(&s.regions),
                project: s.project,
            })
        },
    },
    ProviderDescriptor {
        name: "azure",
        display: "Azure",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(azure::Azure {
                subscriptions: parse_csv(&s.regions),
            })
        },
    },
    ProviderDescriptor {
        name: "tailscale",
        display: "Tailscale",
        build: |_| Box::new(tailscale::Tailscale),
    },
    ProviderDescriptor {
        name: "oracle",
        display: "Oracle Cloud",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(oracle::Oracle {
                regions: parse_csv(&s.regions),
                compartment: s.compartment,
            })
        },
    },
    ProviderDescriptor {
        name: "ovh",
        display: "OVHcloud",
        // OVH overloads `regions` as the API endpoint (e.g. "ovh-eu").
        // Known quirk flagged in the architecture review; kept as-is to
        // avoid schema migration in this refactor.
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(ovh::Ovh {
                project: s.project,
                endpoint: s.regions,
            })
        },
    },
    ProviderDescriptor {
        name: "leaseweb",
        display: "Leaseweb",
        build: |_| Box::new(leaseweb::Leaseweb),
    },
    ProviderDescriptor {
        name: "i3d",
        display: "i3D.net",
        build: |_| Box::new(i3d::I3d),
    },
    ProviderDescriptor {
        name: "transip",
        display: "TransIP",
        build: |_| Box::new(transip::TransIp),
    },
    ProviderDescriptor {
        name: "teleport",
        display: "Teleport",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(teleport::Teleport {
                identity_file: s.identity_file,
            })
        },
    },
    ProviderDescriptor {
        name: "netbox",
        display: "NetBox",
        build: |section| {
            let s = section.cloned().unwrap_or_default();
            Box::new(netbox::NetBox {
                base_url: s.url,
                verify_tls: s.verify_tls,
                filter: s.filter,
                interface: s.interface,
                ssh_port: s.ssh_port,
            })
        },
    },
];

/// Look up a descriptor by bare provider name. Internal helper; public wrappers
/// below strip any `:label` suffix before calling this, so callers cannot
/// accidentally pass a labeled id (`proxmox:server1`) and silently miss.
fn descriptor(name: &str) -> Option<&'static ProviderDescriptor> {
    PROVIDERS.iter().find(|p| p.name == name)
}

/// Return the bare provider name, stripping an optional `:label` suffix.
/// `ProviderConfigId` is the canonical home for this split; this helper keeps
/// `&str`-only public APIs label-tolerant without forcing every caller to
/// parse first.
fn bare_provider_name(name: &str) -> &str {
    name.split_once(':').map(|(p, _)| p).unwrap_or(name)
}

/// All known provider names, in registration order.
pub const PROVIDER_NAMES: &[&str] = &[
    "digitalocean",
    "vultr",
    "linode",
    "hetzner",
    "upcloud",
    "proxmox",
    "aws",
    "scaleway",
    "gcp",
    "azure",
    "tailscale",
    "oracle",
    "ovh",
    "leaseweb",
    "i3d",
    "transip",
    "teleport",
    "netbox",
];

// Compile-time guard: PROVIDER_NAMES and PROVIDERS must stay in lockstep.
const _: () = {
    assert!(
        PROVIDER_NAMES.len() == PROVIDERS.len(),
        "PROVIDER_NAMES and PROVIDERS length must match",
    );
};

/// Get a provider implementation by name with default configuration. Accepts
/// either a bare provider name (`"proxmox"`) or a labeled id (`"proxmox:server1"`).
pub fn get_provider(name: &str) -> Option<Box<dyn Provider>> {
    descriptor(bare_provider_name(name)).map(|d| (d.build)(None))
}

/// Get a provider implementation configured from a provider section. The bare
/// provider name comes from `section.id.provider`, so labeled configs resolve
/// the right descriptor by construction; passing a separate `name` was
/// historically a foot-gun (issue #51) where callers handed in the labeled id
/// string and the lookup missed the registry.
pub fn get_provider_with_config(section: &config::ProviderSection) -> Option<Box<dyn Provider>> {
    descriptor(section.provider()).map(|d| (d.build)(Some(section)))
}

/// Display name for a provider (e.g. "digitalocean" -> "DigitalOcean"). Accepts
/// either a bare name or a labeled id; unknown names fall back to the input.
pub fn provider_display_name(name: &str) -> &str {
    descriptor(bare_provider_name(name))
        .map(|d| d.display)
        .unwrap_or(name)
}

/// Create an HTTP agent with explicit timeouts.
pub(crate) fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .max_redirects(0)
        .build()
        .new_agent()
}

/// Create an HTTP agent that accepts invalid/self-signed TLS certificates.
pub(crate) fn http_agent_insecure() -> Result<ureq::Agent, ProviderError> {
    Ok(ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .max_redirects(0)
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .disable_verification(true)
                .build(),
        )
        .build()
        .new_agent())
}

/// Strip CIDR suffix (/64, /128, etc.) from an IP address.
/// Some provider APIs return IPv6 addresses with prefix length (e.g. "2600:3c00::1/128").
/// SSH requires bare addresses without CIDR notation.
pub(crate) fn strip_cidr(ip: &str) -> &str {
    // Only strip if it looks like a CIDR suffix (slash followed by digits)
    if let Some(pos) = ip.rfind('/')
        && ip[pos + 1..].bytes().all(|b| b.is_ascii_digit())
        && pos + 1 < ip.len()
    {
        return &ip[..pos];
    }
    ip
}

/// RFC 3986 percent-encoding for URL query parameters.
/// Encodes all characters except unreserved ones (A-Z, a-z, 0-9, '-', '_', '.', '~').
pub(crate) fn percent_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

/// Date components from a Unix epoch timestamp (no chrono dependency).
pub(crate) struct EpochDate {
    pub year: u64,
    pub month: u64, // 1-based
    pub day: u64,   // 1-based
    pub hours: u64,
    pub minutes: u64,
    pub seconds: u64,
    /// Days since epoch (for weekday calculation)
    pub epoch_days: u64,
}

/// Convert Unix epoch seconds to date components.
pub(crate) fn epoch_to_date(epoch_secs: u64) -> EpochDate {
    let secs_per_day = 86400u64;
    let epoch_days = epoch_secs / secs_per_day;
    let mut remaining_days = epoch_days;
    let day_secs = epoch_secs % secs_per_day;

    let mut year = 1970u64;
    loop {
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let days_in_year = if leap { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_per_month: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0usize;
    while month < 12 && remaining_days >= days_per_month[month] {
        remaining_days -= days_per_month[month];
        month += 1;
    }

    EpochDate {
        year,
        month: (month + 1) as u64,
        day: remaining_days + 1,
        hours: day_secs / 3600,
        minutes: (day_secs % 3600) / 60,
        seconds: day_secs % 60,
        epoch_days,
    }
}

/// The one status a caller reads rather than reports. A provider that takes
/// the status as data instead of letting ureq raise it still has to map this
/// one, because back-off depends on it.
pub(crate) const HTTP_TOO_MANY_REQUESTS: u16 = 429;

/// Map a ureq error to a ProviderError.
fn map_ureq_error(err: ureq::Error) -> ProviderError {
    match err {
        ureq::Error::StatusCode(code) => match code {
            401 | 403 => {
                error!("[external] HTTP {code}: authentication failed");
                ProviderError::AuthFailed
            }
            HTTP_TOO_MANY_REQUESTS => {
                warn!("[external] HTTP 429: rate limited");
                ProviderError::RateLimited
            }
            _ => {
                error!("[external] HTTP {code}");
                ProviderError::Http(format!("HTTP {}", code))
            }
        },
        other => {
            error!("[external] Request failed: {other}");
            ProviderError::Http(other.to_string())
        }
    }
}

/// Upper bound on pages fetched from one paginated list endpoint. A safety
/// valve for a provider that never signals its last page; 500 pages covers any
/// realistic account.
pub(crate) const MAX_PAGES: u64 = 500;

/// One mapped page from a paginated list endpoint.
pub(crate) struct PageResult {
    /// Hosts mapped from this page, appended to the running total.
    pub hosts: Vec<ProviderHost>,
    /// Whether another page should be fetched.
    pub more: bool,
}

/// Drive a paginated list endpoint under one shared cancellation, runaway-guard
/// and partial-failure contract, so every JSON list provider behaves the same.
///
/// `fetch_page(index)` performs one request (0-based page index), parses it and
/// maps entries into a `PageResult`. The closure owns its own cursor or
/// page-number state across calls.
///
/// Failure policy, matching what `sync` relies on: a failure on the first page
/// (nothing collected) propagates as a hard error so the provider is skipped
/// and the config left untouched. A failure on a later page returns the hosts
/// gathered so far as `PartialResult`, so add and update still run while remove
/// and stale marking are suppressed upstream. `AuthFailed`, `RateLimited` and
/// `Cancelled` always propagate immediately, even mid-run, because they
/// invalidate the whole sync.
pub(crate) fn paginate<F>(
    cancel: &AtomicBool,
    mut fetch_page: F,
) -> Result<Vec<ProviderHost>, ProviderError>
where
    F: FnMut(u64) -> Result<PageResult, ProviderError>,
{
    let mut hosts = Vec::new();
    let mut index = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }
        match fetch_page(index) {
            Ok(page) => {
                hosts.extend(page.hosts);
                if !page.more {
                    return Ok(hosts);
                }
            }
            Err(
                e @ (ProviderError::Cancelled
                | ProviderError::AuthFailed
                | ProviderError::RateLimited),
            ) => return Err(e),
            Err(e) => {
                if hosts.is_empty() {
                    return Err(e);
                }
                debug!(
                    "[external] paginate: page {} failed after {} hosts collected, returning partial result ({e})",
                    index + 1,
                    hosts.len()
                );
                return Err(ProviderError::PartialResult {
                    hosts,
                    failures: 1,
                    total: (index + 1) as usize,
                });
            }
        }
        index += 1;
        if index >= MAX_PAGES {
            debug!(
                "[purple] paginate: reached MAX_PAGES ({MAX_PAGES}) guard, stopping with {} hosts",
                hosts.len()
            );
            return Ok(hosts);
        }
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
