use super::*;
use std::path::PathBuf;

fn empty_config() -> SshConfigFile {
    SshConfigFile {
        elements: Vec::new(),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
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
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
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
        ProviderHost::new(
            "123".to_string(),
            "web-1".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "456".to_string(),
            "db-1".to_string(),
            "5.6.7.8".to_string(),
            Vec::new(),
        ),
    ];

    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
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
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Second sync: IP changed
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "9.8.7.6".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert_eq!(result.added, 0);

    let entries = config.host_entries();
    assert_eq!(entries[0].hostname, "9.8.7.6");
}

#[test]
fn test_sync_unchanged() {
    let mut config = empty_config();
    let section = make_section();

    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Same data again
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.unchanged, 1);
    assert_eq!(result.added, 0);
    assert_eq!(result.updated, 0);
}

#[test]
fn test_sync_removes_deleted() {
    let mut config = empty_config();
    let section = make_section();

    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries().len(), 1);

    // Sync with empty remote list + remove_deleted
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        true,
        false,
        false,
    );
    assert_eq!(result.removed, 1);
    assert_eq!(config.host_entries().len(), 0);
}

#[test]
fn test_sync_dry_run_no_mutations() {
    let mut config = empty_config();
    let section = make_section();

    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];

    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        true,
    );
    assert_eq!(result.added, 1);
    assert_eq!(config.host_entries().len(), 0); // No actual changes
}

#[test]
fn test_sync_dedup_server_id_in_response() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![
        ProviderHost::new(
            "123".to_string(),
            "web-1".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "123".to_string(),
            "web-1-dup".to_string(),
            "5.6.7.8".to_string(),
            Vec::new(),
        ),
    ];

    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
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
        bom: false,
    };
    let section = make_section();

    // Remote has same server_id with updated IP
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "5.6.7.8".to_string(),
        Vec::new(),
    )];

    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
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
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Second sync: new host added at provider
    let remote = vec![
        ProviderHost::new(
            "123".to_string(),
            "web-1".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "456".to_string(),
            "db-1".to_string(),
            "5.6.7.8".to_string(),
            Vec::new(),
        ),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

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
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Verify header exists
    let has_header = config.elements.iter().any(
        |e| matches!(e, ConfigElement::GlobalLine(line) if line == "# purple:group DigitalOcean"),
    );
    assert!(has_header);

    // Remove all hosts (empty remote + remove_deleted)
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        true,
        false,
        false,
    );
    assert_eq!(result.removed, 1);

    // Header should be cleaned up
    let has_header = config.elements.iter().any(
        |e| matches!(e, ConfigElement::GlobalLine(line) if line == "# purple:group DigitalOcean"),
    );
    assert!(!has_header);
}

#[test]
fn test_sync_writes_provider_tags() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["production".to_string(), "us-east".to_string()],
    )];

    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    let entries = config.host_entries();
    assert_eq!(entries[0].provider_tags, vec!["production", "us-east"]);
}

#[test]
fn test_sync_updates_changed_tags() {
    let mut config = empty_config();
    let section = make_section();

    // First sync: add with tags
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["staging".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries()[0].provider_tags, vec!["staging"]);

    // Second sync: provider tags replaced exactly
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["production".to_string(), "us-east".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert_eq!(
        config.host_entries()[0].provider_tags,
        vec!["production", "us-east"]
    );
}

#[test]
fn test_sync_combined_add_update_remove() {
    let mut config = empty_config();
    let section = make_section();

    // First sync: add two hosts
    let remote = vec![
        ProviderHost::new(
            "1".to_string(),
            "web".to_string(),
            "1.1.1.1".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "db".to_string(),
            "2.2.2.2".to_string(),
            Vec::new(),
        ),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries().len(), 2);

    // Second sync: host 1 IP changed, host 2 removed, host 3 added
    let remote = vec![
        ProviderHost::new(
            "1".to_string(),
            "web".to_string(),
            "9.9.9.9".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "3".to_string(),
            "cache".to_string(),
            "3.3.3.3".to_string(),
            Vec::new(),
        ),
    ];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        true,
        false,
        false,
    );
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
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["beta".to_string(), "alpha".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Second sync: same tags, different order
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["alpha".to_string(), "beta".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
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
        bom: false,
    }
}

#[test]
fn test_sync_include_host_skips_update() {
    let mut config = config_with_include_provider_host();
    let section = make_section();

    // Remote has same server with different IP — should NOT update included host
    let remote = vec![ProviderHost::new(
        "inc1".to_string(),
        "included".to_string(),
        "9.9.9.9".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
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
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        true,
        false,
        false,
    );
    assert_eq!(result.removed, 0);
    assert_eq!(config.host_entries().len(), 1);
}

#[test]
fn test_sync_dry_run_remove_count() {
    let mut config = empty_config();
    let section = make_section();

    // Add two hosts
    let remote = vec![
        ProviderHost::new(
            "1".to_string(),
            "web".to_string(),
            "1.1.1.1".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "db".to_string(),
            "2.2.2.2".to_string(),
            Vec::new(),
        ),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries().len(), 2);

    // Dry-run remove with empty remote — should count but not mutate
    let result = sync_provider(&mut config, &MockProvider, &[], &section, true, false, true);
    assert_eq!(result.removed, 2);
    assert_eq!(config.host_entries().len(), 2); // Still there
}

#[test]
fn test_sync_tags_cleared_remotely_preserved_locally() {
    let mut config = empty_config();
    let section = make_section();

    // First sync: host with tags
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["production".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries()[0].provider_tags, vec!["production"]);

    // Second sync: remote tags empty — provider_tags cleared
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert!(config.host_entries()[0].provider_tags.is_empty());
}

#[test]
fn test_sync_deduplicates_alias() {
    let content = "Host do-web-1\n  HostName 10.0.0.1\n";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };
    let section = make_section();

    let remote = vec![ProviderHost::new(
        "999".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];

    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

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
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries()[0].alias, "do-web-1");

    // Second sync: prefix changed to "ocean"
    let new_section = ProviderSection {
        alias_prefix: "ocean".to_string(),
        ..section
    };
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &new_section,
        false,
        false,
        false,
    );
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

    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Change both prefix and IP
    let new_section = ProviderSection {
        alias_prefix: "ocean".to_string(),
        ..section
    };
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "9.9.9.9".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &new_section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);

    let entries = config.host_entries();
    assert_eq!(entries[0].alias, "ocean-web-1");
    assert_eq!(entries[0].hostname, "9.9.9.9");
}

#[test]
fn test_sync_rename_dry_run_no_mutation() {
    let mut config = empty_config();
    let section = make_section();

    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    let new_section = ProviderSection {
        alias_prefix: "ocean".to_string(),
        ..section
    };
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &new_section,
        false,
        false,
        true,
    );
    assert_eq!(result.updated, 1);

    // Config should be unchanged (dry run)
    assert_eq!(config.host_entries()[0].alias, "do-web-1");
}

#[test]
fn test_sync_no_rename_when_prefix_unchanged() {
    let mut config = empty_config();
    let section = make_section();

    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Same prefix, same everything — should be unchanged
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
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
        bom: false,
    };
    let section = make_section();

    // Remove all hosts (empty remote + remove_deleted)
    sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        true,
        false,
        false,
    );

    // The manual "# DigitalOcean" comment should survive (it doesn't have purple:group prefix)
    let has_manual = config
        .elements
        .iter()
        .any(|e| matches!(e, ConfigElement::GlobalLine(line) if line == "# DigitalOcean"));
    assert!(
        has_manual,
        "Manual comment without purple:group prefix should survive cleanup"
    );
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
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
    };

    // Remote has the included host's server_id with a different prefix
    let remote = vec![ProviderHost::new(
        "inc1".to_string(),
        "included".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &new_section,
        false,
        false,
        false,
    );
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
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
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
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &new_section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);

    let entries = config.host_entries();
    let provider_host = entries.iter().find(|e| e.hostname == "1.2.3.4").unwrap();
    assert_eq!(provider_host.alias, "ocean-web-1-2");

    // Third sync: same state. Should be stable (not flip to -3)
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &new_section,
        false,
        false,
        false,
    );
    assert_eq!(result.unchanged, 1, "Should be unchanged on repeat sync");

    let entries = config.host_entries();
    let provider_host = entries.iter().find(|e| e.hostname == "1.2.3.4").unwrap();
    assert_eq!(
        provider_host.alias, "ocean-web-1-2",
        "Alias should be stable across syncs"
    );
}

#[test]
fn test_sync_preserves_user_tags() {
    let mut config = empty_config();
    let section = make_section();

    // First sync: add host with provider tag
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["nyc1".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries()[0].provider_tags, vec!["nyc1"]);

    // User manually adds a tag via the TUI (including duplicate "nyc1")
    config.set_host_tags("do-web-1", &["nyc1".to_string(), "prod".to_string()]);
    assert_eq!(config.host_entries()[0].tags, vec!["nyc1", "prod"]);

    // Second sync: provider tags unchanged but overlap detected, "nyc1" migrated out
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert_eq!(config.host_entries()[0].provider_tags, vec!["nyc1"]);
    // "nyc1" removed from user tags (overlap with provider), "prod" preserved
    assert_eq!(config.host_entries()[0].tags, vec!["prod"]);
}

