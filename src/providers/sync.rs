use std::collections::HashMap;

use crate::ssh_config::model::{ConfigElement, HostEntry, SshConfigFile};

use super::config::ProviderSection;
use super::{Provider, ProviderHost};

/// Result of a sync operation.
#[derive(Debug, Default)]
pub struct SyncResult {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
    /// Hosts marked stale (disappeared from provider but not hard-deleted).
    pub stale: usize,
    /// Alias renames: (old_alias, new_alias) pairs.
    pub renames: Vec<(String, String)>,
}

/// Sanitize a server name into a valid SSH alias component.
/// Lowercase, non-alphanumeric chars become hyphens, collapse consecutive hyphens.
/// Falls back to "server" if the result would be empty (all-symbol/unicode names).
fn sanitize_name(name: &str) -> String {
    let mut result = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            result.push(c.to_ascii_lowercase());
        } else if !result.ends_with('-') {
            result.push('-');
        }
    }
    let trimmed = result.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "server".to_string()
    } else {
        trimmed
    }
}

/// Build an alias from prefix + sanitized name.
/// If prefix is empty, uses just the sanitized name (no leading hyphen).
fn build_alias(prefix: &str, sanitized: &str) -> String {
    if prefix.is_empty() {
        sanitized.to_string()
    } else {
        format!("{}-{}", prefix, sanitized)
    }
}

/// Whether a metadata key is volatile (changes frequently without user action).
/// Volatile keys are excluded from the sync diff comparison so that a status
/// change alone does not trigger an SSH config rewrite. The value is still
/// stored and displayed when the host is updated for other reasons.
fn is_volatile_meta(key: &str) -> bool {
    key == "status"
}

