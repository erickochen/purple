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

    // Build map of server_id -> alias (top-level only, no Include files).
    // Keep first occurrence if duplicate provider markers exist (e.g. manual copy).
    let existing = config.find_hosts_by_provider(provider.name());
    let mut existing_map: HashMap<String, String> = HashMap::new();
    for (alias, server_id) in &existing {
        existing_map
            .entry(server_id.clone())
            .or_insert_with(|| alias.clone());
    }

    // Build alias -> HostEntry lookup once (avoids quadratic host_entries() calls)
    let entries_map: HashMap<String, HostEntry> = config
        .host_entries()
        .into_iter()
        .map(|e| (e.alias.clone(), e))
        .collect();

    // Track which server IDs are still in the remote set (also deduplicates)
    let mut remote_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Only add group header if this provider has no existing hosts in config
    let mut needs_header = !dry_run && existing_map.is_empty();

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
                if let Some(entry) = entries_map.get(alias.as_str()) {
                    if entry.stale.is_some() {
                        if !dry_run {
                            config.clear_host_stale(alias);
                        }
                        result.updated += 1;
                        continue;
                    }
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
                    config.clear_host_stale(existing_alias);
                }

                // Check if alias prefix changed (e.g. "do" → "ocean")
                let sanitized = sanitize_name(&remote.name);
                let expected_alias = build_alias(&section.alias_prefix, &sanitized);
                let alias_changed = *existing_alias != expected_alias;

                let ip_changed = entry.hostname != remote.ip;
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

                if alias_changed
                    || ip_changed
                    || tags_changed
                    || meta_changed
                    || user_tags_overlap
                    || first_migration
                    || was_stale
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
                            || tags_changed
                            || meta_changed
                            || user_tags_overlap
                            || first_migration
                            || was_stale
                        {
                            if alias_changed || ip_changed {
                                let updated = HostEntry {
                                    alias: new_alias.clone(),
                                    hostname: remote.ip.clone(),
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
                            if tags_changed || first_migration {
                                config.set_host_provider_tags(tags_alias, &trimmed_remote);
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
                                config.set_host_tags(tags_alias, &user_only);
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
                                    config.set_host_tags(tags_alias, &cleaned);
                                }
                            }
                            // Update provider marker with new alias
                            if alias_changed {
                                config.set_host_provider(
                                    &new_alias,
                                    provider.name(),
                                    &remote.server_id,
                                );
                                result
                                    .renames
                                    .push((existing_alias.clone(), new_alias.clone()));
                            }
                            // Update metadata
                            if meta_changed {
                                config.set_host_meta(tags_alias, &remote.metadata);
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
            let sanitized = sanitize_name(&remote.name);
            let base_alias = build_alias(&section.alias_prefix, &sanitized);
            let alias = if dry_run {
                base_alias
            } else {
                config.deduplicate_alias(&base_alias)
            };

            if !dry_run {
                // Add group header before the very first host for this provider
                let wrote_header = needs_header;
                if needs_header {
                    if !config.elements.is_empty() && !config.last_element_has_trailing_blank() {
                        config
                            .elements
                            .push(ConfigElement::GlobalLine(String::new()));
                    }
                    config.elements.push(ConfigElement::GlobalLine(format!(
                        "# purple:group {}",
                        super::provider_display_name(provider.name())
                    )));
                    needs_header = false;
                }

                let entry = HostEntry {
                    alias: alias.clone(),
                    hostname: remote.ip.clone(),
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
                    // Insert after last provider host with blank line separation.
                    config.elements.insert(pos, ConfigElement::HostBlock(block));
                    // Ensure blank line after the new block if the next element
                    // is not already a blank (prevents hosts running into group
                    // headers or other host blocks without visual separation).
                    let after = pos + 1;
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
                    if !wrote_header
                        && !config.elements.is_empty()
                        && !config.last_element_has_trailing_blank()
                    {
                        config
                            .elements
                            .push(ConfigElement::GlobalLine(String::new()));
                    }
                    config.elements.push(ConfigElement::HostBlock(block));
                }

                config.set_host_provider(&alias, provider.name(), &remote.server_id);
                if !remote.tags.is_empty() {
                    config.set_host_provider_tags(&alias, &remote.tags);
                }
                if !remote.metadata.is_empty() {
                    config.set_host_meta(&alias, &remote.metadata);
                }
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
                    config.set_host_stale(alias, now);
                }
            }
        }
        result.stale = to_stale.len();
    }

    result
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