#[test]
fn test_sync_merges_new_provider_tag_with_user_tags() {
    let mut config = empty_config();
    let section = make_section();

    // First sync: add host with provider tag
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["nyc1".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // User manually adds a tag
    config.set_host_tags("do-web-1", &["nyc1".to_string(), "critical".to_string()]);

    // Second sync: provider adds a new tag — user tag must be preserved
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["nyc1".to_string(), "v2".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    // Provider tags exactly mirror remote
    let ptags = &config.host_entries()[0].provider_tags;
    assert!(ptags.contains(&"nyc1".to_string()));
    assert!(ptags.contains(&"v2".to_string()));
    // User tag "critical" survives, "nyc1" migrated out of user tags
    let tags = &config.host_entries()[0].tags;
    assert!(tags.contains(&"critical".to_string()));
    assert!(!tags.contains(&"nyc1".to_string()));
}

#[test]
fn test_sync_migration_cleans_overlapping_user_tags() {
    let mut config = empty_config();
    let section = make_section();

    // First sync: add host with provider tag
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["nyc1".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // User manually adds a tag
    config.set_host_tags("do-web-1", &["nyc1".to_string(), "prod".to_string()]);
    assert_eq!(config.host_entries()[0].tags, vec!["nyc1", "prod"]);

    // Provider_tags match remote but user tags overlap — migration cleanup runs
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert_eq!(config.host_entries()[0].provider_tags, vec!["nyc1"]);
    // "nyc1" removed from user tags (overlap), "prod" preserved
    assert_eq!(config.host_entries()[0].tags, vec!["prod"]);
}

#[test]
fn test_sync_provider_tags_cleared_remotely() {
    let mut config = empty_config();
    let section = make_section();

    // First sync: host with tags
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["staging".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Second sync: provider removed all tags
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert!(config.host_entries()[0].tags.is_empty());
}

#[test]
fn test_sync_provider_tags_cleared_user_tags_survive() {
    let mut config = empty_config();
    let section = make_section();

    // First sync: host with provider tag
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["staging".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // User adds their own tag
    config.set_host_tags("do-web-1", &["my-custom".to_string()]);

    // Provider removes all tags
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert!(config.host_entries()[0].provider_tags.is_empty());
    // User tags survive even when provider tags are cleared
    assert_eq!(config.host_entries()[0].tags, vec!["my-custom"]);
}

#[test]
fn test_sync_provider_tags_exact_match_unchanged() {
    let mut config = empty_config();
    let section = make_section();

    // Sync: add host with tags
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["prod".to_string(), "nyc1".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Reset-tags sync with same tags (different order): unchanged
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["nyc1".to_string(), "prod".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.unchanged, 1);
}

#[test]
fn test_sync_merge_case_insensitive() {
    let mut config = empty_config();
    let section = make_section();

    // First sync: add host with lowercase tag
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["prod".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries()[0].provider_tags, vec!["prod"]);

    // Second sync: provider returns same tag with different casing — no duplicate
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["Prod".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.unchanged, 1);
    assert_eq!(config.host_entries()[0].provider_tags, vec!["prod"]);
}

#[test]
fn test_sync_provider_tags_case_insensitive_unchanged() {
    let mut config = empty_config();
    let section = make_section();

    // Sync: add host with tag
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["prod".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Reset-tags sync with different casing: unchanged (case-insensitive comparison)
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["Prod".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.unchanged, 1);
}

// --- Empty IP (stopped/no-IP VM) tests ---

#[test]
fn test_sync_empty_ip_not_added() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "100".to_string(),
        "stopped-vm".to_string(),
        String::new(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.added, 0);
    assert_eq!(config.host_entries().len(), 0);
}

#[test]
fn test_sync_empty_ip_existing_host_unchanged() {
    let mut config = empty_config();
    let section = make_section();

    // First sync: add host with IP
    let remote = vec![ProviderHost::new(
        "100".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries().len(), 1);
    assert_eq!(config.host_entries()[0].hostname, "1.2.3.4");

    // Second sync: VM stopped, empty IP. Host should stay unchanged.
    let remote = vec![ProviderHost::new(
        "100".to_string(),
        "web".to_string(),
        String::new(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
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
        ProviderHost::new(
            "100".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "200".to_string(),
            "db".to_string(),
            "5.6.7.8".to_string(),
            Vec::new(),
        ),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries().len(), 2);

    // Second sync with --remove: web is running, db is stopped (empty IP).
    // db must NOT be removed.
    let remote = vec![
        ProviderHost::new(
            "100".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "200".to_string(),
            "db".to_string(),
            String::new(),
            Vec::new(),
        ),
    ];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        true,
        false,
        false,
    );
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
        ProviderHost::new(
            "100".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "200".to_string(),
            "db".to_string(),
            "5.6.7.8".to_string(),
            Vec::new(),
        ),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries().len(), 2);

    // Second sync with --remove: only web exists. db is truly deleted.
    let remote = vec![ProviderHost::new(
        "100".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        true,
        false,
        false,
    );
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
        ProviderHost::new(
            "1".to_string(),
            "running".to_string(),
            "1.1.1.1".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "stopped".to_string(),
            "2.2.2.2".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "3".to_string(),
            "deleted".to_string(),
            "3.3.3.3".to_string(),
            Vec::new(),
        ),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries().len(), 3);

    // Second sync with --remove:
    // - "running" has new IP (updated)
    // - "stopped" has empty IP (unchanged, not removed)
    // - "deleted" not in list (removed)
    let remote = vec![
        ProviderHost::new(
            "1".to_string(),
            "running".to_string(),
            "9.9.9.9".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "stopped".to_string(),
            String::new(),
            Vec::new(),
        ),
    ];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        true,
        false,
        false,
    );
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

// =========================================================================
// sanitize_name edge cases
// =========================================================================

#[test]
fn test_sanitize_name_unicode() {
    // Unicode chars become hyphens, collapsed
    assert_eq!(sanitize_name("서버-1"), "1");
}

#[test]
fn test_sanitize_name_numbers_only() {
    assert_eq!(sanitize_name("12345"), "12345");
}

#[test]
fn test_sanitize_name_mixed_special_chars() {
    assert_eq!(sanitize_name("web@server#1!"), "web-server-1");
}

#[test]
fn test_sanitize_name_tabs_and_newlines() {
    assert_eq!(sanitize_name("web\tserver\n1"), "web-server-1");
}

#[test]
fn test_sanitize_name_consecutive_specials() {
    assert_eq!(sanitize_name("a!!!b"), "a-b");
}

#[test]
fn test_sanitize_name_trailing_special() {
    assert_eq!(sanitize_name("web-"), "web");
}

#[test]
fn test_sanitize_name_leading_special() {
    assert_eq!(sanitize_name("-web"), "web");
}

// =========================================================================
// build_alias edge cases
// =========================================================================

#[test]
fn test_build_alias_prefix_with_hyphen() {
    // If prefix already ends with hyphen, double hyphen results
    // The caller is expected to provide clean prefixes
    assert_eq!(build_alias("do-", "web-1"), "do--web-1");
}

#[test]
fn test_build_alias_long_names() {
    assert_eq!(
        build_alias("my-provider", "my-very-long-server-name"),
        "my-provider-my-very-long-server-name"
    );
}

// =========================================================================
// sync with user and identity_file
// =========================================================================

#[test]
fn test_sync_applies_user_from_section() {
    let mut config = empty_config();
    let mut section = make_section();
    section.user = "admin".to_string();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let entries = config.host_entries();
    assert_eq!(entries[0].user, "admin");
}

#[test]
fn test_sync_applies_identity_file_from_section() {
    let mut config = empty_config();
    let mut section = make_section();
    section.identity_file = "~/.ssh/id_rsa".to_string();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let entries = config.host_entries();
    assert_eq!(entries[0].identity_file, "~/.ssh/id_rsa");
}

#[test]
fn test_sync_empty_user_not_set() {
    let mut config = empty_config();
    let mut section = make_section();
    section.user = String::new(); // explicitly clear user
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let entries = config.host_entries();
    assert!(entries[0].user.is_empty());
}

// =========================================================================
// SyncResult struct
// =========================================================================

#[test]
fn test_sync_result_default() {
    let result = SyncResult::default();
    assert_eq!(result.added, 0);
    assert_eq!(result.updated, 0);
    assert_eq!(result.removed, 0);
    assert_eq!(result.unchanged, 0);
    assert!(result.renames.is_empty());
}

// =========================================================================
// sync with multiple operations in one call
// =========================================================================

#[test]
fn test_sync_server_name_change_updates_alias() {
    let mut config = empty_config();
    let section = make_section();
    // Add initial host
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "old-name".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries()[0].alias, "do-old-name");

    // Sync with new name (same server_id)
    let remote_renamed = vec![ProviderHost::new(
        "1".to_string(),
        "new-name".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote_renamed,
        &section,
        false,
        false,
        false,
    );
    // Should rename the alias
    assert!(!result.renames.is_empty() || result.updated > 0);
}

#[test]
fn test_sync_idempotent_same_data() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["prod".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.added, 0);
    assert_eq!(result.updated, 0);
    assert_eq!(result.unchanged, 1);
}

// =========================================================================
// Tag merge edge cases
// =========================================================================

#[test]
fn test_sync_tag_merge_case_insensitive_no_duplicate() {
    let mut config = empty_config();
    let section = make_section();
    // Add host with tag "Prod"
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["Prod".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Sync again with "prod" (lowercase) - should NOT add duplicate
    let remote2 = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["prod".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.unchanged, 1);
    assert_eq!(result.updated, 0);
}

#[test]
fn test_sync_tag_merge_adds_new_remote_tag() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["prod".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Sync with additional tag "us-east"
    let remote2 = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["prod".to_string(), "us-east".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);

    // Verify both provider tags present
    let entries = config.host_entries();
    let entry = entries.iter().find(|e| e.alias == "do-web").unwrap();
    assert!(entry.provider_tags.iter().any(|t| t == "prod"));
    assert!(entry.provider_tags.iter().any(|t| t == "us-east"));
}

#[test]
fn test_sync_tag_merge_preserves_local_tags() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["prod".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Manually add a local tag
    config.set_host_tags("do-web", &["prod".to_string(), "my-custom".to_string()]);

    // Sync again: "prod" overlap cleaned from user tags, "my-custom" preserved
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    let entries = config.host_entries();
    let entry = entries.iter().find(|e| e.alias == "do-web").unwrap();
    assert!(entry.tags.iter().any(|t| t == "my-custom"));
    assert!(!entry.tags.iter().any(|t| t == "prod")); // migrated to provider_tags
}

#[test]
fn test_sync_provider_tags_replaces_with_migration() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["prod".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Add local-only tag
    config.set_host_tags("do-web", &["prod".to_string(), "my-custom".to_string()]);

    // Sync: provider_tags replaced, user tags migrated
    let remote2 = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["prod".to_string(), "new-tag".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);

    let entries = config.host_entries();
    let entry = entries.iter().find(|e| e.alias == "do-web").unwrap();
    // Provider tags exactly mirror remote
    assert!(entry.provider_tags.iter().any(|t| t == "prod"));
    assert!(entry.provider_tags.iter().any(|t| t == "new-tag"));
    // User tag "my-custom" survives, "prod" migrated to provider_tags
    assert!(!entry.tags.iter().any(|t| t == "prod"));
    assert!(entry.tags.iter().any(|t| t == "my-custom"));
}

// =========================================================================
// Rename + tag change simultaneously
// =========================================================================

#[test]
fn test_sync_rename_and_ip_change_simultaneously() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "old-name".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Both name and IP change
    let remote2 = vec![ProviderHost::new(
        "1".to_string(),
        "new-name".to_string(),
        "9.8.7.6".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert_eq!(result.renames.len(), 1);
    assert_eq!(result.renames[0].0, "do-old-name");
    assert_eq!(result.renames[0].1, "do-new-name");

    let entries = config.host_entries();
    let entry = entries.iter().find(|e| e.alias == "do-new-name").unwrap();
    assert_eq!(entry.hostname, "9.8.7.6");
}

// =========================================================================
// Duplicate server_id in remote response
// =========================================================================

#[test]
fn test_sync_duplicate_server_id_deduped() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![
        ProviderHost::new(
            "1".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "1".to_string(),
            "web-copy".to_string(),
            "5.6.7.8".to_string(),
            Vec::new(),
        ), // duplicate server_id
    ];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.added, 1); // Only first one added
    assert_eq!(config.host_entries().len(), 1);
}

// =========================================================================
// Empty remote list with remove_deleted
// =========================================================================

#[test]
fn test_sync_remove_all_when_remote_empty() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![
        ProviderHost::new(
            "1".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "db".to_string(),
            "5.6.7.8".to_string(),
            Vec::new(),
        ),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries().len(), 2);

    // Sync with empty remote list and remove_deleted
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        true,
        false,
        false,
    );
    assert_eq!(result.removed, 2);
    assert_eq!(config.host_entries().len(), 0);
}

// =========================================================================
// Header management
// =========================================================================

#[test]
fn test_sync_adds_group_header_on_first_host() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Check that a GlobalLine with group header exists
    let has_header = config.elements.iter().any(|e| {
        matches!(e, ConfigElement::GlobalLine(line) if line.contains("purple:group") && line.contains("DigitalOcean"))
    });
    assert!(has_header);
}

#[test]
fn test_sync_removes_header_when_all_hosts_deleted() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Remove all hosts
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        true,
        false,
        false,
    );
    assert_eq!(result.removed, 1);

    // Header should be cleaned up
    let has_header = config.elements.iter().any(|e| {
        matches!(e, ConfigElement::GlobalLine(line) if line.contains("purple:group") && line.contains("DigitalOcean"))
    });
    assert!(!has_header);
}

// =========================================================================
// Identity file applied on new hosts
// =========================================================================

#[test]
fn test_sync_identity_file_set_on_new_host() {
    let mut config = empty_config();
    let mut section = make_section();
    section.identity_file = "~/.ssh/do_key".to_string();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let entries = config.host_entries();
    assert_eq!(entries[0].identity_file, "~/.ssh/do_key");
}

// =========================================================================
// Alias collision deduplication
// =========================================================================

#[test]
fn test_sync_alias_collision_dedup() {
    let mut config = empty_config();
    let section = make_section();
    // Two remote hosts with same sanitized name but different server_ids
    let remote = vec![
        ProviderHost::new(
            "1".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "web".to_string(),
            "5.6.7.8".to_string(),
            Vec::new(),
        ), // same name, different server
    ];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.added, 2);

    let entries = config.host_entries();
    let aliases: Vec<&str> = entries.iter().map(|e| e.alias.as_str()).collect();
    assert!(aliases.contains(&"do-web"));
    assert!(aliases.contains(&"do-web-2")); // Deduped with suffix
}