/// Sync hosts from a cloud provider into the SSH config.
/// Provider tags are always stored in `# purple:provider_tags` and exactly
/// mirror the remote state. User tags in `# purple:tags` are preserved.
pub fn sync_provider(
    config: &mut SshConfigFile,
    provider: &dyn Provider,
    remote_hosts: &[ProviderHost],
    section: &ProviderSection,
    remove_deleted: bool,
    suppress_stale: bool,
    dry_run: bool,
) -> SyncResult {
    let mut result = SyncResult::default();

    // Build map of server_id -> alias.
    //
    // Bare config: claim EVERY marker for this provider regardless of how
    // many `:`-segments it has. Some providers' server_ids contain colons
    // (Proxmox uses `qemu:300`, OCI compartment IDs are path-like) and the
    // marker `# purple:provider proxmox:qemu:300` is ambiguous in isolation
    // - it could be a labeled marker (`proxmox:qemu`, server_id=`300`) or a
    // legacy 2-segment marker with a colon-bearing server_id. Since a bare
    // section is by definition the only config for its provider (we reject
    // bare+labeled mix), it must own all of those hosts to avoid duplication.
    //
    // Labeled config: scope to its exact ProviderConfigId so two labeled
    // configs of the same provider don't clobber each other's diff.
    let existing = if section.id.label.is_none() {
        // Bare config: use raw 2-segment interpretation so server_ids with
        // colons (Proxmox `qemu:300`, OCI compartment paths) match correctly
        // against the API response and don't get duplicated as "missing".
        config.find_hosts_by_provider_raw(provider.name())
    } else {
        config.find_hosts_by_id(&section.id)
    };
    let mut existing_map: HashMap<String, String> = HashMap::new();
    for (alias, server_id) in &existing {
        existing_map
            .entry(server_id.clone())
            .or_insert_with(|| alias.clone());
    }

    // Build alias -> HostEntry lookup once. Own-block values only: an update
    // writes these fields back, so an inherited User, IdentityFile or
    // ProxyJump must never appear here.
    let entries_map: HashMap<String, HostEntry> = config
        .own_host_entries()
        .into_iter()
        .map(|e| (e.alias.clone(), e))
        .collect();

    // Track which server IDs are still in the remote set (also deduplicates)
    let mut remote_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Add a group header when this provider has no TOP-LEVEL host yet. The
    // header and every synced host live at top level (Include files are
    // read-only), so this must agree with find_provider_insert_position, which
    // is also top-level only. Using the include-aware find_hosts_by_provider
    // here left a new host header-less when the provider's existing hosts lived
    // solely in an Include. Group headers are still shared across labeled
    // configs of one provider: the second labeled config finds the first
    // config's top-level host and skips a duplicate header.
    let mut needs_header = !dry_run
        && config
            .find_provider_insert_position(provider.name())
            .is_none();

    for remote in remote_hosts {
        if !remote_ids.insert(remote.server_id.clone()) {
            continue; // Skip duplicate server_id in same response
        }

        // Empty IP means the resource exists but has no resolvable address
        // (e.g. stopped VM, no static IP). Count it in remote_ids so --remove
        // won't delete it, but skip add/update. Still clear stale if the host
        // reappeared (it exists in the provider, just has no IP).
        if remote.ip.is_empty() {
            if let Some(alias) = existing_map.get(&remote.server_id) {
                if let Some(entry) = entries_map.get(alias.as_str())
                    && entry.stale.is_some()
                {
                    if !dry_run {
                        let _ = config.clear_host_stale(alias);
                    }
                    result.updated += 1;
                    continue;
                }
                result.unchanged += 1;
            }
            continue;
        }

        if let Some(existing_alias) = existing_map.get(&remote.server_id) {
            // Host exists, check if alias, IP or tags changed
            if let Some(entry) = entries_map.get(existing_alias) {
                // Included hosts are read-only; recognize them for dedup but skip mutations
                if entry.source_file.is_some() {
                    result.unchanged += 1;
                    continue;
                }

                // Host reappeared: clear stale marking
                let was_stale = entry.stale.is_some();
                if was_stale && !dry_run {
                    let _ = config.clear_host_stale(existing_alias);
                }

                // Check if alias prefix changed (e.g. "do" → "ocean")
                let sanitized = sanitize_name(&remote.name);
                let expected_alias = build_alias(&section.alias_prefix, &sanitized);
                let alias_changed = *existing_alias != expected_alias;

                let ip_changed = entry.hostname != remote.ip;
                // Port and provider-managed directives (ProxyCommand and the
                // like) mirror the remote side, so a drift on either counts.
                let port_changed = remote.port.is_some_and(|p| p != entry.port);
                let directives_changed = remote.directives.iter().any(|(k, v)| {
                    config.host_directive(existing_alias, k).as_deref() != Some(v.as_str())
                }) || remote.retract_directives.iter().any(|rule| {
                    config
                        .host_directive(existing_alias, &rule.key)
                        .is_some_and(|local| rule.claims(&local))
                });
                let meta_changed = {
                    let mut local: Vec<(&str, &str)> = entry
                        .provider_meta
                        .iter()
                        .filter(|(k, _)| !is_volatile_meta(k))
                        .map(|(k, v)| (k.as_str(), v.as_str()))
                        .collect();
                    local.sort();
                    let mut remote_m: Vec<(&str, &str)> = remote
                        .metadata
                        .iter()
                        .filter(|(k, _)| !is_volatile_meta(k))
                        .map(|(k, v)| (k.as_str(), v.as_str()))
                        .collect();
                    remote_m.sort();
                    local != remote_m
                };
                let trimmed_remote: Vec<String> =
                    remote.tags.iter().map(|t| t.trim().to_string()).collect();
                let tags_changed = {
                    // Compare provider_tags with remote (case-insensitive, sorted)
                    let mut sorted_local: Vec<String> = entry
                        .provider_tags
                        .iter()
                        .map(|t| t.trim().to_lowercase())
                        .collect();
                    sorted_local.sort();
                    let mut sorted_remote: Vec<String> =
                        trimmed_remote.iter().map(|t| t.to_lowercase()).collect();
                    sorted_remote.sort();
                    sorted_local != sorted_remote
                };
                // First migration: host has old-format tags (# purple:tags) but
                // no # purple:provider_tags comment yet. Tags need splitting.
                let first_migration = !entry.has_provider_tags && !entry.tags.is_empty();

                // After first migration: check if user tags overlap with provider tags
                let user_tags_overlap = !first_migration
                    && !trimmed_remote.is_empty()
                    && entry.tags.iter().any(|t| {
                        trimmed_remote
                            .iter()
                            .any(|rt| rt.eq_ignore_ascii_case(t.trim()))
                    });

                // User and identity file come from the section config, not the
                // remote API. Track the last-synced value per host in
                // # purple:provider_user / # purple:provider_key so a change to
                // the provider config propagates to existing hosts, while a
                // manual per-host override (host value != marker) is preserved.
                // A host with no marker yet (synced before this existed) is
                // treated as managed and adopts the current provider value once.
                // Only propagate when BOTH the provider value and the host's
                // current value are non-empty: we never strip a directive by
                // clearing the field, and we never inject one into a host that
                // has none (a fresh sync sets it at creation; an existing host
                // without the directive is left as the user left it).
                let user_changed = !section.user.is_empty()
                    && !entry.user.is_empty()
                    && entry
                        .provider_user
                        .as_deref()
                        .is_none_or(|m| m == entry.user)
                    && entry.user != section.user;
                let key_changed = !section.identity_file.is_empty()
                    && !entry.identity_file.is_empty()
                    && entry
                        .provider_key
                        .as_deref()
                        .is_none_or(|m| m == entry.identity_file)
                    && entry.identity_file != section.identity_file;

                // Establish the tracking baseline on a legacy provider host
                // (synced before these markers existed) whose value already
                // matches the provider config, so a manual override made AFTER
                // this sync is detectable. Without it such a host stays
                // marker-less and a later override would be clobbered on the
                // next provider change. Records the host's current value, so it
                // never changes a directive. `user_changed` (divergence) takes
                // precedence and writes the marker itself.
                let user_needs_baseline = !section.user.is_empty()
                    && !entry.user.is_empty()
                    && entry.provider_user.is_none()
                    && !user_changed;
                let key_needs_baseline = !section.identity_file.is_empty()
                    && !entry.identity_file.is_empty()
                    && entry.provider_key.is_none()
                    && !key_changed;

                if alias_changed
                    || ip_changed
                    || port_changed
                    || directives_changed
                    || tags_changed
                    || meta_changed
                    || user_tags_overlap
                    || first_migration
                    || was_stale
                    || user_changed
                    || key_changed
                    || user_needs_baseline
                    || key_needs_baseline
                {
                    if dry_run {
                        result.updated += 1;
                    } else {
                        // Compute the final alias (dedup handles collisions,
                        // excluding the host being renamed so it doesn't collide with itself)
                        let new_alias = if alias_changed {
                            config
                                .deduplicate_alias_excluding(&expected_alias, Some(existing_alias))
                        } else {
                            existing_alias.clone()
                        };
                        // Re-evaluate: dedup may resolve back to the current alias
                        let alias_changed = new_alias != *existing_alias;

                        if alias_changed
                            || ip_changed
                            || port_changed
                            || directives_changed
                            || tags_changed
                            || meta_changed
                            || user_tags_overlap
                            || first_migration
                            || was_stale
                            || user_changed
                            || key_changed
                            || user_needs_baseline
                            || key_needs_baseline
                        {
                            if alias_changed
                                || ip_changed
                                || port_changed
                                || user_changed
                                || key_changed
                            {
                                let updated = HostEntry {
                                    alias: new_alias.clone(),
                                    hostname: remote.ip.clone(),
                                    port: remote.port.unwrap_or(entry.port),
                                    user: if user_changed {
                                        section.user.clone()
                                    } else {
                                        entry.user.clone()
                                    },
                                    identity_file: if key_changed {
                                        section.identity_file.clone()
                                    } else {
                                        entry.identity_file.clone()
                                    },
                                    ..entry.clone()
                                };
                                config.update_host(existing_alias, &updated);
                            }
                            // Tags lookup uses the new alias after rename
                            let tags_alias = if alias_changed {
                                &new_alias
                            } else {
                                existing_alias
                            };
                            // Record the last-synced provider value so a later
                            // manual edit is detectable and the next provider
                            // change also propagates. On divergence the marker
                            // takes the new provider value; on a pure baseline
                            // it records the host's current (matching) value.
                            if user_changed {
                                let _ = config.set_host_provider_user(tags_alias, &section.user);
                            } else if user_needs_baseline {
                                let _ = config.set_host_provider_user(tags_alias, &entry.user);
                            }
                            if key_changed {
                                let _ = config
                                    .set_host_provider_key(tags_alias, &section.identity_file);
                            } else if key_needs_baseline {
                                let _ =
                                    config.set_host_provider_key(tags_alias, &entry.identity_file);
                            }
                            if tags_changed || first_migration {
                                let _ = config.set_host_provider_tags(tags_alias, &trimmed_remote);
                            }
                            // Migration cleanup
                            if first_migration {
                                // First migration: old # purple:tags had both provider
                                // and user tags mixed. Keep only tags NOT in remote
                                // (those must be user-added). Provider tags move to
                                // # purple:provider_tags.
                                let user_only: Vec<String> = entry
                                    .tags
                                    .iter()
                                    .filter(|t| {
                                        !trimmed_remote
                                            .iter()
                                            .any(|rt| rt.eq_ignore_ascii_case(t.trim()))
                                    })
                                    .cloned()
                                    .collect();
                                let _ = config.set_host_tags(tags_alias, &user_only);
                            } else if tags_changed || user_tags_overlap {
                                // Ongoing: remove user tags that overlap with provider tags
                                let cleaned: Vec<String> = entry
                                    .tags
                                    .iter()
                                    .filter(|t| {
                                        !trimmed_remote
                                            .iter()
                                            .any(|rt| rt.eq_ignore_ascii_case(t.trim()))
                                    })
                                    .cloned()
                                    .collect();
                                if cleaned.len() != entry.tags.len() {
                                    let _ = config.set_host_tags(tags_alias, &cleaned);
                                }
                            }
                            // Update provider marker with new alias.
                            // Use the section's full id so labeled configs
                            // emit 3-segment markers.
                            if alias_changed {
                                let _ = config.set_host_provider_id(
                                    &new_alias,
                                    &section.id,
                                    &remote.server_id,
                                );
                                result
                                    .renames
                                    .push((existing_alias.clone(), new_alias.clone()));
                            }
                            // Update metadata
                            if meta_changed {
                                let _ = config.set_host_meta(tags_alias, &remote.metadata);
                            }
                            if directives_changed {
                                for (key, value) in &remote.directives {
                                    let _ = config.set_host_directive(tags_alias, key, value);
                                }
                                // An empty value removes the directive, and
                                // only a value the provider claims as its own
                                // is removed, so a line the user wrote or
                                // edited survives. Logged on the write's own
                                // result: this is a destructive edit to the
                                // user's config that leaves no other trace,
                                // and a multi-alias block refuses the write.
                                for rule in &remote.retract_directives {
                                    // A key the provider also writes above is
                                    // one it wants, so a rule that would take
                                    // that line straight back out is skipped.
                                    let also_written =
                                        remote.directives.iter().any(|(key, _)| key == &rule.key);
                                    let claimed = !also_written
                                        && config
                                            .host_directive(tags_alias, &rule.key)
                                            .is_some_and(|local| rule.claims(&local));
                                    if claimed
                                        && config.set_host_directive(tags_alias, &rule.key, "")
                                    {
                                        log::debug!(
                                            "[purple] sync withdrew {} from '{}'",
                                            rule.key,
                                            tags_alias
                                        );
                                    }
                                }
                            }
                            result.updated += 1;
                        } else {
                            result.unchanged += 1;
                        }
                    }
                } else {
                    result.unchanged += 1;
                }
            } else {
                result.unchanged += 1;
            }
        } else {
            // New host
            if !dry_run {
                add_remote_host(config, provider, section, remote, &mut needs_header);
            }
            result.added += 1;
        }
    }

    // Remove deleted hosts (skip included hosts which are read-only)
    if remove_deleted && !dry_run {
        let to_remove: Vec<String> = existing_map
            .iter()
            .filter(|(id, _)| !remote_ids.contains(id.as_str()))
            .filter(|(_, alias)| {
                entries_map
                    .get(alias.as_str())
                    .is_none_or(|e| e.source_file.is_none())
            })
            .map(|(_, alias)| alias.clone())
            .collect();
        for alias in &to_remove {
            config.delete_host(alias);
        }
        result.removed = to_remove.len();

        // Clean up orphan provider header if all hosts for this provider were removed
        if config.find_hosts_by_provider(provider.name()).is_empty() {
            let header_text = format!(
                "# purple:group {}",
                super::provider_display_name(provider.name())
            );
            config
                .elements
                .retain(|e| !matches!(e, ConfigElement::GlobalLine(line) if line == &header_text));
        }
    } else if remove_deleted {
        result.removed = existing_map
            .iter()
            .filter(|(id, _)| !remote_ids.contains(id.as_str()))
            .filter(|(_, alias)| {
                entries_map
                    .get(alias.as_str())
                    .is_none_or(|e| e.source_file.is_none())
            })
            .count();
    }

    // Soft-delete: mark disappeared hosts as stale (when not hard-deleting)
    if !remove_deleted && !suppress_stale {
        let to_stale: Vec<String> = existing_map
            .iter()
            .filter(|(id, _)| !remote_ids.contains(id.as_str()))
            .filter(|(_, alias)| {
                entries_map
                    .get(alias.as_str())
                    .is_none_or(|e| e.source_file.is_none())
            })
            .map(|(_, alias)| alias.clone())
            .collect();
        if !dry_run {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            for alias in &to_stale {
                // Preserve original timestamp if already stale
                if entries_map
                    .get(alias.as_str())
                    .is_none_or(|e| e.stale.is_none())
                {
                    let _ = config.set_host_stale(alias, now);
                }
            }
        }
        result.stale = to_stale.len();
    }

    result
}

