pub mod config;
mod digitalocean;
mod hetzner;
mod linode;
pub mod sync;
mod upcloud;
mod vultr;

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
}

/// Trait implemented by each cloud provider.
pub trait Provider {
    /// Full provider name (e.g. "digitalocean").
    fn name(&self) -> &str;
    /// Short label for aliases (e.g. "do").
    fn short_label(&self) -> &str;
    /// Fetch all servers from the provider API.
    fn fetch_hosts(&self, token: &str) -> Result<Vec<ProviderHost>, ProviderError>;
}

/// All known provider names.
pub const PROVIDER_NAMES: &[&str] = &["digitalocean", "vultr", "linode", "hetzner", "upcloud"];

/// Get a provider implementation by name.
pub fn get_provider(name: &str) -> Option<Box<dyn Provider>> {
    match name {
        "digitalocean" => Some(Box::new(digitalocean::DigitalOcean)),
        "vultr" => Some(Box::new(vultr::Vultr)),
        "linode" => Some(Box::new(linode::Linode)),
        "hetzner" => Some(Box::new(hetzner::Hetzner)),
        "upcloud" => Some(Box::new(upcloud::UpCloud)),
        _ => None,
    }
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