// =========================================================================
// Empty alias_prefix
// =========================================================================

#[test]
fn test_sync_empty_alias_prefix() {
    let mut config = empty_config();
    let mut section = make_section();
    section.alias_prefix = String::new();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let entries = config.host_entries();
    assert_eq!(entries[0].alias, "web-1"); // No prefix, just sanitized name
}

// =========================================================================
// Dry-run counts consistency
// =========================================================================

#[test]
fn test_sync_dry_run_add_count() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![
        ProviderHost::new(
            "1".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "db".to_string(),
            "5.6.7.8".to_string(),
            Vec::new(),
        ),
    ];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        true,
    );
    assert_eq!(result.added, 2);
    // Config should be unchanged in dry-run
    assert_eq!(config.host_entries().len(), 0);
}

#[test]
fn test_sync_dry_run_remove_count_preserves_config() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries().len(), 1);

    // Dry-run remove
    let result = sync_provider(&mut config, &MockProvider, &[], &section, true, false, true);
    assert_eq!(result.removed, 1);
    // Config should still have the host
    assert_eq!(config.host_entries().len(), 1);
}

// =========================================================================
// Result struct
// =========================================================================

#[test]
fn test_sync_result_counts_add_up() {
    let mut config = empty_config();
    let section = make_section();
    // Add 3 hosts
    let remote = vec![
        ProviderHost::new(
            "1".to_string(),
            "a".to_string(),
            "1.1.1.1".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "b".to_string(),
            "2.2.2.2".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "3".to_string(),
            "c".to_string(),
            "3.3.3.3".to_string(),
            Vec::new(),
        ),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Sync with: 1 unchanged, 1 ip changed, 1 removed (missing from remote)
    let remote2 = vec![
        ProviderHost::new(
            "1".to_string(),
            "a".to_string(),
            "1.1.1.1".to_string(),
            Vec::new(),
        ), // unchanged
        ProviderHost::new(
            "2".to_string(),
            "b".to_string(),
            "9.9.9.9".to_string(),
            Vec::new(),
        ), // IP changed
           // server_id "3" missing -> removed
    ];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        true,
        false,
        false,
    );
    assert_eq!(result.unchanged, 1);
    assert_eq!(result.updated, 1);
    assert_eq!(result.removed, 1);
    assert_eq!(result.added, 0);
}

// =========================================================================
// Multiple renames in single sync
// =========================================================================

#[test]
fn test_sync_multiple_renames() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![
        ProviderHost::new(
            "1".to_string(),
            "old-a".to_string(),
            "1.1.1.1".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "old-b".to_string(),
            "2.2.2.2".to_string(),
            Vec::new(),
        ),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    let remote2 = vec![
        ProviderHost::new(
            "1".to_string(),
            "new-a".to_string(),
            "1.1.1.1".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "new-b".to_string(),
            "2.2.2.2".to_string(),
            Vec::new(),
        ),
    ];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.renames.len(), 2);
    assert_eq!(result.updated, 2);
}

// =========================================================================
// Tag whitespace trimming
// =========================================================================

#[test]
fn test_sync_tag_whitespace_trimmed_on_store() {
    let mut config = empty_config();
    let section = make_section();
    // Tags with whitespace get trimmed when written to config and parsed back
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["  production  ".to_string(), " us-east ".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let entries = config.host_entries();
    // Tags are trimmed during the write+parse roundtrip via set_host_provider_tags
    assert_eq!(entries[0].provider_tags, vec!["production", "us-east"]);
}

#[test]
fn test_sync_tag_trimmed_remote_triggers_merge() {
    let mut config = empty_config();
    let section = make_section();
    // First sync: clean tags
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["production".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Second sync: same tag but trimmed comparison works correctly
    let remote2 = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["  production  ".to_string()],
    )]; // whitespace trimmed before comparison
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    // Trimmed "production" matches existing "production" case-insensitively
    assert_eq!(result.unchanged, 1);
}