/// Write a brand-new host block for `remote`: alias from the section prefix,
/// the group header on the very first host, then the provider markers,
/// tags, metadata and managed directives.
fn add_remote_host(
    config: &mut SshConfigFile,
    provider: &dyn Provider,
    section: &ProviderSection,
    remote: &ProviderHost,
    needs_header: &mut bool,
) {
    let sanitized = sanitize_name(&remote.name);
    let base_alias = build_alias(&section.alias_prefix, &sanitized);
    let alias = config.deduplicate_alias(&base_alias);

    // Add group header before the very first host for this provider
    let wrote_header = *needs_header;
    if *needs_header {
        if !config.elements.is_empty() && !config.last_element_has_trailing_blank() {
            config
                .elements
                .push(ConfigElement::GlobalLine(String::new()));
        }
        config.elements.push(ConfigElement::GlobalLine(format!(
            "# purple:group {}",
            super::provider_display_name(provider.name())
        )));
        *needs_header = false;
    }

    let entry = HostEntry {
        alias: alias.clone(),
        hostname: remote.ip.clone(),
        port: remote.port.unwrap_or(22),
        user: section.user.clone(),
        identity_file: section.identity_file.clone(),
        provider: Some(provider.name().to_string()),
        ..Default::default()
    };

    let block = SshConfigFile::entry_to_block(&entry);

    // Insert adjacent to existing provider hosts (keeps groups together).
    // For the very first host (wrote_header), fall through to push at end.
    let insert_pos = if !wrote_header {
        config.find_provider_insert_position(provider.name())
    } else {
        None
    };

    if let Some(pos) = insert_pos {
        // Mirror add_host: guarantee a blank line BEFORE the new
        // block (so consecutive synced hosts never glue together)
        // and AFTER it (so it never runs into the next group header
        // or host block).
        let mut idx = pos;
        let needs_blank_before = idx > 0
            && !matches!(
                config.elements.get(idx - 1),
                Some(ConfigElement::GlobalLine(line)) if line.trim().is_empty()
            );
        if needs_blank_before {
            config
                .elements
                .insert(idx, ConfigElement::GlobalLine(String::new()));
            idx += 1;
        }
        config.elements.insert(idx, ConfigElement::HostBlock(block));
        let after = idx + 1;
        let needs_trailing_blank = config.elements.get(after).is_some_and(
            |e| !matches!(e, ConfigElement::GlobalLine(line) if line.trim().is_empty()),
        );
        if needs_trailing_blank {
            config
                .elements
                .insert(after, ConfigElement::GlobalLine(String::new()));
        }
    } else {
        // No existing group or first host: append at end with separator
        if !wrote_header && !config.elements.is_empty() && !config.last_element_has_trailing_blank()
        {
            config
                .elements
                .push(ConfigElement::GlobalLine(String::new()));
        }
        config.elements.push(ConfigElement::HostBlock(block));
    }

    for (key, value) in &remote.directives {
        let _ = config.set_host_directive(&alias, key, value);
    }
    let _ = config.set_host_provider_id(&alias, &section.id, &remote.server_id);
    if !remote.tags.is_empty() {
        let _ = config.set_host_provider_tags(&alias, &remote.tags);
    }
    if !remote.metadata.is_empty() {
        let _ = config.set_host_meta(&alias, &remote.metadata);
    }
    // Baseline the last-synced provider connection values so a
    // later manual override is detectable on the next sync.
    if !section.user.is_empty() {
        let _ = config.set_host_provider_user(&alias, &section.user);
    }
    if !section.identity_file.is_empty() {
        let _ = config.set_host_provider_key(&alias, &section.identity_file);
    }
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
