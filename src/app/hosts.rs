//! Host CRUD operations. Implements `impl App` continuation with host add,
//! edit, deletion, sync-result application, and the nearby selection helpers
//! that skip group headers.

use super::{GroupBy, HostListItem};
use crate::app::App;
use crate::ssh_config::model::HostEntry;

impl App {
    pub fn add_host_from_form(&mut self) -> Result<String, String> {
        let entry = self.form.to_entry();
        let alias = entry.alias.clone();
        let duplicate = if self.form.is_pattern {
            self.config.has_host_block(&alias)
        } else {
            self.config.has_host(&alias)
        };
        if duplicate {
            return Err(if self.form.is_pattern {
                format!("Pattern '{}' already exists.", alias)
            } else {
                format!("'{}' already exists. Aliases must be unique.", alias)
            });
        }
        let len_before = self.config.elements.len();
        self.config.add_host(&entry);
        if !entry.tags.is_empty() {
            self.config.set_host_tags(&alias, &entry.tags);
        }
        if let Some(ref source) = entry.askpass {
            self.config.set_host_askpass(&alias, source);
        }
        if let Some(ref role) = entry.vault_ssh {
            self.config.set_host_vault_ssh(&alias, role);
            // Persist the optional Vault address next to the role. `set_host_vault_addr`
            // is `#[must_use]` but the alias was just upserted above so we only
            // debug-assert the return value here (matches the CertificateFile pattern).
            let addr = entry.vault_addr.as_deref().unwrap_or("");
            let addr_wired = self.config.set_host_vault_addr(&alias, addr);
            debug_assert!(
                addr_wired,
                "add_host_from_form: alias '{}' missing immediately after upsert (set_host_vault_addr)",
                alias
            );
            // For a brand-new host the only existing CertificateFile value can
            // come from the form itself (a power user pasting one in). Honor
            // the same invariant as edit_host_from_form: never overwrite a
            // user-set custom path.
            if crate::should_write_certificate_file(&entry.certificate_file) {
                let cert_path = crate::vault_ssh::cert_path_for(&alias)
                    .map_err(|e| format!("Failed to resolve cert path: {}", e))?;
                // The host block was just upserted above, so the alias MUST
                // exist. Assert the invariant to catch regressions early.
                let wired = self
                    .config
                    .set_host_certificate_file(&alias, &cert_path.to_string_lossy());
                debug_assert!(
                    wired,
                    "add_host_from_form: alias '{}' missing immediately after upsert",
                    alias
                );
            }
        }
        if let Err(e) = self.config.write() {
            self.config.elements.truncate(len_before);
            return Err(format!("Failed to save: {}", e));
        }
        // Form submit writes the full config including any pending vault mutations
        self.pending_vault_config_write = false;
        self.update_last_modified();
        self.reload_hosts();
        self.select_host_by_alias(&alias);
        // Refresh the cert cache so the detail panel reflects reality
        // immediately. No-op when the new host has no vault role or when
        // running in demo mode.
        self.refresh_cert_cache(&alias);
        Ok(format!("Welcome aboard, {}!", alias))
    }

