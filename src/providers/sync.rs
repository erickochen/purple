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

/// Display name for a provider (used in group header comments).
fn provider_header(name: &str) -> &str {
    match name {
        "digitalocean" => "DigitalOcean",
        "vultr" => "Vultr",
        "linode" => "Linode",
        "hetzner" => "Hetzner",
        other => other,
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
    let mut result = SyncResult::default();

    // Build map of server_id -> alias (top-level only, no Include files)
    let existing = config.find_hosts_by_provider(provider.name());
    let mut existing_map: HashMap<String, String> = HashMap::new();
    for (alias, server_id) in &existing {
        existing_map.insert(server_id.clone(), alias.clone());
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

        if let Some(existing_alias) = existing_map.get(&remote.server_id) {
            // Host exists, check if IP or tags changed
            if let Some(entry) = entries_map.get(existing_alias) {
                // Included hosts are read-only; recognize them for dedup but skip mutations
                if entry.source_file.is_some() {
                    result.unchanged += 1;
                    continue;
                }
                let ip_changed = entry.hostname != remote.ip;
                let mut sorted_local = entry.tags.clone();
                sorted_local.sort();
                let mut sorted_remote = remote.tags.clone();
                sorted_remote.sort();
                let tags_changed = sorted_local != sorted_remote;
                if ip_changed || tags_changed {
                    if !dry_run {
                        if ip_changed {
                            let updated = HostEntry {
                                hostname: remote.ip.clone(),
                                ..entry.clone()
                            };
                            config.update_host(existing_alias, &updated);
                        }
                        if tags_changed {
                            config.set_host_tags(existing_alias, &remote.tags);
                        }
                    }
                    result.updated += 1;
                } else {
                    result.unchanged += 1;
                }
            } else {
                result.unchanged += 1;
            }
        } else {
            // New host
            let sanitized = sanitize_name(&remote.name);
            let base_alias = format!("{}-{}", section.alias_prefix, sanitized);
            let alias = if dry_run {
                base_alias
            } else {
                config.deduplicate_alias(&base_alias)
            };

            if !dry_run {
                // Add group header before the very first host for this provider
                if needs_header {
                    if !config.elements.is_empty() && !config.last_element_has_trailing_blank() {
                        config
                            .elements
                            .push(ConfigElement::GlobalLine(String::new()));
                    }
                    config
                        .elements
                        .push(ConfigElement::GlobalLine(format!(
                            "# {}",
                            provider_header(provider.name())
                        )));
                    needs_header = false;
                }

                let entry = HostEntry {
                    alias: alias.clone(),
                    hostname: remote.ip.clone(),
                    user: section.user.clone(),
                    port: 22,
                    identity_file: section.identity_file.clone(),
                    proxy_jump: String::new(),
                    source_file: None,
                    tags: remote.tags.clone(),
                    provider: Some(provider.name().to_string()),
                };

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
            let header_text = format!("# {}", provider_header(provider.name()));
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
        fn fetch_hosts(
            &self,
            _token: &str,
        ) -> Result<Vec<ProviderHost>, super::super::ProviderError> {
            Ok(Vec::new())
        }
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
            .filter(|e| matches!(e, ConfigElement::GlobalLine(line) if line == "# DigitalOcean"))
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
            .any(|e| matches!(e, ConfigElement::GlobalLine(line) if line == "# DigitalOcean"));
        assert!(has_header);

        // Remove all hosts (empty remote + remove_deleted)
        let result = sync_provider(&mut config, &MockProvider, &[], &section, true, false);
        assert_eq!(result.removed, 1);

        // Header should be cleaned up
        let has_header = config
            .elements
            .iter()
            .any(|e| matches!(e, ConfigElement::GlobalLine(line) if line == "# DigitalOcean"));
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

        // Second sync: tags changed (IP same)
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
            vec!["production", "us-east"]
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
    fn test_sync_tags_cleared() {
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

        // Second sync: tags removed
        let remote = vec![ProviderHost {
            server_id: "123".to_string(),
            name: "web-1".to_string(),
            ip: "1.2.3.4".to_string(),
            tags: Vec::new(),
        }];
        let result = sync_provider(&mut config, &MockProvider, &remote, &section, false, false);
        assert_eq!(result.updated, 1);
        assert!(config.host_entries()[0].tags.is_empty());
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
}