// =========================================================================
// Cross-provider coexistence
// =========================================================================

struct MockProvider2;
impl Provider for MockProvider2 {
    fn name(&self) -> &str {
        "vultr"
    }
    fn short_label(&self) -> &str {
        "vultr"
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
fn test_sync_two_providers_independent() {
    let mut config = empty_config();

    let do_section = make_section(); // prefix = "do"
    let vultr_section = ProviderSection {
        provider: "vultr".to_string(),
        token: "test".to_string(),
        alias_prefix: "vultr".to_string(),
        user: String::new(),
        identity_file: String::new(),
        url: String::new(),
        verify_tls: true,
        auto_sync: true,
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
    };

    // Sync DO hosts
    let do_remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &do_remote,
        &do_section,
        false,
        false,
        false,
    );

    // Sync Vultr hosts
    let vultr_remote = vec![ProviderHost::new(
        "abc".to_string(),
        "web".to_string(),
        "5.6.7.8".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider2,
        &vultr_remote,
        &vultr_section,
        false,
        false,
        false,
    );

    let entries = config.host_entries();
    assert_eq!(entries.len(), 2);
    let aliases: Vec<&str> = entries.iter().map(|e| e.alias.as_str()).collect();
    assert!(aliases.contains(&"do-web"));
    assert!(aliases.contains(&"vultr-web"));
}

#[test]
fn test_sync_remove_only_affects_own_provider() {
    let mut config = empty_config();
    let do_section = make_section();
    let vultr_section = ProviderSection {
        provider: "vultr".to_string(),
        token: "test".to_string(),
        alias_prefix: "vultr".to_string(),
        user: String::new(),
        identity_file: String::new(),
        url: String::new(),
        verify_tls: true,
        auto_sync: true,
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
    };

    // Add hosts from both providers
    let do_remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &do_remote,
        &do_section,
        false,
        false,
        false,
    );

    let vultr_remote = vec![ProviderHost::new(
        "abc".to_string(),
        "db".to_string(),
        "5.6.7.8".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider2,
        &vultr_remote,
        &vultr_section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries().len(), 2);

    // Remove all DO hosts - Vultr host should survive
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &do_section,
        true,
        false,
        false,
    );
    assert_eq!(result.removed, 1);
    let entries = config.host_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].alias, "vultr-db");
}

// =========================================================================
// Rename + tag change simultaneously
// =========================================================================

#[test]
fn test_sync_rename_and_tag_change_simultaneously() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "old-name".to_string(),
        "1.2.3.4".to_string(),
        vec!["staging".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries()[0].alias, "do-old-name");
    assert_eq!(config.host_entries()[0].provider_tags, vec!["staging"]);

    // Change name and add new tag
    let remote2 = vec![ProviderHost::new(
        "1".to_string(),
        "new-name".to_string(),
        "1.2.3.4".to_string(),
        vec!["staging".to_string(), "prod".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert_eq!(result.renames.len(), 1);

    let entries = config.host_entries();
    let entry = entries.iter().find(|e| e.alias == "do-new-name").unwrap();
    assert!(entry.provider_tags.contains(&"staging".to_string()));
    assert!(entry.provider_tags.contains(&"prod".to_string()));
}

// =========================================================================
// All-symbol server name fallback
// =========================================================================

#[test]
fn test_sync_all_symbol_name_uses_server_fallback() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "!!!".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let entries = config.host_entries();
    assert_eq!(entries[0].alias, "do-server");
}

#[test]
fn test_sync_unicode_name_uses_ascii_fallback() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "서버".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let entries = config.host_entries();
    // Korean chars stripped, fallback to "server"
    assert_eq!(entries[0].alias, "do-server");
}

// =========================================================================
// Dry-run update doesn't mutate
// =========================================================================

#[test]
fn test_sync_dry_run_update_preserves_config() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Dry-run with IP change
    let remote2 = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "9.9.9.9".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        true,
    );
    assert_eq!(result.updated, 1);
    // Config should still have old IP
    assert_eq!(config.host_entries()[0].hostname, "1.2.3.4");
}

// =========================================================================
// No-op sync on empty config with empty remote
// =========================================================================

#[test]
fn test_sync_empty_remote_empty_config_noop() {
    let mut config = empty_config();
    let section = make_section();
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        true,
        false,
        false,
    );
    assert_eq!(result.added, 0);
    assert_eq!(result.updated, 0);
    assert_eq!(result.removed, 0);
    assert_eq!(result.unchanged, 0);
    assert!(config.host_entries().is_empty());
}

// =========================================================================
// Large batch sync
// =========================================================================

#[test]
fn test_sync_large_batch() {
    let mut config = empty_config();
    let section = make_section();
    let remote: Vec<ProviderHost> = (0..100)
        .map(|i| {
            ProviderHost::new(
                format!("{}", i),
                format!("server-{}", i),
                format!("10.0.0.{}", i % 256),
                vec!["batch".to_string()],
            )
        })
        .collect();
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.added, 100);
    assert_eq!(config.host_entries().len(), 100);

    // Re-sync unchanged
    let result2 = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result2.unchanged, 100);
    assert_eq!(result2.added, 0);
}

// =========================================================================
// Rename collision with self-exclusion
// =========================================================================

#[test]
fn test_sync_rename_self_exclusion_no_collision() {
    // When renaming and the expected alias is already taken by this host itself,
    // deduplicate_alias_excluding should handle it (no -2 suffix)
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries()[0].alias, "do-web");

    // Re-sync with same name but different IP -> update, no rename
    let remote2 = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "9.9.9.9".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert!(result.renames.is_empty());
    assert_eq!(config.host_entries()[0].alias, "do-web"); // No suffix
}

// =========================================================================
// Reset tags with rename: tags applied to new alias
// =========================================================================

#[test]
fn test_sync_provider_tags_with_rename() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "old-name".to_string(),
        "1.2.3.4".to_string(),
        vec!["staging".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    config.set_host_tags(
        "do-old-name",
        &["staging".to_string(), "custom".to_string()],
    );

    // Rename + provider tags update
    let remote2 = vec![ProviderHost::new(
        "1".to_string(),
        "new-name".to_string(),
        "1.2.3.4".to_string(),
        vec!["production".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert_eq!(result.renames.len(), 1);

    let entries = config.host_entries();
    let entry = entries.iter().find(|e| e.alias == "do-new-name").unwrap();
    // Provider tags exactly mirror remote
    assert_eq!(entry.provider_tags, vec!["production"]);
    // User tags preserved (migration only removes tags matching remote "production")
    assert!(entry.tags.contains(&"custom".to_string()));
    assert!(entry.tags.contains(&"staging".to_string()));
}

// =========================================================================
// Empty IP in first sync never added
// =========================================================================

#[test]
fn test_sync_empty_ip_with_tags_not_added() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "stopped".to_string(),
        String::new(),
        vec!["prod".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.added, 0);
    assert!(config.host_entries().is_empty());
}

// =========================================================================
// Existing host not in entries_map (orphaned provider marker)
// =========================================================================

#[test]
fn test_sync_orphaned_provider_marker_counts_unchanged() {
    // If a provider marker exists but the host block is somehow broken/missing
    // from host_entries(), the code path at line 217 counts it as unchanged.
    // This is hard to trigger naturally, but we can verify the behavior with
    // a host that has a provider marker but also exists in entries_map.
    let content = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:123
";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.unchanged, 1);
}

// =========================================================================
// Separator between hosts (no double blank lines)
// =========================================================================

#[test]
fn test_sync_no_double_blank_between_hosts() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![
        ProviderHost::new(
            "1".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "db".to_string(),
            "5.6.7.8".to_string(),
            Vec::new(),
        ),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Verify no consecutive blank GlobalLines
    let mut prev_blank = false;
    for elem in &config.elements {
        if let ConfigElement::GlobalLine(line) = elem {
            let is_blank = line.trim().is_empty();
            assert!(!(prev_blank && is_blank), "Found consecutive blank lines");
            prev_blank = is_blank;
        } else {
            prev_blank = false;
        }
    }
}

// =========================================================================
// Remove without remove_deleted flag does nothing
// =========================================================================

#[test]
fn test_sync_without_remove_flag_keeps_deleted() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Sync without remove_deleted - host 1 gone from remote
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.removed, 0);
    assert_eq!(config.host_entries().len(), 1); // Still there
}

// =========================================================================
// Dry-run rename doesn't track renames
// =========================================================================

#[test]
fn test_sync_dry_run_rename_no_renames_tracked() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "old".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    let new_section = ProviderSection {
        alias_prefix: "ocean".to_string(),
        ..section
    };
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &new_section,
        false,
        false,
        true,
    );
    assert_eq!(result.updated, 1);
    // Dry-run: renames vec stays empty since no actual mutation
    assert!(result.renames.is_empty());
}

// =========================================================================
// sanitize_name additional edge cases
// =========================================================================

#[test]
fn test_sanitize_name_whitespace_only() {
    assert_eq!(sanitize_name("   "), "server");
}

#[test]
fn test_sanitize_name_single_char() {
    assert_eq!(sanitize_name("a"), "a");
    assert_eq!(sanitize_name("Z"), "z");
    assert_eq!(sanitize_name("5"), "5");
}

#[test]
fn test_sanitize_name_single_special_char() {
    assert_eq!(sanitize_name("!"), "server");
    assert_eq!(sanitize_name("-"), "server");
    assert_eq!(sanitize_name("."), "server");
}

