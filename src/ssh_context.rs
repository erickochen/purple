use std::path::{Path, PathBuf};

/// Common SSH connection context passed to remote operations.
pub struct SshContext<'a> {
    pub alias: &'a str,
    pub config_path: &'a Path,
    pub askpass: Option<&'a str>,
    pub bw_session: Option<&'a str>,
    pub has_tunnel: bool,
}

/// Owned variant for spawning into threads.
pub struct OwnedSshContext {
    pub alias: String,
    pub config_path: PathBuf,
    pub askpass: Option<String>,
    pub bw_session: Option<String>,
    pub has_tunnel: bool,
}