    /// Edit an existing host from the current form. Returns status message.
    pub fn edit_host_from_form(&mut self, old_alias: &str) -> Result<String, String> {
        let entry = self.form.to_entry();
        let alias = entry.alias.clone();
        let exists = if self.form.is_pattern {
            self.config.has_host_block(old_alias)
        } else {
            self.config.has_host(old_alias)
        };
        if !exists {
            return Err(if self.form.is_pattern {
                "Pattern no longer exists.".to_string()
            } else {
                "Host no longer exists.".to_string()
            });
        }
        let duplicate = if self.form.is_pattern {
            alias != old_alias && self.config.has_host_block(&alias)
        } else {
            alias != old_alias && self.config.has_host(&alias)
        };
        if duplicate {
            return Err(if self.form.is_pattern {
                format!("Pattern '{}' already exists.", alias)
            } else {
                format!("'{}' already exists. Aliases must be unique.", alias)
            });
        }
        let old_entry = if self.form.is_pattern {
            self.patterns
                .iter()
                .find(|p| p.pattern == old_alias)
                .map(|p| HostEntry {
                    alias: p.pattern.clone(),
                    hostname: p.hostname.clone(),
                    user: p.user.clone(),
                    port: p.port,
                    identity_file: p.identity_file.clone(),
                    proxy_jump: p.proxy_jump.clone(),
                    tags: p.tags.clone(),
                    askpass: p.askpass.clone(),
                    ..Default::default()
                })
                .unwrap_or_default()
        } else {
            self.hosts
                .iter()
                .find(|h| h.alias == old_alias)
                .cloned()
                .unwrap_or_default()
        };
        self.config.update_host(old_alias, &entry);
        self.config.set_host_tags(&entry.alias, &entry.tags);
        self.config
            .set_host_askpass(&entry.alias, entry.askpass.as_deref().unwrap_or(""));
        self.config
            .set_host_vault_ssh(&entry.alias, entry.vault_ssh.as_deref().unwrap_or(""));
        // Persist vault address comment. `set_host_vault_addr` refuses
        // wildcard aliases (mirroring the CertificateFile invariant), so we
        // skip it entirely for Host pattern entries — patterns never carry a
        // vault address. For concrete hosts the alias was just upserted so
        // the #[must_use] return is asserted in debug builds.
        if !self.form.is_pattern {
            let addr_wired = self
                .config
                .set_host_vault_addr(&entry.alias, entry.vault_addr.as_deref().unwrap_or(""));
            debug_assert!(
                addr_wired,
                "edit_host_from_form: alias '{}' missing immediately after update_host (set_host_vault_addr)",
                entry.alias
            );
        }
        // HostForm does not track CertificateFile, so the source of truth for
        // the host's existing CertificateFile is `old_entry` (loaded from
        // disk), not `entry` (rebuilt from the form, which always has it
        // empty). Both branches below honor that distinction so a user-set
        // custom CertificateFile is preserved across an edit.
        if entry.vault_ssh.is_some() {
            if crate::should_write_certificate_file(&old_entry.certificate_file) {
                let cert_path = crate::vault_ssh::cert_path_for(&entry.alias)
                    .map_err(|e| format!("Failed to resolve cert path: {}", e))?;
                // Synchronous mutation: the host block was just updated, so
                // the alias MUST exist. Assert the invariant.
                let wired = self
                    .config
                    .set_host_certificate_file(&entry.alias, &cert_path.to_string_lossy());
                debug_assert!(
                    wired,
                    "edit_host_from_form: alias '{}' missing immediately after update_host",
                    entry.alias
                );
            }
        } else {
            // Vault SSH role removed: clear the CertificateFile only if it
            // points at purple's managed cert path. A user-set custom path is
            // left alone. Compare the expanded form on both sides so a
            // tilde-relative directive (`~/.purple/certs/...`) and the
            // absolute path produced by `cert_path_for` match.
            let purple_managed = crate::vault_ssh::cert_path_for(&entry.alias).ok();
            let existing_resolved = if old_entry.certificate_file.is_empty() {
                None
            } else {
                crate::vault_ssh::resolve_cert_path(&entry.alias, &old_entry.certificate_file).ok()
            };
            if purple_managed.is_some() && purple_managed == existing_resolved {
                let _ = self.config.set_host_certificate_file(&entry.alias, "");
            }
        }
        if let Err(e) = self.config.write() {
            self.config.update_host(&entry.alias, &old_entry);
            self.config.set_host_tags(&old_entry.alias, &old_entry.tags);
            self.config
                .set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
            self.config.set_host_vault_ssh(
                &old_entry.alias,
                old_entry.vault_ssh.as_deref().unwrap_or(""),
            );
            if !self.form.is_pattern {
                let _ = self.config.set_host_vault_addr(
                    &old_entry.alias,
                    old_entry.vault_addr.as_deref().unwrap_or(""),
                );
            }
            if old_entry.vault_ssh.is_some() {
                // Rollback restores the old host's actual CertificateFile
                // value (which may be a user-set custom path), not purple's
                // default. Falling back to the default would silently rewrite
                // the directive on a write failure.
                let _ = self
                    .config
                    .set_host_certificate_file(&old_entry.alias, &old_entry.certificate_file);
            } else {
                let _ = self.config.set_host_certificate_file(&old_entry.alias, "");
            }
            return Err(format!("Failed to save: {}", e));
        }
        // Form submit writes the full config including any pending vault mutations
        self.pending_vault_config_write = false;
        // Migrate active tunnel handle if alias changed
        if alias != old_alias {
            if let Some(tunnel) = self.active_tunnels.remove(old_alias) {
                self.active_tunnels.insert(alias.clone(), tunnel);
            }
            // Clean up old cert file on rename. Best-effort: a missing file is
            // fine (NotFound is expected when no cert was ever signed) but any
            // other error is surfaced via the status bar (never via eprintln,
            // which would corrupt the ratatui screen in raw mode).
            if !crate::demo_flag::is_demo() {
                if let Ok(old_cert) = crate::vault_ssh::cert_path_for(old_alias) {
                    match std::fs::remove_file(&old_cert) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => {
                            self.vault.cleanup_warning = Some(format!(
                                "Warning: failed to clean up old Vault SSH cert {}: {}",
                                old_cert.display(),
                                e
                            ));
                        }
                    }
                }
            }
        }
        self.update_last_modified();
        self.reload_hosts();
        // Refresh the cert cache so the detail panel reflects reality
        // immediately after an edit (e.g. a newly set vault role, a custom
        // CertificateFile path change, or role removal). When the alias
        // itself changed, also clear the stale entry under the old alias.
        if alias != old_alias {
            self.vault.cert_cache.remove(old_alias);
        }
        self.refresh_cert_cache(&alias);
        Ok(format!("{} got a makeover.", alias))
    }

    /// Select a host in the display list (or filtered list) by alias.
    pub fn select_host_by_alias(&mut self, alias: &str) {
        if self.search.query.is_some() {
            // In search mode, list_state indexes into filtered_indices
            for (i, &host_idx) in self.search.filtered_indices.iter().enumerate() {
                if self.hosts.get(host_idx).is_some_and(|h| h.alias == alias) {
                    self.ui.list_state.select(Some(i));
                    return;
                }
            }
            // Also check patterns in search results
            let host_count = self.search.filtered_indices.len();
            for (i, &pat_idx) in self.search.filtered_pattern_indices.iter().enumerate() {
                if self
                    .patterns
                    .get(pat_idx)
                    .is_some_and(|p| p.pattern == alias)
                {
                    self.ui.list_state.select(Some(host_count + i));
                    return;
                }
            }
        } else {
            for (i, item) in self.display_list.iter().enumerate() {
                match item {
                    HostListItem::Host { index } => {
                        if self.hosts.get(*index).is_some_and(|h| h.alias == alias) {
                            self.ui.list_state.select(Some(i));
                            return;
                        }
                    }
                    HostListItem::Pattern { index } => {
                        if self
                            .patterns
                            .get(*index)
                            .is_some_and(|p| p.pattern == alias)
                        {
                            self.ui.list_state.select(Some(i));
                            return;
                        }
                    }
                    HostListItem::GroupHeader(_) => {}
                }
            }
        }
    }

    /// Apply sync results from a background provider fetch.
    /// Returns (message, is_error, server_count, added, updated, stale). Caller must remove from syncing_providers.
    pub fn apply_sync_result(
        &mut self,
        provider: &str,
        hosts: Vec<crate::providers::ProviderHost>,
        partial: bool,
    ) -> (String, bool, usize, usize, usize, usize) {
        let section = match self.provider_config.section(provider).cloned() {
            Some(s) => s,
            None => {
                return (
                    format!(
                        "{} sync skipped: no config.",
                        crate::providers::provider_display_name(provider)
                    ),
                    true,
                    0,
                    0,
                    0,
                    0,
                );
            }
        };
        let provider_impl = match crate::providers::get_provider_with_config(provider, &section) {
            Some(p) => p,
            None => {
                return (
                    format!(
                        "Unknown provider: {}.",
                        crate::providers::provider_display_name(provider)
                    ),
                    true,
                    0,
                    0,
                    0,
                    0,
                );
            }
        };
        let config_backup = self.config.clone();
        let result = crate::providers::sync::sync_provider(
            &mut self.config,
            &*provider_impl,
            &hosts,
            &section,
            false,
            partial, // suppress stale marking on partial failures
            false,
        );
        let total = result.added + result.updated + result.unchanged;
        if result.added > 0 || result.updated > 0 || result.stale > 0 {
            if let Err(e) = self.config.write() {
                self.config = config_backup;
                return (format!("Sync failed to save: {}", e), true, total, 0, 0, 0);
            }
            self.undo_stack.clear();
            self.update_last_modified();
            self.reload_hosts();
            // Migrate active tunnel handles for renamed aliases
            for (old_alias, new_alias) in &result.renames {
                if let Some(tunnel) = self.active_tunnels.remove(old_alias) {
                    self.active_tunnels.insert(new_alias.clone(), tunnel);
                }
            }
        }
        let name = crate::providers::provider_display_name(provider);
        let mut msg = format!(
            "Synced {}: added {}, updated {}, unchanged {}",
            name, result.added, result.updated, result.unchanged
        );
        if result.stale > 0 {
            msg.push_str(&format!(", stale {}", result.stale));
        }
        msg.push('.');
        (
            msg,
            false,
            total,
            result.added,
            result.updated,
            result.stale,
        )
    }

    /// Clear group-by-tag if the tag no longer exists in any host.
    /// Returns true if the tag was cleared.
    pub fn clear_stale_group_tag(&mut self) -> bool {
        if let GroupBy::Tag(ref tag) = self.group_by {
            // Empty tag = "show all tags as tabs" mode, always valid
            if tag.is_empty() {
                return false;
            }
            let tag_exists = self.hosts.iter().any(|h| h.tags.iter().any(|t| t == tag))
                || self
                    .patterns
                    .iter()
                    .any(|p| p.tags.iter().any(|t| t == tag));
            if !tag_exists {
                self.group_by = GroupBy::None;
                self.group_filter = None;
                return true;
            }
        }
        false
    }
}