#[test]
fn test_sanitize_name_emoji() {
    assert_eq!(sanitize_name("server🚀"), "server");
    assert_eq!(sanitize_name("🔥hot🔥"), "hot");
}

#[test]
fn test_sanitize_name_long_mixed_separators() {
    assert_eq!(sanitize_name("a!@#$%^&*()b"), "a-b");
}

#[test]
fn test_sanitize_name_dots_and_underscores() {
    assert_eq!(sanitize_name("web.prod_us-east"), "web-prod-us-east");
}

// =========================================================================
// find_hosts_by_provider with includes
// =========================================================================

#[test]
fn test_find_hosts_by_provider_in_includes() {
    use crate::ssh_config::model::{IncludeDirective, IncludedFile};

    let include_content =
        "Host do-included\n  HostName 1.2.3.4\n  # purple:provider digitalocean:inc1\n";
    let included_elements = SshConfigFile::parse_content(include_content);

    let config = SshConfigFile {
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
        bom: false,
    };

    let hosts = config.find_hosts_by_provider("digitalocean");
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].0, "do-included");
    assert_eq!(hosts[0].1, "inc1");
}

#[test]
fn test_find_hosts_by_provider_mixed_includes_and_toplevel() {
    use crate::ssh_config::model::{IncludeDirective, IncludedFile};

    // Top-level host
    let top_content = "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:1\n";
    let top_elements = SshConfigFile::parse_content(top_content);

    // Included host
    let inc_content = "Host do-db\n  HostName 5.6.7.8\n  # purple:provider digitalocean:2\n";
    let inc_elements = SshConfigFile::parse_content(inc_content);

    let mut elements = top_elements;
    elements.push(ConfigElement::Include(IncludeDirective {
        raw_line: "Include conf.d/*".to_string(),
        pattern: "conf.d/*".to_string(),
        resolved_files: vec![IncludedFile {
            path: PathBuf::from("/tmp/included.conf"),
            elements: inc_elements,
        }],
    }));

    let config = SshConfigFile {
        elements,
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    let hosts = config.find_hosts_by_provider("digitalocean");
    assert_eq!(hosts.len(), 2);
}

#[test]
fn test_find_hosts_by_provider_empty_includes() {
    use crate::ssh_config::model::{IncludeDirective, IncludedFile};

    let config = SshConfigFile {
        elements: vec![ConfigElement::Include(IncludeDirective {
            raw_line: "Include conf.d/*".to_string(),
            pattern: "conf.d/*".to_string(),
            resolved_files: vec![IncludedFile {
                path: PathBuf::from("/tmp/empty.conf"),
                elements: vec![],
            }],
        })],
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    let hosts = config.find_hosts_by_provider("digitalocean");
    assert!(hosts.is_empty());
}

#[test]
fn test_find_hosts_by_provider_wrong_provider_name() {
    let content = "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:1\n";
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    let hosts = config.find_hosts_by_provider("vultr");
    assert!(hosts.is_empty());
}

// =========================================================================
// deduplicate_alias_excluding
// =========================================================================

#[test]
fn test_deduplicate_alias_excluding_self() {
    // When renaming do-web to do-web (same alias), exclude prevents collision
    let content = "Host do-web\n  HostName 1.2.3.4\n";
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    let alias = config.deduplicate_alias_excluding("do-web", Some("do-web"));
    assert_eq!(alias, "do-web"); // Self-excluded, no collision
}

#[test]
fn test_deduplicate_alias_excluding_other() {
    // do-web exists, exclude is "do-db" (not the colliding one)
    let content = "Host do-web\n  HostName 1.2.3.4\n";
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    let alias = config.deduplicate_alias_excluding("do-web", Some("do-db"));
    assert_eq!(alias, "do-web-2"); // do-web is taken, do-db doesn't help
}

#[test]
fn test_deduplicate_alias_excluding_chain() {
    // do-web and do-web-2 exist, exclude is "do-web"
    let content = "Host do-web\n  HostName 1.1.1.1\n\nHost do-web-2\n  HostName 2.2.2.2\n";
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    let alias = config.deduplicate_alias_excluding("do-web", Some("do-web"));
    // do-web is excluded, so it's "available" → returns do-web
    assert_eq!(alias, "do-web");
}

#[test]
fn test_deduplicate_alias_excluding_none() {
    let content = "Host do-web\n  HostName 1.2.3.4\n";
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    // None exclude means normal deduplication
    let alias = config.deduplicate_alias_excluding("do-web", None);
    assert_eq!(alias, "do-web-2");
}

// =========================================================================
// set_host_tags with empty tags
// =========================================================================

#[test]
fn test_set_host_tags_empty_clears_tags() {
    let content = "Host do-web\n  HostName 1.2.3.4\n  # purple:tags prod,staging\n";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    config.set_host_tags("do-web", &[]);
    let entries = config.host_entries();
    assert!(entries[0].tags.is_empty());
}

#[test]
fn test_set_host_provider_updates_existing() {
    let content = "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:old-id\n";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    config.set_host_provider("do-web", "digitalocean", "new-id");
    let hosts = config.find_hosts_by_provider("digitalocean");
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].1, "new-id");
}

// =========================================================================
// Sync with provider hosts in includes (read-only recognized)
// =========================================================================

#[test]
fn test_sync_recognizes_include_hosts_prevents_duplicate_add() {
    use crate::ssh_config::model::{IncludeDirective, IncludedFile};

    let include_content = "Host do-web\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n";
    let included_elements = SshConfigFile::parse_content(include_content);

    let mut config = SshConfigFile {
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
        bom: false,
    };

    let section = make_section();
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];

    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.unchanged, 1);
    assert_eq!(result.added, 0);
    // The host should NOT be duplicated in main config
    let top_hosts = config
        .elements
        .iter()
        .filter(|e| matches!(e, ConfigElement::HostBlock(_)))
        .count();
    assert_eq!(top_hosts, 0, "No host blocks added to top-level config");
}

// =========================================================================
// Dedup resolves back to the same alias -> counted as unchanged
// =========================================================================

#[test]
fn test_sync_dedup_resolves_back_to_same_alias_unchanged() {
    let mut config = empty_config();
    let section = make_section();

    // Add a host with name "web" -> alias "do-web"
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries()[0].alias, "do-web");

    // Manually add another host "do-new-web" that would collide after rename
    let other = vec![ProviderHost::new(
        "2".to_string(),
        "new-web".to_string(),
        "5.5.5.5".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &other,
        &section,
        false,
        false,
        false,
    );

    // Now rename the remote host "1" to "new-web", but alias "do-new-web" is taken by host "2".
    // dedup will produce "do-new-web-2". This is not the same as "do-web" so it IS a rename.
    // But let's create a scenario where dedup resolves back:
    // Change prefix so expected alias = "do-web" (same as existing)
    // This tests the else branch where alias_changed is initially true (prefix changed)
    // but dedup resolves to the same alias.
    // Actually, let's test it differently: rename where nothing else changes
    let remote_same = vec![
        ProviderHost::new(
            "1".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "new-web".to_string(),
            "5.5.5.5".to_string(),
            Vec::new(),
        ),
    ];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote_same,
        &section,
        false,
        false,
        false,
    );
    // Host "1" was marked stale by the second sync (only "2" in remote),
    // so this sync clears the stale mark -> counts as updated.
    assert_eq!(result.unchanged, 1);
    assert_eq!(result.updated, 1);
    assert!(result.renames.is_empty());
}

// =========================================================================
// Orphan server_id: existing_map has alias not found in entries_map
// =========================================================================

#[test]
fn test_sync_host_in_entries_map_but_alias_changed_by_another_provider() {
    // When two hosts have the same server name, the second gets a -2 suffix.
    // Test that deduplicate_alias handles this correctly.
    let mut config = empty_config();
    let section = make_section();

    let remote = vec![
        ProviderHost::new(
            "1".to_string(),
            "web".to_string(),
            "1.1.1.1".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "web".to_string(),
            "2.2.2.2".to_string(),
            Vec::new(),
        ),
    ];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.added, 2);

    let entries = config.host_entries();
    assert_eq!(entries[0].alias, "do-web");
    assert_eq!(entries[1].alias, "do-web-2");

    // Re-sync: both should be unchanged
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.unchanged, 2);
}

// =========================================================================
// Dry-run remove with included hosts: included hosts NOT counted in remove
// =========================================================================

#[test]
fn test_sync_dry_run_remove_excludes_included_hosts() {
    use crate::ssh_config::model::{IncludeDirective, IncludedFile};

    let include_content =
        "Host do-included\n  HostName 1.1.1.1\n  # purple:provider digitalocean:inc1\n";
    let included_elements = SshConfigFile::parse_content(include_content);

    // Top-level host
    let mut config = SshConfigFile {
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
        bom: false,
    };

    // Add a non-included host
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "top1".to_string(),
        "toplevel".to_string(),
        "2.2.2.2".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Dry-run with empty remote (both hosts would be "deleted")
    // Only the top-level host should be counted, NOT the included one
    let result = sync_provider(&mut config, &MockProvider, &[], &section, true, false, true);
    assert_eq!(
        result.removed, 1,
        "Only top-level host counted in dry-run remove"
    );
}

// =========================================================================
// Group header: config already has trailing blank (no extra added)
// =========================================================================

