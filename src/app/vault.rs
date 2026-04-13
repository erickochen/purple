use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Vault SSH certificate and signing state.
pub struct VaultState {
    /// Cached vault certificate status per host alias.
    /// Tuple: (check timestamp, status, cert file mtime at check time).
    pub cert_cache: HashMap<
        String,
        (
            std::time::Instant,
            crate::vault_ssh::CertStatus,
            Option<std::time::SystemTime>,
        ),
    >,
    /// Aliases currently being checked for cert status (prevent duplicate checks).
    pub cert_checks_in_flight: HashSet<String>,
    /// Side-channel warning from cert-cache cleanup.
    pub cleanup_warning: Option<String>,
    /// Cancel flag for the V-key vault signing background thread.
    pub signing_cancel: Option<Arc<AtomicBool>>,
    /// JoinHandle for the V-key vault signing background thread.
    pub sign_thread: Option<std::thread::JoinHandle<()>>,
    /// Aliases currently being signed by the bulk V-key loop.
    pub sign_in_flight: Arc<Mutex<HashSet<String>>>,
}

impl Default for VaultState {
    fn default() -> Self {
        Self {
            cert_cache: HashMap::new(),
            cert_checks_in_flight: HashSet::new(),
            cleanup_warning: None,
            signing_cancel: None,
            sign_thread: None,
            sign_in_flight: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}
