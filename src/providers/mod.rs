pub mod config;
mod digitalocean;
mod hetzner;
mod linode;
mod proxmox;
pub mod sync;
mod upcloud;
mod vultr;

use std::sync::atomic::AtomicBool;

use thiserror::Error;

/// A host discovered from a cloud provider API.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProviderHost {
    /// Provider-assigned server ID.
    pub server_id: String,
    /// Server name/label.
    pub name: String,
    /// Public IP address (IPv4 or IPv6).
    pub ip: String,
    /// Provider tags/labels.
    pub tags: Vec<String>,
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
pub trait Provider {
    /// Full provider name (e.g. "digitalocean").
    fn name(&self) -> &str;
    /// Short label for aliases (e.g. "do").
    fn short_label(&self) -> &str;
    /// Fetch hosts with cancellation support.
    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError>;
    /// Fetch all servers from the provider API.
    #[allow(dead_code)]
    fn fetch_hosts(&self, token: &str) -> Result<Vec<ProviderHost>, ProviderError> {
        self.fetch_hosts_cancellable(token, &AtomicBool::new(false))
    }
    /// Fetch hosts with progress reporting. Default delegates to fetch_hosts_cancellable.
    fn fetch_hosts_with_progress(
        &self,
        token: &str,
        cancel: &AtomicBool,
        _progress: &dyn Fn(&str),
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        self.fetch_hosts_cancellable(token, cancel)
    }
}

/// All known provider names.
pub const PROVIDER_NAMES: &[&str] = &["digitalocean", "vultr", "linode", "hetzner", "upcloud", "proxmox"];

/// Get a provider implementation by name.
pub fn get_provider(name: &str) -> Option<Box<dyn Provider>> {
    match name {
        "digitalocean" => Some(Box::new(digitalocean::DigitalOcean)),
        "vultr" => Some(Box::new(vultr::Vultr)),
        "linode" => Some(Box::new(linode::Linode)),
        "hetzner" => Some(Box::new(hetzner::Hetzner)),
        "upcloud" => Some(Box::new(upcloud::UpCloud)),
        "proxmox" => Some(Box::new(proxmox::Proxmox {
            base_url: String::new(),
            verify_tls: true,
        })),
        _ => None,
    }
}

/// Get a provider implementation configured from a provider section.
/// For providers that need extra config (e.g. Proxmox base URL), this
/// creates a properly configured instance.
pub fn get_provider_with_config(name: &str, section: &config::ProviderSection) -> Option<Box<dyn Provider>> {
    match name {
        "proxmox" => Some(Box::new(proxmox::Proxmox {
            base_url: section.url.clone(),
            verify_tls: section.verify_tls,
        })),
        _ => get_provider(name),
    }
}

/// Display name for a provider (e.g. "digitalocean" -> "DigitalOcean").
pub fn provider_display_name(name: &str) -> &str {
    match name {
        "digitalocean" => "DigitalOcean",
        "vultr" => "Vultr",
        "linode" => "Linode",
        "hetzner" => "Hetzner",
        "upcloud" => "UpCloud",
        "proxmox" => "Proxmox VE",
        other => other,
    }
}

/// Create an HTTP agent with explicit timeouts.
pub(crate) fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .redirects(0)
        .build()
}

/// Create an HTTP agent that accepts invalid/self-signed TLS certificates.
pub(crate) fn http_agent_insecure() -> Result<ureq::Agent, ProviderError> {
    let tls = ureq::native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .map_err(|e| ProviderError::Http(format!("TLS setup failed: {}", e)))?;
    Ok(ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .redirects(0)
        .tls_connector(std::sync::Arc::new(tls))
        .build())
}

/// Strip CIDR suffix (/64, /128, etc.) from an IP address.
/// Some provider APIs return IPv6 addresses with prefix length (e.g. "2600:3c00::1/128").
/// SSH requires bare addresses without CIDR notation.
pub(crate) fn strip_cidr(ip: &str) -> &str {
    // Only strip if it looks like a CIDR suffix (slash followed by digits)
    if let Some(pos) = ip.rfind('/') {
        if ip[pos + 1..].bytes().all(|b| b.is_ascii_digit()) && pos + 1 < ip.len() {
            return &ip[..pos];
        }
    }
    ip
}

/// Map a ureq error to a ProviderError.
fn map_ureq_error(err: ureq::Error) -> ProviderError {
    match err {
        ureq::Error::Status(401, _) | ureq::Error::Status(403, _) => ProviderError::AuthFailed,
        ureq::Error::Status(429, _) => ProviderError::RateLimited,
        ureq::Error::Status(code, _) => ProviderError::Http(format!("HTTP {}", code)),
        ureq::Error::Transport(t) => ProviderError::Http(t.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_cidr_ipv6_with_prefix() {
        assert_eq!(strip_cidr("2600:3c00::1/128"), "2600:3c00::1");
        assert_eq!(strip_cidr("2a01:4f8::1/64"), "2a01:4f8::1");
    }

    #[test]
    fn test_strip_cidr_bare_ipv6() {
        assert_eq!(strip_cidr("2600:3c00::1"), "2600:3c00::1");
    }

    #[test]
    fn test_strip_cidr_ipv4_passthrough() {
        assert_eq!(strip_cidr("1.2.3.4"), "1.2.3.4");
        assert_eq!(strip_cidr("10.0.0.1/24"), "10.0.0.1");
    }

    #[test]
    fn test_strip_cidr_empty() {
        assert_eq!(strip_cidr(""), "");
    }
}
