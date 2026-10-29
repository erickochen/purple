//! Host CRUD operations. Implements `impl App` continuation with host add,
//! edit, deletion, sync-result application, and the nearby selection helpers
//! that skip group headers.

use super::{GroupBy, HostListItem};
use crate::app::App;
use crate::ssh_config::model::HostEntry;

impl App {
    pub fn add_host_from_form(&mut self) -> Result<String, String> {
        let entry = self.forms.host.to_entry();
        let alias = entry.alias.clone();
        let duplicate = if self.forms.host.is_pattern {
            self.hosts_state.ssh_config.has_host_block(&alias)
        } else {
            self.hosts_state.ssh_config.has_host(&alias)
        };
        if duplicate {
            return Err(if self.forms.host.is_pattern {
                crate::messages::pattern_already_exists(&alias)
            } else {
                crate::messages::host_alias_already_exists(&alias)
            });
        }
        let len_before = self.hosts_state.ssh_config.elements.len();
        self.hosts_state.ssh_config.add_host(&entry);
        if !entry.tags.is_empty() {
            let tags_wired = self
                .hosts_state
                .ssh_config
                .set_host_tags(&alias, &entry.tags);
            debug_assert!(
                tags_wired,
                "add_host_from_form: alias '{}' missing immediately after add_host (set_host_tags)",
                alias
            );
        }
        if let Some(ref source) = entry.askpass {
            let askpass_wired = self.hosts_state.ssh_config.set_host_askpass(&alias, source);
            debug_assert!(
                askpass_wired,
                "add_host_from_form: alias '{}' missing immediately after add_host (set_host_askpass)",
                alias
            );
        }
        if let Some(ref role) = entry.vault_ssh {
            // `set_host_vault_ssh` is `#[must_use]` since the multi-alias
            // refuse-guard was added. The alias was upserted in `add_host`
            // immediately above, so it MUST exist as a single-alias block
            // here. Debug-assert the invariant to catch regressions early.
            let role_wired = self.hosts_state.ssh_config.set_host_vault_ssh(&alias, role);
            debug_assert!(
                role_wired,
                "add_host_from_form: alias '{}' missing immediately after upsert (set_host_vault_ssh)",
                alias
            );
            // Persist the optional Vault address next to the role. `set_host_vault_addr`
            // is `#[must_use]` but the alias was just upserted above so we only
            // debug-assert the return value here (matches the CertificateFile pattern).
            let addr = entry.vault_addr.as_deref().unwrap_or("");
            let addr_wired = self
                .hosts_state
                .ssh_config
                .set_host_vault_addr(&alias, addr);
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
                let cert_path = crate::vault_ssh::cert_path_for(self.env().paths(), &alias)
                    .map_err(|e| crate::messages::cert_path_resolve_failed(&e))?;
                // The host block was just upserted above, so the alias MUST
                // exist. Assert the invariant to catch regressions early.
                let wired = self
                    .hosts_state
                    .ssh_config
                    .set_host_certificate_file(&alias, &cert_path.to_string_lossy());
                debug_assert!(
                    wired,
                    "add_host_from_form: alias '{}' missing immediately after upsert",
                    alias
                );
            }
        }
        if let Err(e) = self.hosts_state.ssh_config.write() {
            self.hosts_state.ssh_config.elements.truncate(len_before);
            log::warn!("[config] failed to save new host: alias={alias}: {e}");
            return Err(crate::messages::failed_to_save(&e));
        }
        log::debug!(
            "[purple] host added: alias={alias} is_pattern={}",
            self.forms.host.is_pattern
        );
        // Form submit writes the full config including any pending vault mutations
        self.vault.pending_config_write = false;
        self.update_last_modified();
        self.reload_hosts();
        self.select_host_by_alias(&alias);
        // Refresh the cert cache so the detail panel reflects reality
        // immediately. No-op when the new host has no vault role or when
        // running in demo mode.
        self.refresh_cert_cache(&alias);
        Ok(crate::messages::welcome_aboard(&alias))
    }

    /// Edit an existing host from the current form. Returns status message.
    pub fn edit_host_from_form(&mut self, old_alias: &str) -> Result<String, String> {
        let entry = self.forms.host.to_entry();
        let alias = entry.alias.clone();
        let exists = if self.forms.host.is_pattern {
            self.hosts_state.ssh_config.has_host_block(old_alias)
        } else {
            self.hosts_state.ssh_config.has_host(old_alias)
        };
        if !exists {
            return Err(if self.forms.host.is_pattern {
                crate::i18n::text(crate::messages::PATTERN_NO_LONGER_EXISTS).to_string()
            } else {
                crate::i18n::text(crate::messages::HOST_NO_LONGER_EXISTS).to_string()
            });
        }
        let duplicate = if self.forms.host.is_pattern {
            alias != old_alias && self.hosts_state.ssh_config.has_host_block(&alias)
        } else {
            alias != old_alias && self.hosts_state.ssh_config.has_host(&alias)
        };
        if duplicate {
            return Err(if self.forms.host.is_pattern {
                crate::messages::pattern_already_exists(&alias)
            } else {
                crate::messages::host_alias_already_exists(&alias)
            });
        }
        let old_entry = if self.forms.host.is_pattern {
            self.hosts_state
                .patterns
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
            self.hosts_state
                .list
                .iter()
                .find(|h| h.alias == old_alias)
                .cloned()
                .unwrap_or_default()
        };
        self.hosts_state.ssh_config.update_host(old_alias, &entry);
        // Patterns and concrete hosts both flow through here; tags/askpass
        // setters refuse pattern blocks (per the symmetric multi-alias guard),
        // so the boolean return is asserted only for non-pattern edits.
        if !self.forms.host.is_pattern {
            let tags_wired = self
                .hosts_state
                .ssh_config
                .set_host_tags(&entry.alias, &entry.tags);
            debug_assert!(
                tags_wired,
                "edit_host_from_form: alias '{}' missing immediately after update_host (set_host_tags)",
                entry.alias
            );
            let askpass_wired = self
                .hosts_state
                .ssh_config
                .set_host_askpass(&entry.alias, entry.askpass.as_deref().unwrap_or(""));
            debug_assert!(
                askpass_wired,
                "edit_host_from_form: alias '{}' missing immediately after update_host (set_host_askpass)",
                entry.alias
            );
        } else {
            // Pattern blocks refuse purple metadata; this is the documented
            // ExactAliasOnly policy. Drop the result explicitly.
            let _ = self
                .hosts_state
                .ssh_config
                .set_host_tags(&entry.alias, &entry.tags);
            let _ = self
                .hosts_state
                .ssh_config
                .set_host_askpass(&entry.alias, entry.askpass.as_deref().unwrap_or(""));
        }
        // `set_host_vault_ssh` refuses patterns and multi-alias blocks
        // (same invariant as set_host_vault_addr / set_host_certificate_file)
        // so we only call it for concrete host edits. Patterns never carry a
        // vault role. For concrete hosts the alias was just updated above so
        // the #[must_use] return is asserted in debug builds.
        if !self.forms.host.is_pattern {
            let role_wired = self
                .hosts_state
                .ssh_config
                .set_host_vault_ssh(&entry.alias, entry.vault_ssh.as_deref().unwrap_or(""));
            debug_assert!(
                role_wired,
                "edit_host_from_form: alias '{}' missing immediately after update_host (set_host_vault_ssh)",
                entry.alias
            );
            let addr_wired = self
                .hosts_state
                .ssh_config
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
                let cert_path = crate::vault_ssh::cert_path_for(self.env().paths(), &entry.alias)
                    .map_err(|e| crate::messages::cert_path_resolve_failed(&e))?;
                // Synchronous mutation: the host block was just updated, so
                // the alias MUST exist. Assert the invariant.
                let wired = self
                    .hosts_state
                    .ssh_config
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
            let purple_managed =
                crate::vault_ssh::cert_path_for(self.env().paths(), &entry.alias).ok();
            let existing_resolved = if old_entry.certificate_file.is_empty() {
                None
            } else {
                crate::vault_ssh::resolve_cert_path(
                    self.env().paths(),
                    &entry.alias,
                    &old_entry.certificate_file,
                )
                .ok()
            };
            if purple_managed.is_some() && purple_managed == existing_resolved {
                let _ = self
                    .hosts_state
                    .ssh_config
                    .set_host_certificate_file(&entry.alias, "");
            }
        }
        if let Err(e) = self.hosts_state.ssh_config.write() {
            self.hosts_state
                .ssh_config
                .update_host(&entry.alias, &old_entry);
            let _ = self
                .hosts_state
                .ssh_config
                .set_host_tags(&old_entry.alias, &old_entry.tags);
            let _ = self
                .hosts_state
                .ssh_config
                .set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
            if !self.forms.host.is_pattern {
                let _ = self.hosts_state.ssh_config.set_host_vault_ssh(
                    &old_entry.alias,
                    old_entry.vault_ssh.as_deref().unwrap_or(""),
                );
                let _ = self.hosts_state.ssh_config.set_host_vault_addr(
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
                    .hosts_state
                    .ssh_config
                    .set_host_certificate_file(&old_entry.alias, &old_entry.certificate_file);
            } else {
                let _ = self
                    .hosts_state
                    .ssh_config
                    .set_host_certificate_file(&old_entry.alias, "");
            }
            log::warn!(
                "[config] failed to save host edit: alias={alias} old_alias={old_alias}: {e}"
            );
            return Err(crate::messages::failed_to_save(&e));
        }
        log::debug!(
            "[purple] host edited: alias={alias} old_alias={old_alias} renamed={}",
            alias != old_alias
        );
        // Form submit writes the full config including any pending vault mutations
        self.vault.pending_config_write = false;
        self.update_last_modified();
        let renames: Vec<(String, String)> = if alias != old_alias {
            vec![(old_alias.to_string(), alias.clone())]
        } else {
            Vec::new()
        };
        self.rename_aliases(&renames);
        // cert_cache is intentionally NOT migrated by rename_aliases; clear
        // the stale entry under the old alias and refresh under the new one
        // so the detail panel reflects the freshly-signed cert (or the
        // absence of a vault role) immediately.
        if alias != old_alias {
            self.vault.cert_cache.remove(old_alias);
        }
        self.refresh_cert_cache(&alias);
        Ok(format!("{} got a makeover.", alias))
    }

    /// Apply a batch of `(old, new)` alias renames after the SSH config
    /// has been written. Single entry point: orders cache migration,
    /// stale-cert cleanup, reload and persistent-state migration so
    /// callers cannot forget a step. Used by `submit_form` (host edit)
    /// and provider sync. Empty `renames` collapses to a plain reload.
    pub(crate) fn rename_aliases(&mut self, renames: &[(String, String)]) {
        self.migrate_alias_keyed_caches(renames);
        self.cleanup_stale_cert_files_for_renames(renames);
        self.reload_hosts();
        self.apply_alias_renames(renames);
    }

    /// Best-effort: remove on-disk Vault SSH cert files keyed under the
    /// pre-rename alias. NotFound is fine (no cert was ever signed); any
    /// other failure surfaces via `vault.cleanup_warning` so the status
    /// bar shows it. Skipped in demo mode.
    fn cleanup_stale_cert_files_for_renames(&mut self, renames: &[(String, String)]) {
        if crate::demo_flag::is_demo() {
            return;
        }
        for (old_alias, new_alias) in renames {
            if old_alias == new_alias {
                continue;
            }
            let Ok(old_cert) = crate::vault_ssh::cert_path_for(self.env().paths(), old_alias)
            else {
                continue;
            };
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

    /// Migrate persistent per-host state (history, jump recents,
    /// collapsed-fleet preference) and re-sort. Must run AFTER
    /// `reload_hosts` so `apply_sort` sees the migrated history.
    /// Production callers go through `rename_aliases`; this is
    /// `pub(crate)` only to keep whitebox unit tests possible.
    pub(crate) fn apply_alias_renames(&mut self, renames: &[(String, String)]) {
        let mut applied = false;
        let paths = self.env.paths().cloned();
        for (old_alias, new_alias) in renames {
            if old_alias == new_alias {
                continue;
            }
            applied = true;
            log::debug!("[purple] apply_alias_renames: {old_alias} -> {new_alias}");
            self.history.rename(old_alias, new_alias);
            let mut recents = crate::app::jump::load_recents(paths.as_ref());
            if crate::app::jump::rename_host_recent(&mut recents, old_alias, new_alias)
                && let Err(e) = crate::app::jump::save_recents(&recents, paths.as_ref())
            {
                log::warn!("[purple] failed to save recents after rename: {e}");
            }
        }
        if applied {
            self.apply_sort();
        }
    }

    /// Move non-persistent alias-keyed caches and active tunnel handles
    /// from `old` to `new`. Must run BEFORE `reload_hosts`, whose prune
    /// step would otherwise drop entries still under the old alias.
    /// `vault.cert_cache` is excluded: a rename invalidates the prior
    /// cert path, so the caller refreshes it instead of migrating.
    /// Production callers go through `rename_aliases`; this is
    /// `pub(crate)` only to keep whitebox unit tests possible.
    pub(crate) fn migrate_alias_keyed_caches(&mut self, renames: &[(String, String)]) {
        let mut container_cache_changed = false;
        let mut collapsed_hosts_changed = false;
        for (old_alias, new_alias) in renames {
            if old_alias == new_alias {
                continue;
            }
            log::debug!("[purple] migrate_alias_keyed_caches: {old_alias} -> {new_alias}");
            self.ping.migrate_alias(old_alias, new_alias);
            if self.container_state.migrate_alias(old_alias, new_alias) {
                container_cache_changed = true;
            }
            // collapsed_hosts (inside containers_overview) is persistent so
            // the returned flag drives the save below. auto_list_in_flight
            // and refresh_batch.in_flight_aliases are non-persistent and
            // handled inside the same method.
            if self.containers_overview.migrate_alias(old_alias, new_alias) {
                collapsed_hosts_changed = true;
            }
            // Vault: cert_checks_in_flight and sign_in_flight move with
            // the rename. cert_cache is intentionally excluded by
            // `VaultState::migrate_alias` because the rename invalidates
            // the prior cert path; the caller refreshes instead.
            self.vault.migrate_alias(old_alias, new_alias);
            self.tunnels.migrate_alias(old_alias, new_alias);
            self.file_browser_state.migrate_alias(old_alias, new_alias);
            // A password typed this session belongs to the machine, not to
            // the name. Leaving it behind would hand it to whatever host
            // takes the old alias next.
            if let Some(password) = self.session_passwords.remove(old_alias) {
                self.session_passwords.insert(new_alias.clone(), password);
            }
        }
        if container_cache_changed {
            crate::containers::save_container_cache(
                self.env().paths(),
                self.container_state.cache(),
            );
        }
        if collapsed_hosts_changed
            && let Err(e) = crate::preferences::save_containers_collapsed_hosts(
                self.env().paths(),
                self.containers_overview.collapsed_hosts(),
            )
        {
            log::warn!("[config] failed to save collapsed_hosts after rename: {e}");
        }
    }

    /// Select a host in the display list (or filtered list) by alias.
    pub fn select_host_by_alias(&mut self, alias: &str) {
        if self.search.query.is_some() {
            // In search mode, list_state indexes into filtered_indices
            for (i, &host_idx) in self.search.filtered_indices.iter().enumerate() {
                if self
                    .hosts_state
                    .list
                    .get(host_idx)
                    .is_some_and(|h| h.alias == alias)
                {
                    self.ui.list_state.select(Some(i));
                    return;
                }
            }
            // Also check patterns in search results
            let host_count = self.search.filtered_indices.len();
            for (i, &pat_idx) in self.search.filtered_pattern_indices.iter().enumerate() {
                if self
                    .hosts_state
                    .patterns
                    .get(pat_idx)
                    .is_some_and(|p| p.pattern == alias)
                {
                    self.ui.list_state.select(Some(host_count + i));
                    return;
                }
            }
        } else {
            for (i, item) in self.hosts_state.display_list.iter().enumerate() {
                match item {
                    HostListItem::Host { index } => {
                        if self
                            .hosts_state
                            .list
                            .get(*index)
                            .is_some_and(|h| h.alias == alias)
                        {
                            self.ui.list_state.select(Some(i));
                            return;
                        }
                    }
                    HostListItem::Pattern { index } => {
                        if self
                            .hosts_state
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
    ///
    /// `provider` is the full ProviderConfigId display string (`do` for bare,
    /// `do:work` for labeled). We look up by exact id so multi-config
    /// providers route to the correct section.
    pub fn apply_sync_result(
        &mut self,
        provider: &str,
        hosts: Vec<crate::providers::ProviderHost>,
        partial: bool,
    ) -> (String, bool, usize, usize, usize, usize) {
        let id: crate::providers::config::ProviderConfigId = match provider.parse() {
            Ok(id) => id,
            Err(_) => crate::providers::config::ProviderConfigId::bare(provider),
        };
        let section = match self.providers.config.section_by_id(&id).cloned() {
            Some(s) => s,
            None => {
                log::warn!("[config] sync skipped: no config for provider={provider}");
                return (
                    format!(
                        "{} sync skipped: no config.",
                        crate::providers::provider_display_name(&id.provider)
                    ),
                    true,
                    0,
                    0,
                    0,
                    0,
                );
            }
        };
        let provider_impl = match crate::providers::get_provider_with_config(&section) {
            Some(p) => p,
            None => {
                log::warn!("[config] sync skipped: unknown provider={provider}");
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
        let config_backup = self.hosts_state.ssh_config.clone();
        let result = crate::providers::sync::sync_provider(
            &mut self.hosts_state.ssh_config,
            &*provider_impl,
            &hosts,
            &section,
            false,
            partial, // suppress stale marking on partial failures
            false,
        );
        let total = result.added + result.updated + result.unchanged;
        if result.added > 0 || result.updated > 0 || result.stale > 0 {
            // External-change guard: provider sync runs in the background
            // (10-30s of network latency) and can race against a user editing
            // ~/.ssh/config in another process. If the on-disk file changed
            // since the in-memory model was loaded, refuse the write so we
            // don't silently overwrite those edits. Roll back the in-memory
            // sync mutations and surface the conflict; the user can re-run
            // sync after reviewing their edits.
            if self.external_config_changed() {
                self.hosts_state.ssh_config = config_backup;
                log::warn!(
                    "[config] sync write refused: external config change for provider={provider}"
                );
                return (
                    crate::messages::sync_skipped_external_change().to_string(),
                    true,
                    total,
                    0,
                    0,
                    0,
                );
            }
            if let Err(e) = self.hosts_state.ssh_config.write() {
                self.hosts_state.ssh_config = config_backup;
                log::warn!("[purple] sync write failed for provider={provider}, rolled back: {e}");
                return (format!("Sync failed to save: {}", e), true, total, 0, 0, 0);
            }
            self.hosts_state.undo_stack.clear();
            self.update_last_modified();
            self.rename_aliases(&result.renames);
        }
        log::debug!(
            "[purple] sync applied: provider={provider} added={} updated={} unchanged={} stale={} renames={}",
            result.added,
            result.updated,
            result.unchanged,
            result.stale,
            result.renames.len()
        );
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
        if let GroupBy::Tag(ref tag) = self.hosts_state.group_by {
            // Empty tag = "show all tags as tabs" mode, always valid
            if tag.is_empty() {
                return false;
            }
            let tag_exists = self
                .hosts_state
                .list
                .iter()
                .any(|h| h.tags.iter().any(|t| t == tag))
                || self
                    .hosts_state
                    .patterns
                    .iter()
                    .any(|p| p.tags.iter().any(|t| t == tag));
            if !tag_exists {
                self.hosts_state.set_group_by(GroupBy::None);
                return true;
            }
        }
        false
    }
}

/// File-level rename migration for the CLI `purple sync` subcommand,
/// which writes the SSH config without an `App` in the picture and so
/// cannot use `App::apply_alias_renames`. Performs the same persistent
/// migrations: `~/.purple/history.tsv`, `~/.purple/recents.json`, and
/// the `containers_collapsed_hosts` line in `~/.purple/preferences`.
///
/// Pairs where `old == new` are skipped so a caller can hand over the
/// raw `SyncResult.renames` vec without filtering.
///
/// Errors during individual file writes are logged with `[config]` and
/// the migration continues with the remaining state stores. Losing one
/// store is a degradation; aborting the whole migration would leave the
/// SSH config diverged from the on-disk per-host state stores.
pub fn migrate_renames_persistent_state(
    paths: Option<&crate::runtime::env::Paths>,
    renames: &[(String, String)],
) {
    for (old_alias, new_alias) in renames {
        if old_alias == new_alias {
            continue;
        }
        // ConnectionHistory::rename calls save() internally.
        let mut history = crate::history::ConnectionHistory::load(paths);
        history.rename(old_alias, new_alias);

        let mut recents = crate::app::jump::load_recents(paths);
        if crate::app::jump::rename_host_recent(&mut recents, old_alias, new_alias)
            && let Err(e) = crate::app::jump::save_recents(&recents, paths)
        {
            log::warn!("[purple] failed to save recents after cli sync rename: {e}");
        }

        let mut collapsed = crate::preferences::load_containers_collapsed_hosts(paths);
        if collapsed.remove(old_alias) {
            collapsed.insert(new_alias.clone());
            if let Err(e) = crate::preferences::save_containers_collapsed_hosts(paths, &collapsed) {
                log::warn!("[config] failed to save collapsed_hosts after cli sync rename: {e}");
            }
        }
    }
}
