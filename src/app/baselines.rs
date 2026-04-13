//! Form baselines and dirty-state detection. Implements `impl App` continuation
//! with capture/compare logic for every form kind (host, tunnel, snippet,
//! provider) plus the mtime helpers that detect external config changes.

use std::path::PathBuf;
use std::time::SystemTime;

use super::{FormBaseline, ProviderFormBaseline, SnippetFormBaseline, TunnelFormBaseline};
use crate::app::App;
use crate::ssh_config::model::SshConfigFile;

impl App {
    /// Clear form mtime state (call on form cancel or successful submit).
    pub fn clear_form_mtime(&mut self) {
        self.conflict.form_mtime = None;
        self.conflict.form_include_mtimes.clear();
        self.conflict.form_include_dir_mtimes.clear();
        self.conflict.provider_form_mtime = None;
    }

    /// Capture config and Include file mtimes when opening a host form.
    pub fn capture_form_mtime(&mut self) {
        self.conflict.form_mtime = Self::get_mtime(&self.reload.config_path);
        self.conflict.form_include_mtimes = Self::snapshot_include_mtimes(&self.config);
        self.conflict.form_include_dir_mtimes = Self::snapshot_include_dir_mtimes(&self.config);
    }

    /// Capture ~/.purple/providers mtime when opening a provider form.
    pub fn capture_provider_form_mtime(&mut self) {
        let path = dirs::home_dir().map(|h| h.join(".purple/providers"));
        self.conflict.provider_form_mtime = path.as_ref().and_then(|p| Self::get_mtime(p));
    }

    /// Capture a baseline snapshot of the host form for dirty-check on Esc.
    pub fn capture_form_baseline(&mut self) {
        self.form_baseline = Some(FormBaseline {
            alias: self.form.alias.clone(),
            hostname: self.form.hostname.clone(),
            user: self.form.user.clone(),
            port: self.form.port.clone(),
            identity_file: self.form.identity_file.clone(),
            proxy_jump: self.form.proxy_jump.clone(),
            askpass: self.form.askpass.clone(),
            vault_ssh: self.form.vault_ssh.clone(),
            vault_addr: self.form.vault_addr.clone(),
            tags: self.form.tags.clone(),
        });
    }

    /// Check if the host form has been modified since baseline was captured.
    pub fn host_form_is_dirty(&self) -> bool {
        match &self.form_baseline {
            Some(b) => {
                self.form.alias != b.alias
                    || self.form.hostname != b.hostname
                    || self.form.user != b.user
                    || self.form.port != b.port
                    || self.form.identity_file != b.identity_file
                    || self.form.proxy_jump != b.proxy_jump
                    || self.form.askpass != b.askpass
                    || self.form.vault_ssh != b.vault_ssh
                    || self.form.vault_addr != b.vault_addr
                    || self.form.tags != b.tags
            }
            None => false,
        }
    }

    /// Capture a baseline snapshot of the tunnel form for dirty-check on Esc.
    pub fn capture_tunnel_form_baseline(&mut self) {
        self.tunnel_form_baseline = Some(TunnelFormBaseline {
            tunnel_type: self.tunnel_form.tunnel_type,
            bind_port: self.tunnel_form.bind_port.clone(),
            remote_host: self.tunnel_form.remote_host.clone(),
            remote_port: self.tunnel_form.remote_port.clone(),
            bind_address: self.tunnel_form.bind_address.clone(),
        });
    }

    /// Check if the tunnel form has been modified since baseline was captured.
    pub fn tunnel_form_is_dirty(&self) -> bool {
        match &self.tunnel_form_baseline {
            Some(b) => {
                self.tunnel_form.tunnel_type != b.tunnel_type
                    || self.tunnel_form.bind_port != b.bind_port
                    || self.tunnel_form.remote_host != b.remote_host
                    || self.tunnel_form.remote_port != b.remote_port
                    || self.tunnel_form.bind_address != b.bind_address
            }
            None => false,
        }
    }