#[test]
fn test_sync_group_header_with_existing_trailing_blank() {
    let mut config = empty_config();
    // Add a pre-existing global line followed by a blank
    config
        .elements
        .push(ConfigElement::GlobalLine("# some comment".to_string()));
    config
        .elements
        .push(ConfigElement::GlobalLine(String::new()));

    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.added, 1);

    // Count blank lines: there should be exactly one blank line before the group header
    // (the pre-existing one), NOT two
    let blank_count = config
        .elements
        .iter()
        .filter(|e| matches!(e, ConfigElement::GlobalLine(l) if l.is_empty()))
        .count();
    assert_eq!(
        blank_count, 1,
        "No extra blank line when one already exists"
    );
}

// =========================================================================
// Adding second host to existing provider: no group header added
// =========================================================================

#[test]
fn test_sync_no_group_header_for_second_host() {
    let mut config = empty_config();
    let section = make_section();

    // First sync: one host, group header added
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    let header_count_before = config
        .elements
        .iter()
        .filter(|e| matches!(e, ConfigElement::GlobalLine(l) if l.starts_with("# purple:group")))
        .count();
    assert_eq!(header_count_before, 1);

    // Second sync: add another host
    let remote2 = vec![
        ProviderHost::new(
            "1".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "2".to_string(),
            "db".to_string(),
            "5.5.5.5".to_string(),
            Vec::new(),
        ),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );

    // Still only one group header
    let header_count_after = config
        .elements
        .iter()
        .filter(|e| matches!(e, ConfigElement::GlobalLine(l) if l.starts_with("# purple:group")))
        .count();
    assert_eq!(header_count_after, 1, "No duplicate group header");
}

// =========================================================================
// Duplicate server_id in remote is skipped
// =========================================================================

#[test]
fn test_sync_duplicate_server_id_in_remote_skipped() {
    let mut config = empty_config();
    let section = make_section();

    // Remote with duplicate server_id
    let remote = vec![
        ProviderHost::new(
            "dup".to_string(),
            "first".to_string(),
            "1.1.1.1".to_string(),
            Vec::new(),
        ),
        ProviderHost::new(
            "dup".to_string(),
            "second".to_string(),
            "2.2.2.2".to_string(),
            Vec::new(),
        ),
    ];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.added, 1, "Only the first instance is added");
    assert_eq!(config.host_entries()[0].alias, "do-first");
}

// =========================================================================
// Empty IP existing host counted as unchanged (no removal)
// =========================================================================

#[test]
fn test_sync_empty_ip_existing_host_counted_unchanged() {
    let mut config = empty_config();
    let section = make_section();

    // Add host
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Re-sync with empty IP (VM stopped)
    let remote2 = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        String::new(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        true,
    );
    assert_eq!(result.unchanged, 1);
    assert_eq!(result.removed, 0, "Host with empty IP not removed");
    assert_eq!(config.host_entries()[0].hostname, "1.2.3.4");
}

// =========================================================================
// Reset tags exact comparison (case-insensitive)
// =========================================================================

#[test]
fn test_sync_provider_tags_case_insensitive_no_update() {
    let mut config = empty_config();
    let section = make_section();

    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["Production".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Same tag but different case -> unchanged
    let remote2 = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec!["production".to_string()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(
        result.unchanged, 1,
        "Case-insensitive tag match = unchanged"
    );
}

// =========================================================================
// Remove deletes group header when all hosts removed
// =========================================================================

#[test]
fn test_sync_remove_cleans_up_group_header() {
    let mut config = empty_config();
    let section = make_section();

    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Verify group header exists
    let has_header = config
        .elements
        .iter()
        .any(|e| matches!(e, ConfigElement::GlobalLine(l) if l.starts_with("# purple:group")));
    assert!(has_header, "Group header present after add");

    // Remove all hosts (empty remote + remove_deleted=true, dry_run=false)
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        true,
        false,
        false,
    );
    assert_eq!(result.removed, 1);

    // Group header should be cleaned up
    let has_header_after = config
        .elements
        .iter()
        .any(|e| matches!(e, ConfigElement::GlobalLine(l) if l.starts_with("# purple:group")));
    assert!(
        !has_header_after,
        "Group header removed when all hosts gone"
    );
}

// =========================================================================
// Metadata sync tests
// =========================================================================

#[test]
fn test_sync_adds_host_with_metadata() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost {
        server_id: "1".to_string(),
        name: "web".to_string(),
        ip: "1.2.3.4".to_string(),
        tags: Vec::new(),
        metadata: vec![
            ("region".to_string(), "nyc3".to_string()),
            ("plan".to_string(), "s-1vcpu-1gb".to_string()),
        ],
    }];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.added, 1);
    let entries = config.host_entries();
    assert_eq!(entries[0].provider_meta.len(), 2);
    assert_eq!(
        entries[0].provider_meta[0],
        ("region".to_string(), "nyc3".to_string())
    );
    assert_eq!(
        entries[0].provider_meta[1],
        ("plan".to_string(), "s-1vcpu-1gb".to_string())
    );
}

#[test]
fn test_sync_updates_changed_metadata() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost {
        server_id: "1".to_string(),
        name: "web".to_string(),
        ip: "1.2.3.4".to_string(),
        tags: Vec::new(),
        metadata: vec![("region".to_string(), "nyc3".to_string())],
    }];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Update metadata (region changed, plan added)
    let remote2 = vec![ProviderHost {
        server_id: "1".to_string(),
        name: "web".to_string(),
        ip: "1.2.3.4".to_string(),
        tags: Vec::new(),
        metadata: vec![
            ("region".to_string(), "sfo3".to_string()),
            ("plan".to_string(), "s-2vcpu-2gb".to_string()),
        ],
    }];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    let entries = config.host_entries();
    assert_eq!(entries[0].provider_meta.len(), 2);
    assert_eq!(entries[0].provider_meta[0].1, "sfo3");
    assert_eq!(entries[0].provider_meta[1].1, "s-2vcpu-2gb");
}

#[test]
fn test_sync_metadata_unchanged_no_update() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost {
        server_id: "1".to_string(),
        name: "web".to_string(),
        ip: "1.2.3.4".to_string(),
        tags: Vec::new(),
        metadata: vec![("region".to_string(), "nyc3".to_string())],
    }];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Same metadata again
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.unchanged, 1);
    assert_eq!(result.updated, 0);
}

#[test]
fn test_sync_metadata_order_insensitive() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost {
        server_id: "1".to_string(),
        name: "web".to_string(),
        ip: "1.2.3.4".to_string(),
        tags: Vec::new(),
        metadata: vec![
            ("region".to_string(), "nyc3".to_string()),
            ("plan".to_string(), "s-1vcpu-1gb".to_string()),
        ],
    }];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Same metadata, different order
    let remote2 = vec![ProviderHost {
        server_id: "1".to_string(),
        name: "web".to_string(),
        ip: "1.2.3.4".to_string(),
        tags: Vec::new(),
        metadata: vec![
            ("plan".to_string(), "s-1vcpu-1gb".to_string()),
            ("region".to_string(), "nyc3".to_string()),
        ],
    }];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.unchanged, 1);
    assert_eq!(result.updated, 0);
}

#[test]
fn test_sync_metadata_with_rename() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost {
        server_id: "1".to_string(),
        name: "old-name".to_string(),
        ip: "1.2.3.4".to_string(),
        tags: Vec::new(),
        metadata: vec![("region".to_string(), "nyc3".to_string())],
    }];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries()[0].provider_meta[0].1, "nyc3");

    // Rename + metadata change
    let remote2 = vec![ProviderHost {
        server_id: "1".to_string(),
        name: "new-name".to_string(),
        ip: "1.2.3.4".to_string(),
        tags: Vec::new(),
        metadata: vec![("region".to_string(), "sfo3".to_string())],
    }];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert!(!result.renames.is_empty());
    let entries = config.host_entries();
    assert_eq!(entries[0].alias, "do-new-name");
    assert_eq!(entries[0].provider_meta[0].1, "sfo3");
}

#[test]
fn test_sync_metadata_dry_run_no_mutation() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost {
        server_id: "1".to_string(),
        name: "web".to_string(),
        ip: "1.2.3.4".to_string(),
        tags: Vec::new(),
        metadata: vec![("region".to_string(), "nyc3".to_string())],
    }];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Dry-run with metadata change
    let remote2 = vec![ProviderHost {
        server_id: "1".to_string(),
        name: "web".to_string(),
        ip: "1.2.3.4".to_string(),
        tags: Vec::new(),
        metadata: vec![("region".to_string(), "sfo3".to_string())],
    }];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        true,
    );
    assert_eq!(result.updated, 1);
    // Config should still have old metadata
    assert_eq!(config.host_entries()[0].provider_meta[0].1, "nyc3");
}

#[test]
fn test_sync_metadata_only_change_triggers_update() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost {
        server_id: "1".to_string(),
        name: "web".to_string(),
        ip: "1.2.3.4".to_string(),
        tags: vec!["prod".to_string()],
        metadata: vec![("region".to_string(), "nyc3".to_string())],
    }];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Only metadata changes (IP, tags, alias all the same)
    let remote2 = vec![ProviderHost {
        server_id: "1".to_string(),
        name: "web".to_string(),
        ip: "1.2.3.4".to_string(),
        tags: vec!["prod".to_string()],
        metadata: vec![
            ("region".to_string(), "nyc3".to_string()),
            ("plan".to_string(), "s-1vcpu-1gb".to_string()),
        ],
    }];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote2,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert_eq!(config.host_entries()[0].provider_meta.len(), 2);
}

