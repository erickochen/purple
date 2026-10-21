use std::path::{Path, PathBuf};

/// Common SSH connection context passed to remote operations.
pub struct SshContext<'a> {
    pub alias: &'a str,
    pub config_path: &'a Path,
    pub askpass: Option<&'a str>,
    /// Password typed in the TUI for this session only. Reaches ssh through
    /// the one-shot askpass channel and is never written to disk.
    pub session_password: Option<&'a str>,
    pub bw_session: Option<&'a str>,
    pub has_tunnel: bool,
    /// Set only by a retry that follows the trust dialog: ssh runs with
    /// `StrictHostKeyChecking=accept-new` for that one run so the host key
    /// is recorded on first contact.
    pub trust_new_host_key: bool,
    /// Security key PINs this run may hand out plus the flow that asks for
    /// a missing one.
    pub security_key: &'a crate::security_key::SecurityKeyRun,
    pub env: &'a crate::runtime::env::Env,
}

/// Owned variant for spawning into threads.
pub struct OwnedSshContext {
    pub alias: String,
    pub config_path: PathBuf,
    pub askpass: Option<String>,
    pub session_password: Option<String>,
    pub bw_session: Option<String>,
    pub has_tunnel: bool,
    pub trust_new_host_key: bool,
    pub security_key: crate::security_key::SecurityKeyRun,
    pub env: std::sync::Arc<crate::runtime::env::Env>,
}

impl OwnedSshContext {
    /// Borrow every field for a synchronous call inside the worker thread.
    pub fn borrow(&self) -> SshContext<'_> {
        SshContext {
            alias: &self.alias,
            config_path: &self.config_path,
            askpass: self.askpass.as_deref(),
            session_password: self.session_password.as_deref(),
            bw_session: self.bw_session.as_deref(),
            has_tunnel: self.has_tunnel,
            trust_new_host_key: self.trust_new_host_key,
            security_key: &self.security_key,
            env: &self.env,
        }
    }
}