    /// Capture a baseline snapshot of the snippet form for dirty-check on Esc.
    pub fn capture_snippet_form_baseline(&mut self) {
        self.snippet_form_baseline = Some(SnippetFormBaseline {
            name: self.snippet_form.name.clone(),
            command: self.snippet_form.command.clone(),
            description: self.snippet_form.description.clone(),
        });
    }

    /// Check if the snippet form has been modified since baseline was captured.
    pub fn snippet_form_is_dirty(&self) -> bool {
        match &self.snippet_form_baseline {
            Some(b) => {
                self.snippet_form.name != b.name
                    || self.snippet_form.command != b.command
                    || self.snippet_form.description != b.description
            }
            None => false,
        }
    }

    /// Capture a baseline snapshot of the provider form for dirty-check on Esc.
    pub fn capture_provider_form_baseline(&mut self) {
        self.provider_form_baseline = Some(ProviderFormBaseline {
            url: self.provider_form.url.clone(),
            token: self.provider_form.token.clone(),
            profile: self.provider_form.profile.clone(),
            project: self.provider_form.project.clone(),
            compartment: self.provider_form.compartment.clone(),
            regions: self.provider_form.regions.clone(),
            alias_prefix: self.provider_form.alias_prefix.clone(),
            user: self.provider_form.user.clone(),
            identity_file: self.provider_form.identity_file.clone(),
            verify_tls: self.provider_form.verify_tls,
            auto_sync: self.provider_form.auto_sync,
            vault_role: self.provider_form.vault_role.clone(),
            vault_addr: self.provider_form.vault_addr.clone(),
        });
    }

    /// Check if the provider form has been modified since baseline was captured.
    pub fn provider_form_is_dirty(&self) -> bool {
        match &self.provider_form_baseline {
            Some(b) => {
                self.provider_form.url != b.url
                    || self.provider_form.token != b.token
                    || self.provider_form.profile != b.profile
                    || self.provider_form.project != b.project
                    || self.provider_form.compartment != b.compartment
                    || self.provider_form.regions != b.regions
                    || self.provider_form.alias_prefix != b.alias_prefix
                    || self.provider_form.user != b.user
                    || self.provider_form.identity_file != b.identity_file
                    || self.provider_form.verify_tls != b.verify_tls
                    || self.provider_form.auto_sync != b.auto_sync
                    || self.provider_form.vault_role != b.vault_role
                    || self.provider_form.vault_addr != b.vault_addr
            }
            None => false,
        }
    }

    /// Check if config or any Include file/directory has changed since the form was opened.
    pub fn config_changed_since_form_open(&self) -> bool {
        match self.conflict.form_mtime {
            Some(open_mtime) => {
                if Self::get_mtime(&self.reload.config_path) != Some(open_mtime) {
                    return true;
                }
                self.conflict
                    .form_include_mtimes
                    .iter()
                    .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
                    || self
                        .conflict
                        .form_include_dir_mtimes
                        .iter()
                        .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
            }
            None => false,
        }
    }

    /// Check if ~/.purple/providers has changed since the provider form was opened.
    pub fn provider_config_changed_since_form_open(&self) -> bool {
        let path = dirs::home_dir().map(|h| h.join(".purple/providers"));
        let current_mtime = path.as_ref().and_then(|p| Self::get_mtime(p));
        self.conflict.provider_form_mtime != current_mtime
    }

    /// Snapshot mtimes of all resolved Include files.
    pub(super) fn snapshot_include_mtimes(
        config: &SshConfigFile,
    ) -> Vec<(PathBuf, Option<SystemTime>)> {
        config
            .include_paths()
            .into_iter()
            .map(|p| {
                let mtime = Self::get_mtime(&p);
                (p, mtime)
            })
            .collect()
    }

    /// Snapshot mtimes of parent directories of Include glob patterns.
    pub(super) fn snapshot_include_dir_mtimes(
        config: &SshConfigFile,
    ) -> Vec<(PathBuf, Option<SystemTime>)> {
        config
            .include_glob_dirs()
            .into_iter()
            .map(|p| {
                let mtime = Self::get_mtime(&p);
                (p, mtime)
            })
            .collect()
    }
}