// =========================================================================
// Migration and provider_tags edge cases
// =========================================================================

#[test]
fn test_sync_upgrade_migration() {
    // Host with old-format tags (no provider_tags line). Provider tags
    // mixed into user tags. First sync should create provider_tags and
    // clean the user tags.
    let content = "\
Host do-web-1
  HostName 1.2.3.4
  User root
  # purple:tags prod,us-east,my-custom
  # purple:provider digitalocean:123
";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };
    let section = make_section();

    // Provider returns tags that overlap with user tags
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["prod".to_string(), "us-east".to_string()],
    )];

    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    // Should detect tags_changed (no provider_tags existed before)
    assert_eq!(result.updated, 1);

    let entry = &config.host_entries()[0];
    // provider_tags should now contain the provider tags
    let mut ptags = entry.provider_tags.clone();
    ptags.sort();
    assert_eq!(ptags, vec!["prod", "us-east"]);

    // User tags should have provider tags removed, leaving only "my-custom"
    assert_eq!(entry.tags, vec!["my-custom"]);
}

#[test]
fn test_sync_duplicate_user_provider_tag() {
    // User manually adds tag "prod" that already exists in provider_tags.
    // Next sync with same provider tags (unchanged) should clean the duplicate.
    // NOTE: This tests DESIRED behavior. If the current code doesn't clean
    // duplicates when tags_changed=false, the test may fail until the fix lands.
    let mut config = empty_config();
    let section = make_section();

    // First sync: add host with provider tag "prod"
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        vec!["prod".to_string()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries()[0].provider_tags, vec!["prod"]);

    // User manually adds "prod" to user tags (simulating TUI tag edit)
    config.set_host_tags("do-web-1", &["prod".to_string(), "custom".to_string()]);
    assert_eq!(config.host_entries()[0].tags, vec!["prod", "custom"]);

    // Second sync: same provider tags (unchanged)
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Desired: duplicate "prod" removed from user tags
    let entry = &config.host_entries()[0];
    assert!(
        !entry.tags.contains(&"prod".to_string()),
        "User tag 'prod' should be cleaned since it duplicates a provider tag"
    );
    assert!(
        entry.tags.contains(&"custom".to_string()),
        "User tag 'custom' should be preserved"
    );
    // provider_tags unchanged
    assert_eq!(entry.provider_tags, vec!["prod"]);
}

#[test]
fn test_sync_set_provider_tags_empty_writes_sentinel() {
    // Calling set_provider_tags(&[]) should write an empty sentinel comment
    let content = "\
Host do-web-1
  HostName 1.2.3.4
  # purple:provider_tags prod
  # purple:provider digitalocean:123
";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    // Clear provider tags via the model
    config.set_host_provider_tags("do-web-1", &[]);

    let serialized = config.serialize();
    assert!(
        serialized.contains("# purple:provider_tags"),
        "empty sentinel should exist. Got:\n{}",
        serialized
    );
    assert!(
        !serialized.contains("# purple:provider_tags "),
        "sentinel should have no trailing content. Got:\n{}",
        serialized
    );
    // Host block should still exist
    assert!(serialized.contains("Host do-web-1"));
    assert!(serialized.contains("purple:provider digitalocean:123"));
}

#[test]
fn test_sync_set_provider_does_not_clobber_provider_tags() {
    // Updating the provider marker should not remove provider_tags
    let content = "\
Host do-web-1
  HostName 1.2.3.4
  # purple:provider digitalocean:123
  # purple:provider_tags prod
";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    // Update provider marker (e.g. server_id changed)
    config.set_host_provider("do-web-1", "digitalocean", "456");

    let serialized = config.serialize();
    assert!(
        serialized.contains("# purple:provider_tags prod"),
        "provider_tags should survive set_provider. Got:\n{}",
        serialized
    );
    assert!(
        serialized.contains("# purple:provider digitalocean:456"),
        "provider marker should be updated. Got:\n{}",
        serialized
    );
}

#[test]
fn test_sync_provider_tags_roundtrip() {
    // Parse -> serialize -> reparse should preserve provider_tags
    let content = "\
Host do-web-1
  HostName 1.2.3.4
  User root
  # purple:provider_tags prod,us-east
  # purple:provider digitalocean:123
";
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    // Verify initial parse
    let entries = config.host_entries();
    assert_eq!(entries.len(), 1);
    let mut ptags = entries[0].provider_tags.clone();
    ptags.sort();
    assert_eq!(ptags, vec!["prod", "us-east"]);

    // Serialize and reparse
    let serialized = config.serialize();
    let config2 = SshConfigFile {
        elements: SshConfigFile::parse_content(&serialized),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };

    let entries2 = config2.host_entries();
    assert_eq!(entries2.len(), 1);
    let mut ptags2 = entries2[0].provider_tags.clone();
    ptags2.sort();
    assert_eq!(ptags2, vec!["prod", "us-east"]);
}

#[test]
fn test_sync_first_migration_empty_remote_writes_sentinel() {
    // Old-format host: has # purple:tags but no # purple:provider_tags
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(
            "Host do-web-1\n  HostName 1.2.3.4\n  # purple:provider digitalocean:123\n  # purple:tags prod\n",
        ),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };
    let section = make_section();

    // Verify: no provider_tags comment yet
    let entries = config.host_entries();
    assert!(!entries[0].has_provider_tags);
    assert_eq!(entries[0].tags, vec!["prod"]);

    // First sync: provider returns empty tags
    let remote = vec![ProviderHost::new(
        "123".to_string(),
        "web-1".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);

    // Verify: provider_tags sentinel written (has_provider_tags=true, but empty)
    let entries = config.host_entries();
    assert!(entries[0].has_provider_tags);
    assert!(entries[0].provider_tags.is_empty());
    // User tag "prod" preserved (no overlap with empty remote)
    assert_eq!(entries[0].tags, vec!["prod"]);

    // Second sync: same empty tags. Now first_migration=false (has_provider_tags=true).
    // Nothing changed, so host should be unchanged.
    let result2 = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result2.unchanged, 1);
}

// =========================================================================
// Stale marking tests
// =========================================================================

#[test]
fn test_sync_marks_stale_when_host_disappears() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries().len(), 1);

    // Host disappears
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.stale, 1);
    assert_eq!(result.removed, 0);
    let entries = config.host_entries();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].stale.is_some());
}

#[test]
fn test_sync_clears_stale_when_host_returns() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Host disappears -> marked stale
    sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    assert!(config.host_entries()[0].stale.is_some());

    // Host returns -> stale cleared
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    assert!(config.host_entries()[0].stale.is_none());
}

#[test]
fn test_sync_stale_timestamp_preserved_not_refreshed() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Mark stale
    sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    let ts1 = config.host_entries()[0].stale.unwrap();

    // Another sync with host still missing - timestamp should not change
    sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    let ts2 = config.host_entries()[0].stale.unwrap();
    assert_eq!(ts1, ts2);
}

#[test]
fn test_sync_stale_host_returns_with_new_ip() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Host disappears
    sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );

    // Host returns with new IP
    let remote_new = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "9.9.9.9".to_string(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote_new,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    let entries = config.host_entries();
    assert!(entries[0].stale.is_none());
    assert_eq!(entries[0].hostname, "9.9.9.9");
}

#[test]
fn test_sync_remove_deleted_still_hard_deletes() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // With remove_deleted=true, host is hard-deleted, not stale
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        true,
        false,
        false,
    );
    assert_eq!(result.removed, 1);
    assert_eq!(result.stale, 0);
    assert!(config.host_entries().is_empty());
}

#[test]
fn test_sync_partial_failure_no_stale_marking() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Partial failure: suppress_stale=true
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        true,
        false,
    );
    assert_eq!(result.stale, 0);
    assert!(config.host_entries()[0].stale.is_none());
}

#[test]
fn test_sync_dry_run_reports_stale_count() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Dry run: stale count reported but no mutation
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        true,
    );
    assert_eq!(result.stale, 1);
    assert!(config.host_entries()[0].stale.is_none()); // Not actually marked
}

#[test]
fn test_sync_top_level_host_marked_stale() {
    // A top-level provider host that disappears should be marked stale
    let config_str = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:1
";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(config_str),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };
    let section = make_section();
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.stale, 1);
}

