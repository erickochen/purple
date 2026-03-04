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


/// Sync hosts from a cloud provider into the SSH config.
pub fn sync_provider(
    config: &mut SshConfigFile,
    provider: &dyn Provider,
    remote_hosts: &[ProviderHost],
    section: &ProviderSection,
    remove_deleted: bool,
    dry_run: bool,
) -> SyncResult {
    sync_provider_with_options(
        config,
        provider,
        remote_hosts,
        section,
        remove_deleted,
        dry_run,
        false,
    )
}

/// Sync hosts from a cloud provider into the SSH config.
/// When `reset_tags` is true, local tags are replaced with provider tags
/// instead of being merged (cleans up stale tags).
pub fn sync_provider_with_options(
    config: &mut SshConfigFile,
    provider: &dyn Provider,
    remote_hosts: &[ProviderHost],
    section: &ProviderSection,
    remove_deleted: bool,
    dry_run: bool,
    reset_tags: bool,
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
        // won't delete it, but skip add/update.
        if remote.ip.is_empty() {
            if existing_map.contains_key(&remote.server_id) {
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

                // Check if alias prefix changed (e.g. "do" → "ocean")
                let sanitized = sanitize_name(&remote.name);
                let expected_alias = build_alias(&section.alias_prefix, &sanitized);
                let alias_changed = *existing_alias != expected_alias;

                let ip_changed = entry.hostname != remote.ip;
                let trimmed_remote: Vec<String> =
                    remote.tags.iter().map(|t| t.trim().to_string()).collect();
                let tags_changed = if reset_tags {
                    // Exact comparison (case-insensitive): replace local tags with provider tags
                    let mut sorted_local: Vec<String> =
                        entry.tags.iter().map(|t| t.to_lowercase()).collect();
                    sorted_local.sort();
                    let mut sorted_remote: Vec<String> =
                        trimmed_remote.iter().map(|t| t.to_lowercase()).collect();
                    sorted_remote.sort();
                    sorted_local != sorted_remote
                } else {
                    // Subset check (case-insensitive): only trigger when provider tags are missing locally
                    trimmed_remote.iter().any(|rt| {
                        !entry
                            .tags
                            .iter()
                            .any(|lt| lt.eq_ignore_ascii_case(rt))
                    })
                };
                if alias_changed || ip_changed || tags_changed {
                    if dry_run {
                        result.updated += 1;
                    } else {
                        // Compute the final alias (dedup handles collisions,
                        // excluding the host being renamed so it doesn't collide with itself)
                        let new_alias = if alias_changed {
                            config.deduplicate_alias_excluding(
                                &expected_alias,
                                Some(existing_alias),
                            )
                        } else {
                            existing_alias.clone()
                        };
                        // Re-evaluate: dedup may resolve back to the current alias
                        let alias_changed = new_alias != *existing_alias;

                        if alias_changed || ip_changed || tags_changed {
                            if alias_changed || ip_changed {
                                let updated = HostEntry {
                                    alias: new_alias.clone(),
                                    hostname: remote.ip.clone(),
                                    ..entry.clone()
                                };
                                config.update_host(existing_alias, &updated);
                            }
                            // Tags lookup uses the new alias after rename
                            let tags_alias =
                                if alias_changed { &new_alias } else { existing_alias };
                            if tags_changed {
                                if reset_tags {
                                    config.set_host_tags(tags_alias, &trimmed_remote);
                                } else {
                                    // Merge (case-insensitive): keep existing local tags, add missing remote tags
                                    let mut merged = entry.tags.clone();
                                    for rt in &trimmed_remote {
                                        if !merged.iter().any(|t| t.eq_ignore_ascii_case(rt)) {
                                            merged.push(rt.clone());
                                        }
                                    }
                                    config.set_host_tags(tags_alias, &merged);
                                }
                            }
                            // Update provider marker with new alias
                            if alias_changed {
                                config.set_host_provider(
                                    &new_alias,
                                    provider.name(),
                                    &remote.server_id,
                                );
                                result.renames.push((existing_alias.clone(), new_alias.clone()));
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
                    config
                        .elements
                        .push(ConfigElement::GlobalLine(format!(
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
                    tags: remote.tags.clone(),
                    provider: Some(provider.name().to_string()),
                    ..Default::default()
                };

                // Add blank line separator before host (skip when preceded by group header
                // so the header stays adjacent to the first host)
                if !wrote_header
                    && !config.elements.is_empty()
                    && !config.last_element_has_trailing_blank()
                {
                    config
                        .elements
                        .push(ConfigElement::GlobalLine(String::new()));
                }

                let block = SshConfigFile::entry_to_block(&entry);
                config.elements.push(ConfigElement::HostBlock(block));
                config.set_host_provider(&alias, provider.name(), &remote.server_id);
                if !remote.tags.is_empty() {
                    config.set_host_tags(&alias, &remote.tags);
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
            let header_text = format!("# purple:group {}", super::provider_display_name(provider.name()));
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

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn empty_config() -> SshConfigFile {
        SshConfigFile {
            elements: Vec::new(),
            path: PathBuf::from("/tmp/test_config"),
            crlf: false,
        }
    }

    fn make_section() -> ProviderSection {
        ProviderSection {
            provider: "digitalocean".to_string(),
            token: "test".to_string(),
            alias_prefix: "do".to_string(),
            user: "root".to_string(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
        }
    }

    struct MockProvider;
    impl Provider for MockProvider {
        fn name(&self) -> &str {
            "digitalocean"
        }
        fn short_label(&self) -> &str {
            "do"
        }
        fn fetch_hosts_cancellable(
            &self,
            _token: &str,
            _cancel: &std::sync::atomic::AtomicBool,
        ) -> Result<Vec<ProviderHost>, super::super::ProviderError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn test_build_alias() {
        assert_eq!(build_alias("do", "web-1"), "do-web-1");
        assert_eq!(build_alias("", "web-1"), "web-1");
        assert_eq!(build_alias("ocean", "db"), "ocean-db");
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("web-1"), "web-1");
        assert_eq!(sanitize_name("My Server"), "my-server");
        assert_eq!(sanitize_name("test.prod.us"), "test-prod-us");
        assert_eq!(sanitize_name("--weird--"), "weird");
        assert_eq!(sanitize_name("UPPER"), "upper");
        assert_eq!(sanitize_name("a--b"), "a-b");
        assert_eq!(sanitize_name(""), "server");
        assert_eq!(sanitize_name("..."), "server");
    }

    #[test]
    fn test_sync_adds_new_hosts() {
        let mut config = empty_config();
        let section = make_section();
        let remote = vec![
            ProviderHost {
                server_id: "123".to_string(),
                name: "web-1".to_string(),
                ip: "1.2.3.4".to_string(),
                tags: Vec::new(),
            },
            ProviderHost {
                server_id: "456".to_string(),
                name: "db-1".to_string(),
                ip: "5.6.7.8".to_string(),
                tags: Vec::new(),
            },
        ];

        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.added, 2);
        assert_eq!(result.updated, 0);
        assert_eq!(result.unchanged, 0);

        let entries = config.host_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].alias, "do-web-1");
        assert_eq!(entries[0].hostname, "1.2.3.4");
        assert_eq!(entries[1].alias, "do-db-1");
    }

    #[test]
    fn test_sync_updates_changed_ip() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: add host
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // Second sync: IP changed
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "9.8.7.6".to_string(),
            tags: Vec::new(),
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.updated, 1);
        assert_eq!(result.added, 0);

        let entries = config.host_entries();
        assert_eq!(entries[0].hostname, "9.8.7.6");
    }

    #[test]
    fn test_sync_unchanged() {
        let mut config = empty_config();
        let section = make_section();

        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // Same data again
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.unchanged, 1);
        assert_eq!(result.added, 0);
        assert_eq!(result.updated, 0);
    }

    #[test]
    fn test_sync_removes_deleted() {
        let mut config = empty_config();
        let section = make_section();

        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries().len(), 1);

        // Sync with empty remote list + remove_deleted
        let result =
            sync_provider(&mut config, &MockProvider, &[], &section, true, false);
        assert_eq!(result.removed, 1);
        assert_eq!(config.host_entries().len(), 0);
    }

    #[test]
    fn test_sync_dry_run_no_mutations() {
        let mut config = empty_config();
        let section = make_section();

        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];

        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, true);
        assert_eq!(result.added, 1);
        assert_eq!(config.host_entries().len(), 0); // No actual changes
    }

    #[test]
    fn test_sync_dedup_server_id_in_response() {
        let mut config = empty_config();
        let section = make_section();
        let remote = vec![
            ProviderHost {
                server_id: "123".to_string(),
                name: "web-1".to_string(),
                ip: "1.2.3.4".to_string(),
                tags: Vec::new(),
            },
            ProviderHost {
                server_id: "123".to_string(),
                name: "web-1-dup".to_string(),
                ip: "5.6.7.8".to_string(),
                tags: Vec::new(),
            },
        ];

        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.added, 1);
        assert_eq!(config.host_entries().len(), 1);
        assert_eq!(config.host_entries()[0].alias, "do-web-1");
    }

    #[test]
    fn test_sync_duplicate_local_server_id_keeps_first() {
        // If duplicate provider markers exist locally, sync should use the first alias
        let content = "\
Host do-web-1
  HostName 1.2.3.4
  # purple:provider digitalocean:123

Host do-web-1-copy
  HostName 1.2.3.4
  # purple:provider digitalocean:123
";
        let mut config = SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: PathBuf::from("/tmp/test_config"),
            crlf: false,
        };
        let section = make_section();

        // Remote has same server_id with updated IP
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "5.6.7.8".to_string(),
            tags: Vec::new(),
        }];

        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        // Should update the first alias (do-web-1), not the copy
        assert_eq!(result.updated, 1);
        assert_eq!(result.added, 0);
        let entries = config.host_entries();
        let first = entries.iter().find(|e| e.alias == "do-web-1").unwrap();
        assert_eq!(first.hostname, "5.6.7.8");
        // Copy should remain unchanged
        let copy = entries.iter().find(|e| e.alias == "do-web-1-copy").unwrap();
        assert_eq!(copy.hostname, "1.2.3.4");
    }

    #[test]
    fn test_sync_no_duplicate_header_on_repeated_sync() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: adds header + host
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // Second sync: new host added at provider
        let remote = vec![
            ProviderHost {
                server_id: "123".to_string(),
                name: "web-1".to_string(),
                ip: "1.2.3.4".to_string(),
                tags: Vec::new(),
            },
            ProviderHost {
                server_id: "456".to_string(),
                name: "db-1".to_string(),
                ip: "5.6.7.8".to_string(),
                tags: Vec::new(),
            },
        ];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // Should have exactly one header
        let header_count = config
            .elements
            .iter()
            .filter(|e| matches!(e, ConfigElement::GlobalLine(line) if line == "# purple:group DigitalOcean"))
            .count();
        assert_eq!(header_count, 1);
        assert_eq!(config.host_entries().len(), 2);
    }

    #[test]
    fn test_sync_removes_orphan_header() {
        let mut config = empty_config();
        let section = make_section();

        // Add a host
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // Verify header exists
        let has_header = config
            .elements
            .iter()
            .any(|e| matches!(e, ConfigElement::GlobalLine(line) if line == "# purple:group DigitalOcean"));
        assert!(has_header);

        // Remove all hosts (empty remote + remove_deleted)
        let result = sync_provider(&mut config, &MockProvider, &[], &section, true, false);
        assert_eq!(result.removed, 1);

        // Header should be cleaned up
        let has_header = config
            .elements
            .iter()
            .any(|e| matches!(e, ConfigElement::GlobalLine(line) if line == "# purple:group DigitalOcean"));
        assert!(!has_header);
    }

    #[test]
    fn test_sync_writes_provider_tags() {
        let mut config = empty_config();
        let section = make_section();
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["production".to_string(), "us-east".to_string()],
        }];

        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        let entries = config.host_entries();
        assert_eq!(entries[0].tags, vec!["production", "us-east"]);
    }

    #[test]
    fn test_sync_updates_changed_tags() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: add with tags
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["staging".to_string()],
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries()[0].tags, vec!["staging"]);

        // Second sync: new provider tags added — existing tags are preserved (merge)
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["production".to_string(), "us-east".to_string()],
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.updated, 1);
        assert_eq!(
            config.host_entries()[0].tags,
            vec!["staging", "production", "us-east"]
        );
    }

    #[test]
    fn test_sync_combined_add_update_remove() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: add two hosts
        let remote = vec![
            ProviderHost {
                server_id: "1".to_string(),
                name: "web".to_string(),
                ip: "1.1.1.1".to_string(),
                tags: Vec::new(),
            },
            ProviderHost {
                server_id: "2".to_string(),
                name: "db".to_string(),
                ip: "2.2.2.2".to_string(),
                tags: Vec::new(),
            },
        ];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries().len(), 2);

        // Second sync: host 1 IP changed, host 2 removed, host 3 added
        let remote = vec![
            ProviderHost {
                server_id: "1".to_string(),
                name: "web".to_string(),
                ip: "9.9.9.9".to_string(),
                tags: Vec::new(),
            },
            ProviderHost {
                server_id: "3".to_string(),
                name: "cache".to_string(),
                ip: "3.3.3.3".to_string(),
                tags: Vec::new(),
            },
        ];
        let result =
            sync_provider(&mut config, &MockProvider, &remote, &section, true, false);
        assert_eq!(result.updated, 1);
        assert_eq!(result.added, 1);
        assert_eq!(result.removed, 1);

        let entries = config.host_entries();
        assert_eq!(entries.len(), 2); // web (updated) + cache (added), db removed
        assert_eq!(entries[0].alias, "do-web");
        assert_eq!(entries[0].hostname, "9.9.9.9");
        assert_eq!(entries[1].alias, "do-cache");
    }

    #[test]
    fn test_sync_tag_order_insensitive() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: tags in one order
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["beta".to_string(), "alpha".to_string()],
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // Second sync: same tags, different order
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["alpha".to_string(), "beta".to_string()],
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.unchanged, 1);
        assert_eq!(result.updated, 0);
    }

    fn config_with_include_provider_host() -> SshConfigFile {
        use crate::ssh_config::model::{IncludeDirective, IncludedFile};

        // Build an included host block with provider marker
        let content = "Host do-included\n  HostName 1.2.3.4\n  User root\n  # purple:provider digitalocean:inc1\n";
        let included_elements = SshConfigFile::parse_content(content);

        SshConfigFile {
            elements: vec![ConfigElement::Include(IncludeDirective {
                raw_line: "Include conf.d/*".to_string(),
                pattern: "conf.d/*".to_string(),
                resolved_files: vec![IncludedFile {
                    path: PathBuf::from("/tmp/included.conf"),
                    elements: included_elements,
                }],
            })],
            path: PathBuf::from("/tmp/test_config"),
            crlf: false,
        }
    }

    #[test]
    fn test_sync_include_host_skips_update() {
        let mut config = config_with_include_provider_host();
        let section = make_section();

        // Remote has same server with different IP — should NOT update included host
        let remote = vec![ProviderHost {
            server_id: "inc1".to_string(),
            name: "included".to_string(),
            ip: "9.9.9.9".to_string(),
            tags: Vec::new(),
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.unchanged, 1);
        assert_eq!(result.updated, 0);
        assert_eq!(result.added, 0);

        // Verify IP was NOT changed
        let entries = config.host_entries();
        let included = entries.iter().find(|e| e.alias == "do-included").unwrap();
        assert_eq!(included.hostname, "1.2.3.4");
    }

    #[test]
    fn test_sync_include_host_skips_remove() {
        let mut config = config_with_include_provider_host();
        let section = make_section();

        // Empty remote + remove_deleted — should NOT remove included host
        let result = sync_provider(&mut config, &MockProvider, &[], &section, true, false);
        assert_eq!(result.removed, 0);
        assert_eq!(config.host_entries().len(), 1);
    }

    #[test]
    fn test_sync_dry_run_remove_count() {
        let mut config = empty_config();
        let section = make_section();

        // Add two hosts
        let remote = vec![
            ProviderHost {
                server_id: "1".to_string(),
                name: "web".to_string(),
                ip: "1.1.1.1".to_string(),
                tags: Vec::new(),
            },
            ProviderHost {
                server_id: "2".to_string(),
                name: "db".to_string(),
                ip: "2.2.2.2".to_string(),
                tags: Vec::new(),
            },
        ];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries().len(), 2);

        // Dry-run remove with empty remote — should count but not mutate
        let result = sync_provider(&mut config, &MockProvider, &[], &section, true, true);
        assert_eq!(result.removed, 2);
        assert_eq!(config.host_entries().len(), 2); // Still there
    }

    #[test]
    fn test_sync_tags_cleared_remotely_preserved_locally() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: host with tags
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["production".to_string()],
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries()[0].tags, vec!["production"]);

        // Second sync: remote tags empty — local tags preserved (may be user-added)
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.unchanged, 1);
        assert_eq!(config.host_entries()[0].tags, vec!["production"]);
    }

    #[test]
    fn test_sync_deduplicates_alias() {
        let content = "Host do-web-1\n  HostName 10.0.0.1\n";
        let mut config = SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: PathBuf::from("/tmp/test_config"),
            crlf: false,
        };
        let section = make_section();

        let remote = vec![ProviderHost {
            server_id: "999".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];

        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        let entries = config.host_entries();
        // Should have the original + a deduplicated one
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].alias, "do-web-1");
        assert_eq!(entries[1].alias, "do-web-1-2");
    }

    #[test]
    fn test_sync_renames_on_prefix_change() {
        let mut config = empty_config();
        let section = make_section(); // prefix = "do"

        // First sync: add host with "do" prefix
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries()[0].alias, "do-web-1");

        // Second sync: prefix changed to "ocean"
        let new_section = ProviderSection {
            alias_prefix: "ocean".to_string(),
            ..section
        };
        let result = sync_provider(&mut config, &MockProvider, &remote, &new_section, false, false);
        assert_eq!(result.updated, 1);
        assert_eq!(result.unchanged, 0);

        let entries = config.host_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].alias, "ocean-web-1");
        assert_eq!(entries[0].hostname, "1.2.3.4");
    }

    #[test]
    fn test_sync_rename_and_ip_change() {
        let mut config = empty_config();
        let section = make_section();

        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // Change both prefix and IP
        let new_section = ProviderSection {
            alias_prefix: "ocean".to_string(),
            ..section
        };
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "9.9.9.9".to_string(),
            tags: Vec::new(),
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &new_section, false, false);
        assert_eq!(result.updated, 1);

        let entries = config.host_entries();
        assert_eq!(entries[0].alias, "ocean-web-1");
        assert_eq!(entries[0].hostname, "9.9.9.9");
    }

    #[test]
    fn test_sync_rename_dry_run_no_mutation() {
        let mut config = empty_config();
        let section = make_section();

        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        let new_section = ProviderSection {
            alias_prefix: "ocean".to_string(),
            ..section
        };
        let result = sync_provider(&mut config, &MockProvider, &remote, &new_section, false, true);
        assert_eq!(result.updated, 1);

        // Config should be unchanged (dry run)
        assert_eq!(config.host_entries()[0].alias, "do-web-1");
    }

    #[test]
    fn test_sync_no_rename_when_prefix_unchanged() {
        let mut config = empty_config();
        let section = make_section();

        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // Same prefix, same everything — should be unchanged
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.unchanged, 1);
        assert_eq!(result.updated, 0);
        assert_eq!(config.host_entries()[0].alias, "do-web-1");
    }

    #[test]
    fn test_sync_manual_comment_survives_cleanup() {
        // A manual "# DigitalOcean" comment (without purple:group prefix)
        // should NOT be removed when provider hosts are deleted
        let content = "# DigitalOcean\nHost do-web\n  HostName 1.2.3.4\n  User root\n  # purple:provider digitalocean:123\n";
        let mut config = SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: PathBuf::from("/tmp/test_config"),
            crlf: false,
        };
        let section = make_section();

        // Remove all hosts (empty remote + remove_deleted)
        sync_provider(&mut config, &MockProvider, &[], &section, true, false);

        // The manual "# DigitalOcean" comment should survive (it doesn't have purple:group prefix)
        let has_manual = config
            .elements
            .iter()
            .any(|e| matches!(e, ConfigElement::GlobalLine(line) if line == "# DigitalOcean"));
        assert!(has_manual, "Manual comment without purple:group prefix should survive cleanup");
    }

    #[test]
    fn test_sync_rename_skips_included_host() {
        let mut config = config_with_include_provider_host();

        let new_section = ProviderSection {
            provider: "digitalocean".to_string(),
            token: "test".to_string(),
            alias_prefix: "ocean".to_string(), // Different prefix
            user: "root".to_string(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            auto_sync: true,
        };

        // Remote has the included host's server_id with a different prefix
        let remote = vec![ProviderHost {
            server_id: "inc1".to_string(),
            name: "included".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &new_section, false, false);
        assert_eq!(result.unchanged, 1);
        assert_eq!(result.updated, 0);

        // Alias should remain unchanged (included hosts are read-only)
        assert_eq!(config.host_entries()[0].alias, "do-included");
    }

    #[test]
    fn test_sync_rename_stable_with_manual_collision() {
        let mut config = empty_config();
        let section = make_section(); // prefix = "do"

        // First sync: add provider host
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries()[0].alias, "do-web-1");

        // Manually add a host that will collide with the renamed alias
        let manual = HostEntry {
            alias: "ocean-web-1".to_string(),
            hostname: "5.5.5.5".to_string(),
            ..Default::default()
        };
        config.add_host(&manual);

        // Second sync: prefix changes to "ocean", collides with manual host
        let new_section = ProviderSection {
            alias_prefix: "ocean".to_string(),
            ..section.clone()
        };
        let result = sync_provider(&mut config, &MockProvider, &remote, &new_section, false, false);
        assert_eq!(result.updated, 1);

        let entries = config.host_entries();
        let provider_host = entries.iter().find(|e| e.hostname == "1.2.3.4").unwrap();
        assert_eq!(provider_host.alias, "ocean-web-1-2");

        // Third sync: same state. Should be stable (not flip to -3)
        let result = sync_provider(&mut config, &MockProvider, &remote, &new_section, false, false);
        assert_eq!(result.unchanged, 1, "Should be unchanged on repeat sync");

        let entries = config.host_entries();
        let provider_host = entries.iter().find(|e| e.hostname == "1.2.3.4").unwrap();
        assert_eq!(provider_host.alias, "ocean-web-1-2", "Alias should be stable across syncs");
    }

    #[test]
    fn test_sync_preserves_user_tags() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: add host with provider tag
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["nyc1".to_string()],
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries()[0].tags, vec!["nyc1"]);

        // User manually adds a tag via the TUI
        config.set_host_tags("do-web-1", &["nyc1".to_string(), "prod".to_string()]);
        assert_eq!(config.host_entries()[0].tags, vec!["nyc1", "prod"]);

        // Second sync: same provider tags — user tag "prod" must survive
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.unchanged, 1);
        assert_eq!(config.host_entries()[0].tags, vec!["nyc1", "prod"]);
    }

    #[test]
    fn test_sync_merges_new_provider_tag_with_user_tags() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: add host with provider tag
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["nyc1".to_string()],
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // User manually adds a tag
        config.set_host_tags("do-web-1", &["nyc1".to_string(), "critical".to_string()]);

        // Second sync: provider adds a new tag — user tag must be preserved
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["nyc1".to_string(), "v2".to_string()],
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.updated, 1);
        let tags = &config.host_entries()[0].tags;
        assert!(tags.contains(&"nyc1".to_string()));
        assert!(tags.contains(&"critical".to_string()));
        assert!(tags.contains(&"v2".to_string()));
    }

    #[test]
    fn test_sync_reset_tags_replaces_local_tags() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: add host with provider tag
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["nyc1".to_string()],
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // User manually adds a tag
        config.set_host_tags("do-web-1", &["nyc1".to_string(), "prod".to_string()]);
        assert_eq!(config.host_entries()[0].tags, vec!["nyc1", "prod"]);

        // Sync with reset_tags: user tag "prod" is removed
        let result = sync_provider_with_options(
            &mut config, &MockProvider, &remote, &section, false, false, true,
        );
        assert_eq!(result.updated, 1);
        assert_eq!(config.host_entries()[0].tags, vec!["nyc1"]);
    }

    #[test]
    fn test_sync_reset_tags_clears_stale_tags() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: host with tags
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["staging".to_string()],
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // Second sync with reset_tags: provider removed all tags
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        let result = sync_provider_with_options(
            &mut config, &MockProvider, &remote, &section, false, false, true,
        );
        assert_eq!(result.updated, 1);
        assert!(config.host_entries()[0].tags.is_empty());
    }

    #[test]
    fn test_sync_reset_tags_unchanged_when_matching() {
        let mut config = empty_config();
        let section = make_section();

        // Sync: add host with tags
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["prod".to_string(), "nyc1".to_string()],
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // Reset-tags sync with same tags (different order): unchanged
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["nyc1".to_string(), "prod".to_string()],
        }];
        let result = sync_provider_with_options(
            &mut config, &MockProvider, &remote, &section, false, false, true,
        );
        assert_eq!(result.unchanged, 1);
    }

    #[test]
    fn test_sync_merge_case_insensitive() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: add host with lowercase tag
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["prod".to_string()],
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries()[0].tags, vec!["prod"]);

        // Second sync: provider returns same tag with different casing — no duplicate
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["Prod".to_string()],
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.unchanged, 1);
        assert_eq!(config.host_entries()[0].tags, vec!["prod"]);
    }

    #[test]
    fn test_sync_reset_tags_case_insensitive_unchanged() {
        let mut config = empty_config();
        let section = make_section();

        // Sync: add host with tag
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["prod".to_string()],
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);

        // Reset-tags sync with different casing: unchanged (case-insensitive comparison)
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: vec!["Prod".to_string()],
        }];
        let result = sync_provider_with_options(
            &mut config, &MockProvider, &remote, &section, false, false, true,
        );
        assert_eq!(result.unchanged, 1);
    }

    // --- Empty IP (stopped/no-IP VM) tests ---

    #[test]
    fn test_sync_empty_ip_not_added() {
        let mut config = empty_config();
        let section = make_section();
        let remote = vec![ProviderHost {
            server_id: "100".to_string(),
            name: "stopped-vm".to_string(),
            ip: String::new(),
            tags: Vec::new(),
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.added, 0);
        assert_eq!(config.host_entries().len(), 0);
    }

    #[test]
    fn test_sync_empty_ip_existing_host_unchanged() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: add host with IP
        let remote = vec![ProviderHost {
            server_id: "100".to_string(),
            name: "web".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries().len(), 1);
        assert_eq!(config.host_entries()[0].hostname, "1.2.3.4");

        // Second sync: VM stopped, empty IP. Host should stay unchanged.
        let remote = vec![ProviderHost {
            server_id: "100".to_string(),
            name: "web".to_string(),
            ip: String::new(),
            tags: Vec::new(),
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.unchanged, 1);
        assert_eq!(result.updated, 0);
        assert_eq!(config.host_entries()[0].hostname, "1.2.3.4");
    }

    #[test]
    fn test_sync_remove_skips_empty_ip_hosts() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: add two hosts
        let remote = vec![
            ProviderHost {
                server_id: "100".to_string(),
                name: "web".to_string(),
                ip: "1.2.3.4".to_string(),
                tags: Vec::new(),
            },
            ProviderHost {
                server_id: "200".to_string(),
                name: "db".to_string(),
                ip: "5.6.7.8".to_string(),
                tags: Vec::new(),
            },
        ];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries().len(), 2);

        // Second sync with --remove: web is running, db is stopped (empty IP).
        // db must NOT be removed.
        let remote = vec![
            ProviderHost {
                server_id: "100".to_string(),
                name: "web".to_string(),
                ip: "1.2.3.4".to_string(),
                tags: Vec::new(),
            },
            ProviderHost {
                server_id: "200".to_string(),
                name: "db".to_string(),
                ip: String::new(),
                tags: Vec::new(),
            },
        ];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, true, false);
        assert_eq!(result.removed, 0);
        assert_eq!(result.unchanged, 2);
        assert_eq!(config.host_entries().len(), 2);
    }

    #[test]
    fn test_sync_remove_deletes_truly_gone_hosts() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: add two hosts
        let remote = vec![
            ProviderHost {
                server_id: "100".to_string(),
                name: "web".to_string(),
                ip: "1.2.3.4".to_string(),
                tags: Vec::new(),
            },
            ProviderHost {
                server_id: "200".to_string(),
                name: "db".to_string(),
                ip: "5.6.7.8".to_string(),
                tags: Vec::new(),
            },
        ];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries().len(), 2);

        // Second sync with --remove: only web exists. db is truly deleted.
        let remote = vec![ProviderHost {
            server_id: "100".to_string(),
            name: "web".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, true, false);
        assert_eq!(result.removed, 1);
        assert_eq!(config.host_entries().len(), 1);
        assert_eq!(config.host_entries()[0].alias, "do-web");
    }

    #[test]
    fn test_sync_mixed_resolved_empty_and_missing() {
        let mut config = empty_config();
        let section = make_section();

        // First sync: add three hosts
        let remote = vec![
            ProviderHost {
                server_id: "1".to_string(),
                name: "running".to_string(),
                ip: "1.1.1.1".to_string(),
                tags: Vec::new(),
            },
            ProviderHost {
                server_id: "2".to_string(),
                name: "stopped".to_string(),
                ip: "2.2.2.2".to_string(),
                tags: Vec::new(),
            },
            ProviderHost {
                server_id: "3".to_string(),
                name: "deleted".to_string(),
                ip: "3.3.3.3".to_string(),
                tags: Vec::new(),
            },
        ];
        sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(config.host_entries().len(), 3);

        // Second sync with --remove:
        // - "running" has new IP (updated)
        // - "stopped" has empty IP (unchanged, not removed)
        // - "deleted" not in list (removed)
        let remote = vec![
            ProviderHost {
                server_id: "1".to_string(),
                name: "running".to_string(),
                ip: "9.9.9.9".to_string(),
                tags: Vec::new(),
            },
            ProviderHost {
                server_id: "2".to_string(),
                name: "stopped".to_string(),
                ip: String::new(),
                tags: Vec::new(),
            },
        ];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, true, false);
        assert_eq!(result.updated, 1);
        assert_eq!(result.unchanged, 1);
        assert_eq!(result.removed, 1);

        let entries = config.host_entries();
        assert_eq!(entries.len(), 2);
        // Running host got new IP
        let running = entries.iter().find(|e| e.alias == "do-running").unwrap();
        assert_eq!(running.hostname, "9.9.9.9");
        // Stopped host kept old IP
        let stopped = entries.iter().find(|e| e.alias == "do-stopped").unwrap();
        assert_eq!(stopped.hostname, "2.2.2.2");
    }
}