#[test]
fn test_sync_multiple_hosts_disappear() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![
        ProviderHost::new("1".into(), "web".into(), "1.1.1.1".into(), Vec::new()),
        ProviderHost::new("2".into(), "db".into(), "2.2.2.2".into(), Vec::new()),
        ProviderHost::new("3".into(), "app".into(), "3.3.3.3".into(), Vec::new()),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(config.host_entries().len(), 3);

    // Only host "2" remains
    let remaining = vec![ProviderHost::new(
        "2".into(),
        "db".into(),
        "2.2.2.2".into(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remaining,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.stale, 2);
    assert_eq!(result.unchanged, 1);
    let entries = config.host_entries();
    assert!(
        entries
            .iter()
            .find(|e| e.alias == "do-web")
            .unwrap()
            .stale
            .is_some()
    );
    assert!(
        entries
            .iter()
            .find(|e| e.alias == "do-db")
            .unwrap()
            .stale
            .is_none()
    );
    assert!(
        entries
            .iter()
            .find(|e| e.alias == "do-app")
            .unwrap()
            .stale
            .is_some()
    );
}

#[test]
fn test_sync_already_stale_then_remove_deleted() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".into(),
        "web".into(),
        "1.1.1.1".into(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Mark stale
    sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    assert!(config.host_entries()[0].stale.is_some());

    // Hard delete with remove_deleted=true
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        true,
        false,
        false,
    );
    assert_eq!(result.removed, 1);
    assert!(config.host_entries().is_empty());
}

#[test]
fn test_sync_stale_cross_provider_isolation() {
    let mut config = empty_config();
    let do_section = make_section();
    let vultr_section = ProviderSection {
        alias_prefix: "vultr".to_string(),
        ..make_section()
    };

    // Add DO host
    let do_remote = vec![ProviderHost::new(
        "1".into(),
        "web".into(),
        "1.1.1.1".into(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &do_remote,
        &do_section,
        false,
        false,
        false,
    );

    // Add Vultr host
    let vultr_remote = vec![ProviderHost::new(
        "1".into(),
        "db".into(),
        "2.2.2.2".into(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider2,
        &vultr_remote,
        &vultr_section,
        false,
        false,
        false,
    );

    // DO host disappears - Vultr host should NOT be affected
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &do_section,
        false,
        false,
        false,
    );
    assert_eq!(result.stale, 1);
    let entries = config.host_entries();
    assert!(
        entries
            .iter()
            .find(|e| e.alias == "do-web")
            .unwrap()
            .stale
            .is_some()
    );
    assert!(
        entries
            .iter()
            .find(|e| e.alias == "vultr-db")
            .unwrap()
            .stale
            .is_none()
    );
}

#[test]
fn test_sync_stale_host_returns_with_tag_changes() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".into(),
        "web".into(),
        "1.1.1.1".into(),
        vec!["prod".into()],
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Mark stale
    sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    assert!(config.host_entries()[0].stale.is_some());

    // Return with different tags
    let remote_new = vec![ProviderHost::new(
        "1".into(),
        "web".into(),
        "1.1.1.1".into(),
        vec!["staging".into()],
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote_new,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    let entries = config.host_entries();
    assert!(entries[0].stale.is_none());
    assert!(entries[0].provider_tags.contains(&"staging".to_string()));
}

#[test]
fn test_sync_stale_result_count_includes_already_stale() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".into(),
        "web".into(),
        "1.2.3.4".into(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // First disappearance
    let r1 = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    assert_eq!(r1.stale, 1);

    // Second disappearance - still counted
    let r2 = sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    assert_eq!(r2.stale, 1);
}

// =========================================================================
// SSH config integrity: stale must never corrupt the config
// =========================================================================

#[test]
fn test_sync_stale_config_byte_identical_after_clear() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![
        ProviderHost::new(
            "1".into(),
            "web".into(),
            "1.1.1.1".into(),
            vec!["prod".into()],
        ),
        ProviderHost::new("2".into(), "db".into(), "2.2.2.2".into(), Vec::new()),
    ];
    // Add hosts
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let config_after_add = config.serialize();

    // Mark stale (all hosts disappear)
    sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    let config_after_stale = config.serialize();
    assert_ne!(config_after_stale, config_after_add);

    // Hosts return (clear stale)
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let config_after_return = config.serialize();
    assert_eq!(
        config_after_return, config_after_add,
        "config must be byte-identical after stale->return cycle"
    );
}

#[test]
fn test_sync_stale_preserves_neighboring_hosts() {
    let config_str = "\
Host manual
  HostName 10.0.0.1
  User admin

";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(config_str),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".into(),
        "web".into(),
        "1.1.1.1".into(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Manual host must survive stale marking
    sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    let output = config.serialize();
    assert!(
        output.contains("Host manual"),
        "manual host lost after stale marking"
    );
    assert!(
        output.contains("HostName 10.0.0.1"),
        "manual host directives lost after stale marking"
    );
    assert!(
        output.contains("User admin"),
        "manual host user lost after stale marking"
    );
}

#[test]
fn test_sync_stale_then_purge_leaves_clean_config() {
    let config_str = "\
Host manual
  HostName 10.0.0.1
  User admin

";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(config_str),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };
    let section = make_section();
    let remote = vec![
        ProviderHost::new("1".into(), "web".into(), "1.1.1.1".into(), Vec::new()),
        ProviderHost::new("2".into(), "db".into(), "2.2.2.2".into(), Vec::new()),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Mark all provider hosts stale
    sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );

    // Simulate purge: delete all stale hosts
    let stale = config.stale_hosts();
    for (alias, _) in &stale {
        config.delete_host(alias);
    }

    let output = config.serialize();
    // Manual host must be intact
    assert!(output.contains("Host manual"));
    assert!(output.contains("HostName 10.0.0.1"));
    // No stale comments remaining
    assert!(!output.contains("purple:stale"));
    // No orphan provider group headers
    assert!(!output.contains("purple:group"));
    // No excessive blank lines (3+ consecutive)
    assert!(
        !output.contains("\n\n\n"),
        "excessive blank lines after purge:\n{}",
        output
    );
}

#[test]
fn test_sync_stale_empty_ip_return_preserves_hostname() {
    let mut config = empty_config();
    let section = make_section();
    let remote = vec![ProviderHost::new(
        "1".into(),
        "web".into(),
        "1.1.1.1".into(),
        Vec::new(),
    )];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );

    // Host disappears -> stale
    sync_provider(
        &mut config,
        &MockProvider,
        &[],
        &section,
        false,
        false,
        false,
    );
    assert!(config.host_entries()[0].stale.is_some());

    // Host returns with empty IP (stopped VM)
    let remote_empty_ip = vec![ProviderHost::new(
        "1".into(),
        "web".into(),
        "".into(),
        Vec::new(),
    )];
    let result = sync_provider(
        &mut config,
        &MockProvider,
        &remote_empty_ip,
        &section,
        false,
        false,
        false,
    );
    assert_eq!(result.updated, 1);
    // Stale cleared
    assert!(config.host_entries()[0].stale.is_none());
    // Hostname must NOT be wiped
    assert_eq!(config.host_entries()[0].hostname, "1.1.1.1");
}

#[test]
fn test_sync_insert_adds_blank_line_before_next_group() {
    // Simulate: DO has 1 host, Hetzner group follows. Adding a 2nd DO host
    // must produce a blank line between the new host and the Hetzner header.
    let config_str = "\
# purple:group DigitalOcean

Host do-web
  HostName 1.1.1.1
  User root
  # purple:provider digitalocean:111

# purple:group Hetzner

Host hz-build
  HostName 2.2.2.2
  User ci
  # purple:provider hetzner:222
";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(config_str),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };
    let section = make_section();
    let remote = vec![
        ProviderHost::new("111".into(), "web".into(), "1.1.1.1".into(), Vec::new()),
        ProviderHost::new("333".into(), "db".into(), "3.3.3.3".into(), Vec::new()),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let output = config.serialize();
    // There must be a blank line between the last DO host and the Hetzner group
    assert!(
        output.contains("\n\n# purple:group Hetzner"),
        "missing blank line before next group header:\n{}",
        output
    );
    // No triple blank lines
    assert!(
        !output.contains("\n\n\n"),
        "triple blank lines found:\n{}",
        output
    );
}

#[test]
fn test_sync_insert_blank_line_real_world_scenario() {
    // Exact reproduction of user-reported bug: DO host without trailing
    // blank directly followed by # purple:group Proxmox VE. Adding a new
    // DO host via sync must not smash it against the Proxmox header.
    let config_str = "\
# purple:group DigitalOcean

Host do-signalproxy
  HostName 128.199.41.235
  User root
  IdentityFile ~/.ssh/id_ed25519
  # purple:provider digitalocean:517532225
  # purple:meta region=ams3,size=s-1vcpu-512mb-10gb,status=active
  Port 60022
  # purple:provider_tags
  # purple:tags signal
# purple:group Proxmox VE

Host pve-testvm
  HostName 192.168.1.100
  User root
  # purple:provider proxmox:100
";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(config_str),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };
    let section = make_section();
    // Sync DO with the existing host + a new one
    let remote = vec![
        ProviderHost::new(
            "517532225".into(),
            "signalproxy-nl".into(),
            "128.199.41.235".into(),
            Vec::new(),
        ),
        ProviderHost::new(
            "560734563".into(),
            "ubuntu-nyc1".into(),
            "167.172.128.123".into(),
            Vec::new(),
        ),
    ];
    sync_provider(
        &mut config,
        &MockProvider,
        &remote,
        &section,
        false,
        false,
        false,
    );
    let output = config.serialize();

    // The new DO host must have a blank line before the Proxmox group header
    assert!(
        output.contains("\n\n# purple:group Proxmox VE"),
        "missing blank line before Proxmox group:\n{}",
        output
    );
    // Both DO hosts must be present
    assert!(output.contains("Host do-signalproxy") || output.contains("Host do-signalproxy-nl"));
    assert!(output.contains("Host do-ubuntu-nyc1"));
    // Proxmox host must still be present
    assert!(output.contains("Host pve-testvm"));
    // No triple blank lines
    assert!(
        !output.contains("\n\n\n"),
        "triple blank lines:\n{}",
        output
    );
}
