use super::*;

/// Unique scratch path per call so parallel `cargo test` threads cannot
/// race on the same config file during `SshConfigFile::write()`.
fn test_config_path() -> PathBuf {
    tempfile::tempdir()
        .expect("tempdir")
        .keep()
        .join("test_config")
}

fn parse_str(content: &str) -> SshConfigFile {
    SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: test_config_path(),
        crlf: false,
        bom: false,
    }
}

#[test]
fn tunnel_directives_extracts_forwards() {
    let config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n  RemoteForward 9090 localhost:3000\n  DynamicForward 1080\n",
    );
    if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
        let rules = block.tunnel_directives();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].tunnel_type, crate::tunnel::TunnelType::Local);
        assert_eq!(rules[0].bind_port, 8080);
        assert_eq!(rules[1].tunnel_type, crate::tunnel::TunnelType::Remote);
        assert_eq!(rules[2].tunnel_type, crate::tunnel::TunnelType::Dynamic);
    } else {
        panic!("Expected HostBlock");
    }
}

#[test]
fn tunnel_count_counts_forwards() {
    let config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n  RemoteForward 9090 localhost:3000\n",
    );
    if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
        assert_eq!(block.tunnel_count(), 2);
    } else {
        panic!("Expected HostBlock");
    }
}

#[test]
fn tunnel_count_zero_for_no_forwards() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  User admin\n");
    if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
        assert_eq!(block.tunnel_count(), 0);
        assert!(!block.has_tunnels());
    } else {
        panic!("Expected HostBlock");
    }
}

#[test]
fn has_tunnels_true_with_forward() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  DynamicForward 1080\n");
    if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
        assert!(block.has_tunnels());
    } else {
        panic!("Expected HostBlock");
    }
}

#[test]
fn add_forward_inserts_directive() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  User admin\n");
    config.add_forward("myserver", "LocalForward", "8080 localhost:80");
    let output = config.serialize();
    assert!(output.contains("LocalForward 8080 localhost:80"));
    // Existing directives preserved
    assert!(output.contains("HostName 10.0.0.1"));
    assert!(output.contains("User admin"));
}

#[test]
fn add_forward_preserves_indentation() {
    let mut config = parse_str("Host myserver\n\tHostName 10.0.0.1\n");
    config.add_forward("myserver", "LocalForward", "8080 localhost:80");
    let output = config.serialize();
    assert!(output.contains("\tLocalForward 8080 localhost:80"));
}

#[test]
fn add_multiple_forwards_same_type() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    config.add_forward("myserver", "LocalForward", "8080 localhost:80");
    config.add_forward("myserver", "LocalForward", "9090 localhost:90");
    let output = config.serialize();
    assert!(output.contains("LocalForward 8080 localhost:80"));
    assert!(output.contains("LocalForward 9090 localhost:90"));
}

#[test]
fn remove_forward_removes_exact_match() {
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n  LocalForward 9090 localhost:90\n",
    );
    config.remove_forward("myserver", "LocalForward", "8080 localhost:80");
    let output = config.serialize();
    assert!(!output.contains("8080 localhost:80"));
    assert!(output.contains("9090 localhost:90"));
}

#[test]
fn remove_forward_leaves_other_directives() {
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n  User admin\n",
    );
    config.remove_forward("myserver", "LocalForward", "8080 localhost:80");
    let output = config.serialize();
    assert!(!output.contains("LocalForward"));
    assert!(output.contains("HostName 10.0.0.1"));
    assert!(output.contains("User admin"));
}

#[test]
fn remove_forward_no_match_is_noop() {
    let original = "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n";
    let mut config = parse_str(original);
    config.remove_forward("myserver", "LocalForward", "9999 localhost:99");
    assert_eq!(config.serialize(), original);
}

#[test]
fn host_entry_tunnel_count_populated() {
    let config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n  DynamicForward 1080\n",
    );
    let entries = config.host_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].tunnel_count, 2);
}

#[test]
fn remove_forward_returns_true_on_match() {
    let mut config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n");
    assert!(config.remove_forward("myserver", "LocalForward", "8080 localhost:80"));
}

#[test]
fn remove_forward_returns_false_on_no_match() {
    let mut config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n");
    assert!(!config.remove_forward("myserver", "LocalForward", "9999 localhost:99"));
}

#[test]
fn remove_forward_returns_false_for_unknown_host() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    assert!(!config.remove_forward("nohost", "LocalForward", "8080 localhost:80"));
}

#[test]
fn has_forward_finds_match() {
    let config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n");
    assert!(config.has_forward("myserver", "LocalForward", "8080 localhost:80"));
}

#[test]
fn has_forward_no_match() {
    let config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n");
    assert!(!config.has_forward("myserver", "LocalForward", "9999 localhost:99"));
    assert!(!config.has_forward("nohost", "LocalForward", "8080 localhost:80"));
}

#[test]
fn has_forward_case_insensitive_key() {
    let config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  localforward 8080 localhost:80\n");
    assert!(config.has_forward("myserver", "LocalForward", "8080 localhost:80"));
}

#[test]
fn add_forward_to_empty_block() {
    let mut config = parse_str("Host myserver\n");
    config.add_forward("myserver", "LocalForward", "8080 localhost:80");
    let output = config.serialize();
    assert!(output.contains("LocalForward 8080 localhost:80"));
}

#[test]
fn remove_forward_case_insensitive_key_match() {
    let mut config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  localforward 8080 localhost:80\n");
    assert!(config.remove_forward("myserver", "LocalForward", "8080 localhost:80"));
    assert!(!config.serialize().contains("localforward"));
}

#[test]
fn tunnel_count_case_insensitive() {
    let config = parse_str(
        "Host myserver\n  localforward 8080 localhost:80\n  REMOTEFORWARD 9090 localhost:90\n  dynamicforward 1080\n",
    );
    if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
        assert_eq!(block.tunnel_count(), 3);
    } else {
        panic!("Expected HostBlock");
    }
}

#[test]
fn tunnel_directives_extracts_all_types() {
    let config = parse_str(
        "Host myserver\n  LocalForward 8080 localhost:80\n  RemoteForward 9090 localhost:3000\n  DynamicForward 1080\n",
    );
    if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
        let rules = block.tunnel_directives();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].tunnel_type, crate::tunnel::TunnelType::Local);
        assert_eq!(rules[1].tunnel_type, crate::tunnel::TunnelType::Remote);
        assert_eq!(rules[2].tunnel_type, crate::tunnel::TunnelType::Dynamic);
    } else {
        panic!("Expected HostBlock");
    }
}

#[test]
fn tunnel_directives_skips_malformed() {
    let config = parse_str("Host myserver\n  LocalForward not_valid\n  DynamicForward 1080\n");
    if let Some(ConfigElement::HostBlock(block)) = config.elements.first() {
        let rules = block.tunnel_directives();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].bind_port, 1080);
    } else {
        panic!("Expected HostBlock");
    }
}

#[test]
fn find_tunnel_directives_multi_pattern_host() {
    let config =
        parse_str("Host prod staging\n  HostName 10.0.0.1\n  LocalForward 8080 localhost:80\n");
    let rules = config.find_tunnel_directives("prod");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].bind_port, 8080);
    let rules2 = config.find_tunnel_directives("staging");
    assert_eq!(rules2.len(), 1);
}

#[test]
fn find_tunnel_directives_no_match() {
    let config = parse_str("Host myserver\n  LocalForward 8080 localhost:80\n");
    let rules = config.find_tunnel_directives("nohost");
    assert!(rules.is_empty());
}

#[test]
fn has_forward_exact_match() {
    let config = parse_str("Host myserver\n  LocalForward 8080 localhost:80\n");
    assert!(config.has_forward("myserver", "LocalForward", "8080 localhost:80"));
    assert!(!config.has_forward("myserver", "LocalForward", "9090 localhost:80"));
    assert!(!config.has_forward("myserver", "RemoteForward", "8080 localhost:80"));
    assert!(!config.has_forward("nohost", "LocalForward", "8080 localhost:80"));
}

#[test]
fn has_forward_whitespace_normalized() {
    let config = parse_str("Host myserver\n  LocalForward 8080  localhost:80\n");
    // Extra space in config value vs single space in query: should still match
    assert!(config.has_forward("myserver", "LocalForward", "8080 localhost:80"));
}

#[test]
fn has_forward_multi_pattern_host() {
    let config = parse_str("Host prod staging\n  LocalForward 8080 localhost:80\n");
    assert!(config.has_forward("prod", "LocalForward", "8080 localhost:80"));
    assert!(config.has_forward("staging", "LocalForward", "8080 localhost:80"));
}

#[test]
fn add_forward_multi_pattern_host() {
    let mut config = parse_str("Host prod staging\n  HostName 10.0.0.1\n");
    config.add_forward("prod", "LocalForward", "8080 localhost:80");
    assert!(config.has_forward("prod", "LocalForward", "8080 localhost:80"));
    assert!(config.has_forward("staging", "LocalForward", "8080 localhost:80"));
}

#[test]
fn remove_forward_multi_pattern_host() {
    let mut config = parse_str(
        "Host prod staging\n  LocalForward 8080 localhost:80\n  LocalForward 9090 localhost:90\n",
    );
    assert!(config.remove_forward("staging", "LocalForward", "8080 localhost:80"));
    assert!(!config.has_forward("staging", "LocalForward", "8080 localhost:80"));
    // Other forward should remain
    assert!(config.has_forward("staging", "LocalForward", "9090 localhost:90"));
}

#[test]
fn edit_tunnel_detects_duplicate_after_remove() {
    // Simulates edit flow: remove old, then check if new value already exists
    let mut config = parse_str(
        "Host myserver\n  LocalForward 8080 localhost:80\n  LocalForward 9090 localhost:90\n",
    );
    // Edit rule A (8080) toward rule B (9090): remove A first
    assert!(config.remove_forward("myserver", "LocalForward", "8080 localhost:80"));
    // Now check if the target value already exists: should detect duplicate
    assert!(config.has_forward("myserver", "LocalForward", "9090 localhost:90"));
}

#[test]
fn has_forward_tab_whitespace_normalized() {
    let config = parse_str("Host myserver\n  LocalForward 8080\tlocalhost:80\n");
    // Tab in config value vs space in query: should match via values_match
    assert!(config.has_forward("myserver", "LocalForward", "8080 localhost:80"));
}

#[test]
fn remove_forward_tab_whitespace_normalized() {
    let mut config = parse_str("Host myserver\n  LocalForward 8080\tlocalhost:80\n");
    // Remove with single space should match tab-separated value
    assert!(config.remove_forward("myserver", "LocalForward", "8080 localhost:80"));
    assert!(!config.has_forward("myserver", "LocalForward", "8080 localhost:80"));
}

#[test]
fn upsert_preserves_space_separator_when_value_contains_equals() {
    let mut config = parse_str("Host myserver\n  IdentityFile ~/.ssh/id=prod\n");
    let entry = HostEntry {
        alias: "myserver".to_string(),
        hostname: "10.0.0.1".to_string(),
        identity_file: "~/.ssh/id=staging".to_string(),
        port: 22,
        ..Default::default()
    };
    config.update_host("myserver", &entry);
    let output = config.serialize();
    // Separator should remain a space, not pick up the = from the value
    assert!(
        output.contains("  IdentityFile ~/.ssh/id=staging"),
        "got: {}",
        output
    );
    assert!(!output.contains("IdentityFile="), "got: {}", output);
}

#[test]
fn upsert_preserves_equals_separator() {
    let mut config = parse_str("Host myserver\n  IdentityFile=~/.ssh/id_rsa\n");
    let entry = HostEntry {
        alias: "myserver".to_string(),
        hostname: "10.0.0.1".to_string(),
        identity_file: "~/.ssh/id_ed25519".to_string(),
        port: 22,
        ..Default::default()
    };
    config.update_host("myserver", &entry);
    let output = config.serialize();
    assert!(
        output.contains("IdentityFile=~/.ssh/id_ed25519"),
        "got: {}",
        output
    );
}

#[test]
fn upsert_preserves_spaced_equals_separator() {
    let mut config = parse_str("Host myserver\n  IdentityFile = ~/.ssh/id_rsa\n");
    let entry = HostEntry {
        alias: "myserver".to_string(),
        hostname: "10.0.0.1".to_string(),
        identity_file: "~/.ssh/id_ed25519".to_string(),
        port: 22,
        ..Default::default()
    };
    config.update_host("myserver", &entry);
    let output = config.serialize();
    assert!(
        output.contains("IdentityFile = ~/.ssh/id_ed25519"),
        "got: {}",
        output
    );
}

#[test]
fn is_included_host_false_for_main_config() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    assert!(!config.is_included_host("myserver"));
}

#[test]
fn is_included_host_false_for_nonexistent() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    assert!(!config.is_included_host("nohost"));
}

#[test]
fn is_included_host_multi_pattern_main_config() {
    let config = parse_str("Host prod staging\n  HostName 10.0.0.1\n");
    assert!(!config.is_included_host("prod"));
    assert!(!config.is_included_host("staging"));
}

// =========================================================================
// HostBlock::askpass() and set_askpass() tests
// =========================================================================

fn first_block(config: &SshConfigFile) -> &HostBlock {
    match config.elements.first().unwrap() {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("Expected HostBlock"),
    }
}

fn first_block_mut(config: &mut SshConfigFile) -> &mut HostBlock {
    match config.elements.first_mut().unwrap() {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("Expected HostBlock"),
    }
}

fn block_by_index(config: &SshConfigFile, idx: usize) -> &HostBlock {
    let mut count = 0;
    for el in &config.elements {
        if let ConfigElement::HostBlock(b) = el {
            if count == idx {
                return b;
            }
            count += 1;
        }
    }
    panic!("No HostBlock at index {}", idx);
}

#[test]
fn askpass_returns_none_when_absent() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    assert_eq!(first_block(&config).askpass(), None);
}

#[test]
fn askpass_returns_keychain() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    assert_eq!(first_block(&config).askpass(), Some("keychain".to_string()));
}

#[test]
fn askpass_returns_op_uri() {
    let config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://Vault/Item/field\n");
    assert_eq!(
        first_block(&config).askpass(),
        Some("op://Vault/Item/field".to_string())
    );
}

#[test]
fn askpass_returns_vault_with_field() {
    let config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#password\n",
    );
    assert_eq!(
        first_block(&config).askpass(),
        Some("vault:secret/ssh#password".to_string())
    );
}

#[test]
fn askpass_returns_bw_source() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass bw:my-item\n");
    assert_eq!(
        first_block(&config).askpass(),
        Some("bw:my-item".to_string())
    );
}

#[test]
fn askpass_returns_pass_source() {
    let config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass pass:ssh/prod\n");
    assert_eq!(
        first_block(&config).askpass(),
        Some("pass:ssh/prod".to_string())
    );
}

#[test]
fn askpass_returns_custom_command() {
    let config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass get-pass %a %h\n");
    assert_eq!(
        first_block(&config).askpass(),
        Some("get-pass %a %h".to_string())
    );
}

#[test]
fn askpass_ignores_empty_value() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass \n");
    assert_eq!(first_block(&config).askpass(), None);
}

#[test]
fn askpass_ignores_non_askpass_purple_comments() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:tags prod\n");
    assert_eq!(first_block(&config).askpass(), None);
}

#[test]
fn set_askpass_adds_comment() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let _ = config.set_host_askpass("myserver", "keychain");
    assert_eq!(first_block(&config).askpass(), Some("keychain".to_string()));
}

#[test]
fn set_askpass_replaces_existing() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    let _ = config.set_host_askpass("myserver", "op://V/I/p");
    assert_eq!(
        first_block(&config).askpass(),
        Some("op://V/I/p".to_string())
    );
}

#[test]
fn set_askpass_empty_removes_comment() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    let _ = config.set_host_askpass("myserver", "");
    assert_eq!(first_block(&config).askpass(), None);
}

#[test]
fn set_askpass_preserves_other_directives() {
    let mut config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  User admin\n  # purple:tags prod\n");
    let _ = config.set_host_askpass("myserver", "vault:secret/ssh");
    assert_eq!(
        first_block(&config).askpass(),
        Some("vault:secret/ssh".to_string())
    );
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.user, "admin");
    assert!(entry.tags.contains(&"prod".to_string()));
}

#[test]
fn set_askpass_preserves_indent() {
    let mut config = parse_str("Host myserver\n    HostName 10.0.0.1\n");
    let _ = config.set_host_askpass("myserver", "keychain");
    let raw = first_block(&config)
        .directives
        .iter()
        .find(|d| d.raw_line.contains("purple:askpass"))
        .unwrap();
    assert!(
        raw.raw_line.starts_with("    "),
        "Expected 4-space indent, got: {:?}",
        raw.raw_line
    );
}

#[test]
fn set_askpass_on_nonexistent_host() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let _ = config.set_host_askpass("nohost", "keychain");
    assert_eq!(first_block(&config).askpass(), None);
}

#[test]
fn to_entry_includes_askpass() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass bw:item\n");
    let entries = config.host_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].askpass, Some("bw:item".to_string()));
}

#[test]
fn to_entry_askpass_none_when_absent() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let entries = config.host_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].askpass, None);
}

#[test]
fn set_askpass_vault_with_hash_field() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let _ = config.set_host_askpass("myserver", "vault:secret/data/team#api_key");
    assert_eq!(
        first_block(&config).askpass(),
        Some("vault:secret/data/team#api_key".to_string())
    );
}

#[test]
fn set_askpass_custom_command_with_percent() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let _ = config.set_host_askpass("myserver", "get-pass %a %h");
    assert_eq!(
        first_block(&config).askpass(),
        Some("get-pass %a %h".to_string())
    );
}

#[test]
fn multiple_hosts_independent_askpass() {
    let mut config = parse_str("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
    let _ = config.set_host_askpass("alpha", "keychain");
    let _ = config.set_host_askpass("beta", "vault:secret/ssh");
    assert_eq!(
        block_by_index(&config, 0).askpass(),
        Some("keychain".to_string())
    );
    assert_eq!(
        block_by_index(&config, 1).askpass(),
        Some("vault:secret/ssh".to_string())
    );
}

#[test]
fn set_askpass_then_clear_then_set_again() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let _ = config.set_host_askpass("myserver", "keychain");
    assert_eq!(first_block(&config).askpass(), Some("keychain".to_string()));
    let _ = config.set_host_askpass("myserver", "");
    assert_eq!(first_block(&config).askpass(), None);
    let _ = config.set_host_askpass("myserver", "op://V/I/p");
    assert_eq!(
        first_block(&config).askpass(),
        Some("op://V/I/p".to_string())
    );
}

#[test]
fn askpass_tab_indent_preserved() {
    let mut config = parse_str("Host myserver\n\tHostName 10.0.0.1\n");
    let _ = config.set_host_askpass("myserver", "pass:ssh/prod");
    let raw = first_block(&config)
        .directives
        .iter()
        .find(|d| d.raw_line.contains("purple:askpass"))
        .unwrap();
    assert!(
        raw.raw_line.starts_with("\t"),
        "Expected tab indent, got: {:?}",
        raw.raw_line
    );
}

#[test]
fn askpass_coexists_with_provider_comment() {
    let config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:provider do:123\n  # purple:askpass keychain\n",
    );
    let block = first_block(&config);
    assert_eq!(block.askpass(), Some("keychain".to_string()));
    assert!(block.provider().is_some());
}

#[test]
fn set_askpass_does_not_remove_tags() {
    let mut config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:tags prod,staging\n");
    let _ = config.set_host_askpass("myserver", "keychain");
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.askpass, Some("keychain".to_string()));
    assert!(entry.tags.contains(&"prod".to_string()));
    assert!(entry.tags.contains(&"staging".to_string()));
}

#[test]
fn askpass_idempotent_set_same_value() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    let _ = config.set_host_askpass("myserver", "keychain");
    assert_eq!(first_block(&config).askpass(), Some("keychain".to_string()));
    let serialized = config.serialize();
    assert_eq!(
        serialized.matches("purple:askpass").count(),
        1,
        "Should have exactly one askpass comment"
    );
}

#[test]
fn askpass_with_value_containing_equals() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let _ = config.set_host_askpass("myserver", "cmd --opt=val %h");
    assert_eq!(
        first_block(&config).askpass(),
        Some("cmd --opt=val %h".to_string())
    );
}

#[test]
fn askpass_with_value_containing_hash() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:a/b#c\n");
    assert_eq!(
        first_block(&config).askpass(),
        Some("vault:a/b#c".to_string())
    );
}

#[test]
fn askpass_with_long_op_uri() {
    let uri = "op://My Personal Vault/SSH Production Server/password";
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let _ = config.set_host_askpass("myserver", uri);
    assert_eq!(first_block(&config).askpass(), Some(uri.to_string()));
}

#[test]
fn askpass_does_not_interfere_with_host_matching() {
    // askpass is stored as a non-directive comment; it shouldn't affect SSH matching
    let config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  User root\n  # purple:askpass keychain\n");
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.user, "root");
    assert_eq!(entry.hostname, "10.0.0.1");
    assert_eq!(entry.askpass, Some("keychain".to_string()));
}

#[test]
fn set_askpass_on_host_with_many_directives() {
    let config_str = "\
Host myserver
  HostName 10.0.0.1
  User admin
  Port 2222
  IdentityFile ~/.ssh/id_ed25519
  ProxyJump bastion
  # purple:tags prod,us-east
";
    let mut config = parse_str(config_str);
    let _ = config.set_host_askpass("myserver", "pass:ssh/prod");
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.askpass, Some("pass:ssh/prod".to_string()));
    assert_eq!(entry.user, "admin");
    assert_eq!(entry.port, 2222);
    assert!(entry.tags.contains(&"prod".to_string()));
}

#[test]
fn askpass_with_crlf_line_endings() {
    let config =
        parse_str("Host myserver\r\n  HostName 10.0.0.1\r\n  # purple:askpass keychain\r\n");
    assert_eq!(first_block(&config).askpass(), Some("keychain".to_string()));
}

#[test]
fn askpass_only_on_first_matching_host() {
    // If two Host blocks have the same alias (unusual), askpass comes from first
    let config = parse_str(
        "Host dup\n  HostName a.com\n  # purple:askpass keychain\n\nHost dup\n  HostName b.com\n  # purple:askpass vault:x\n",
    );
    let entries = config.host_entries();
    // First match
    assert_eq!(entries[0].askpass, Some("keychain".to_string()));
}

#[test]
fn set_askpass_preserves_other_non_directive_comments() {
    let config_str = "Host myserver\n  HostName 10.0.0.1\n  # This is a user comment\n  # purple:askpass old\n  # Another comment\n";
    let mut config = parse_str(config_str);
    let _ = config.set_host_askpass("myserver", "new-source");
    let serialized = config.serialize();
    assert!(serialized.contains("# This is a user comment"));
    assert!(serialized.contains("# Another comment"));
    assert!(serialized.contains("# purple:askpass new-source"));
    assert!(!serialized.contains("# purple:askpass old"));
}

#[test]
fn askpass_mixed_with_tunnel_directives() {
    let config_str = "\
Host myserver
  HostName 10.0.0.1
  LocalForward 8080 localhost:80
  # purple:askpass bw:item
  RemoteForward 9090 localhost:9090
";
    let config = parse_str(config_str);
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.askpass, Some("bw:item".to_string()));
    assert_eq!(entry.tunnel_count, 2);
}

// =========================================================================
// askpass: set_askpass idempotent (same value)
// =========================================================================

#[test]
fn set_askpass_idempotent_same_value() {
    let config_str = "Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n";
    let mut config = parse_str(config_str);
    let _ = config.set_host_askpass("myserver", "keychain");
    let output = config.serialize();
    // Should still have exactly one askpass comment
    assert_eq!(output.matches("purple:askpass").count(), 1);
    assert!(output.contains("# purple:askpass keychain"));
}

#[test]
fn set_askpass_with_equals_in_value() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let _ = config.set_host_askpass("myserver", "cmd --opt=val");
    let entries = config.host_entries();
    assert_eq!(entries[0].askpass, Some("cmd --opt=val".to_string()));
}

#[test]
fn set_askpass_with_hash_in_value() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let _ = config.set_host_askpass("myserver", "vault:secret/data#field");
    let entries = config.host_entries();
    assert_eq!(
        entries[0].askpass,
        Some("vault:secret/data#field".to_string())
    );
}

#[test]
fn set_askpass_long_op_uri() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let long_uri = "op://My Personal Vault/SSH Production Server Key/password";
    let _ = config.set_host_askpass("myserver", long_uri);
    assert_eq!(config.host_entries()[0].askpass, Some(long_uri.to_string()));
}

#[test]
fn askpass_host_with_multi_pattern_is_skipped() {
    // Multi-pattern host blocks ("Host prod staging") are treated as patterns
    // and are not included in host_entries(), so set_askpass is a no-op
    let config_str = "Host prod staging\n  HostName 10.0.0.1\n";
    let mut config = parse_str(config_str);
    let _ = config.set_host_askpass("prod", "keychain");
    // No entries because multi-pattern hosts are pattern hosts
    assert!(config.host_entries().is_empty());
}

#[test]
fn askpass_survives_directive_reorder() {
    // askpass should survive even when directives are in unusual order
    let config_str = "\
Host myserver
  # purple:askpass op://V/I/p
  HostName 10.0.0.1
  User root
";
    let config = parse_str(config_str);
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.askpass, Some("op://V/I/p".to_string()));
    assert_eq!(entry.hostname, "10.0.0.1");
}

#[test]
fn askpass_among_many_purple_comments() {
    let config_str = "\
Host myserver
  HostName 10.0.0.1
  # purple:tags prod,us-east
  # purple:provider do:12345
  # purple:askpass pass:ssh/prod
";
    let config = parse_str(config_str);
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.askpass, Some("pass:ssh/prod".to_string()));
    assert!(entry.tags.contains(&"prod".to_string()));
}

#[test]
fn meta_empty_when_no_comment() {
    let config_str = "Host myhost\n  HostName 1.2.3.4\n";
    let config = parse_str(config_str);
    let meta = first_block(&config).meta();
    assert!(meta.is_empty());
}

#[test]
fn meta_parses_key_value_pairs() {
    let config_str = "\
Host myhost
  HostName 1.2.3.4
  # purple:meta region=nyc3,plan=s-1vcpu-1gb
";
    let config = parse_str(config_str);
    let meta = first_block(&config).meta();
    assert_eq!(meta.len(), 2);
    assert_eq!(meta[0], ("region".to_string(), "nyc3".to_string()));
    assert_eq!(meta[1], ("plan".to_string(), "s-1vcpu-1gb".to_string()));
}

#[test]
fn meta_round_trip() {
    let config_str = "Host myhost\n  HostName 1.2.3.4\n";
    let mut config = parse_str(config_str);
    let meta = vec![
        ("region".to_string(), "fra1".to_string()),
        ("plan".to_string(), "cx11".to_string()),
    ];
    let _ = config.set_host_meta("myhost", &meta);
    let output = config.serialize();
    assert!(output.contains("# purple:meta region=fra1,plan=cx11"));

    let config2 = parse_str(&output);
    let parsed = first_block(&config2).meta();
    assert_eq!(parsed, meta);
}

#[test]
fn meta_replaces_existing() {
    let config_str = "\
Host myhost
  HostName 1.2.3.4
  # purple:meta region=old
";
    let mut config = parse_str(config_str);
    let _ = config.set_host_meta("myhost", &[("region".to_string(), "new".to_string())]);
    let output = config.serialize();
    assert!(!output.contains("region=old"));
    assert!(output.contains("region=new"));
}

#[test]
fn meta_removed_when_empty() {
    let config_str = "\
Host myhost
  HostName 1.2.3.4
  # purple:meta region=nyc3
";
    let mut config = parse_str(config_str);
    let _ = config.set_host_meta("myhost", &[]);
    let output = config.serialize();
    assert!(!output.contains("purple:meta"));
}

#[test]
fn meta_sanitizes_commas_in_values() {
    let config_str = "Host myhost\n  HostName 1.2.3.4\n";
    let mut config = parse_str(config_str);
    let meta = vec![("plan".to_string(), "s-1vcpu,1gb".to_string())];
    let _ = config.set_host_meta("myhost", &meta);
    let output = config.serialize();
    // Comma stripped to prevent parse corruption
    assert!(output.contains("plan=s-1vcpu1gb"));

    let config2 = parse_str(&output);
    let parsed = first_block(&config2).meta();
    assert_eq!(parsed[0].1, "s-1vcpu1gb");
}

#[test]
fn meta_in_host_entry() {
    let config_str = "\
Host myhost
  HostName 1.2.3.4
  # purple:meta region=nyc3,plan=s-1vcpu-1gb
";
    let config = parse_str(config_str);
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.provider_meta.len(), 2);
    assert_eq!(entry.provider_meta[0].0, "region");
    assert_eq!(entry.provider_meta[1].0, "plan");
}

#[test]
fn repair_absorbed_group_comment() {
    // Simulate the bug: group comment absorbed into preceding block's directives.
    let mut config = SshConfigFile {
        elements: vec![ConfigElement::HostBlock(HostBlock {
            host_pattern: "myserver".to_string(),
            raw_host_line: "Host myserver".to_string(),
            directives: vec![
                Directive {
                    key: "HostName".to_string(),
                    value: "10.0.0.1".to_string(),
                    raw_line: "  HostName 10.0.0.1".to_string(),
                    is_non_directive: false,
                },
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: "# purple:group DigitalOcean".to_string(),
                    is_non_directive: true,
                },
            ],
        })],
        path: test_config_path(),
        crlf: false,
        bom: false,
    };
    let count = config.repair_absorbed_group_comments();
    assert_eq!(count, 1);
    assert_eq!(config.elements.len(), 2);
    // Block should only have the HostName directive.
    if let ConfigElement::HostBlock(block) = &config.elements[0] {
        assert_eq!(block.directives.len(), 1);
        assert_eq!(block.directives[0].key, "HostName");
    } else {
        panic!("Expected HostBlock");
    }
    // Group comment should be a GlobalLine.
    if let ConfigElement::GlobalLine(line) = &config.elements[1] {
        assert_eq!(line, "# purple:group DigitalOcean");
    } else {
        panic!("Expected GlobalLine for group comment");
    }
}

#[test]
fn repair_strips_trailing_blanks_before_group() {
    let mut config = SshConfigFile {
        elements: vec![ConfigElement::HostBlock(HostBlock {
            host_pattern: "myserver".to_string(),
            raw_host_line: "Host myserver".to_string(),
            directives: vec![
                Directive {
                    key: "HostName".to_string(),
                    value: "10.0.0.1".to_string(),
                    raw_line: "  HostName 10.0.0.1".to_string(),
                    is_non_directive: false,
                },
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: "".to_string(),
                    is_non_directive: true,
                },
                Directive {
                    key: String::new(),
                    value: String::new(),
                    raw_line: "# purple:group Hetzner".to_string(),
                    is_non_directive: true,
                },
            ],
        })],
        path: test_config_path(),
        crlf: false,
        bom: false,
    };
    let count = config.repair_absorbed_group_comments();
    assert_eq!(count, 1);
    // Block keeps only HostName.
    if let ConfigElement::HostBlock(block) = &config.elements[0] {
        assert_eq!(block.directives.len(), 1);
    } else {
        panic!("Expected HostBlock");
    }
    // Blank line and group comment are now GlobalLines.
    assert_eq!(config.elements.len(), 3);
    if let ConfigElement::GlobalLine(line) = &config.elements[1] {
        assert!(line.trim().is_empty());
    } else {
        panic!("Expected blank GlobalLine");
    }
    if let ConfigElement::GlobalLine(line) = &config.elements[2] {
        assert!(line.starts_with("# purple:group"));
    } else {
        panic!("Expected group GlobalLine");
    }
}

#[test]
fn repair_clean_config_returns_zero() {
    let mut config = parse_str("# purple:group DigitalOcean\nHost myserver\n  HostName 10.0.0.1\n");
    let count = config.repair_absorbed_group_comments();
    assert_eq!(count, 0);
}

#[test]
fn repair_roundtrip_serializes_correctly() {
    // Build a corrupted config manually.
    let mut config = SshConfigFile {
        elements: vec![
            ConfigElement::HostBlock(HostBlock {
                host_pattern: "server1".to_string(),
                raw_host_line: "Host server1".to_string(),
                directives: vec![
                    Directive {
                        key: "HostName".to_string(),
                        value: "10.0.0.1".to_string(),
                        raw_line: "  HostName 10.0.0.1".to_string(),
                        is_non_directive: false,
                    },
                    Directive {
                        key: String::new(),
                        value: String::new(),
                        raw_line: "".to_string(),
                        is_non_directive: true,
                    },
                    Directive {
                        key: String::new(),
                        value: String::new(),
                        raw_line: "# purple:group Hetzner".to_string(),
                        is_non_directive: true,
                    },
                ],
            }),
            ConfigElement::HostBlock(HostBlock {
                host_pattern: "server2".to_string(),
                raw_host_line: "Host server2".to_string(),
                directives: vec![Directive {
                    key: "HostName".to_string(),
                    value: "10.0.0.2".to_string(),
                    raw_line: "  HostName 10.0.0.2".to_string(),
                    is_non_directive: false,
                }],
            }),
        ],
        path: test_config_path(),
        crlf: false,
        bom: false,
    };
    let count = config.repair_absorbed_group_comments();
    assert_eq!(count, 1);
    let output = config.serialize();
    // The group comment should appear between the two host blocks.
    let expected = "\
Host server1
  HostName 10.0.0.1

# purple:group Hetzner
Host server2
  HostName 10.0.0.2
";
    assert_eq!(output, expected);
}

// =========================================================================
// delete_host: orphaned group header cleanup
// =========================================================================

#[test]
fn delete_last_provider_host_removes_group_header() {
    let config_str = "\
# purple:group DigitalOcean
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:123
";
    let mut config = parse_str(config_str);
    config.delete_host("do-web");
    let has_header = config
        .elements
        .iter()
        .any(|e| matches!(e, ConfigElement::GlobalLine(l) if l.contains("purple:group")));
    assert!(
        !has_header,
        "Group header should be removed when last provider host is deleted"
    );
}

#[test]
fn delete_one_of_multiple_provider_hosts_preserves_group_header() {
    let config_str = "\
# purple:group DigitalOcean
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:123

Host do-db
  HostName 5.6.7.8
  # purple:provider digitalocean:456
";
    let mut config = parse_str(config_str);
    config.delete_host("do-web");
    let has_header = config.elements.iter().any(
        |e| matches!(e, ConfigElement::GlobalLine(l) if l.contains("purple:group DigitalOcean")),
    );
    assert!(
        has_header,
        "Group header should be preserved when other provider hosts remain"
    );
    assert_eq!(config.host_entries().len(), 1);
}

#[test]
fn delete_non_provider_host_leaves_group_headers() {
    let config_str = "\
Host personal
  HostName 10.0.0.1

# purple:group DigitalOcean
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:123
";
    let mut config = parse_str(config_str);
    config.delete_host("personal");
    let has_header = config.elements.iter().any(
        |e| matches!(e, ConfigElement::GlobalLine(l) if l.contains("purple:group DigitalOcean")),
    );
    assert!(
        has_header,
        "Group header should not be affected by deleting a non-provider host"
    );
    assert_eq!(config.host_entries().len(), 1);
}

#[test]
fn delete_host_undoable_keeps_group_header_for_undo() {
    // delete_host_undoable does NOT remove orphaned group headers so that
    // undo (insert_host_at) can restore the config to its original state.
    // Orphaned headers are cleaned up at startup instead.
    let config_str = "\
# purple:group Vultr
Host vultr-web
  HostName 2.3.4.5
  # purple:provider vultr:789
";
    let mut config = parse_str(config_str);
    let result = config.delete_host_undoable("vultr-web");
    assert!(result.is_some());
    let has_header = config
        .elements
        .iter()
        .any(|e| matches!(e, ConfigElement::GlobalLine(l) if l.contains("purple:group")));
    assert!(has_header, "Group header should be kept for undo");
}

#[test]
fn delete_host_undoable_preserves_header_when_others_remain() {
    let config_str = "\
# purple:group AWS EC2
Host aws-web
  HostName 3.4.5.6
  # purple:provider aws:i-111

Host aws-db
  HostName 7.8.9.0
  # purple:provider aws:i-222
";
    let mut config = parse_str(config_str);
    let result = config.delete_host_undoable("aws-web");
    assert!(result.is_some());
    let has_header = config
        .elements
        .iter()
        .any(|e| matches!(e, ConfigElement::GlobalLine(l) if l.contains("purple:group AWS EC2")));
    assert!(
        has_header,
        "Group header preserved when other provider hosts remain (undoable)"
    );
}

#[test]
fn delete_host_undoable_returns_original_position_for_undo() {
    // Group header at index 0, host at index 1. Undo re-inserts at index 1
    // which correctly restores the host after the group header.
    let config_str = "\
# purple:group Vultr
Host vultr-web
  HostName 2.3.4.5
  # purple:provider vultr:789

Host manual
  HostName 10.0.0.1
";
    let mut config = parse_str(config_str);
    let (element, pos) = config.delete_host_undoable("vultr-web").unwrap();
    // Position is the original index (1), not adjusted, since no header was removed
    assert_eq!(pos, 1, "Position should be the original host index");
    // Undo: re-insert at the original position
    config.insert_host_at(element, pos);
    // The host should be back, group header intact, manual host accessible
    let output = config.serialize();
    assert!(
        output.contains("# purple:group Vultr"),
        "Group header should be present"
    );
    assert!(output.contains("Host vultr-web"), "Host should be restored");
    assert!(output.contains("Host manual"), "Manual host should survive");
    assert_eq!(config_str, output);
}

// =========================================================================
// add_host: wildcard ordering
// =========================================================================

#[test]
fn add_host_inserts_before_trailing_wildcard() {
    let config_str = "\
Host existing
  HostName 10.0.0.1

Host *
  ServerAliveInterval 60
";
    let mut config = parse_str(config_str);
    let entry = HostEntry {
        alias: "newhost".to_string(),
        hostname: "10.0.0.2".to_string(),
        port: 22,
        ..Default::default()
    };
    config.add_host(&entry);
    let output = config.serialize();
    let new_pos = output.find("Host newhost").unwrap();
    let wildcard_pos = output.find("Host *").unwrap();
    assert!(
        new_pos < wildcard_pos,
        "New host should appear before Host *: {}",
        output
    );
    let existing_pos = output.find("Host existing").unwrap();
    assert!(existing_pos < new_pos);
}

#[test]
fn add_host_appends_when_no_wildcards() {
    let config_str = "\
Host existing
  HostName 10.0.0.1
";
    let mut config = parse_str(config_str);
    let entry = HostEntry {
        alias: "newhost".to_string(),
        hostname: "10.0.0.2".to_string(),
        port: 22,
        ..Default::default()
    };
    config.add_host(&entry);
    let output = config.serialize();
    let existing_pos = output.find("Host existing").unwrap();
    let new_pos = output.find("Host newhost").unwrap();
    assert!(existing_pos < new_pos, "New host should be appended at end");
}

#[test]
fn add_host_appends_when_wildcard_at_beginning() {
    // Host * at the top acts as global defaults. New hosts go after it.
    let config_str = "\
Host *
  ServerAliveInterval 60

Host existing
  HostName 10.0.0.1
";
    let mut config = parse_str(config_str);
    let entry = HostEntry {
        alias: "newhost".to_string(),
        hostname: "10.0.0.2".to_string(),
        port: 22,
        ..Default::default()
    };
    config.add_host(&entry);
    let output = config.serialize();
    let existing_pos = output.find("Host existing").unwrap();
    let new_pos = output.find("Host newhost").unwrap();
    assert!(
        existing_pos < new_pos,
        "New host should be appended at end when wildcard is at top: {}",
        output
    );
}

#[test]
fn add_host_inserts_before_trailing_pattern_host() {
    let config_str = "\
Host existing
  HostName 10.0.0.1

Host *.example.com
  ProxyJump bastion
";
    let mut config = parse_str(config_str);
    let entry = HostEntry {
        alias: "newhost".to_string(),
        hostname: "10.0.0.2".to_string(),
        port: 22,
        ..Default::default()
    };
    config.add_host(&entry);
    let output = config.serialize();
    let new_pos = output.find("Host newhost").unwrap();
    let pattern_pos = output.find("Host *.example.com").unwrap();
    assert!(
        new_pos < pattern_pos,
        "New host should appear before pattern host: {}",
        output
    );
}

#[test]
fn add_host_no_triple_blank_lines() {
    let config_str = "\
Host existing
  HostName 10.0.0.1

Host *
  ServerAliveInterval 60
";
    let mut config = parse_str(config_str);
    let entry = HostEntry {
        alias: "newhost".to_string(),
        hostname: "10.0.0.2".to_string(),
        port: 22,
        ..Default::default()
    };
    config.add_host(&entry);
    let output = config.serialize();
    assert!(
        !output.contains("\n\n\n"),
        "Should not have triple blank lines: {}",
        output
    );
}

#[test]
fn provider_group_display_name_matches_providers_mod() {
    // Ensure the duplicated display name function in model.rs stays in sync
    // with providers::provider_display_name(). If these diverge, group header
    // cleanup (remove_orphaned_group_header) will fail to match headers
    // written by the sync engine.
    let providers = [
        "digitalocean",
        "vultr",
        "linode",
        "hetzner",
        "upcloud",
        "proxmox",
        "aws",
        "scaleway",
        "gcp",
        "azure",
        "tailscale",
        "oracle",
    ];
    for name in &providers {
        assert_eq!(
            provider_group_display_name(name),
            crate::providers::provider_display_name(name),
            "Display name mismatch for provider '{}': model.rs has '{}' but providers/mod.rs has '{}'",
            name,
            provider_group_display_name(name),
            crate::providers::provider_display_name(name),
        );
    }
}

#[test]
fn test_sanitize_tag_strips_control_chars() {
    assert_eq!(HostBlock::sanitize_tag("prod"), "prod");
    assert_eq!(HostBlock::sanitize_tag("prod\n"), "prod");
    assert_eq!(HostBlock::sanitize_tag("pr\x00od"), "prod");
    assert_eq!(HostBlock::sanitize_tag("\t\r\n"), "");
}

#[test]
fn test_sanitize_tag_strips_commas() {
    assert_eq!(HostBlock::sanitize_tag("prod,staging"), "prodstaging");
    assert_eq!(HostBlock::sanitize_tag(",,,"), "");
}

#[test]
fn test_sanitize_tag_strips_bidi() {
    assert_eq!(HostBlock::sanitize_tag("prod\u{202E}tset"), "prodtset");
    assert_eq!(HostBlock::sanitize_tag("\u{200B}zero\u{FEFF}"), "zero");
}

#[test]
fn test_sanitize_tag_truncates_long() {
    let long = "a".repeat(200);
    assert_eq!(HostBlock::sanitize_tag(&long).len(), 128);
}

#[test]
fn test_sanitize_tag_preserves_unicode() {
    assert_eq!(HostBlock::sanitize_tag("日本語"), "日本語");
    assert_eq!(HostBlock::sanitize_tag("café"), "café");
}

// =========================================================================
// provider_tags parsing and has_provider_tags_comment tests
// =========================================================================

#[test]
fn test_provider_tags_parsing() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:provider_tags a,b,c\n");
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.provider_tags, vec!["a", "b", "c"]);
}

#[test]
fn test_provider_tags_empty() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let entry = first_block(&config).to_host_entry();
    assert!(entry.provider_tags.is_empty());
}

#[test]
fn test_has_provider_tags_comment_present() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:provider_tags prod\n");
    assert!(first_block(&config).has_provider_tags_comment());
    assert!(first_block(&config).to_host_entry().has_provider_tags);
}

#[test]
fn test_has_provider_tags_comment_sentinel() {
    // Bare sentinel (no tags) still counts as "has provider_tags"
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:provider_tags\n");
    assert!(first_block(&config).has_provider_tags_comment());
    assert!(first_block(&config).to_host_entry().has_provider_tags);
    assert!(
        first_block(&config)
            .to_host_entry()
            .provider_tags
            .is_empty()
    );
}

#[test]
fn test_has_provider_tags_comment_absent() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    assert!(!first_block(&config).has_provider_tags_comment());
    assert!(!first_block(&config).to_host_entry().has_provider_tags);
}

#[test]
fn test_set_tags_does_not_delete_provider_tags() {
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:tags user1\n  # purple:provider_tags cloud1,cloud2\n",
    );
    let _ = config.set_host_tags("myserver", &["newuser".to_string()]);
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.tags, vec!["newuser"]);
    assert_eq!(entry.provider_tags, vec!["cloud1", "cloud2"]);
}

#[test]
fn test_set_provider_tags_does_not_delete_user_tags() {
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:tags user1,user2\n  # purple:provider_tags old\n",
    );
    let _ = config.set_host_provider_tags("myserver", &["new1".to_string(), "new2".to_string()]);
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.tags, vec!["user1", "user2"]);
    assert_eq!(entry.provider_tags, vec!["new1", "new2"]);
}

// =========================================================================
// provider_user / provider_key marker parsing and round-trip tests
// =========================================================================

#[test]
fn test_provider_user_parsing() {
    let config = parse_str("Host s\n  HostName 10.0.0.1\n  # purple:provider_user admin\n");
    assert_eq!(
        first_block(&config)
            .to_host_entry()
            .provider_user
            .as_deref(),
        Some("admin")
    );
}

#[test]
fn test_provider_key_parsing_preserves_spaces() {
    let config =
        parse_str("Host s\n  HostName 10.0.0.1\n  # purple:provider_key ~/my keys/id_ed25519\n");
    assert_eq!(
        first_block(&config).to_host_entry().provider_key.as_deref(),
        Some("~/my keys/id_ed25519")
    );
}

#[test]
fn test_provider_user_key_absent() {
    let config = parse_str("Host s\n  HostName 10.0.0.1\n");
    let entry = first_block(&config).to_host_entry();
    assert!(entry.provider_user.is_none());
    assert!(entry.provider_key.is_none());
}

#[test]
fn test_empty_provider_user_marker_parses_as_none() {
    let config = parse_str("Host s\n  HostName 10.0.0.1\n  # purple:provider_user\n");
    assert!(first_block(&config).to_host_entry().provider_user.is_none());
}

#[test]
fn test_set_host_provider_user_round_trip() {
    let mut config = parse_str("Host s\n  HostName 10.0.0.1\n");
    assert!(config.set_host_provider_user("s", "admin"));
    let serialized = config.serialize();
    let reparsed = parse_str(&serialized);
    assert_eq!(
        reparsed.host_entries()[0].provider_user.as_deref(),
        Some("admin")
    );
}

#[test]
fn test_set_host_provider_user_empty_removes_marker() {
    let mut config = parse_str("Host s\n  HostName 10.0.0.1\n  # purple:provider_user admin\n");
    assert!(config.set_host_provider_user("s", ""));
    assert!(first_block(&config).to_host_entry().provider_user.is_none());
}

#[test]
fn test_provider_user_marker_does_not_collide_with_provider_marker() {
    // The `_user` suffix (no space) must not be swallowed by the
    // `# purple:provider ` marker parser, and vice versa.
    let config = parse_str(
        "Host s\n  HostName 10.0.0.1\n  # purple:provider digitalocean:123\n  # purple:provider_user admin\n",
    );
    let block = first_block(&config);
    assert_eq!(
        block.provider(),
        Some(("digitalocean".to_string(), "123".to_string()))
    );
    assert_eq!(
        block.to_host_entry().provider_user.as_deref(),
        Some("admin")
    );
}

#[test]
fn test_set_provider_user_preserves_provider_marker() {
    let mut config =
        parse_str("Host s\n  HostName 10.0.0.1\n  # purple:provider digitalocean:123\n");
    assert!(config.set_host_provider_user("s", "admin"));
    let block = first_block(&config);
    assert_eq!(
        block.provider(),
        Some(("digitalocean".to_string(), "123".to_string()))
    );
    assert_eq!(
        block.to_host_entry().provider_user.as_deref(),
        Some("admin")
    );
}

#[test]
fn test_set_host_provider_key_round_trip() {
    let mut config = parse_str("Host s\n  HostName 10.0.0.1\n");
    assert!(config.set_host_provider_key("s", "~/.ssh/id_ed25519"));
    let reparsed = parse_str(&config.serialize());
    assert_eq!(
        reparsed.host_entries()[0].provider_key.as_deref(),
        Some("~/.ssh/id_ed25519")
    );
}

#[test]
fn test_set_host_provider_key_empty_removes_marker() {
    let mut config = parse_str("Host s\n  HostName 10.0.0.1\n  # purple:provider_key ~/.ssh/id\n");
    assert!(config.set_host_provider_key("s", ""));
    assert!(first_block(&config).to_host_entry().provider_key.is_none());
}

#[test]
fn test_set_provider_key_preserves_provider_marker() {
    let mut config =
        parse_str("Host s\n  HostName 10.0.0.1\n  # purple:provider digitalocean:123\n");
    assert!(config.set_host_provider_key("s", "~/.ssh/id"));
    let block = first_block(&config);
    assert_eq!(
        block.provider(),
        Some(("digitalocean".to_string(), "123".to_string()))
    );
    assert_eq!(
        block.to_host_entry().provider_key.as_deref(),
        Some("~/.ssh/id")
    );
}

#[test]
fn test_set_host_provider_user_refuses_pattern_alias() {
    // Wildcard aliases must be refused so a `Host *.prod` pattern can never
    // have a provider marker silently assigned to every host it resolves.
    let mut config = parse_str("Host *.prod\n  User root\n");
    assert!(!config.set_host_provider_user("*.prod", "admin"));
    assert!(!config.set_host_provider_key("*.prod", "~/.ssh/id"));
}

#[test]
fn test_set_askpass_does_not_delete_similar_comments() {
    // A hypothetical "# purple:askpass_backup test" should NOT be deleted by set_askpass
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n  # purple:askpass_backup test\n",
    );
    let _ = config.set_host_askpass("myserver", "op://vault/item/pass");
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.askpass, Some("op://vault/item/pass".to_string()));
    // The similar-but-different comment survives
    let serialized = config.serialize();
    assert!(serialized.contains("purple:askpass_backup test"));
}

#[test]
fn test_set_meta_does_not_delete_similar_comments() {
    // A hypothetical "# purple:metadata foo" should NOT be deleted by set_meta
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:meta region=us-east\n  # purple:metadata foo\n",
    );
    let _ = config.set_host_meta("myserver", &[("region".to_string(), "eu-west".to_string())]);
    let serialized = config.serialize();
    assert!(serialized.contains("purple:meta region=eu-west"));
    assert!(serialized.contains("purple:metadata foo"));
}

#[test]
fn test_set_meta_sanitizes_control_chars() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let _ = config.set_host_meta(
        "myserver",
        &[
            ("region".to_string(), "us\x00east".to_string()),
            ("zone".to_string(), "a\u{202E}b".to_string()),
        ],
    );
    let serialized = config.serialize();
    // Control chars and bidi should be stripped from values
    assert!(serialized.contains("region=useast"));
    assert!(serialized.contains("zone=ab"));
    assert!(!serialized.contains('\x00'));
    assert!(!serialized.contains('\u{202E}'));
}

// ── stale tests ──────────────────────────────────────────────────────

#[test]
fn stale_returns_timestamp() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:stale 1711900000
";
    let config = parse_str(config_str);
    assert_eq!(first_block(&config).stale(), Some(1711900000));
}

#[test]
fn stale_returns_none_when_absent() {
    let config_str = "Host web\n  HostName 1.2.3.4\n";
    let config = parse_str(config_str);
    assert_eq!(first_block(&config).stale(), None);
}

#[test]
fn stale_returns_none_for_malformed() {
    for bad in &[
        "Host w\n  HostName 1.2.3.4\n  # purple:stale abc\n",
        "Host w\n  HostName 1.2.3.4\n  # purple:stale\n",
        "Host w\n  HostName 1.2.3.4\n  # purple:stale -1\n",
    ] {
        let config = parse_str(bad);
        assert_eq!(first_block(&config).stale(), None, "input: {bad}");
    }
}

#[test]
fn set_stale_adds_comment() {
    let config_str = "Host web\n  HostName 1.2.3.4\n";
    let mut config = parse_str(config_str);
    first_block_mut(&mut config).set_stale(1711900000);
    assert_eq!(first_block(&config).stale(), Some(1711900000));
    assert!(config.serialize().contains("# purple:stale 1711900000"));
}

#[test]
fn set_stale_replaces_existing() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:stale 1000
";
    let mut config = parse_str(config_str);
    first_block_mut(&mut config).set_stale(2000);
    assert_eq!(first_block(&config).stale(), Some(2000));
    let output = config.serialize();
    assert!(!output.contains("1000"));
    assert!(output.contains("# purple:stale 2000"));
}

#[test]
fn clear_stale_removes_comment() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:stale 1711900000
";
    let mut config = parse_str(config_str);
    first_block_mut(&mut config).clear_stale();
    assert_eq!(first_block(&config).stale(), None);
    assert!(!config.serialize().contains("purple:stale"));
}

#[test]
fn clear_stale_when_absent_is_noop() {
    let config_str = "Host web\n  HostName 1.2.3.4\n";
    let mut config = parse_str(config_str);
    let before = config.serialize();
    first_block_mut(&mut config).clear_stale();
    assert_eq!(config.serialize(), before);
}

#[test]
fn stale_roundtrip() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:stale 1711900000
";
    let config = parse_str(config_str);
    let output = config.serialize();
    let config2 = parse_str(&output);
    assert_eq!(first_block(&config2).stale(), Some(1711900000));
}

#[test]
fn stale_in_host_entry() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:stale 1711900000
";
    let config = parse_str(config_str);
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.stale, Some(1711900000));
}

#[test]
fn stale_coexists_with_other_annotations() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:tags prod
  # purple:provider do:12345
  # purple:askpass keychain
  # purple:meta region=nyc3
  # purple:stale 1711900000
";
    let config = parse_str(config_str);
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.stale, Some(1711900000));
    assert!(entry.tags.contains(&"prod".to_string()));
    assert_eq!(entry.provider, Some("do".to_string()));
    assert_eq!(entry.askpass, Some("keychain".to_string()));
    assert_eq!(entry.provider_meta[0].0, "region");
}

#[test]
fn set_host_stale_delegates() {
    let config_str = "\
Host web
  HostName 1.2.3.4

Host db
  HostName 5.6.7.8
";
    let mut config = parse_str(config_str);
    let _ = config.set_host_stale("db", 1234567890);
    assert_eq!(config.host_entries()[1].stale, Some(1234567890));
    assert_eq!(config.host_entries()[0].stale, None);
}

#[test]
fn clear_host_stale_delegates() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:stale 1711900000
";
    let mut config = parse_str(config_str);
    let _ = config.clear_host_stale("web");
    assert_eq!(first_block(&config).stale(), None);
}

#[test]
fn stale_hosts_collects_all() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:stale 1000

Host db
  HostName 5.6.7.8

Host app
  HostName 9.10.11.12
  # purple:stale 2000
";
    let config = parse_str(config_str);
    let stale = config.stale_hosts();
    assert_eq!(stale.len(), 2);
    assert_eq!(stale[0], ("web".to_string(), 1000));
    assert_eq!(stale[1], ("app".to_string(), 2000));
}

#[test]
fn set_stale_preserves_indent() {
    let config_str = "Host web\n\tHostName 1.2.3.4\n";
    let mut config = parse_str(config_str);
    first_block_mut(&mut config).set_stale(1711900000);
    assert!(config.serialize().contains("\t# purple:stale 1711900000"));
}

#[test]
fn stale_does_not_match_similar_comments() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:stale_backup 999
";
    let config = parse_str(config_str);
    assert_eq!(first_block(&config).stale(), None);
}

#[test]
fn stale_with_whitespace_in_timestamp() {
    let config_str = "Host w\n  HostName 1.2.3.4\n  # purple:stale  1711900000 \n";
    let config = parse_str(config_str);
    assert_eq!(first_block(&config).stale(), Some(1711900000));
}

#[test]
fn stale_with_u64_max() {
    let ts = u64::MAX;
    let config_str = format!("Host w\n  HostName 1.2.3.4\n  # purple:stale {}\n", ts);
    let config = parse_str(&config_str);
    assert_eq!(first_block(&config).stale(), Some(ts));
    // Round-trip
    let output = config.serialize();
    let config2 = parse_str(&output);
    assert_eq!(first_block(&config2).stale(), Some(ts));
}

#[test]
fn stale_with_u64_overflow() {
    let config_str = "Host w\n  HostName 1.2.3.4\n  # purple:stale 18446744073709551616\n";
    let config = parse_str(config_str);
    assert_eq!(first_block(&config).stale(), None);
}

#[test]
fn stale_timestamp_zero() {
    let config_str = "Host w\n  HostName 1.2.3.4\n  # purple:stale 0\n";
    let config = parse_str(config_str);
    assert_eq!(first_block(&config).stale(), Some(0));
}

#[test]
fn set_host_stale_nonexistent_alias_is_noop() {
    let config_str = "Host web\n  HostName 1.2.3.4\n";
    let mut config = parse_str(config_str);
    let before = config.serialize();
    let _ = config.set_host_stale("nonexistent", 12345);
    assert_eq!(config.serialize(), before);
}

#[test]
fn clear_host_stale_nonexistent_alias_is_noop() {
    let config_str = "Host web\n  HostName 1.2.3.4\n";
    let mut config = parse_str(config_str);
    let before = config.serialize();
    let _ = config.clear_host_stale("nonexistent");
    assert_eq!(config.serialize(), before);
}

#[test]
fn stale_hosts_empty_config() {
    let config_str = "";
    let config = parse_str(config_str);
    assert!(config.stale_hosts().is_empty());
}

#[test]
fn stale_hosts_no_stale() {
    let config_str = "Host web\n  HostName 1.2.3.4\n\nHost db\n  HostName 5.6.7.8\n";
    let config = parse_str(config_str);
    assert!(config.stale_hosts().is_empty());
}

#[test]
fn clear_stale_preserves_other_purple_comments() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:tags prod
  # purple:provider do:123
  # purple:askpass keychain
  # purple:meta region=nyc3
  # purple:stale 1711900000
";
    let mut config = parse_str(config_str);
    let _ = config.clear_host_stale("web");
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.stale, None);
    assert!(entry.tags.contains(&"prod".to_string()));
    assert_eq!(entry.provider, Some("do".to_string()));
    assert_eq!(entry.askpass, Some("keychain".to_string()));
    assert_eq!(entry.provider_meta[0].0, "region");
}

#[test]
fn set_stale_preserves_other_purple_comments() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:tags prod
  # purple:provider do:123
  # purple:askpass keychain
  # purple:meta region=nyc3
";
    let mut config = parse_str(config_str);
    let _ = config.set_host_stale("web", 1711900000);
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.stale, Some(1711900000));
    assert!(entry.tags.contains(&"prod".to_string()));
    assert_eq!(entry.provider, Some("do".to_string()));
    assert_eq!(entry.askpass, Some("keychain".to_string()));
    assert_eq!(entry.provider_meta[0].0, "region");
}

#[test]
fn stale_multiple_comments_first_wins() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:stale 1000
  # purple:stale 2000
";
    let config = parse_str(config_str);
    assert_eq!(first_block(&config).stale(), Some(1000));
}

#[test]
fn set_stale_removes_multiple_stale_comments() {
    let config_str = "\
Host web
  HostName 1.2.3.4
  # purple:stale 1000
  # purple:stale 2000
";
    let mut config = parse_str(config_str);
    first_block_mut(&mut config).set_stale(3000);
    assert_eq!(first_block(&config).stale(), Some(3000));
    let output = config.serialize();
    assert_eq!(output.matches("purple:stale").count(), 1);
}

#[test]
fn stale_absent_in_host_entry() {
    let config_str = "Host web\n  HostName 1.2.3.4\n";
    let config = parse_str(config_str);
    assert_eq!(first_block(&config).to_host_entry().stale, None);
}

#[test]
fn set_stale_four_space_indent() {
    let config_str = "Host web\n    HostName 1.2.3.4\n";
    let mut config = parse_str(config_str);
    first_block_mut(&mut config).set_stale(1711900000);
    assert!(config.serialize().contains("    # purple:stale 1711900000"));
}

#[test]
fn clear_stale_removes_bare_comment() {
    let config_str = "Host web\n  HostName 1.2.3.4\n  # purple:stale\n";
    let mut config = parse_str(config_str);
    first_block_mut(&mut config).clear_stale();
    assert!(!config.serialize().contains("purple:stale"));
}

// ── SSH config integrity tests for stale operations ──────────────

#[test]
fn stale_preserves_blank_line_between_hosts() {
    let config_str = "\
Host web
  HostName 1.2.3.4

Host db
  HostName 5.6.7.8
";
    let mut config = parse_str(config_str);
    let _ = config.set_host_stale("web", 1711900000);
    let output = config.serialize();
    // There must still be a blank line between hosts
    assert!(
        output.contains("# purple:stale 1711900000\n\nHost db"),
        "blank line between hosts lost after set_stale:\n{}",
        output
    );
}

#[test]
fn stale_preserves_blank_line_before_group_header() {
    let config_str = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:111

# purple:group Hetzner

Host hz-cache
  HostName 9.10.11.12
  # purple:provider hetzner:333
";
    let mut config = parse_str(config_str);
    let _ = config.set_host_stale("do-web", 1711900000);
    let output = config.serialize();
    // There must still be a blank line before the Hetzner group header
    assert!(
        output.contains("\n\n# purple:group Hetzner"),
        "blank line before group header lost after set_stale:\n{}",
        output
    );
}

#[test]
fn stale_set_and_clear_is_byte_identical() {
    let config_str = "\
Host manual
  HostName 10.0.0.1
  User admin

# purple:group DigitalOcean

Host do-web
  HostName 1.2.3.4
  User root
  # purple:provider digitalocean:111
  # purple:tags prod

Host do-db
  HostName 5.6.7.8
  User root
  # purple:provider digitalocean:222
  # purple:meta region=nyc3

# purple:group Hetzner

Host hz-cache
  HostName 9.10.11.12
  User root
  # purple:provider hetzner:333
";
    let original = config_str.to_string();
    let mut config = parse_str(config_str);

    // Mark stale
    let _ = config.set_host_stale("do-db", 1711900000);
    let after_stale = config.serialize();
    assert_ne!(after_stale, original, "stale should change the config");

    // Clear stale
    let _ = config.clear_host_stale("do-db");
    let after_clear = config.serialize();
    assert_eq!(
        after_clear, original,
        "clearing stale must restore byte-identical config"
    );
}

#[test]
fn stale_does_not_accumulate_blank_lines() {
    let config_str = "Host web\n  HostName 1.2.3.4\n\nHost db\n  HostName 5.6.7.8\n";
    let mut config = parse_str(config_str);

    // Set and clear stale 10 times
    for _ in 0..10 {
        let _ = config.set_host_stale("web", 1711900000);
        let _ = config.clear_host_stale("web");
    }

    let output = config.serialize();
    assert_eq!(
        output, config_str,
        "repeated set/clear must not accumulate blank lines"
    );
}

#[test]
fn stale_preserves_all_directives_and_comments() {
    let config_str = "\
Host complex
  HostName 1.2.3.4
  User deploy
  Port 2222
  IdentityFile ~/.ssh/id_ed25519
  ProxyJump bastion
  LocalForward 8080 localhost:80
  # purple:provider digitalocean:999
  # purple:tags prod,us-east
  # purple:provider_tags web-tier
  # purple:askpass keychain
  # purple:meta region=nyc3,plan=s-1vcpu-1gb
  # This is a user comment
";
    let mut config = parse_str(config_str);
    let entry_before = first_block(&config).to_host_entry();

    let _ = config.set_host_stale("complex", 1711900000);
    let entry_after = first_block(&config).to_host_entry();

    // Every field must survive stale marking
    assert_eq!(entry_after.hostname, entry_before.hostname);
    assert_eq!(entry_after.user, entry_before.user);
    assert_eq!(entry_after.port, entry_before.port);
    assert_eq!(entry_after.identity_file, entry_before.identity_file);
    assert_eq!(entry_after.proxy_jump, entry_before.proxy_jump);
    assert_eq!(entry_after.tags, entry_before.tags);
    assert_eq!(entry_after.provider_tags, entry_before.provider_tags);
    assert_eq!(entry_after.provider, entry_before.provider);
    assert_eq!(entry_after.askpass, entry_before.askpass);
    assert_eq!(entry_after.provider_meta, entry_before.provider_meta);
    assert_eq!(entry_after.tunnel_count, entry_before.tunnel_count);
    assert_eq!(entry_after.stale, Some(1711900000));

    // Clear stale and verify everything still intact
    let _ = config.clear_host_stale("complex");
    let entry_cleared = first_block(&config).to_host_entry();
    assert_eq!(entry_cleared.stale, None);
    assert_eq!(entry_cleared.hostname, entry_before.hostname);
    assert_eq!(entry_cleared.tags, entry_before.tags);
    assert_eq!(entry_cleared.provider, entry_before.provider);
    assert_eq!(entry_cleared.askpass, entry_before.askpass);
    assert_eq!(entry_cleared.provider_meta, entry_before.provider_meta);

    // User comment must survive
    assert!(config.serialize().contains("# This is a user comment"));
}

#[test]
fn stale_on_last_host_preserves_trailing_newline() {
    let config_str = "Host web\n  HostName 1.2.3.4\n";
    let mut config = parse_str(config_str);
    let _ = config.set_host_stale("web", 1711900000);
    let output = config.serialize();
    assert!(output.ends_with('\n'), "config must end with newline");

    let _ = config.clear_host_stale("web");
    let output2 = config.serialize();
    assert_eq!(output2, config_str);
}

#[test]
fn stale_with_crlf_preserves_line_endings() {
    let config_str = "Host web\r\n  HostName 1.2.3.4\r\n";
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(config_str),
        path: test_config_path(),
        crlf: true,
        bom: false,
    };
    let mut config = config;
    let _ = config.set_host_stale("web", 1711900000);
    let output = config.serialize();
    // All lines must use CRLF
    for line in output.split('\n') {
        if !line.is_empty() {
            assert!(
                line.ends_with('\r'),
                "CRLF lost after set_stale. Line: {:?}",
                line
            );
        }
    }

    let _ = config.clear_host_stale("web");
    assert_eq!(config.serialize(), config_str);
}

#[test]
fn pattern_match_star_wildcard() {
    assert!(ssh_pattern_match("*", "anything"));
    assert!(ssh_pattern_match("10.30.0.*", "10.30.0.5"));
    assert!(ssh_pattern_match("10.30.0.*", "10.30.0.100"));
    assert!(!ssh_pattern_match("10.30.0.*", "10.30.1.5"));
    assert!(ssh_pattern_match("*.example.com", "web.example.com"));
    assert!(!ssh_pattern_match("*.example.com", "example.com"));
    assert!(ssh_pattern_match("prod-*-web", "prod-us-web"));
    assert!(!ssh_pattern_match("prod-*-web", "prod-us-api"));
}

#[test]
fn pattern_match_question_mark() {
    assert!(ssh_pattern_match("server-?", "server-1"));
    assert!(ssh_pattern_match("server-?", "server-a"));
    assert!(!ssh_pattern_match("server-?", "server-10"));
    assert!(!ssh_pattern_match("server-?", "server-"));
}

#[test]
fn pattern_match_character_class() {
    assert!(ssh_pattern_match("server-[abc]", "server-a"));
    assert!(ssh_pattern_match("server-[abc]", "server-c"));
    assert!(!ssh_pattern_match("server-[abc]", "server-d"));
    assert!(ssh_pattern_match("server-[0-9]", "server-5"));
    assert!(!ssh_pattern_match("server-[0-9]", "server-a"));
    assert!(ssh_pattern_match("server-[!abc]", "server-d"));
    assert!(!ssh_pattern_match("server-[!abc]", "server-a"));
    assert!(ssh_pattern_match("server-[^abc]", "server-d"));
    assert!(!ssh_pattern_match("server-[^abc]", "server-a"));
}

#[test]
fn pattern_match_negation() {
    assert!(!ssh_pattern_match("!prod-*", "prod-web"));
    assert!(ssh_pattern_match("!prod-*", "staging-web"));
}

#[test]
fn pattern_match_exact() {
    assert!(ssh_pattern_match("myserver", "myserver"));
    assert!(!ssh_pattern_match("myserver", "myserver2"));
    assert!(!ssh_pattern_match("myserver", "other"));
}

#[test]
fn pattern_match_empty() {
    assert!(!ssh_pattern_match("", "anything"));
    assert!(!ssh_pattern_match("*", ""));
    assert!(ssh_pattern_match("", ""));
}

#[test]
fn host_pattern_matches_multi_pattern() {
    assert!(host_pattern_matches("prod staging", "prod"));
    assert!(host_pattern_matches("prod staging", "staging"));
    assert!(!host_pattern_matches("prod staging", "dev"));
}

#[test]
fn host_pattern_matches_with_negation() {
    assert!(host_pattern_matches(
        "*.example.com !internal.example.com",
        "web.example.com",
    ));
    assert!(!host_pattern_matches(
        "*.example.com !internal.example.com",
        "internal.example.com",
    ));
}

#[test]
fn host_pattern_matches_alias_only() {
    // OpenSSH Host keyword matches only against alias, not HostName
    assert!(!host_pattern_matches("10.30.0.*", "production"));
    assert!(host_pattern_matches("prod*", "production"));
    assert!(!host_pattern_matches("staging*", "production"));
}

#[test]
fn pattern_entries_collects_wildcards() {
    let config = parse_str(
        "Host myserver\n  Hostname 10.0.0.1\n\nHost 10.30.0.*\n  User debian\n  ProxyJump bastion\n\nHost *\n  ServerAliveInterval 60\n",
    );
    let patterns = config.pattern_entries();
    assert_eq!(patterns.len(), 2);
    assert_eq!(patterns[0].pattern, "10.30.0.*");
    assert_eq!(patterns[0].user, "debian");
    assert_eq!(patterns[0].proxy_jump, "bastion");
    assert_eq!(patterns[1].pattern, "*");
    assert!(
        patterns[1]
            .directives
            .iter()
            .any(|(k, v)| k == "ServerAliveInterval" && v == "60")
    );
}

#[test]
fn pattern_entries_empty_when_no_patterns() {
    let config = parse_str("Host myserver\n  Hostname 10.0.0.1\n");
    let patterns = config.pattern_entries();
    assert!(patterns.is_empty());
}

#[test]
fn matching_patterns_returns_in_config_order() {
    let config = parse_str(
        "Host 10.30.0.*\n  User debian\n\nHost myserver\n  Hostname 10.30.0.5\n\nHost *\n  ServerAliveInterval 60\n",
    );
    // "myserver" matches "*" but not "10.30.0.*" (alias-only matching)
    let matches = config.matching_patterns("myserver");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].pattern, "*");
}

#[test]
fn matching_patterns_negation_excludes() {
    let config = parse_str(
        "Host * !bastion\n  ServerAliveInterval 60\n\nHost bastion\n  Hostname 10.0.0.1\n",
    );
    let matches = config.matching_patterns("bastion");
    assert!(matches.is_empty());
}

#[test]
fn pattern_entries_and_host_entries_are_disjoint() {
    let config = parse_str(
        "Host myserver\n  Hostname 10.0.0.1\n\nHost 10.30.0.*\n  User debian\n\nHost *\n  ServerAliveInterval 60\n",
    );
    let hosts = config.host_entries();
    let patterns = config.pattern_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].alias, "myserver");
    assert_eq!(patterns.len(), 2);
    assert_eq!(patterns[0].pattern, "10.30.0.*");
    assert_eq!(patterns[1].pattern, "*");
}

#[test]
fn pattern_crud_round_trip() {
    let mut config = parse_str("Host myserver\n  Hostname 10.0.0.1\n");
    // Add a pattern via HostEntry (the form uses HostEntry for submission)
    let entry = HostEntry {
        alias: "10.30.0.*".to_string(),
        user: "debian".to_string(),
        ..Default::default()
    };
    config.add_host(&entry);
    let output = config.serialize();
    assert!(output.contains("Host 10.30.0.*"));
    assert!(output.contains("User debian"));
    // Verify it appears in pattern_entries, not host_entries
    let reparsed = parse_str(&output);
    assert_eq!(reparsed.host_entries().len(), 1);
    assert_eq!(reparsed.pattern_entries().len(), 1);
    assert_eq!(reparsed.pattern_entries()[0].pattern, "10.30.0.*");
}

#[test]
fn host_entries_inherit_proxy_jump_from_wildcard_pattern() {
    // Host "web-*" defines ProxyJump bastion. Host "web-prod" should inherit it.
    let config =
        parse_str("Host web-*\n  ProxyJump bastion\n\nHost web-prod\n  Hostname 10.0.0.1\n");
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].alias, "web-prod");
    assert_eq!(hosts[0].proxy_jump, "bastion");
}

#[test]
fn host_entries_inherit_proxy_jump_from_star_pattern() {
    // Host "*" defines ProxyJump bastion. All hosts without their own ProxyJump inherit it.
    let config = parse_str(
        "Host myserver\n  Hostname 10.0.0.1\n\nHost *\n  ProxyJump gateway\n  User admin\n",
    );
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].proxy_jump, "gateway");
    assert_eq!(hosts[0].user, "admin");
}

#[test]
fn host_entries_inherit_proxy_command_from_star_pattern() {
    // A ProxyCommand on a pattern routes every host it matches through a
    // machine of its own, the same way a ProxyJump does.
    let config = parse_str(
        "Host myserver\n  Hostname 10.0.0.1\n\nHost *\n  ProxyCommand ssh -W %h:%p bastion\n",
    );
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert!(hosts[0].has_proxy_command);
}

#[test]
fn a_host_opting_out_keeps_its_own_proxy_command_none() {
    // `ProxyCommand none` is the opt-out spelling. The host answered for
    // itself, so the pattern does not answer over it.
    let config = parse_str(
        "Host myserver\n  Hostname 10.0.0.1\n  ProxyCommand none\n\nHost *\n  ProxyCommand ssh -W %h:%p bastion\n",
    );
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert!(!hosts[0].has_proxy_command);
}

#[test]
fn a_pattern_above_the_host_wins_over_the_hosts_own_none() {
    // ssh takes the first value it obtains for the keyword, and the pattern
    // is read first here, so the opt-out below it never applies and the
    // host really does go through the bastion.
    let config = parse_str(
        "Host *\n  ProxyCommand ssh -W %h:%p bastion\n\nHost myserver\n  Hostname 10.0.0.1\n  ProxyCommand none\n",
    );
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert!(hosts[0].has_proxy_command);
}

#[test]
fn a_pattern_setting_proxy_command_none_adds_no_bastion() {
    let config = parse_str("Host myserver\n  Hostname 10.0.0.1\n\nHost *\n  ProxyCommand none\n");
    let hosts = config.host_entries();
    assert!(!hosts[0].has_proxy_command);
}

#[test]
fn a_negated_pattern_proxy_command_skips_the_host_it_excludes() {
    // The shape `tsh config` writes: every node in the cluster goes through
    // the proxy, except the proxy itself.
    let config = parse_str(
        "Host node1.example.com\n  HostName node1.example.com\n\nHost proxy.example.com\n  HostName proxy.example.com\n\nHost *.example.com !proxy.example.com\n  Port = 3022\n  ProxyCommand tsh proxy ssh %r@%h:%p\n",
    );
    let hosts = config.host_entries();
    let node = hosts
        .iter()
        .find(|h| h.alias == "node1.example.com")
        .expect("node1");
    let proxy = hosts
        .iter()
        .find(|h| h.alias == "proxy.example.com")
        .expect("proxy");
    assert!(node.has_proxy_command, "the node goes through the proxy");
    assert!(
        !proxy.has_proxy_command,
        "the pattern excludes the proxy itself"
    );
}

#[test]
fn an_earlier_pattern_saying_none_beats_a_later_one_setting_a_command() {
    // ssh takes the first value it obtains, so the broad opt-out at the top
    // settles it and the narrower block below never applies.
    let config = parse_str(
        "Host prod-db\n  Hostname 10.0.0.1\n\nHost *\n  ProxyCommand none\n\nHost prod-*\n  ProxyCommand ssh -W %h:%p bastion\n",
    );
    let hosts = config.host_entries();
    assert!(!hosts[0].has_proxy_command);
}

#[test]
fn an_earlier_pattern_setting_a_command_wins_over_a_later_none() {
    let config = parse_str(
        "Host prod-db\n  Hostname 10.0.0.1\n\nHost prod-*\n  ProxyCommand ssh -W %h:%p bastion\n\nHost *\n  ProxyCommand none\n",
    );
    let hosts = config.host_entries();
    assert!(hosts[0].has_proxy_command);
}

#[test]
fn a_pattern_proxy_command_does_not_reach_a_host_it_does_not_match() {
    let config = parse_str(
        "Host myserver\n  Hostname 10.0.0.1\n\nHost prod-*\n  ProxyCommand ssh -W %h:%p bastion\n",
    );
    let hosts = config.host_entries();
    assert!(!hosts[0].has_proxy_command);
}

#[test]
fn host_entries_own_proxy_jump_takes_precedence() {
    // Host's own ProxyJump should not be overridden by pattern.
    let config = parse_str(
        "Host web-*\n  ProxyJump gateway\n\nHost web-prod\n  Hostname 10.0.0.1\n  ProxyJump bastion\n",
    );
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].proxy_jump, "bastion"); // own value, not gateway
}

#[test]
fn host_entries_hostname_pattern_does_not_match_by_hostname() {
    // SSH Host patterns match alias only, not Hostname. Pattern "10.30.0.*"
    // should NOT match alias "myserver" even though Hostname is 10.30.0.5.
    let config = parse_str(
        "Host 10.30.0.*\n  ProxyJump bastion\n  User debian\n\nHost myserver\n  Hostname 10.30.0.5\n",
    );
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].alias, "myserver");
    assert_eq!(hosts[0].proxy_jump, ""); // no match: alias doesn't match pattern
    assert_eq!(hosts[0].user, ""); // no match
}

#[test]
fn host_entries_first_match_wins() {
    // Two patterns match: first one's value should win.
    let config = parse_str(
        "Host web-*\n  User team\n\nHost *\n  User fallback\n\nHost web-prod\n  Hostname 10.0.0.1\n",
    );
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].user, "team"); // web-* matches first
}

#[test]
fn host_entries_no_inheritance_when_all_set() {
    // Host has all inheritable fields set. No pattern should override.
    let config = parse_str(
        "Host *\n  User fallback\n  ProxyJump gw\n  IdentityFile ~/.ssh/other\n\n\
         Host myserver\n  Hostname 10.0.0.1\n  User root\n  ProxyJump bastion\n  IdentityFile ~/.ssh/mine\n",
    );
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].user, "root");
    assert_eq!(hosts[0].proxy_jump, "bastion");
    assert_eq!(hosts[0].identity_file, "~/.ssh/mine");
}

#[test]
fn host_entries_negation_excludes_from_inheritance() {
    // "Host * !bastion" should NOT apply to bastion.
    let config =
        parse_str("Host * !bastion\n  ProxyJump gateway\n\nHost bastion\n  Hostname 10.0.0.1\n");
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].alias, "bastion");
    assert_eq!(hosts[0].proxy_jump, ""); // excluded by negation
}

#[test]
fn host_entries_inherit_identity_file_from_pattern() {
    // Positive test: IdentityFile inherited when host block lacks it.
    let config = parse_str(
        "Host *\n  IdentityFile ~/.ssh/default_key\n\nHost myserver\n  Hostname 10.0.0.1\n",
    );
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].identity_file, "~/.ssh/default_key");
}

#[test]
fn host_entries_multiple_hosts_mixed_inheritance() {
    // Three hosts: one inherits ProxyJump, one has its own, one is the bastion.
    let config = parse_str(
        "Host web-*\n  ProxyJump bastion\n\n\
         Host web-prod\n  Hostname 10.0.0.1\n\n\
         Host web-staging\n  Hostname 10.0.0.2\n  ProxyJump gateway\n\n\
         Host bastion\n  Hostname 10.0.0.99\n",
    );
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 3);
    let prod = hosts.iter().find(|h| h.alias == "web-prod").unwrap();
    let staging = hosts.iter().find(|h| h.alias == "web-staging").unwrap();
    let bastion = hosts.iter().find(|h| h.alias == "bastion").unwrap();
    assert_eq!(prod.proxy_jump, "bastion"); // inherited
    assert_eq!(staging.proxy_jump, "gateway"); // own value
    assert_eq!(bastion.proxy_jump, ""); // no match
}

#[test]
fn host_entries_partial_inheritance() {
    // Host has ProxyJump and User set, but no IdentityFile. Only IdentityFile inherited.
    let config = parse_str(
        "Host *\n  User fallback\n  ProxyJump gw\n  IdentityFile ~/.ssh/default\n\n\
         Host myserver\n  Hostname 10.0.0.1\n  User root\n  ProxyJump bastion\n",
    );
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].user, "root"); // own
    assert_eq!(hosts[0].proxy_jump, "bastion"); // own
    assert_eq!(hosts[0].identity_file, "~/.ssh/default"); // inherited
}

#[test]
fn host_entries_alias_is_ip_matches_ip_pattern() {
    // When alias itself is an IP, it matches IP-based patterns directly.
    let config = parse_str("Host 10.0.0.*\n  ProxyJump bastion\n\nHost 10.0.0.5\n  User root\n");
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].alias, "10.0.0.5");
    assert_eq!(hosts[0].proxy_jump, "bastion");
}

#[test]
fn host_entries_no_hostname_still_inherits_by_alias() {
    // Host without Hostname directive still inherits via alias matching.
    let config = parse_str("Host *\n  User admin\n\nHost myserver\n  Port 2222\n");
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].user, "admin"); // inherited via alias match on "*"
    assert!(hosts[0].hostname.is_empty()); // no hostname set
}

#[test]
fn host_entries_self_referencing_proxy_jump_assigned() {
    // Self-referencing ProxyJump IS assigned (SSH would do the same).
    // The UI detects and warns via proxy_jump_contains_self.
    let config = parse_str(
        "Host *\n  ProxyJump gateway\n\n\
         Host gateway\n  Hostname 10.0.0.1\n\n\
         Host backend\n  Hostname 10.0.0.2\n",
    );
    let hosts = config.host_entries();
    let gateway = hosts.iter().find(|h| h.alias == "gateway").unwrap();
    let backend = hosts.iter().find(|h| h.alias == "backend").unwrap();
    assert_eq!(gateway.proxy_jump, "gateway"); // self-ref assigned
    assert_eq!(backend.proxy_jump, "gateway");
    // Detection helper identifies the loop.
    assert!(proxy_jump_contains_self(
        &gateway.proxy_jump,
        &gateway.alias
    ));
    assert!(!proxy_jump_contains_self(
        &backend.proxy_jump,
        &backend.alias
    ));
}

#[test]
fn proxy_jump_contains_self_comma_separated() {
    assert!(proxy_jump_contains_self("hop1,gateway", "gateway"));
    assert!(proxy_jump_contains_self("gateway,hop2", "gateway"));
    assert!(proxy_jump_contains_self("hop1, gateway", "gateway"));
    assert!(proxy_jump_contains_self("gateway", "gateway"));
    assert!(!proxy_jump_contains_self("hop1,hop2", "gateway"));
    assert!(!proxy_jump_contains_self("", "gateway"));
    assert!(!proxy_jump_contains_self("gateway-2", "gateway"));
    // user@host and host:port forms
    assert!(proxy_jump_contains_self("admin@gateway", "gateway"));
    assert!(proxy_jump_contains_self("gateway:2222", "gateway"));
    assert!(proxy_jump_contains_self("admin@gateway:2222", "gateway"));
    assert!(proxy_jump_contains_self(
        "hop1,admin@gateway:2222",
        "gateway"
    ));
    assert!(!proxy_jump_contains_self("admin@gateway-2", "gateway"));
    assert!(!proxy_jump_contains_self("admin@other:2222", "gateway"));
    // IPv6 bracket notation
    assert!(proxy_jump_contains_self("[::1]:2222", "::1"));
    assert!(proxy_jump_contains_self("user@[::1]:2222", "::1"));
    assert!(!proxy_jump_contains_self("[::2]:2222", "::1"));
    assert!(proxy_jump_contains_self("hop1,[::1]:2222", "::1"));
}

// =========================================================================
// raw_host_entry tests
// =========================================================================

#[test]
fn raw_host_entry_returns_without_inheritance() {
    let config =
        parse_str("Host *\n  ProxyJump gw\n  User admin\n\nHost myserver\n  Hostname 10.0.0.1\n");
    let raw = config.raw_host_entry("myserver").unwrap();
    assert_eq!(raw.alias, "myserver");
    assert_eq!(raw.hostname, "10.0.0.1");
    assert_eq!(raw.proxy_jump, ""); // not inherited
    assert_eq!(raw.user, ""); // not inherited
    // Contrast with host_entries which applies inheritance:
    let enriched = config.host_entries();
    assert_eq!(enriched[0].proxy_jump, "gw");
    assert_eq!(enriched[0].user, "admin");
}

#[test]
fn raw_host_entry_preserves_own_values() {
    let config = parse_str(
        "Host *\n  ProxyJump gw\n\nHost myserver\n  Hostname 10.0.0.1\n  ProxyJump bastion\n",
    );
    let raw = config.raw_host_entry("myserver").unwrap();
    assert_eq!(raw.proxy_jump, "bastion"); // own value preserved
}

#[test]
fn raw_host_entry_returns_none_for_missing() {
    let config = parse_str("Host myserver\n  Hostname 10.0.0.1\n");
    assert!(config.raw_host_entry("nonexistent").is_none());
}

#[test]
fn raw_host_entry_returns_none_for_pattern() {
    let config = parse_str("Host 10.30.0.*\n  ProxyJump bastion\n");
    assert!(config.raw_host_entry("10.30.0.*").is_none());
}

// =========================================================================
// inherited_hints tests
// =========================================================================

#[test]
fn inherited_hints_returns_value_and_source() {
    let config = parse_str(
        "Host web-*\n  ProxyJump bastion\n  User team\n\nHost web-prod\n  Hostname 10.0.0.1\n",
    );
    let hints = config.inherited_hints("web-prod");
    let (val, src) = hints.proxy_jump.unwrap();
    assert_eq!(val, "bastion");
    assert_eq!(src, "web-*");
    let (val, src) = hints.user.unwrap();
    assert_eq!(val, "team");
    assert_eq!(src, "web-*");
    assert!(hints.identity_file.is_none());
}

#[test]
fn inherited_hints_first_match_wins_with_source() {
    let config = parse_str(
        "Host web-*\n  User team\n\nHost *\n  User fallback\n  ProxyJump gw\n\nHost web-prod\n  Hostname 10.0.0.1\n",
    );
    let hints = config.inherited_hints("web-prod");
    // User comes from web-* (first match), not * (second match).
    let (val, src) = hints.user.unwrap();
    assert_eq!(val, "team");
    assert_eq!(src, "web-*");
    // ProxyJump comes from * (only source).
    let (val, src) = hints.proxy_jump.unwrap();
    assert_eq!(val, "gw");
    assert_eq!(src, "*");
}

#[test]
fn inherited_hints_no_match_returns_default() {
    let config =
        parse_str("Host web-*\n  ProxyJump bastion\n\nHost myserver\n  Hostname 10.0.0.1\n");
    let hints = config.inherited_hints("myserver");
    // "myserver" does not match "web-*"
    assert!(hints.proxy_jump.is_none());
    assert!(hints.user.is_none());
    assert!(hints.identity_file.is_none());
}

#[test]
fn inherited_hints_partial_fields_from_different_patterns() {
    let config = parse_str(
        "Host web-*\n  ProxyJump bastion\n\nHost *\n  IdentityFile ~/.ssh/default\n\nHost web-prod\n  Hostname 10.0.0.1\n",
    );
    let hints = config.inherited_hints("web-prod");
    let (val, src) = hints.proxy_jump.unwrap();
    assert_eq!(val, "bastion");
    assert_eq!(src, "web-*");
    let (val, src) = hints.identity_file.unwrap();
    assert_eq!(val, "~/.ssh/default");
    assert_eq!(src, "*");
    assert!(hints.user.is_none());
}

#[test]
fn inherited_hints_negation_excludes() {
    // "Host * !bastion" should NOT produce hints for "bastion".
    let config = parse_str(
        "Host * !bastion\n  ProxyJump gateway\n  User admin\n\n\
         Host bastion\n  Hostname 10.0.0.1\n",
    );
    let hints = config.inherited_hints("bastion");
    assert!(hints.proxy_jump.is_none());
    assert!(hints.user.is_none());
}

#[test]
fn inherited_hints_returned_even_when_host_has_own_values() {
    // inherited_hints is independent of the host's own values: it reports
    // what patterns provide. The form decides visibility via value.is_empty().
    let config = parse_str(
        "Host *\n  ProxyJump gateway\n  User admin\n\n\
         Host myserver\n  Hostname 10.0.0.1\n  ProxyJump bastion\n  User root\n",
    );
    let hints = config.inherited_hints("myserver");
    // Hints are returned even though host has own ProxyJump and User.
    let (val, _) = hints.proxy_jump.unwrap();
    assert_eq!(val, "gateway");
    let (val, _) = hints.user.unwrap();
    assert_eq!(val, "admin");
}

#[test]
fn inheritance_across_include_boundary() {
    // Pattern in an included file applies to a host in the main config.
    let included_elements =
        SshConfigFile::parse_content("Host web-*\n  ProxyJump bastion\n  User team\n");
    let main_elements = vec![
        ConfigElement::Include(IncludeDirective {
            raw_line: "Include conf.d/*".to_string(),
            pattern: "conf.d/*".to_string(),
            resolved_files: vec![IncludedFile {
                path: PathBuf::from("/etc/ssh/conf.d/patterns.conf"),
                elements: included_elements,
            }],
        }),
        // Host in main config, after the include.
        ConfigElement::HostBlock(HostBlock {
            host_pattern: "web-prod".to_string(),
            raw_host_line: "Host web-prod".to_string(),
            directives: vec![Directive {
                key: "HostName".to_string(),
                value: "10.0.0.1".to_string(),
                raw_line: "  HostName 10.0.0.1".to_string(),
                is_non_directive: false,
            }],
        }),
    ];
    let config = SshConfigFile {
        elements: main_elements,
        path: test_config_path(),
        crlf: false,
        bom: false,
    };
    // host_entries should inherit from the included pattern.
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].alias, "web-prod");
    assert_eq!(hosts[0].proxy_jump, "bastion");
    assert_eq!(hosts[0].user, "team");
    // inherited_hints should also find the included pattern.
    let hints = config.inherited_hints("web-prod");
    let (val, src) = hints.proxy_jump.unwrap();
    assert_eq!(val, "bastion");
    assert_eq!(src, "web-*");
}

#[test]
fn inheritance_host_in_include_pattern_in_main() {
    // Host in an included file, pattern in main config.
    let included_elements = SshConfigFile::parse_content("Host web-prod\n  HostName 10.0.0.1\n");
    let mut main_elements = SshConfigFile::parse_content("Host web-*\n  ProxyJump bastion\n");
    main_elements.push(ConfigElement::Include(IncludeDirective {
        raw_line: "Include conf.d/*".to_string(),
        pattern: "conf.d/*".to_string(),
        resolved_files: vec![IncludedFile {
            path: PathBuf::from("/etc/ssh/conf.d/hosts.conf"),
            elements: included_elements,
        }],
    }));
    let config = SshConfigFile {
        elements: main_elements,
        path: test_config_path(),
        crlf: false,
        bom: false,
    };
    let hosts = config.host_entries();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].alias, "web-prod");
    assert_eq!(hosts[0].proxy_jump, "bastion");
}

#[test]
fn matching_patterns_full_ssh_semantics() {
    let config = parse_str(
        "Host 10.30.0.*\n  User debian\n  IdentityFile ~/.ssh/id_bootstrap\n  ProxyJump bastion\n\n\
         Host *.internal !secret.internal\n  ForwardAgent yes\n\n\
         Host myserver\n  Hostname 10.30.0.5\n\n\
         Host *\n  ServerAliveInterval 60\n",
    );
    // "myserver" only matches "*" (alias-only, not hostname)
    let matches = config.matching_patterns("myserver");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].pattern, "*");
    assert!(
        matches[0]
            .directives
            .iter()
            .any(|(k, v)| k == "ServerAliveInterval" && v == "60")
    );
}

#[test]
fn pattern_entries_preserve_all_directives() {
    let config = parse_str(
        "Host *.example.com\n  User admin\n  Port 2222\n  IdentityFile ~/.ssh/id_example\n  ProxyJump gateway\n  ServerAliveInterval 30\n  ForwardAgent yes\n",
    );
    let patterns = config.pattern_entries();
    assert_eq!(patterns.len(), 1);
    let p = &patterns[0];
    assert_eq!(p.pattern, "*.example.com");
    assert_eq!(p.user, "admin");
    assert_eq!(p.port, 2222);
    assert_eq!(p.identity_file, "~/.ssh/id_example");
    assert_eq!(p.proxy_jump, "gateway");
    // All directives should be in the directives vec
    assert_eq!(p.directives.len(), 6);
    assert!(
        p.directives
            .iter()
            .any(|(k, v)| k == "ForwardAgent" && v == "yes")
    );
    assert!(
        p.directives
            .iter()
            .any(|(k, v)| k == "ServerAliveInterval" && v == "30")
    );
}

// --- Pattern visibility tests ---

#[test]
fn roundtrip_pattern_blocks_preserved() {
    let input = "Host myserver\n  Hostname 10.0.0.1\n  User root\n\nHost 10.30.0.*\n  User debian\n  IdentityFile ~/.ssh/id_bootstrap\n  ProxyJump bastion\n\nHost *\n  ServerAliveInterval 60\n  AddKeysToAgent yes\n";
    let config = parse_str(input);
    let output = config.serialize();
    assert_eq!(
        input, output,
        "Pattern blocks must survive round-trip exactly"
    );
}

#[test]
fn add_pattern_preserves_existing_config() {
    let input = "Host myserver\n  Hostname 10.0.0.1\n\nHost otherserver\n  Hostname 10.0.0.2\n\nHost *\n  ServerAliveInterval 60\n";
    let mut config = parse_str(input);
    let entry = HostEntry {
        alias: "10.30.0.*".to_string(),
        user: "debian".to_string(),
        ..Default::default()
    };
    config.add_host(&entry);
    let output = config.serialize();
    // Original hosts must still be there
    assert!(output.contains("Host myserver"));
    assert!(output.contains("Hostname 10.0.0.1"));
    assert!(output.contains("Host otherserver"));
    assert!(output.contains("Hostname 10.0.0.2"));
    // New pattern must be present
    assert!(output.contains("Host 10.30.0.*"));
    assert!(output.contains("User debian"));
    // Host * must still be at the end
    assert!(output.contains("Host *"));
    // New pattern must be BEFORE Host * (SSH first-match-wins)
    let new_pos = output.find("Host 10.30.0.*").unwrap();
    let star_pos = output.find("Host *").unwrap();
    assert!(new_pos < star_pos, "New pattern must be before Host *");
    // Reparse and verify counts
    let reparsed = parse_str(&output);
    assert_eq!(reparsed.host_entries().len(), 2);
    assert_eq!(reparsed.pattern_entries().len(), 2); // 10.30.0.* and *
}

#[test]
fn update_pattern_preserves_other_blocks() {
    let input = "Host myserver\n  Hostname 10.0.0.1\n\nHost 10.30.0.*\n  User debian\n\nHost *\n  ServerAliveInterval 60\n";
    let mut config = parse_str(input);
    let updated = HostEntry {
        alias: "10.30.0.*".to_string(),
        user: "admin".to_string(),
        ..Default::default()
    };
    config.update_host("10.30.0.*", &updated);
    let output = config.serialize();
    // Pattern updated
    assert!(output.contains("User admin"));
    assert!(!output.contains("User debian"));
    // Other blocks unchanged
    assert!(output.contains("Host myserver"));
    assert!(output.contains("Hostname 10.0.0.1"));
    assert!(output.contains("Host *"));
    assert!(output.contains("ServerAliveInterval 60"));
}

#[test]
fn delete_pattern_preserves_other_blocks() {
    let input = "Host myserver\n  Hostname 10.0.0.1\n\nHost 10.30.0.*\n  User debian\n\nHost *\n  ServerAliveInterval 60\n";
    let mut config = parse_str(input);
    config.delete_host("10.30.0.*");
    let output = config.serialize();
    assert!(!output.contains("Host 10.30.0.*"));
    assert!(!output.contains("User debian"));
    assert!(output.contains("Host myserver"));
    assert!(output.contains("Hostname 10.0.0.1"));
    assert!(output.contains("Host *"));
    assert!(output.contains("ServerAliveInterval 60"));
    let reparsed = parse_str(&output);
    assert_eq!(reparsed.host_entries().len(), 1);
    assert_eq!(reparsed.pattern_entries().len(), 1); // only Host *
}

#[test]
fn update_pattern_rename() {
    let input = "Host *.example.com\n  User admin\n\nHost myserver\n  Hostname 10.0.0.1\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "*.prod.example.com".to_string(),
        user: "admin".to_string(),
        ..Default::default()
    };
    config.update_host("*.example.com", &renamed);
    let output = config.serialize();
    assert!(
        !output.contains("Host *.example.com\n"),
        "Old pattern removed"
    );
    assert!(
        output.contains("Host *.prod.example.com"),
        "New pattern present"
    );
    assert!(output.contains("Host myserver"), "Other host preserved");
}

#[test]
fn config_with_only_patterns() {
    let input = "Host *.example.com\n  User admin\n\nHost *\n  ServerAliveInterval 60\n";
    let config = parse_str(input);
    assert!(config.host_entries().is_empty());
    assert_eq!(config.pattern_entries().len(), 2);
    // Round-trip
    let output = config.serialize();
    assert_eq!(input, output);
}

#[test]
fn host_pattern_matches_all_negative_returns_false() {
    assert!(!host_pattern_matches("!prod !staging", "anything"));
    assert!(!host_pattern_matches("!prod !staging", "dev"));
}

#[test]
fn host_pattern_matches_negation_only_checks_alias() {
    // Negation matches against alias only
    assert!(host_pattern_matches("* !10.0.0.1", "myserver"));
    assert!(!host_pattern_matches("* !myserver", "myserver"));
}

#[test]
fn pattern_match_malformed_char_class() {
    // Unmatched bracket: should not panic, treat as no-match
    assert!(!ssh_pattern_match("[abc", "a"));
    assert!(!ssh_pattern_match("[", "a"));
    // Empty class body before ]
    assert!(!ssh_pattern_match("[]", "a"));
}

#[test]
fn host_pattern_matches_whitespace_edge_cases() {
    assert!(host_pattern_matches("prod  staging", "prod"));
    assert!(host_pattern_matches("  prod  ", "prod"));
    assert!(host_pattern_matches("prod\tstaging", "prod"));
    assert!(!host_pattern_matches("   ", "anything"));
    assert!(!host_pattern_matches("", "anything"));
}

#[test]
fn pattern_with_metadata_roundtrip() {
    let input = "Host 10.30.0.*\n  User debian\n  # purple:tags internal,vpn\n  # purple:askpass keychain\n\nHost myserver\n  Hostname 10.0.0.1\n";
    let config = parse_str(input);
    let patterns = config.pattern_entries();
    assert_eq!(patterns.len(), 1);
    assert_eq!(patterns[0].tags, vec!["internal", "vpn"]);
    assert_eq!(patterns[0].askpass.as_deref(), Some("keychain"));
    // Round-trip
    let output = config.serialize();
    assert_eq!(input, output);
}

#[test]
fn matching_patterns_multiple_in_config_order() {
    // Use alias-based patterns that match the alias "my-10-server"
    let input = "Host my-*\n  User fallback\n\nHost my-10*\n  User team\n\nHost my-10-*\n  User specific\n\nHost other\n  Hostname 10.30.0.5\n\nHost *\n  ServerAliveInterval 60\n";
    let config = parse_str(input);
    let matches = config.matching_patterns("my-10-server");
    assert_eq!(matches.len(), 4);
    assert_eq!(matches[0].pattern, "my-*");
    assert_eq!(matches[1].pattern, "my-10*");
    assert_eq!(matches[2].pattern, "my-10-*");
    assert_eq!(matches[3].pattern, "*");
}

#[test]
fn add_pattern_to_empty_config() {
    let mut config = parse_str("");
    let entry = HostEntry {
        alias: "*.example.com".to_string(),
        user: "admin".to_string(),
        ..Default::default()
    };
    config.add_host(&entry);
    let output = config.serialize();
    assert!(output.contains("Host *.example.com"));
    assert!(output.contains("User admin"));
    let reparsed = parse_str(&output);
    assert!(reparsed.host_entries().is_empty());
    assert_eq!(reparsed.pattern_entries().len(), 1);
}

#[test]
fn vault_ssh_parsed_from_comment() {
    let config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:vault-ssh ssh/sign/engineer\n");
    let entries = config.host_entries();
    assert_eq!(entries[0].vault_ssh.as_deref(), Some("ssh/sign/engineer"));
}

// ---- vault_addr parse + set tests ----

#[test]
fn vault_addr_parsed_from_comment() {
    let config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:vault-addr http://127.0.0.1:8200\n",
    );
    let entries = config.host_entries();
    assert_eq!(
        entries[0].vault_addr.as_deref(),
        Some("http://127.0.0.1:8200")
    );
}

#[test]
fn vault_addr_none_when_absent() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    assert!(config.host_entries()[0].vault_addr.is_none());
}

#[test]
fn vault_addr_empty_comment_ignored() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:vault-addr \n");
    assert!(config.host_entries()[0].vault_addr.is_none());
}

#[test]
fn vault_addr_with_whitespace_value_rejected() {
    let config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:vault-addr http://a b:8200\n");
    // The parser splits on the single space after `vault-addr`, so a
    // value that itself contains whitespace gets truncated at the first
    // space. `is_valid_vault_addr` additionally rejects any remaining
    // whitespace, so such a value never makes it past parse.
    assert!(
        config.host_entries()[0]
            .vault_addr
            .as_deref()
            .is_none_or(|v| !v.contains(' '))
    );
}

#[test]
fn vault_addr_round_trip_preserved() {
    let input =
        "Host myserver\n  HostName 10.0.0.1\n  # purple:vault-addr https://vault.example:8200\n";
    let config = parse_str(input);
    assert_eq!(config.serialize(), input);
}

#[test]
fn set_vault_addr_adds_comment() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    assert!(config.set_host_vault_addr("myserver", "http://127.0.0.1:8200"));
    assert_eq!(
        first_block(&config).vault_addr(),
        Some("http://127.0.0.1:8200".to_string())
    );
}

#[test]
fn set_vault_addr_replaces_existing() {
    let mut config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:vault-addr http://old:8200\n");
    assert!(config.set_host_vault_addr("myserver", "https://new:8200"));
    assert_eq!(
        first_block(&config).vault_addr(),
        Some("https://new:8200".to_string())
    );
    assert_eq!(
        config.serialize().matches("purple:vault-addr").count(),
        1,
        "Should have exactly one vault-addr comment after replace"
    );
}

#[test]
fn set_vault_addr_empty_removes() {
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:vault-addr http://127.0.0.1:8200\n",
    );
    assert!(config.set_host_vault_addr("myserver", ""));
    assert!(first_block(&config).vault_addr().is_none());
    assert!(!config.serialize().contains("vault-addr"));
}

#[test]
fn set_vault_addr_preserves_other_comments() {
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:tags a,b\n  # purple:vault-ssh ssh/sign/engineer\n",
    );
    assert!(config.set_host_vault_addr("myserver", "http://127.0.0.1:8200"));
    let entry = config.host_entries().into_iter().next().unwrap();
    assert_eq!(entry.vault_ssh.as_deref(), Some("ssh/sign/engineer"));
    assert_eq!(entry.tags, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(entry.vault_addr.as_deref(), Some("http://127.0.0.1:8200"));
}

#[test]
fn set_vault_addr_preserves_indent() {
    let mut config = parse_str("Host myserver\n    HostName 10.0.0.1\n");
    assert!(config.set_host_vault_addr("myserver", "http://127.0.0.1:8200"));
    let serialized = config.serialize();
    assert!(
        serialized.contains("    # purple:vault-addr http://127.0.0.1:8200"),
        "indent not preserved: {}",
        serialized
    );
}

#[test]
fn set_vault_addr_twice_replaces_not_appends() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    assert!(config.set_host_vault_addr("myserver", "http://one:8200"));
    assert!(config.set_host_vault_addr("myserver", "http://two:8200"));
    let serialized = config.serialize();
    assert_eq!(
        serialized.matches("purple:vault-addr").count(),
        1,
        "Should have exactly one vault-addr comment"
    );
    assert!(serialized.contains("purple:vault-addr http://two:8200"));
}

#[test]
fn set_vault_addr_removes_duplicate_comments() {
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:vault-addr http://a:8200\n  # purple:vault-addr http://b:8200\n",
    );
    assert!(config.set_host_vault_addr("myserver", "http://c:8200"));
    assert_eq!(
        config.serialize().matches("purple:vault-addr").count(),
        1,
        "duplicate comments must collapse on rewrite"
    );
    assert_eq!(
        first_block(&config).vault_addr(),
        Some("http://c:8200".to_string())
    );
}

#[test]
fn set_host_vault_addr_returns_false_when_alias_missing() {
    let mut config = parse_str("Host alpha\n  HostName 10.0.0.1\n");
    assert!(!config.set_host_vault_addr("ghost", "http://127.0.0.1:8200"));
    // Config unchanged
    assert_eq!(config.serialize(), "Host alpha\n  HostName 10.0.0.1\n");
}

#[test]
fn set_host_vault_addr_refuses_wildcard_alias() {
    let mut config = parse_str("Host *\n  HostName 10.0.0.1\n");
    assert!(!config.set_host_vault_addr("*", "http://127.0.0.1:8200"));
    assert!(!config.set_host_vault_addr("", "http://127.0.0.1:8200"));
    assert!(!config.set_host_vault_addr("a?b", "http://127.0.0.1:8200"));
    assert!(!config.set_host_vault_addr("a[bc]", "http://127.0.0.1:8200"));
    assert!(!config.set_host_vault_addr("!a", "http://127.0.0.1:8200"));
    // Multi-host patterns use whitespace separators. Refuse those too
    // so a caller cannot accidentally target a multi-host block.
    assert!(!config.set_host_vault_addr("web-* db-*", "http://127.0.0.1:8200"));
    assert!(!config.set_host_vault_addr("a b", "http://127.0.0.1:8200"));
    assert!(!config.set_host_vault_addr("a\tb", "http://127.0.0.1:8200"));
}

// ---- end vault_addr tests ----

#[test]
fn vault_ssh_none_when_absent() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    assert!(config.host_entries()[0].vault_ssh.is_none());
}

#[test]
fn vault_ssh_empty_comment_ignored() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:vault-ssh \n");
    assert!(config.host_entries()[0].vault_ssh.is_none());
}

#[test]
fn vault_ssh_round_trip_preserved() {
    let input = "Host myserver\n  HostName 10.0.0.1\n  # purple:vault-ssh ssh/sign/engineer\n";
    let config = parse_str(input);
    assert_eq!(config.serialize(), input);
}

#[test]
fn set_vault_ssh_adds_comment() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    assert!(config.set_host_vault_ssh("myserver", "ssh/sign/engineer"));
    assert_eq!(
        first_block(&config).vault_ssh(),
        Some("ssh/sign/engineer".to_string())
    );
}

#[test]
fn set_vault_ssh_replaces_existing() {
    let mut config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:vault-ssh ssh/sign/old\n");
    assert!(config.set_host_vault_ssh("myserver", "ssh/sign/new"));
    assert_eq!(
        first_block(&config).vault_ssh(),
        Some("ssh/sign/new".to_string())
    );
    assert_eq!(
        config.serialize().matches("purple:vault-ssh").count(),
        1,
        "Should have exactly one vault-ssh comment"
    );
}

#[test]
fn set_vault_ssh_empty_removes() {
    let mut config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:vault-ssh ssh/sign/old\n");
    assert!(config.set_host_vault_ssh("myserver", ""));
    assert!(first_block(&config).vault_ssh().is_none());
    assert!(!config.serialize().contains("vault-ssh"));
}

#[test]
fn set_vault_ssh_preserves_other_comments() {
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n  # purple:tags prod\n",
    );
    assert!(config.set_host_vault_ssh("myserver", "ssh/sign/engineer"));
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.askpass, Some("keychain".to_string()));
    assert!(entry.tags.contains(&"prod".to_string()));
    assert_eq!(entry.vault_ssh.as_deref(), Some("ssh/sign/engineer"));
}

#[test]
fn set_vault_ssh_preserves_indent() {
    let mut config = parse_str("Host myserver\n    HostName 10.0.0.1\n");
    assert!(config.set_host_vault_ssh("myserver", "ssh/sign/engineer"));
    let raw = first_block(&config)
        .directives
        .iter()
        .find(|d| d.raw_line.contains("purple:vault-ssh"))
        .unwrap();
    assert!(
        raw.raw_line.starts_with("    "),
        "Expected 4-space indent, got: {:?}",
        raw.raw_line
    );
}

#[test]
fn certificate_file_parsed_from_directive() {
    let config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  CertificateFile ~/.ssh/my-cert.pub\n");
    let entries = config.host_entries();
    assert_eq!(entries[0].certificate_file, "~/.ssh/my-cert.pub");
}

#[test]
fn certificate_file_empty_when_absent() {
    let config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    let entries = config.host_entries();
    assert!(entries[0].certificate_file.is_empty());
}

#[test]
fn set_host_certificate_file_adds_and_removes() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    assert!(config.set_host_certificate_file("myserver", "~/.purple/certs/myserver-cert.pub"));
    assert!(
        config
            .serialize()
            .contains("CertificateFile ~/.purple/certs/myserver-cert.pub")
    );
    assert!(config.set_host_certificate_file("myserver", ""));
    assert!(!config.serialize().contains("CertificateFile"));
}

#[test]
fn set_host_certificate_file_removes_when_empty() {
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  CertificateFile ~/.purple/certs/myserver-cert.pub\n",
    );
    assert!(config.set_host_certificate_file("myserver", ""));
    assert!(!config.serialize().contains("CertificateFile"));
}

#[test]
fn set_host_certificate_file_returns_false_when_alias_missing() {
    let mut config = parse_str("Host alpha\n  HostName 10.0.0.1\n");
    assert!(!config.set_host_certificate_file("ghost", "/tmp/cert.pub"));
    // Config unchanged
    assert_eq!(config.serialize(), "Host alpha\n  HostName 10.0.0.1\n");
}

#[test]
fn set_host_certificate_file_ignores_match_blocks() {
    // Match blocks are stored as GlobalLines; a `CertificateFile` directive
    // inside a Match block is never the target of set_host_certificate_file,
    // even if the pattern would "match" the alias.
    //
    // Uses a real purple-managed path (`~/.purple/certs/...`) so the gate
    // in `set_host_certificate_file` allows the insert. A non-purple path
    // with no existing purple line is a no-op by design (see
    // `set_host_certificate_file_non_purple_path_is_noop_when_no_purple_line`).
    let input = "\
Host alpha
  HostName 10.0.0.1

Match host alpha
  CertificateFile /user/set/match-cert.pub
";
    let mut config = parse_str(input);
    assert!(config.set_host_certificate_file("alpha", "~/.purple/certs/alpha-cert.pub"));
    let out = config.serialize();
    // Top-level alpha block got the directive
    assert!(out.contains(
        "Host alpha\n  HostName 10.0.0.1\n  CertificateFile ~/.purple/certs/alpha-cert.pub"
    ));
    // Match block's own CertificateFile is untouched
    assert!(out.contains("Match host alpha\n  CertificateFile /user/set/match-cert.pub"));
}

#[test]
fn set_vault_ssh_twice_replaces_not_appends() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    assert!(config.set_host_vault_ssh("myserver", "ssh/sign/one"));
    assert!(config.set_host_vault_ssh("myserver", "ssh/sign/two"));
    let serialized = config.serialize();
    assert_eq!(
        serialized.matches("purple:vault-ssh").count(),
        1,
        "expected a single comment after two calls, got: {}",
        serialized
    );
    assert!(serialized.contains("purple:vault-ssh ssh/sign/two"));
}

#[test]
fn vault_ssh_indentation_preserved_with_other_purple_comments() {
    let input = "Host myserver\n    HostName 10.0.0.1\n    # purple:tags prod,web\n";
    let mut config = parse_str(input);
    assert!(config.set_host_vault_ssh("myserver", "ssh/sign/engineer"));
    let serialized = config.serialize();
    assert!(
        serialized.contains("    # purple:vault-ssh ssh/sign/engineer"),
        "indent preserved: {}",
        serialized
    );
    assert!(serialized.contains("    # purple:tags prod,web"));
}

#[test]
fn clear_vault_ssh_removes_comment_line() {
    let mut config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:vault-ssh ssh/sign/old\n");
    assert!(config.set_host_vault_ssh("myserver", ""));
    let serialized = config.serialize();
    assert!(
        !serialized.contains("vault-ssh"),
        "comment should be gone: {}",
        serialized
    );
    assert!(first_block(&config).vault_ssh().is_none());
}

#[test]
fn set_vault_ssh_removes_duplicate_comments() {
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:vault-ssh ssh/sign/old1\n  # purple:vault-ssh ssh/sign/old2\n",
    );
    assert!(config.set_host_vault_ssh("myserver", "ssh/sign/new"));
    assert_eq!(
        config.serialize().matches("purple:vault-ssh").count(),
        1,
        "Should have exactly one vault-ssh comment after set"
    );
    assert_eq!(
        first_block(&config).vault_ssh(),
        Some("ssh/sign/new".to_string())
    );
}

// Regression tests: multi-alias Host blocks must be addressable from any
// of their whitespace-separated tokens, for both reads and writes. Before
// the `find_host_block_mut` helper, writers compared the full
// `host_pattern` for exact equality, which silently no-op'd on blocks like
// `Host web-01 web-01.prod 10.0.1.5`. Users saw the host in the TUI, hit
// edit or delete, and purple accepted the interaction while the on-disk
// config stayed untouched.

#[test]
fn delete_host_strips_single_alias_from_multi_alias_block() {
    let input = "Host web-01 web-01.prod 10.0.1.5\n  HostName 10.0.1.5\n  User deploy\n";
    let mut config = parse_str(input);
    config.delete_host("web-01.prod");
    let output = config.serialize();
    assert!(
        output.contains("Host web-01 10.0.1.5"),
        "sibling aliases must survive: {}",
        output
    );
    assert!(
        !output.contains("web-01.prod"),
        "deleted alias must be gone: {}",
        output
    );
    // Directives untouched: they still apply to the remaining aliases.
    assert!(output.contains("HostName 10.0.1.5"));
    assert!(output.contains("User deploy"));
    // has_host reflects the on-disk state.
    assert!(!config.has_host("web-01.prod"));
    assert!(config.has_host("web-01"));
    assert!(config.has_host("10.0.1.5"));
}

#[test]
fn delete_host_removes_block_when_last_alias_is_stripped() {
    // After repeated strips the block empties out; final strip removes the
    // whole block so we do not leave behind a dangling `Host ` line.
    let input = "Host alpha beta\n  User deploy\n";
    let mut config = parse_str(input);
    config.delete_host("alpha");
    config.delete_host("beta");
    let output = config.serialize();
    assert!(!output.contains("Host "), "block must be gone: {}", output);
    assert!(!config.has_host("alpha"));
    assert!(!config.has_host("beta"));
}

#[test]
fn delete_host_single_alias_block_still_removes_entire_block() {
    // Regression guard: single-alias behaviour must match the pre-refactor
    // semantics (whole block removed plus blank-line collapse).
    let input = "Host alpha\n  User a\n\nHost beta\n  User b\n";
    let mut config = parse_str(input);
    config.delete_host("alpha");
    let output = config.serialize();
    assert!(!output.contains("Host alpha"));
    assert!(output.contains("Host beta"));
    assert!(output.contains("User b"));
}

#[test]
fn update_host_rename_on_multi_alias_replaces_only_matching_token() {
    let input = "Host web-01 web-01.prod 10.0.1.5\n  HostName 10.0.1.5\n  User deploy\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "web-prod".to_string(),
        hostname: "10.0.1.5".to_string(),
        user: "deploy".to_string(),
        ..Default::default()
    };
    config.update_host("web-01.prod", &renamed);
    let output = config.serialize();
    assert!(
        output.contains("Host web-01 web-prod 10.0.1.5"),
        "only the renamed token must change: {}",
        output
    );
    assert!(!output.contains("web-01.prod"));
    assert!(config.has_host("web-prod"));
    assert!(config.has_host("web-01"));
    assert!(config.has_host("10.0.1.5"));
}

#[test]
fn update_host_field_on_multi_alias_affects_all_siblings() {
    // Per SSH semantics all tokens in a Host line share the same directives;
    // updating a non-alias field via any token must therefore apply to the
    // whole block (not silently no-op as the pre-refactor == match did).
    let input = "Host web-01 web-01.prod\n  User old\n";
    let mut config = parse_str(input);
    let entry = HostEntry {
        alias: "web-01.prod".to_string(),
        user: "deploy".to_string(),
        ..Default::default()
    };
    config.update_host("web-01.prod", &entry);
    let output = config.serialize();
    assert!(output.contains("User deploy"));
    assert!(!output.contains("User old"));
    assert!(output.contains("Host web-01 web-01.prod"));
}

#[test]
fn set_host_tags_refuses_multi_alias_block() {
    // ExactAliasOnly policy (symmetric with vault/cert setters): writing
    // tags onto a shared `Host web-01 web-01.prod` block would silently
    // apply to every sibling alias, so the setter refuses and returns false.
    let input = "Host web-01 web-01.prod\n  User deploy\n";
    let mut config = parse_str(input);
    let ok = config.set_host_tags("web-01.prod", &["prod".to_string(), "web".to_string()]);
    assert!(!ok, "set_host_tags must refuse multi-alias blocks");
    let output = config.serialize();
    assert!(
        !output.contains("purple:tags"),
        "no tags comment must be written: {}",
        output
    );
}

#[test]
fn set_host_provider_refuses_multi_alias_block() {
    // Claiming `web-01` as a Hetzner host would silently extend ownership to
    // `web-01.prod`, then sync's stale-mark + purge flow could delete the
    // user's hand-curated sibling. Refuse the mutation.
    let input = "Host web-01 web-01.prod\n  User deploy\n";
    let mut config = parse_str(input);
    let ok = config.set_host_provider_id(
        "web-01",
        &crate::providers::config::ProviderConfigId::bare("hetzner"),
        "12345",
    );
    assert!(!ok, "set_host_provider_id must refuse multi-alias blocks");
    let block = first_block(&config);
    assert_eq!(block.provider(), None);
}

#[test]
fn set_host_certificate_file_refuses_multi_alias_block() {
    // ExactAliasOnly policy: a multi-alias block is refused even though the
    // input alias itself is a clean token, because writing `CertificateFile`
    // would silently apply to every sibling alias.
    let input = "Host web-01 web-01.prod\n  User deploy\n";
    let mut config = parse_str(input);
    let ok = config.set_host_certificate_file("web-01", "/tmp/cert.pub");
    assert!(
        !ok,
        "set_host_certificate_file must refuse multi-alias blocks"
    );
    assert!(!config.serialize().contains("CertificateFile"));
}

#[test]
fn set_host_vault_addr_refuses_multi_alias_block() {
    let input = "Host web-01 web-01.prod\n  User deploy\n";
    let mut config = parse_str(input);
    let ok = config.set_host_vault_addr("web-01", "https://vault.example.com:8200");
    assert!(!ok, "set_host_vault_addr must refuse multi-alias blocks");
    assert!(!config.serialize().contains("purple:vault-addr"));
}

#[test]
fn siblings_of_returns_other_tokens_in_multi_alias_block() {
    let config = parse_str("Host web-01 web-01.prod 10.0.1.5\n  HostName 10.0.1.5\n");
    let siblings = config.siblings_of("web-01.prod");
    assert_eq!(siblings, vec!["web-01".to_string(), "10.0.1.5".to_string()]);
}

#[test]
fn siblings_of_returns_empty_for_single_alias_block() {
    let config = parse_str("Host solo\n  HostName 1.2.3.4\n");
    assert!(config.siblings_of("solo").is_empty());
}

#[test]
fn siblings_of_returns_empty_for_full_pattern_match() {
    // Full-pattern match means the caller targets the whole block (pattern
    // browser). All tokens are the target so there are no siblings to keep.
    let config = parse_str("Host web-01 web-01.prod\n  HostName 10.0.0.1\n");
    assert!(config.siblings_of("web-01 web-01.prod").is_empty());
}

#[test]
fn siblings_of_returns_empty_for_unknown_alias() {
    let config = parse_str("Host solo\n  HostName 1.2.3.4\n");
    assert!(config.siblings_of("nonexistent").is_empty());
    assert!(config.siblings_of("").is_empty());
}

#[test]
fn delete_host_full_pattern_still_removes_whole_block() {
    // Pattern browser delete: the caller passes the full host_pattern as
    // alias. The whole block must be removed, not token-stripped.
    let input = "Host web-01 web-01.prod\n  HostName shared.com\n\nHost other\n  User root\n";
    let mut config = parse_str(input);
    config.delete_host("web-01 web-01.prod");
    let output = config.serialize();
    assert!(!output.contains("web-01"));
    assert!(!output.contains("web-01.prod"));
    assert!(output.contains("Host other"));
}

#[test]
fn update_host_full_pattern_rename_replaces_whole_pattern() {
    // Pattern browser rename with a multi-token pattern must replace the
    // whole host_pattern, not try to rename a single token.
    let input = "Host web-01 web-01.prod\n  User root\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "prod".to_string(),
        user: "root".to_string(),
        ..Default::default()
    };
    config.update_host("web-01 web-01.prod", &renamed);
    let output = config.serialize();
    assert!(output.contains("Host prod"));
    assert!(!output.contains("web-01"));
}

// =========================================================================
// Rename preservation: `# purple:*` comments and unknown directives must
// survive a rename byte-for-byte. `update_host` rewrites only the `Host`
// line of the matched block; the block body must stay untouched. These
// tests are the load-bearing guarantee for the per-host state migration
// in `App::apply_alias_renames`. SSH config is a USP and round-trip
// fidelity across a rename is non-negotiable.
//
// `# purple:*` comment vocabulary used in this section, all defined in
// `src/ssh_config/host_block.rs`:
//   askpass, vault-ssh, vault-addr, tags, provider_tags, provider, meta,
//   stale. Tunnels are stored as plain `LocalForward`/`RemoteForward`/
//   `DynamicForward` SSH directives in the block body, not as comments.
// =========================================================================

#[test]
fn update_host_rename_preserves_forward_directives() {
    // Tunnels live as plain SSH forward directives. Rename must keep
    // every forward intact, in source order, with original spacing.
    let input = "Host web\n  HostName 10.0.0.1\n  LocalForward 5432 db.internal:5432\n  RemoteForward 9090 localhost:3000\n  DynamicForward 1080\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "web-prod".to_string(),
        hostname: "10.0.0.1".to_string(),
        ..Default::default()
    };
    config.update_host("web", &renamed);
    let output = config.serialize();
    assert!(output.contains("Host web-prod"));
    assert!(output.contains("LocalForward 5432 db.internal:5432"));
    assert!(output.contains("RemoteForward 9090 localhost:3000"));
    assert!(output.contains("DynamicForward 1080"));
    // Order: LocalForward before RemoteForward before DynamicForward.
    let local_pos = output.find("LocalForward").unwrap();
    let remote_pos = output.find("RemoteForward").unwrap();
    let dynamic_pos = output.find("DynamicForward").unwrap();
    assert!(local_pos < remote_pos && remote_pos < dynamic_pos);
}

#[test]
fn update_host_rename_preserves_purple_vault_ssh_and_addr() {
    let input = "Host bastion\n  HostName 10.0.0.1\n  # purple:vault-ssh prod-role\n  # purple:vault-addr https://vault.internal:8200\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "bastion-eu".to_string(),
        hostname: "10.0.0.1".to_string(),
        ..Default::default()
    };
    config.update_host("bastion", &renamed);
    let output = config.serialize();
    assert!(output.contains("Host bastion-eu"));
    assert!(
        output.contains("# purple:vault-ssh prod-role"),
        "vault-ssh role must survive: {output}"
    );
    assert!(
        output.contains("# purple:vault-addr https://vault.internal:8200"),
        "vault-addr override must survive: {output}"
    );
}

#[test]
fn update_host_rename_preserves_purple_askpass_each_source() {
    // All four askpass source families: keychain, op:// (1Password),
    // bw: (Bitwarden), vault: (HashiCorp Vault KV). Each must survive
    // a rename byte-for-byte.
    let cases = [
        "keychain",
        "op://Vault/Item/password",
        "bw:my-ssh-server",
        "vault:secret/data/ssh/web#password",
    ];
    for askpass in cases {
        let input = format!("Host web\n  HostName 1.2.3.4\n  # purple:askpass {askpass}\n");
        let mut config = parse_str(&input);
        let renamed = HostEntry {
            alias: "web-new".to_string(),
            hostname: "1.2.3.4".to_string(),
            ..Default::default()
        };
        config.update_host("web", &renamed);
        let output = config.serialize();
        let comment = format!("# purple:askpass {askpass}");
        assert!(
            output.contains(&comment),
            "askpass comment {askpass:?} must survive: {output}"
        );
        assert!(output.contains("Host web-new"));
    }
}

#[test]
fn update_host_rename_preserves_purple_tags_provider_tags_provider() {
    let input = "Host vm-01\n  HostName 10.0.0.1\n  # purple:tags ops,critical\n  # purple:provider_tags prod,europe\n  # purple:provider hetzner 12345\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "vm-frankfurt".to_string(),
        hostname: "10.0.0.1".to_string(),
        ..Default::default()
    };
    config.update_host("vm-01", &renamed);
    let output = config.serialize();
    assert!(output.contains("Host vm-frankfurt"));
    assert!(output.contains("# purple:tags ops,critical"));
    assert!(output.contains("# purple:provider_tags prod,europe"));
    assert!(output.contains("# purple:provider hetzner 12345"));
}

#[test]
fn update_host_rename_preserves_purple_meta_and_stale() {
    let input = "Host vm\n  HostName 1.2.3.4\n  # purple:meta region=eu-west-1,instance=t3.micro\n  # purple:stale 1700000000\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "vm-eu".to_string(),
        hostname: "1.2.3.4".to_string(),
        ..Default::default()
    };
    config.update_host("vm", &renamed);
    let output = config.serialize();
    assert!(output.contains("Host vm-eu"));
    assert!(
        output.contains("# purple:meta region=eu-west-1,instance=t3.micro"),
        "meta comment must survive: {output}"
    );
    assert!(
        output.contains("# purple:stale 1700000000"),
        "stale comment must survive: {output}"
    );
}

#[test]
fn update_host_rename_preserves_unknown_directives_in_order() {
    // Directives purple does not know about (Compression, ForwardAgent,
    // ServerAliveInterval, IdentitiesOnly, etc.) must round-trip across
    // a rename. Order matters: SSH applies directives top-down, so the
    // writer must not reshuffle.
    let input = "Host gpu-01\n  HostName 1.2.3.4\n  ServerAliveInterval 60\n  ServerAliveCountMax 3\n  Compression yes\n  ForwardAgent yes\n  IdentitiesOnly no\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "gpu-prod".to_string(),
        hostname: "1.2.3.4".to_string(),
        ..Default::default()
    };
    config.update_host("gpu-01", &renamed);
    let output = config.serialize();
    let body_start = output.find("Host gpu-prod").expect("renamed host present");
    let body = &output[body_start..];
    let interval_pos = body
        .find("ServerAliveInterval 60")
        .expect("interval present");
    let countmax_pos = body
        .find("ServerAliveCountMax 3")
        .expect("countmax present");
    let compression_pos = body.find("Compression yes").expect("compression present");
    let forward_pos = body.find("ForwardAgent yes").expect("forward present");
    let identities_pos = body
        .find("IdentitiesOnly no")
        .expect("identities-only present");
    assert!(
        interval_pos < countmax_pos
            && countmax_pos < compression_pos
            && compression_pos < forward_pos
            && forward_pos < identities_pos,
        "directive order must be preserved: {body}"
    );
}

#[test]
fn update_host_rename_preserves_blank_lines_and_inline_comments() {
    // Authors interleave blank lines and user comments inside a Host
    // block to group related directives. Round-trip fidelity demands
    // both survive a rename.
    let input = "Host db\n  HostName 10.0.0.1\n\n  # Use shared bastion\n  ProxyJump bastion\n\n  User deploy\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "db-prod".to_string(),
        hostname: "10.0.0.1".to_string(),
        user: "deploy".to_string(),
        proxy_jump: "bastion".to_string(),
        ..Default::default()
    };
    config.update_host("db", &renamed);
    let output = config.serialize();
    assert!(output.contains("Host db-prod"));
    assert!(
        output.contains("# Use shared bastion"),
        "user comment inside the block must survive: {output}"
    );
    assert!(output.contains("ProxyJump bastion"));
}

#[test]
fn update_host_rename_full_round_trip_reparse_preserves_purple_metadata() {
    // Parse → rename → serialize → reparse. The reparsed model must
    // expose every `# purple:*` accessor with the same value as the
    // original. Strongest possible guarantee that the writer did not
    // mutate anything in the body.
    let input = "Host web\n  HostName 10.0.0.1\n  User deploy\n  LocalForward 5432 db.internal:5432\n  # purple:vault-ssh prod-role\n  # purple:vault-addr https://vault.internal:8200\n  # purple:askpass keychain\n  # purple:tags ops,critical\n  # purple:provider_tags prod,europe\n  # purple:meta region=eu-west-1\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "web-prod".to_string(),
        hostname: "10.0.0.1".to_string(),
        user: "deploy".to_string(),
        ..Default::default()
    };
    config.update_host("web", &renamed);
    let serialized = config.serialize();
    let reparsed = parse_str(&serialized);
    let block = first_block(&reparsed);
    assert_eq!(block.host_pattern, "web-prod");
    assert_eq!(block.askpass().as_deref(), Some("keychain"));
    assert_eq!(block.vault_ssh().as_deref(), Some("prod-role"));
    assert_eq!(
        block.vault_addr().as_deref(),
        Some("https://vault.internal:8200")
    );
    assert_eq!(
        block.tags(),
        vec!["ops".to_string(), "critical".to_string()]
    );
    assert_eq!(
        block.provider_tags(),
        vec!["prod".to_string(), "europe".to_string()]
    );
    let tunnels = block.tunnel_directives();
    assert_eq!(tunnels.len(), 1);
    assert_eq!(tunnels[0].bind_port, 5432);
    assert_eq!(tunnels[0].remote_host, "db.internal");
    assert_eq!(tunnels[0].remote_port, 5432);
}

#[test]
fn update_host_rename_on_multi_alias_preserves_block_body() {
    // Token rename in a multi-alias block must not disturb the body.
    // The sibling alias `web-01` keeps the same directives and `# purple:*`
    // comments after `web-prod` is renamed to `web-eu`.
    let input = "Host web-01 web-prod\n  HostName 10.0.0.1\n  User deploy\n  LocalForward 5432 db.internal:5432\n  # purple:tags ops,critical\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "web-eu".to_string(),
        hostname: "10.0.0.1".to_string(),
        user: "deploy".to_string(),
        ..Default::default()
    };
    config.update_host("web-prod", &renamed);
    let output = config.serialize();
    assert!(
        output.contains("Host web-01 web-eu"),
        "only matching token renamed: {output}"
    );
    assert!(output.contains("LocalForward 5432 db.internal:5432"));
    assert!(output.contains("# purple:tags ops,critical"));
    assert!(output.contains("User deploy"));
}

#[test]
fn update_host_rename_pattern_preserves_block_body() {
    // Pattern rename keeps every directive and `# purple:*` comment.
    let input = "Host *.prod\n  User deploy\n  ProxyJump bastion\n  # purple:tags prod\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "*.qa".to_string(),
        user: "deploy".to_string(),
        proxy_jump: "bastion".to_string(),
        ..Default::default()
    };
    config.update_host("*.prod", &renamed);
    let output = config.serialize();
    assert!(output.contains("Host *.qa"));
    assert!(!output.contains("Host *.prod"));
    assert!(output.contains("ProxyJump bastion"));
    assert!(output.contains("# purple:tags prod"));
}

#[test]
fn update_host_rename_preserves_neighbouring_blocks_untouched() {
    // A rename on `web` must not touch the `db` block that follows.
    let input = "Host web\n  HostName 1.1.1.1\n  # purple:tags web\n\nHost db\n  HostName 2.2.2.2\n  # purple:vault-ssh db-role\n";
    let mut config = parse_str(input);
    let renamed = HostEntry {
        alias: "web-new".to_string(),
        hostname: "1.1.1.1".to_string(),
        ..Default::default()
    };
    config.update_host("web", &renamed);
    let output = config.serialize();
    assert!(output.contains("Host web-new"));
    assert!(output.contains("Host db\n"));
    assert!(output.contains("HostName 2.2.2.2"));
    assert!(output.contains("# purple:vault-ssh db-role"));
}

#[test]
fn update_host_rename_preserves_crlf_line_endings() {
    // Windows-authored configs use CRLF. After a rename the writer
    // must keep CRLF intact; mixing LF and CRLF corrupts editors that
    // detect line endings from the first line.
    let input = "Host web\r\n  HostName 10.0.0.1\r\n  # purple:tags ops\r\n";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(input),
        path: test_config_path(),
        crlf: true,
        bom: false,
    };
    let renamed = HostEntry {
        alias: "web-new".to_string(),
        hostname: "10.0.0.1".to_string(),
        ..Default::default()
    };
    config.update_host("web", &renamed);
    let output = config.serialize();
    assert!(output.contains("Host web-new"));
    assert!(
        output.contains("\r\n"),
        "CRLF line endings must be preserved: {output:?}"
    );
    // Every LF must be paired with a leading CR. Counting both bytes
    // catches a mixed-ending corruption that text-only `contains`
    // probes would miss.
    let cr_count = output.bytes().filter(|&b| b == b'\r').count();
    let lf_count = output.bytes().filter(|&b| b == b'\n').count();
    assert_eq!(
        cr_count, lf_count,
        "CRLF must be paired: cr={cr_count} lf={lf_count} body={output:?}"
    );
    assert!(output.contains("# purple:tags ops"));
}

#[test]
fn update_host_rename_preserves_bom_prefix() {
    // Some Windows editors prepend a UTF-8 BOM. The writer must keep
    // the BOM exactly once at the start of the file; a missing or
    // duplicated BOM corrupts the file for those editors.
    let input = "Host web\n  HostName 10.0.0.1\n  # purple:tags ops\n";
    let mut config = SshConfigFile {
        elements: SshConfigFile::parse_content(input),
        path: test_config_path(),
        crlf: false,
        bom: true,
    };
    let renamed = HostEntry {
        alias: "web-new".to_string(),
        hostname: "10.0.0.1".to_string(),
        ..Default::default()
    };
    config.update_host("web", &renamed);
    let output = config.serialize();
    assert!(
        output.starts_with('\u{feff}'),
        "BOM must remain at the start: bytes start with {:?}",
        output.chars().take(2).collect::<String>()
    );
    let trailing = &output[output.char_indices().nth(1).map(|(i, _)| i).unwrap_or(0)..];
    assert!(
        !trailing.contains('\u{feff}'),
        "BOM must appear exactly once: {output:?}"
    );
    assert!(output.contains("Host web-new"));
    assert!(output.contains("# purple:tags ops"));
}

#[test]
fn delete_host_undoable_refuses_multi_alias_block() {
    // Undoable delete returns `None` for multi-alias blocks because
    // re-inserting the whole element via `insert_host_at` would not reverse
    // a token-strip. The caller should fall back to `delete_host` (which
    // strips the token safely) and skip the undo stack entry.
    let input = "Host web-01 web-01.prod\n  User deploy\n";
    let mut config = parse_str(input);
    let undo = config.delete_host_undoable("web-01.prod");
    assert!(undo.is_none());
    // Config untouched by the refused undoable delete.
    assert!(config.has_host("web-01.prod"));
    assert!(config.has_host("web-01"));
}

// --- ProviderConfigId marker tests ---

#[test]
fn marker_legacy_two_segment_parses_as_bare() {
    let content = "Host server\n  HostName 1.2.3.4\n  # purple:provider digitalocean:12345\n";
    let config = parse_str(content);
    let block = match &config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    let (id, server_id) = block.provider_id().unwrap();
    assert_eq!(id.provider, "digitalocean");
    assert_eq!(id.label, None);
    assert_eq!(server_id, "12345");
}

#[test]
fn marker_three_segment_parses_as_labeled() {
    let content = "Host server\n  HostName 1.2.3.4\n  # purple:provider digitalocean:work:12345\n";
    let config = parse_str(content);
    let block = match &config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    let (id, server_id) = block.provider_id().unwrap();
    assert_eq!(id.provider, "digitalocean");
    assert_eq!(id.label.as_deref(), Some("work"));
    assert_eq!(server_id, "12345");
}

#[test]
fn marker_three_segment_with_invalid_label_falls_back_to_legacy() {
    // Middle segment that fails label validation (uppercase) is treated as
    // part of the server_id, not as a label.
    let content = "Host server\n  HostName 1.2.3.4\n  # purple:provider azure:RES:12345\n";
    let config = parse_str(content);
    let block = match &config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    let (id, server_id) = block.provider_id().unwrap();
    assert_eq!(id.provider, "azure");
    assert_eq!(id.label, None);
    assert_eq!(server_id, "RES:12345");
}

#[test]
fn set_provider_id_emits_two_segment_when_bare() {
    use crate::providers::config::ProviderConfigId;
    let content = "Host server\n  HostName 1.2.3.4\n";
    let mut config = parse_str(content);
    let block = match &mut config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    block.set_provider_id(&ProviderConfigId::bare("aws"), "i-abc");
    let serialized = config.serialize();
    assert!(serialized.contains("# purple:provider aws:i-abc"));
    assert!(!serialized.contains("aws::"));
}

#[test]
fn set_provider_id_emits_three_segment_when_labeled() {
    use crate::providers::config::ProviderConfigId;
    let content = "Host server\n  HostName 1.2.3.4\n";
    let mut config = parse_str(content);
    let block = match &mut config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    block.set_provider_id(&ProviderConfigId::labeled("aws", "work"), "i-abc");
    let serialized = config.serialize();
    assert!(serialized.contains("# purple:provider aws:work:i-abc"));
}

#[test]
fn marker_round_trip_labeled() {
    use crate::providers::config::ProviderConfigId;
    let content = "Host server\n  HostName 1.2.3.4\n";
    let mut config = parse_str(content);
    let id = ProviderConfigId::labeled("digitalocean", "personal");
    if let ConfigElement::HostBlock(b) = &mut config.elements[0] {
        b.set_provider_id(&id, "67890");
    }
    let serialized = config.serialize();
    let reparsed = parse_str(&serialized);
    let block = match &reparsed.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    let (parsed_id, server_id) = block.provider_id().unwrap();
    assert_eq!(parsed_id, id);
    assert_eq!(server_id, "67890");
}

#[test]
fn provider_legacy_getter_drops_label() {
    // The legacy `provider()` getter must still return (provider, server_id)
    // for code paths that don't care about the label.
    let content = "Host server\n  HostName 1.2.3.4\n  # purple:provider aws:work:i-12345\n";
    let config = parse_str(content);
    let block = match &config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    let (name, server_id) = block.provider().unwrap();
    assert_eq!(name, "aws");
    assert_eq!(server_id, "i-12345");
}

#[test]
fn marker_three_segment_keeps_server_id_with_embedded_colon() {
    // Some provider server_ids may contain ':' (Azure, OCI). The splitn(3)
    // contract is that the LAST segment carries the entire server_id even
    // if it has more colons. Without this, a path-like id would be torn.
    let content = "Host vm\n  HostName 1.2.3.4\n  # purple:provider aws:work:us-east-1:i-abc\n";
    let config = parse_str(content);
    let block = match &config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    let (id, server_id) = block.provider_id().unwrap();
    assert_eq!(id.provider, "aws");
    assert_eq!(id.label.as_deref(), Some("work"));
    assert_eq!(server_id, "us-east-1:i-abc");
}

#[test]
fn host_entry_carries_provider_label_for_three_segment_marker() {
    let content = "Host vm\n  HostName 1.2.3.4\n  # purple:provider aws:work:i-12345\n";
    let config = parse_str(content);
    let entries = config.host_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].provider.as_deref(), Some("aws"));
    assert_eq!(entries[0].provider_label.as_deref(), Some("work"));
}

#[test]
fn host_entry_provider_label_is_none_for_legacy_marker() {
    let content = "Host vm\n  HostName 1.2.3.4\n  # purple:provider aws:i-12345\n";
    let config = parse_str(content);
    let entries = config.host_entries();
    assert_eq!(entries[0].provider.as_deref(), Some("aws"));
    assert_eq!(entries[0].provider_label, None);
}

// --- Malformed marker safety ---

#[test]
fn marker_empty_middle_segment_returns_none() {
    // `aws::123` has an empty label: neither a valid labeled marker nor
    // unambiguous as legacy. Must be treated as malformed (host unowned)
    // rather than guessing a server_id with a leading colon.
    let content = "Host vm\n  HostName 1.2.3.4\n  # purple:provider aws::123\n";
    let config = parse_str(content);
    let block = match &config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    assert_eq!(
        block.provider_id(),
        None,
        "empty middle segment must not be claimed by any provider"
    );
}

#[test]
fn marker_empty_server_id_returns_none() {
    let content = "Host vm\n  HostName 1.2.3.4\n  # purple:provider aws:work:\n";
    let config = parse_str(content);
    let block = match &config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    assert_eq!(block.provider_id(), None);
}

#[test]
fn marker_legacy_empty_server_id_returns_none() {
    let content = "Host vm\n  HostName 1.2.3.4\n  # purple:provider aws:\n";
    let config = parse_str(content);
    let block = match &config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    assert_eq!(block.provider_id(), None);
}

#[test]
fn marker_no_colon_returns_none() {
    // Single token after the prefix is not a valid marker.
    let content = "Host vm\n  HostName 1.2.3.4\n  # purple:provider aws\n";
    let config = parse_str(content);
    let block = match &config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    assert_eq!(block.provider_id(), None);
}

#[test]
fn marker_empty_provider_returns_none() {
    let content = "Host vm\n  HostName 1.2.3.4\n  # purple:provider :work:123\n";
    let config = parse_str(content);
    let block = match &config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    assert_eq!(block.provider_id(), None);
}

// --- set_provider_id overwrite + indentation preservation ---

#[test]
fn set_provider_id_replaces_existing_legacy_marker() {
    use crate::providers::config::ProviderConfigId;
    let content = "Host vm\n  HostName 1.2.3.4\n  # purple:provider aws:i-old\n";
    let mut config = parse_str(content);
    let block = match &mut config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    block.set_provider_id(&ProviderConfigId::labeled("aws", "work"), "i-new");
    let serialized = config.serialize();
    assert!(serialized.contains("# purple:provider aws:work:i-new"));
    assert!(
        !serialized.contains("aws:i-old"),
        "old legacy marker must be removed:\n{}",
        serialized
    );
    // Exactly one marker survives.
    assert_eq!(serialized.matches("# purple:provider").count(), 1);
}

#[test]
fn set_provider_id_replaces_existing_labeled_marker() {
    use crate::providers::config::ProviderConfigId;
    let content = "Host vm\n  HostName 1.2.3.4\n  # purple:provider aws:work:i-old\n";
    let mut config = parse_str(content);
    let block = match &mut config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    block.set_provider_id(&ProviderConfigId::labeled("aws", "personal"), "i-new");
    let serialized = config.serialize();
    assert!(serialized.contains("# purple:provider aws:personal:i-new"));
    assert!(!serialized.contains("aws:work"));
    assert_eq!(serialized.matches("# purple:provider").count(), 1);
}

#[test]
fn set_provider_id_can_downgrade_labeled_to_bare() {
    use crate::providers::config::ProviderConfigId;
    let content = "Host vm\n  HostName 1.2.3.4\n  # purple:provider aws:work:i-old\n";
    let mut config = parse_str(content);
    let block = match &mut config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    block.set_provider_id(&ProviderConfigId::bare("aws"), "i-new");
    let serialized = config.serialize();
    assert!(serialized.contains("# purple:provider aws:i-new"));
    assert!(!serialized.contains("aws:work"));
}

#[test]
fn set_provider_id_preserves_tab_indent() {
    use crate::providers::config::ProviderConfigId;
    let content = "Host vm\n\tHostName 1.2.3.4\n\t# purple:provider aws:i-old\n";
    let mut config = parse_str(content);
    let block = match &mut config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    block.set_provider_id(&ProviderConfigId::labeled("aws", "work"), "i-new");
    let serialized = config.serialize();
    // Marker must be tab-indented like the surrounding directives.
    assert!(
        serialized.contains("\t# purple:provider aws:work:i-new"),
        "tab indent not preserved:\n{}",
        serialized
    );
}

#[test]
fn set_provider_id_preserves_4space_indent() {
    use crate::providers::config::ProviderConfigId;
    let content = "Host vm\n    HostName 1.2.3.4\n    # purple:provider aws:i-old\n";
    let mut config = parse_str(content);
    let block = match &mut config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    block.set_provider_id(&ProviderConfigId::labeled("aws", "work"), "i-new");
    let serialized = config.serialize();
    assert!(
        serialized.contains("    # purple:provider aws:work:i-new"),
        "4-space indent not preserved:\n{}",
        serialized
    );
}

#[test]
fn set_provider_id_dedupes_existing_markers() {
    // Hand-edited config can have duplicate markers (rare, but possible).
    // set_provider_id retains only one.
    use crate::providers::config::ProviderConfigId;
    let content = "Host vm\n  HostName 1.2.3.4\n  # purple:provider aws:i-1\n  # purple:provider aws:work:i-2\n";
    let mut config = parse_str(content);
    let block = match &mut config.elements[0] {
        ConfigElement::HostBlock(b) => b,
        _ => panic!("expected HostBlock"),
    };
    block.set_provider_id(&ProviderConfigId::labeled("aws", "personal"), "i-final");
    let serialized = config.serialize();
    assert_eq!(
        serialized.matches("# purple:provider").count(),
        1,
        "duplicate markers must collapse to one:\n{}",
        serialized
    );
    assert!(serialized.contains("aws:personal:i-final"));
}

// --- find_hosts_by_id recurses into Include files ---

#[test]
fn find_hosts_by_id_finds_hosts_in_include_files() {
    use crate::providers::config::ProviderConfigId;
    let mut included = parse_str(
        "Host included-vm\n  HostName 5.5.5.5\n  # purple:provider aws:work:i-included\n",
    );
    included.path = std::path::PathBuf::from("/tmp/included");
    let main_content = "Host main-vm\n  HostName 1.1.1.1\n  # purple:provider aws:work:i-main\n";
    let mut main = parse_str(main_content);
    main.elements.push(ConfigElement::Include(
        crate::ssh_config::model::IncludeDirective {
            raw_line: "Include /tmp/included".to_string(),
            pattern: "/tmp/included".to_string(),
            resolved_files: vec![crate::ssh_config::model::IncludedFile {
                path: std::path::PathBuf::from("/tmp/included"),
                elements: included.elements,
            }],
        },
    ));
    let hosts = main.find_hosts_by_id(&ProviderConfigId::labeled("aws", "work"));
    let aliases: Vec<&str> = hosts.iter().map(|(a, _)| a.as_str()).collect();
    assert!(aliases.contains(&"main-vm"));
    assert!(
        aliases.contains(&"included-vm"),
        "find_hosts_by_id must recurse into Include files: {:?}",
        aliases
    );
}

#[test]
fn rewrite_legacy_markers_to_label_migrates_only_matching_provider() {
    let content = "Host a\n  HostName 1.1.1.1\n  # purple:provider digitalocean:1\n\nHost b\n  HostName 2.2.2.2\n  # purple:provider digitalocean:2\n\nHost c\n  HostName 3.3.3.3\n  # purple:provider aws:i-3\n\nHost d\n  HostName 4.4.4.4\n  # purple:provider digitalocean:work:9\n";
    let mut config = parse_str(content);
    let count = config.rewrite_legacy_markers_to_label("digitalocean", "default");
    assert_eq!(count, 2, "exactly two legacy DO markers must be rewritten");

    let serialized = config.serialize();
    assert!(serialized.contains("# purple:provider digitalocean:default:1"));
    assert!(serialized.contains("# purple:provider digitalocean:default:2"));
    // AWS untouched.
    assert!(serialized.contains("# purple:provider aws:i-3"));
    // Already-labeled DO host untouched (label "work" stays).
    assert!(serialized.contains("# purple:provider digitalocean:work:9"));
    // Legacy 2-segment DO markers must be gone.
    assert!(!serialized.contains("# purple:provider digitalocean:1\n"));
    assert!(!serialized.contains("# purple:provider digitalocean:2\n"));
}

#[test]
fn provider_label_survives_update_host() {
    // update_host preserves the existing # purple:provider marker if the
    // caller didn't override it. The 3-segment form must round-trip.
    let content = "Host vm\n  HostName 1.2.3.4\n  User old\n  # purple:provider aws:work:i-abc\n";
    let mut config = parse_str(content);
    let updated = HostEntry {
        alias: "vm".to_string(),
        hostname: "1.2.3.4".to_string(),
        user: "newuser".to_string(),
        port: 22,
        ..Default::default()
    };
    config.update_host("vm", &updated);
    let serialized = config.serialize();
    assert!(
        serialized.contains("# purple:provider aws:work:i-abc"),
        "labeled marker must survive update_host:\n{}",
        serialized
    );
    let entries = config.host_entries();
    assert_eq!(entries[0].provider.as_deref(), Some("aws"));
    assert_eq!(entries[0].provider_label.as_deref(), Some("work"));
    assert_eq!(entries[0].user, "newuser");
}

#[test]
fn provider_label_survives_swap_hosts() {
    let content = "Host a\n  HostName 1.1.1.1\n  # purple:provider aws:work:i-1\n\nHost b\n  HostName 2.2.2.2\n  # purple:provider aws:personal:i-2\n";
    let mut config = parse_str(content);
    config.swap_hosts("a", "b");
    let serialized = config.serialize();
    assert!(serialized.contains("# purple:provider aws:work:i-1"));
    assert!(serialized.contains("# purple:provider aws:personal:i-2"));
    let entries = config.host_entries();
    let a = entries.iter().find(|e| e.alias == "a").unwrap();
    let b = entries.iter().find(|e| e.alias == "b").unwrap();
    assert_eq!(a.provider_label.as_deref(), Some("work"));
    assert_eq!(b.provider_label.as_deref(), Some("personal"));
}

#[test]
fn provider_label_survives_delete_undo() {
    let content = "Host a\n  HostName 1.1.1.1\n  # purple:provider aws:work:i-1\n\nHost b\n  HostName 2.2.2.2\n";
    let mut config = parse_str(content);
    let undo = config.delete_host_undoable("a");
    assert!(undo.is_some());
    let (element, position) = undo.unwrap();
    config.insert_host_at(element, position);
    let serialized = config.serialize();
    assert!(
        serialized.contains("# purple:provider aws:work:i-1"),
        "labeled marker must survive delete + undo:\n{}",
        serialized
    );
    let entries = config.host_entries();
    let a = entries.iter().find(|e| e.alias == "a").unwrap();
    assert_eq!(a.provider_label.as_deref(), Some("work"));
}

#[test]
fn provider_label_survives_serialize_reparse() {
    let content = "Host a\n  HostName 1.1.1.1\n  # purple:provider aws:work:i-1\n  # purple:provider_tags prod,db\n  # purple:askpass keychain\n";
    let config = parse_str(content);
    let s1 = config.serialize();
    let reparsed = parse_str(&s1);
    let entries = reparsed.host_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].provider.as_deref(), Some("aws"));
    assert_eq!(entries[0].provider_label.as_deref(), Some("work"));
    assert_eq!(entries[0].provider_tags, vec!["prod", "db"]);
    assert_eq!(entries[0].askpass.as_deref(), Some("keychain"));
    assert_eq!(s1, reparsed.serialize());
}

#[test]
fn group_header_survives_one_of_many_label_deletion() {
    // When multiple labeled configs share one provider group header, hosts
    // from one labeled config disappear but the others' hosts remain. The
    // group header must NOT be removed.
    let content = "# purple:group AWS EC2\n\nHost aws-work-1\n  HostName 1.1.1.1\n  # purple:provider aws:work:i-1\n\nHost aws-personal-1\n  HostName 2.2.2.2\n  # purple:provider aws:personal:i-2\n";
    let mut config = parse_str(content);
    // Delete the work host (simulating that config's --remove sync).
    config.delete_host("aws-work-1");
    let serialized = config.serialize();
    assert!(
        serialized.contains("# purple:group AWS EC2"),
        "group header must remain while another labeled config still has hosts:\n{}",
        serialized
    );
    assert!(serialized.contains("aws-personal-1"));
    assert!(!serialized.contains("aws-work-1"));
}

#[test]
fn rewrite_legacy_markers_to_label_skips_include_files() {
    // Include files are read-only by project rule; the helper must not touch them.
    let mut included =
        parse_str("Host inc-vm\n  HostName 5.5.5.5\n  # purple:provider digitalocean:99\n");
    included.path = std::path::PathBuf::from("/tmp/included");
    let mut main =
        parse_str("Host top-vm\n  HostName 1.1.1.1\n  # purple:provider digitalocean:1\n");
    main.elements.push(ConfigElement::Include(
        crate::ssh_config::model::IncludeDirective {
            raw_line: "Include /tmp/included".to_string(),
            pattern: "/tmp/included".to_string(),
            resolved_files: vec![crate::ssh_config::model::IncludedFile {
                path: std::path::PathBuf::from("/tmp/included"),
                elements: included.elements,
            }],
        },
    ));
    let count = main.rewrite_legacy_markers_to_label("digitalocean", "default");
    assert_eq!(count, 1, "only top-level marker counted");
    let serialized = main.serialize();
    assert!(serialized.contains("# purple:provider digitalocean:default:1"));
}

#[test]
fn find_hosts_by_id_does_not_match_other_label_in_include() {
    use crate::providers::config::ProviderConfigId;
    let mut included = parse_str(
        "Host included-vm\n  HostName 5.5.5.5\n  # purple:provider aws:personal:i-other\n",
    );
    included.path = std::path::PathBuf::from("/tmp/included");
    let mut main = parse_str("Host main-vm\n  HostName 1.1.1.1\n");
    main.elements.push(ConfigElement::Include(
        crate::ssh_config::model::IncludeDirective {
            raw_line: "Include /tmp/included".to_string(),
            pattern: "/tmp/included".to_string(),
            resolved_files: vec![crate::ssh_config::model::IncludedFile {
                path: std::path::PathBuf::from("/tmp/included"),
                elements: included.elements,
            }],
        },
    ));
    let hosts = main.find_hosts_by_id(&ProviderConfigId::labeled("aws", "work"));
    assert!(
        hosts.is_empty(),
        "label scoping must hold across Include boundary: {:?}",
        hosts
    );
}

// ── Data-loss and injection regression tests ───────────────────────
// Each block below pins one way the parser or writer can lose user data
// or let an injected line become a live directive. Removing or weakening
// any of these tests re-opens the corresponding hole.

// Helper: every line of `serialized` must NOT, after re-parsing, become an
// active SSH directive for `key`. Comments containing the keyword in their
// text are fine; what we forbid is the parser seeing the injected line as
// a real directive (which it would, the moment a `\n` lands inside the
// purple-managed comment).
fn assert_no_directive_injection(serialized: &str, key: &str) {
    let reparsed = parse_str(serialized);
    for el in &reparsed.elements {
        if let ConfigElement::HostBlock(b) = el {
            for d in &b.directives {
                assert!(
                    d.is_non_directive || !d.key.eq_ignore_ascii_case(key),
                    "directive injection: {} became a real {} directive in serialized output:\n{}",
                    key,
                    key,
                    serialized
                );
            }
        }
    }
}

#[test]
fn set_provider_id_strips_newline_injection_from_server_id() {
    use crate::providers::config::ProviderConfigId;
    let mut config = parse_str("Host victim\n  HostName 10.0.0.1\n");
    let id = ProviderConfigId::bare("evilcloud");
    // Malicious server_id attempts to inject a ProxyCommand directive into
    // the user's config. The sanitiser must rewrite the line-breaking
    // characters before the value is interpolated into raw_line.
    let _ = config.set_host_provider_id("victim", &id, "1\n  ProxyCommand nc evil 22");
    let serialized = config.serialize();
    assert_no_directive_injection(&serialized, "ProxyCommand");
    // The marker stays on a single physical line.
    let marker_count = serialized.matches("# purple:provider").count();
    assert_eq!(marker_count, 1, "expected exactly one marker line");
}

#[test]
fn set_host_vault_ssh_strips_newline_injection_from_role() {
    let mut config = parse_str("Host victim\n  HostName 10.0.0.1\n");
    assert!(config.set_host_vault_ssh("victim", "ssh/sign/role\n  ProxyJump attacker"));
    let serialized = config.serialize();
    assert_no_directive_injection(&serialized, "ProxyJump");
}

#[test]
fn set_host_vault_addr_strips_newline_injection_from_url() {
    let mut config = parse_str("Host victim\n  HostName 10.0.0.1\n");
    assert!(config.set_host_vault_addr("victim", "http://v:8200\n  ProxyCommand evil"));
    let serialized = config.serialize();
    assert_no_directive_injection(&serialized, "ProxyCommand");
}

#[test]
fn set_host_askpass_strips_newline_injection_from_source() {
    let mut config = parse_str("Host victim\n  HostName 10.0.0.1\n");
    let _ = config.set_host_askpass("victim", "vault:secret#pass\n  ProxyJump evil");
    let serialized = config.serialize();
    assert_no_directive_injection(&serialized, "ProxyJump");
}

#[test]
fn entry_to_block_sanitizes_newlines_in_all_fields() {
    let entry = HostEntry {
        alias: "victim".to_string(),
        hostname: "10.0.0.1\n  ProxyJump attacker".to_string(),
        user: "deploy\n  ProxyCommand evil".to_string(),
        port: 22,
        identity_file: "~/.ssh/id\n  RemoteForward 22:bad:22".to_string(),
        proxy_jump: "host\n  ForwardAgent yes".to_string(),
        ..Default::default()
    };
    let block = SshConfigFile::entry_to_block(&entry);
    for d in &block.directives {
        assert!(
            !d.raw_line.contains('\n') && !d.raw_line.contains('\r'),
            "raw_line still contains line-breaking bytes: {:?}",
            d.raw_line
        );
    }
    assert!(!block.raw_host_line.contains('\n'));
}

#[test]
fn upsert_directive_empty_value_removes_only_first_identityfile() {
    // OpenSSH IdentityFile is cumulative: a host with three identities is
    // intentionally multi-key. Wiping all matching directives on an empty
    // form field is the C-001 data-loss bug.
    let input = "\
Host alpha
  HostName 10.0.0.1
  IdentityFile ~/.ssh/id_rsa
  IdentityFile ~/.ssh/id_ed25519
  IdentityFile ~/.ssh/id_ecdsa
";
    let mut config = parse_str(input);
    let mut entry = first_block(&config).to_host_entry();
    // Form returns empty identity_file (user cleared the field).
    entry.identity_file = String::new();
    config.update_host("alpha", &entry);
    let serialized = config.serialize();
    // The first IdentityFile is gone; the other two survive.
    assert!(
        !serialized.contains("id_rsa"),
        "first IdentityFile should be removed: {}",
        serialized
    );
    assert!(
        serialized.contains("IdentityFile ~/.ssh/id_ed25519"),
        "second IdentityFile must survive empty-field clear: {}",
        serialized
    );
    assert!(
        serialized.contains("IdentityFile ~/.ssh/id_ecdsa"),
        "third IdentityFile must survive empty-field clear: {}",
        serialized
    );
}

#[test]
fn set_host_certificate_file_preserves_user_set_corporate_cert() {
    // C-003 / C-004: vault sign must not overwrite a user-managed
    // CertificateFile. Purple-managed paths live under .purple/certs/.
    let input = "\
Host alpha
  HostName 10.0.0.1
  CertificateFile ~/.ssh/corporate-cert.pub
";
    let mut config = parse_str(input);
    assert!(config.set_host_certificate_file("alpha", "~/.purple/certs/alpha-cert.pub"));
    let serialized = config.serialize();
    assert!(
        serialized.contains("CertificateFile ~/.ssh/corporate-cert.pub"),
        "user-set corporate cert must survive vault sign: {}",
        serialized
    );
    assert!(
        serialized.contains("CertificateFile ~/.purple/certs/alpha-cert.pub"),
        "purple-managed cert must be added: {}",
        serialized
    );
}

#[test]
fn set_host_certificate_file_clear_preserves_user_cert() {
    // C-003 / C-004: clearing the vault role must remove only the
    // purple-managed cert line, never the user's own.
    let input = "\
Host alpha
  HostName 10.0.0.1
  CertificateFile ~/.ssh/corporate-cert.pub
  CertificateFile ~/.purple/certs/alpha-cert.pub
";
    let mut config = parse_str(input);
    assert!(config.set_host_certificate_file("alpha", ""));
    let serialized = config.serialize();
    assert!(
        serialized.contains("CertificateFile ~/.ssh/corporate-cert.pub"),
        "user cert must survive empty-path clear: {}",
        serialized
    );
    assert!(
        !serialized.contains(".purple/certs/"),
        "purple-managed cert must be removed: {}",
        serialized
    );
}

#[test]
fn set_host_certificate_file_replaces_existing_purple_managed() {
    let input = "\
Host alpha
  HostName 10.0.0.1
  CertificateFile ~/.ssh/corp.pub
  CertificateFile ~/.purple/certs/alpha-cert.pub
";
    let mut config = parse_str(input);
    assert!(config.set_host_certificate_file("alpha", "~/.purple/certs/alpha-cert-v2.pub"));
    let serialized = config.serialize();
    assert!(serialized.contains("CertificateFile ~/.ssh/corp.pub"));
    assert!(serialized.contains("CertificateFile ~/.purple/certs/alpha-cert-v2.pub"));
    assert!(!serialized.contains("alpha-cert.pub\n") && !serialized.ends_with("alpha-cert.pub"));
}

#[test]
fn set_host_vault_ssh_refuses_pattern_alias() {
    let mut config = parse_str("Host *.prod\n  HostName placeholder\n");
    assert!(!config.set_host_vault_ssh("*.prod", "ssh/sign/role"));
    let serialized = config.serialize();
    assert!(
        !serialized.contains("vault-ssh"),
        "pattern alias must not get a vault-ssh marker: {}",
        serialized
    );
}

#[test]
fn set_host_vault_ssh_refuses_multi_alias_block() {
    let input = "\
Host web-01 web-01.prod
  HostName 10.0.0.1
";
    let mut config = parse_str(input);
    // Even though the alias token exists, the matched block is multi-alias.
    // Applying the role would silently affect sibling aliases.
    assert!(!config.set_host_vault_ssh("web-01", "ssh/sign/role"));
    let serialized = config.serialize();
    assert!(
        !serialized.contains("vault-ssh"),
        "multi-alias block must not get a vault-ssh marker: {}",
        serialized
    );
}

#[test]
fn set_host_vault_ssh_returns_false_for_missing_alias() {
    let mut config = parse_str("Host alpha\n  HostName 10.0.0.1\n");
    assert!(!config.set_host_vault_ssh("ghost", "ssh/sign/role"));
    assert_eq!(config.serialize(), "Host alpha\n  HostName 10.0.0.1\n");
}

#[test]
fn parse_include_cycle_terminates_without_duplicate_hosts() {
    // C-004 / P-005: A→B→A must not produce 17 duplicate hosts via
    // MAX_INCLUDE_DEPTH explosion. The cycle detector skips files whose
    // canonical path is already up the include chain.
    let dir = tempfile::tempdir().expect("tempdir");
    let main_path = dir.path().join("config");
    let other_path = dir.path().join("other");
    std::fs::write(
        &main_path,
        format!(
            "Include {}\nHost from-main\n  HostName 1.1.1.1\n",
            other_path.display()
        ),
    )
    .unwrap();
    std::fs::write(
        &other_path,
        format!(
            "Include {}\nHost from-other\n  HostName 2.2.2.2\n",
            main_path.display()
        ),
    )
    .unwrap();

    let config = SshConfigFile::parse(&main_path).expect("parse must terminate");
    let entries = config.host_entries();
    let from_main = entries.iter().filter(|e| e.alias == "from-main").count();
    let from_other = entries.iter().filter(|e| e.alias == "from-other").count();
    assert_eq!(from_main, 1, "from-main duplicated: {:?}", entries);
    assert_eq!(
        from_other, 1,
        "from-other missing or duplicated: {:?}",
        entries
    );
}

#[test]
fn set_host_certificate_file_non_purple_path_is_noop_when_no_purple_line() {
    // C-004 rollback edge case: when `set_host_certificate_file` is called
    // with a user-managed path (NOT under .purple/certs/) on a block that
    // has no existing purple-managed entry, the function must NOT insert
    // a new CertificateFile line. The rollback flow in `app/hosts.rs`
    // passes `old_entry.certificate_file` which could be the user's own
    // path; inserting it would duplicate or fabricate a directive the
    // user never authored.
    let input = "Host alpha\n  HostName 10.0.0.1\n";
    let mut config = parse_str(input);
    assert!(config.set_host_certificate_file("alpha", "~/.ssh/personal-cert.pub"));
    let serialized = config.serialize();
    assert!(
        !serialized.contains("CertificateFile"),
        "non-purple path with no purple-managed line should be a no-op: {}",
        serialized
    );

    // But: a non-purple path WHEN a purple-managed line already exists
    // should still update the purple line (caller intent unclear; treat as
    // overwrite of the purple slot). Actually no: the lookup is by
    // is_purple_managed_cert_value on the EXISTING line, not the path
    // argument. So we'd overwrite the purple line's value with the
    // user-set path, marking it as non-purple. That's a separate edge
    // case we accept (caller responsibility).
}

#[test]
fn parse_self_include_cycle_terminates() {
    // Self-include: the top-level config includes itself. The cycle
    // detector inserts the top-level canonical path into `visited` in
    // `parse_with_depth` before recursing, so the Include should be
    // skipped on the first lookup.
    let dir = tempfile::tempdir().expect("tempdir");
    let self_path = dir.path().join("config");
    std::fs::write(
        &self_path,
        format!(
            "Include {}\nHost only-once\n  HostName 1.1.1.1\n",
            self_path.display()
        ),
    )
    .unwrap();

    let config = SshConfigFile::parse(&self_path).expect("self-include must terminate");
    let entries = config.host_entries();
    let count = entries.iter().filter(|e| e.alias == "only-once").count();
    assert_eq!(
        count, 1,
        "self-include must not duplicate hosts: {:?}",
        entries
    );
}

#[test]
fn is_purple_managed_cert_value_matches_expected_paths() {
    use super::is_purple_managed_cert_value;
    assert!(is_purple_managed_cert_value("~/.purple/certs/foo-cert.pub"));
    assert!(is_purple_managed_cert_value(
        "/home/user/.purple/certs/foo.pub"
    ));
    assert!(is_purple_managed_cert_value("\"~/.purple/certs/foo.pub\""));
    assert!(!is_purple_managed_cert_value("~/.ssh/corp.pub"));
    assert!(!is_purple_managed_cert_value("/etc/ssh/cert.pub"));
    assert!(!is_purple_managed_cert_value(""));
}

#[test]
fn update_host_sanitizes_newline_injection_in_hostname() {
    // Security regression: the provider-sync update path passes
    // `remote.ip` straight into `update_host`, which routes the value
    // through `upsert_directive` -> raw_line interpolation. Without the
    // sanitiser inside upsert_directive a malicious provider API could
    // inject a real ProxyCommand by setting remote.ip = "1.2.3.4\n
    // ProxyCommand evil". This test ensures the symmetric edit path is
    // closed (mirror of `set_provider_id_strips_newline_injection_*`).
    let mut config = parse_str("Host victim\n  HostName 10.0.0.1\n");
    let mut entry = first_block(&config).to_host_entry();
    entry.hostname = "1.2.3.4\n  ProxyCommand nc evil 22".to_string();
    config.update_host("victim", &entry);
    let serialized = config.serialize();
    assert_no_directive_injection(&serialized, "ProxyCommand");
}

#[test]
fn update_host_sanitizes_newline_injection_in_alias() {
    // Rename path: the new alias flows into raw_host_line via
    // `format!("Host {}", new_pattern)`. A malicious alias containing
    // `\n` would inject an extra Host block. Sanitiser applied in
    // update_host before constructing raw_host_line.
    let mut config = parse_str("Host victim\n  HostName 10.0.0.1\n");
    let mut entry = first_block(&config).to_host_entry();
    entry.alias = "renamed\nHost evil".to_string();
    config.update_host("victim", &entry);
    let serialized = config.serialize();
    // Re-parse: there must be exactly one HostBlock with a sanitised
    // pattern, no second "Host evil" block created via injection.
    let reparsed = parse_str(&serialized);
    let host_block_count = reparsed
        .elements
        .iter()
        .filter(|e| matches!(e, ConfigElement::HostBlock(_)))
        .count();
    assert_eq!(
        host_block_count, 1,
        "newline in alias must not produce extra blocks: {}",
        serialized
    );
}

#[test]
fn upsert_directive_non_empty_value_replaces_only_first_identityfile() {
    // Symmetric companion to
    // `upsert_directive_empty_value_removes_only_first_identityfile`.
    // The form edits the value and saves; only the first IdentityFile
    // line must change. Subsequent cumulative entries survive.
    let input = "\
Host alpha
  HostName 10.0.0.1
  IdentityFile ~/.ssh/id_rsa
  IdentityFile ~/.ssh/id_ed25519
  IdentityFile ~/.ssh/id_ecdsa
";
    let mut config = parse_str(input);
    let mut entry = first_block(&config).to_host_entry();
    entry.identity_file = "~/.ssh/id_rotated".to_string();
    config.update_host("alpha", &entry);
    let serialized = config.serialize();
    assert!(serialized.contains("IdentityFile ~/.ssh/id_rotated"));
    assert!(!serialized.contains("id_rsa"));
    assert!(
        serialized.contains("IdentityFile ~/.ssh/id_ed25519"),
        "second IdentityFile must survive edit: {}",
        serialized
    );
    assert!(
        serialized.contains("IdentityFile ~/.ssh/id_ecdsa"),
        "third IdentityFile must survive edit: {}",
        serialized
    );
}

#[test]
fn parse_three_node_include_cycle_terminates() {
    // C-005: 3-node cycle (A includes B includes C includes A) exercises
    // the visited set across two recursive frames, not just one. The
    // 2-node test catches direct mutual recursion; this catches the
    // case where the cycle closes through an intermediary.
    let dir = tempfile::tempdir().expect("tempdir");
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    let c = dir.path().join("c");
    std::fs::write(
        &a,
        format!("Include {}\nHost from-a\n  HostName 1.1.1.1\n", b.display()),
    )
    .unwrap();
    std::fs::write(
        &b,
        format!("Include {}\nHost from-b\n  HostName 2.2.2.2\n", c.display()),
    )
    .unwrap();
    std::fs::write(
        &c,
        format!("Include {}\nHost from-c\n  HostName 3.3.3.3\n", a.display()),
    )
    .unwrap();

    let config = SshConfigFile::parse(&a).expect("3-node cycle must terminate");
    let entries = config.host_entries();
    // Each unique host appears at most once. Before the fix, the
    // MAX_INCLUDE_DEPTH=16 explosion would produce dozens of copies.
    for alias in ["from-a", "from-b", "from-c"] {
        let count = entries.iter().filter(|e| e.alias == alias).count();
        assert!(
            count <= 1,
            "host {} appears {} times in {:?}",
            alias,
            count,
            entries.iter().map(|e| &e.alias).collect::<Vec<_>>()
        );
    }
}

// Value quoting / escaping on write. A directive value containing whitespace
// (or a "#"-after-space that the parser reads as an inline comment) was
// written unquoted and silently corrupted on the next parse. The writer must
// wrap single-token values in double quotes, and the parser must strip the
// surrounding quotes back to the logical value on the way in.

#[test]
fn update_host_preserves_value_with_inline_hash() {
    let mut config = parse_str("Host h\n  HostName 10.0.0.1\n");
    let entry = HostEntry {
        alias: "h".to_string(),
        hostname: "10.0.0.1".to_string(),
        identity_file: "~/id #note".to_string(),
        port: 22,
        ..Default::default()
    };
    config.update_host("h", &entry);
    let out = config.serialize();
    let reparsed = parse_str(&out);
    let entries = reparsed.host_entries();
    assert_eq!(
        entries[0].identity_file, "~/id #note",
        "the ' #note' suffix must survive a write/parse round-trip; got output:\n{out}"
    );
}

#[test]
fn update_host_quotes_value_with_space_and_round_trips() {
    let mut config = parse_str("Host h\n  HostName 10.0.0.1\n");
    let entry = HostEntry {
        alias: "h".to_string(),
        hostname: "10.0.0.1".to_string(),
        identity_file: "~/my key/id".to_string(),
        port: 22,
        ..Default::default()
    };
    config.update_host("h", &entry);
    let out = config.serialize();
    assert!(
        out.contains("IdentityFile \"~/my key/id\""),
        "a value with a space must be double-quoted so real OpenSSH reads it as one token; got:\n{out}"
    );
    let reparsed = parse_str(&out);
    assert_eq!(reparsed.host_entries()[0].identity_file, "~/my key/id");
}

#[test]
fn add_host_quotes_hostname_with_space() {
    let mut config = parse_str("");
    config.add_host(&HostEntry {
        alias: "h".to_string(),
        hostname: "a b".to_string(),
        port: 22,
        ..Default::default()
    });
    let out = config.serialize();
    assert!(
        out.contains("HostName \"a b\""),
        "a provider-supplied hostname with a space must be quoted; got:\n{out}"
    );
    assert_eq!(parse_str(&out).host_entries()[0].hostname, "a b");
}

#[test]
fn add_forward_does_not_quote_multi_arg_value() {
    let mut config = parse_str("Host h\n  HostName 10.0.0.1\n");
    config.add_forward("h", "LocalForward", "8080 localhost:80");
    let out = config.serialize();
    assert!(
        out.contains("LocalForward 8080 localhost:80"),
        "a multi-argument forward spec must stay unquoted; got:\n{out}"
    );
    assert!(
        !out.contains("\"8080 localhost:80\""),
        "the forward spec must not be wrapped as a single quoted token; got:\n{out}"
    );
}

#[test]
fn set_certificate_file_quotes_path_with_space() {
    let mut config = parse_str("Host h\n  HostName 10.0.0.1\n");
    // A home directory with a space yields a purple-managed cert path that
    // still contains a space; it must be quoted on write and round-trip.
    assert!(config.set_host_certificate_file("h", "~/.purple/certs/my host-cert.pub"));
    let out = config.serialize();
    assert!(
        out.contains("CertificateFile \"~/.purple/certs/my host-cert.pub\""),
        "a cert path with a space must be quoted; got:\n{out}"
    );
    assert_eq!(
        parse_str(&out).host_entries()[0].certificate_file,
        "~/.purple/certs/my host-cert.pub"
    );
}

#[test]
fn unchanged_quoted_value_round_trips_byte_identical() {
    // A hand-authored, fully-quoted directive that the user never edits must
    // serialize back byte-for-byte (serialize emits raw_line verbatim).
    let content = "Host h\n  IdentityFile \"~/my key/id\"\n";
    let config = parse_str(content);
    assert_eq!(config.serialize(), content);
}

#[test]
fn update_host_round_trips_value_with_space_and_quote() {
    // A single-token value containing BOTH whitespace and a literal quote
    // must round-trip: render_value escapes the quote and quotes the value;
    // the parser unescapes it back. Before the escape fix this value was
    // emitted unquoted and OpenSSH would split it on the space.
    let mut config = parse_str("Host h\n  HostName 10.0.0.1\n");
    let entry = HostEntry {
        alias: "h".to_string(),
        hostname: "10.0.0.1".to_string(),
        identity_file: r#"~/a "b c/id"#.to_string(),
        port: 22,
        ..Default::default()
    };
    config.update_host("h", &entry);
    let out = config.serialize();
    assert!(
        out.contains(r#"IdentityFile "~/a \"b c/id""#),
        "value must be quoted with the embedded quote escaped; got:\n{out}"
    );
    let reparsed = parse_str(&out);
    assert_eq!(reparsed.host_entries()[0].identity_file, r#"~/a "b c/id"#);
}

#[test]
fn update_host_quotes_proxy_jump_with_space() {
    let mut config = parse_str("Host h\n  HostName 10.0.0.1\n");
    let entry = HostEntry {
        alias: "h".to_string(),
        hostname: "10.0.0.1".to_string(),
        proxy_jump: "jump host".to_string(),
        port: 22,
        ..Default::default()
    };
    config.update_host("h", &entry);
    let out = config.serialize();
    assert!(
        out.contains("ProxyJump \"jump host\""),
        "a ProxyJump value with a space must be quoted; got:\n{out}"
    );
    assert_eq!(parse_str(&out).host_entries()[0].proxy_jump, "jump host");
}

#[test]
fn value_with_hash_but_no_space_stays_unquoted() {
    // A "#" with no preceding whitespace is not an inline comment, so the
    // value needs no quoting and must not gain any.
    let mut config = parse_str("Host h\n  HostName 10.0.0.1\n");
    let entry = HostEntry {
        alias: "h".to_string(),
        hostname: "10.0.0.1".to_string(),
        identity_file: "~/id#frag".to_string(),
        port: 22,
        ..Default::default()
    };
    config.update_host("h", &entry);
    let out = config.serialize();
    assert!(out.contains("IdentityFile ~/id#frag"), "got:\n{out}");
    assert!(
        !out.contains("\"~/id#frag\""),
        "must not be quoted; got:\n{out}"
    );
    assert_eq!(parse_str(&out).host_entries()[0].identity_file, "~/id#frag");
}

#[test]
fn set_host_directive_adds_updates_and_removes_first_only() {
    let mut config = parse_str(
        "Host tp-node1\n  HostName node1\n  Port 3022\n\nHost other\n  HostName 10.0.0.2\n",
    );
    assert_eq!(config.host_directive("tp-node1", "ProxyCommand"), None);

    assert!(config.set_host_directive(
        "tp-node1",
        "ProxyCommand",
        "\"/usr/local/bin/tsh\" proxy ssh --cluster=example.com --proxy=proxy.example.com:443 %r@%h:%p",
    ));
    let out = config.serialize();
    assert!(
        out.contains("  ProxyCommand \"/usr/local/bin/tsh\" proxy ssh --cluster=example.com --proxy=proxy.example.com:443 %r@%h:%p\n"),
        "got:\n{out}"
    );
    assert_eq!(
        config.host_directive("tp-node1", "proxycommand").as_deref(),
        Some(
            "\"/usr/local/bin/tsh\" proxy ssh --cluster=example.com --proxy=proxy.example.com:443 %r@%h:%p"
        )
    );
    // The other host is untouched.
    assert_eq!(config.host_directive("other", "ProxyCommand"), None);

    // Update in place keeps a single directive.
    assert!(config.set_host_directive("tp-node1", "ProxyCommand", "nc %h %p"));
    let out = config.serialize();
    assert_eq!(out.matches("ProxyCommand").count(), 1, "got:\n{out}");
    assert!(out.contains("  ProxyCommand nc %h %p\n"), "got:\n{out}");

    // Empty value removes it.
    assert!(config.set_host_directive("tp-node1", "ProxyCommand", ""));
    assert_eq!(config.host_directive("tp-node1", "ProxyCommand"), None);
    assert!(!config.serialize().contains("ProxyCommand"));

    // Round trip keeps the rest intact.
    let entries = parse_str(&config.serialize()).host_entries();
    assert_eq!(entries[0].hostname, "node1");
    assert_eq!(entries[0].port, 3022);
}

#[test]
fn set_host_directive_refuses_patterns_shared_blocks_and_missing_hosts() {
    let mut config = parse_str("Host web-*\n  User root\n\nHost a b\n  HostName 10.0.0.1\n");
    assert!(!config.set_host_directive("web-*", "ProxyCommand", "nc %h %p"));
    assert!(!config.set_host_directive("a", "ProxyCommand", "nc %h %p"));
    assert!(!config.set_host_directive("missing", "ProxyCommand", "nc %h %p"));
    assert!(!config.set_host_directive("a", "", "x"));
    assert!(!config.serialize().contains("ProxyCommand"));
    // Reads on a shared block still work, they are harmless.
    assert_eq!(
        config.host_directive("a", "HostName").as_deref(),
        Some("10.0.0.1")
    );
    assert_eq!(config.host_directive("missing", "HostName"), None);
}

#[test]
fn set_host_directive_quotes_single_token_values_and_round_trips() {
    let mut config = parse_str("Host tp-node1\n  HostName node1\n");
    assert!(config.set_host_directive(
        "tp-node1",
        "UserKnownHostsFile",
        "/Users/alice/my dir/.tsh/known_hosts"
    ));
    assert!(config.set_host_directive(
        "tp-node1",
        "CertificateFile",
        "/Users/alice/.tsh/keys/p/bob-ssh/c-cert.pub"
    ));
    let out = config.serialize();
    assert!(
        out.contains("  UserKnownHostsFile \"/Users/alice/my dir/.tsh/known_hosts\"\n"),
        "got:\n{out}"
    );
    assert!(
        out.contains("  CertificateFile /Users/alice/.tsh/keys/p/bob-ssh/c-cert.pub\n"),
        "got:\n{out}"
    );
    // Re-parsed values are the logical ones, so a repeat set is a no-op.
    let mut reparsed = parse_str(&out);
    assert_eq!(
        reparsed
            .host_directive("tp-node1", "UserKnownHostsFile")
            .as_deref(),
        Some("/Users/alice/my dir/.tsh/known_hosts")
    );
    assert!(reparsed.set_host_directive(
        "tp-node1",
        "UserKnownHostsFile",
        "/Users/alice/my dir/.tsh/known_hosts"
    ));
    assert_eq!(reparsed.serialize(), out);
}

#[test]
fn set_host_directive_strips_newline_injection() {
    // A provider-supplied value carrying line breaks must stay one directive.
    let mut config = parse_str("Host victim\n  HostName 10.0.0.1\n");
    assert!(config.set_host_directive(
        "victim",
        "ProxyCommand",
        "nc %h %p\n  ProxyCommand nc evil 22\r\nHost injected\n  HostName 6.6.6.6"
    ));
    assert!(config.set_host_directive(
        "victim",
        "UserKnownHostsFile",
        "/tmp/k\n  IdentityFile /tmp/evil"
    ));
    let out = config.serialize();
    assert_no_directive_injection(&out, "IdentityFile");
    let reparsed = parse_str(&out);
    let entries = reparsed.host_entries();
    assert_eq!(entries.len(), 1, "no extra Host block may appear:\n{out}");
    assert_eq!(
        out.matches("ProxyCommand").count(),
        2,
        "value stays on one line:\n{out}"
    );
    assert!(!out.contains("\nHost injected"), "got:\n{out}");
    assert_eq!(reparsed.host_directive("victim", "IdentityFile"), None);
}

#[test]
fn pattern_list_matches_comma_list_with_exclusion() {
    assert!(pattern_list_matches("*.prod,*.lab", "db1.prod", false));
    assert!(!pattern_list_matches("*.prod,!db1.*", "db1.prod", false));
    assert!(!pattern_list_matches("!db1.*", "db1.prod", false));
    assert!(!pattern_list_matches("", "anything", false));
    assert!(pattern_list_matches(" web-* , api-* ", "api-3", false));
    assert!(pattern_list_matches("w?b-*", "web-1", false));
    assert!(pattern_list_matches("*", "", false));
    assert!(!pattern_list_matches("a*b", "acbd", false));
    assert!(
        pattern_list_matches("*a", "*xa", false),
        "a literal star in the text"
    );
    assert!(pattern_list_matches("a*b*c", "axbyc", false));
}

#[test]
fn pattern_list_matches_folds_case_only_when_asked() {
    assert!(pattern_list_matches(
        "*.example.com",
        "Web.Example.COM",
        true
    ));
    assert!(!pattern_list_matches("!*.PROD", "a.prod", false));
    assert!(!pattern_list_matches(
        "*.example.com",
        "Web.Example.COM",
        false
    ));
    assert!(!pattern_list_matches("Root", "root", false));
}

#[test]
fn pattern_list_matches_has_no_character_classes() {
    // OpenSSH treats `[` literally in Match criteria (verified with ssh -G).
    assert!(!pattern_list_matches("web[12]", "web1", false));
    assert!(pattern_list_matches("web[12]", "web[12]", false));
}

#[test]
fn match_block_entries_lists_blocks_in_order_with_bodies() {
    let cfg = parse_str(
        "Host a\n  HostName 1\n\n\
         Match originalhost a\n  User x\n  # purple:tags t1\n\n\
         Match all\n  Port 2\n\n\
         Host b\n  HostName 2\n",
    );
    let blocks = cfg.match_block_entries();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].criteria_text, "originalhost a");
    assert_eq!(blocks[0].user, "x");
    assert_eq!(blocks[0].tags, vec!["t1"]);
    assert_eq!(blocks[0].order, 1);
    assert_eq!(blocks[1].criteria_text, "all");
    assert_eq!(
        blocks[1].directives,
        vec![("Port".to_string(), "2".to_string())]
    );
    assert_eq!(blocks[1].order, 2);
}

#[test]
fn matching_match_blocks_skips_no_and_keeps_undecided() {
    let cfg = parse_str(
        "Host web\n  HostName web.prod\n\n\
         Match originalhost web localuser deploy\n  User deploy\n\n\
         Match originalhost db-*\n  User dba\n\n\
         Match all\n  ServerAliveInterval 30\n",
    );
    let m = cfg.matching_match_blocks("web");
    assert_eq!(m.len(), 2, "the db-* block is skipped");
    assert_eq!(m[0].status, BlockStatus::Undecided);
    assert_eq!(m[0].verdicts[0].verdict, Verdict::Yes);
    assert_eq!(m[0].verdicts[1].verdict, Verdict::Unknown);
    assert!(!m[0].before_host);
    assert_eq!(m[1].status, BlockStatus::Applies);
}

#[test]
fn matching_match_blocks_host_criterion_sees_hostname_from_block_and_pattern() {
    let cfg = parse_str(
        "Host *.lab\n  HostName lab.example.com\n\n\
         Host db\n  HostName db1.example.com\n\n\
         Host node.lab\n\n\
         Match host *.example.com\n  User root\n",
    );
    assert_eq!(
        cfg.matching_match_blocks("db")[0].status,
        BlockStatus::Applies
    );
    assert_eq!(
        cfg.matching_match_blocks("node.lab")[0].status,
        BlockStatus::Applies,
        "HostName inherited from the pattern"
    );
}

#[test]
fn matching_match_blocks_host_criterion_expands_percent_h() {
    let cfg = parse_str(
        "Host web1\n  HostName %h.example.com\n\n\
         Match host web1.example.com\n  User root\n\n\
         Match host web1\n  Port 2\n",
    );
    let m = cfg.matching_match_blocks("web1");
    assert_eq!(m.len(), 1, "the block matching the bare alias is skipped");
    assert_eq!(m[0].entry.criteria_text, "host web1.example.com");
    assert_eq!(m[0].status, BlockStatus::Applies);
}

#[test]
fn matching_match_blocks_host_without_hostname_uses_alias() {
    let cfg = parse_str("Host web-1\n\nMatch host web-*\n  User root\n");
    assert_eq!(
        cfg.matching_match_blocks("web-1")[0].status,
        BlockStatus::Applies
    );
}

#[test]
fn matching_match_blocks_undecided_hostname_makes_later_host_unknown() {
    let cfg = parse_str(
        "Host a\n\n\
         Match originalhost a localuser x\n  HostName a.example.com\n\n\
         Match host *.example.com\n  User root\n",
    );
    let m = cfg.matching_match_blocks("a");
    assert_eq!(m[1].status, BlockStatus::Undecided);
    assert_eq!(m[1].verdicts[0].verdict, Verdict::Unknown);
}

#[test]
fn matching_match_blocks_excluding_pattern_never_feeds_the_context() {
    let cfg = parse_str(
        "Host a\n\n\
         Host * !a\n  HostName other.example.com\n\n\
         Match host *.example.com\n  User root\n",
    );
    assert!(
        cfg.matching_match_blocks("a").is_empty(),
        "the pattern excludes a, so HostName stays the alias"
    );
}

#[test]
fn matching_match_blocks_before_host_is_set_for_blocks_above_the_host() {
    let cfg = parse_str("Match all\n  User root\n\nHost a\n  User eric\n");
    assert!(cfg.matching_match_blocks("a")[0].before_host);
}

#[test]
fn matching_match_blocks_user_criterion_reads_user_set_above() {
    let cfg = parse_str(
        "Host a\n  User root\n\n\
         Match user root\n  IdentityFile ~/.ssh/id_root\n\n\
         Host b\n\n\
         Match user root\n  IdentityFile ~/.ssh/id_root\n",
    );
    assert_eq!(
        cfg.matching_match_blocks("a")[0].status,
        BlockStatus::Applies
    );
    assert_eq!(
        cfg.matching_match_blocks("b")[0].status,
        BlockStatus::Undecided,
        "no User set, so it is the local account"
    );
}

#[test]
fn host_entries_inherit_from_applying_match_block_only() {
    let cfg = parse_str(
        "Host web\n  HostName 1\n\n\
         Match originalhost web\n  User root\n\n\
         Match originalhost web localuser x\n  IdentityFile ~/.ssh/undecided\n",
    );
    let h = &cfg.host_entries()[0];
    assert_eq!(h.user, "root");
    assert_eq!(
        h.identity_file, "",
        "undecided blocks never feed inheritance"
    );
}

#[test]
fn host_entries_merge_patterns_and_match_blocks_in_file_order() {
    let cfg = parse_str(
        "Host web\n  HostName 1\n\n\
         Match originalhost web\n  User from-match\n\n\
         Host *\n  User from-pattern\n  IdentityFile ~/.ssh/id\n",
    );
    let h = &cfg.host_entries()[0];
    assert_eq!(
        h.user, "from-match",
        "the Match block comes first in the file"
    );
    assert_eq!(h.identity_file, "~/.ssh/id");
}

#[test]
fn host_entries_proxy_command_from_applying_match_block_counts() {
    let cfg = parse_str(
        "Host web\n  HostName 1\n\n\
         Match originalhost web\n  ProxyCommand ssh -W %h:%p gw\n\n\
         Host db\n  HostName 2\n\n\
         Match originalhost db\n  ProxyCommand none\n\n\
         Host *\n  ProxyCommand ssh -W %h:%p gw\n",
    );
    let hosts = cfg.host_entries();
    assert!(hosts[0].has_proxy_command);
    assert!(!hosts[1].has_proxy_command, "the Match opt-out comes first");
}

#[test]
fn host_entries_undecided_match_block_proxy_command_is_ignored() {
    let cfg = parse_str(
        "Host web\n  HostName 1\n\n\
         Match originalhost web localuser x\n  ProxyCommand ssh -W %h:%p gw\n",
    );
    assert!(!cfg.host_entries()[0].has_proxy_command);
}

#[test]
fn host_entries_proxy_command_keeps_own_block_and_pattern_order() {
    let cfg = parse_str(
        "Host *\n  ProxyCommand none\n\n\
         Host web\n  ProxyCommand ssh -W %h:%p gw\n\n\
         Host db\n  ProxyCommand ssh -W %h:%p gw\n\n\
         Host db*\n  ProxyCommand none\n",
    );
    let hosts = cfg.host_entries();
    assert!(!hosts[0].has_proxy_command, "Host * is read first");
    assert!(!hosts[1].has_proxy_command, "Host * is read first");
    let cfg = parse_str(
        "Host web\n  ProxyCommand ssh -W %h:%p gw\n\n\
         Host *\n  ProxyCommand none\n",
    );
    assert!(cfg.host_entries()[0].has_proxy_command, "own block first");
}

#[test]
fn inherited_hints_label_match_block_sources() {
    let cfg = parse_str(
        "Host web\n  HostName 1\n\n\
         Match originalhost web\n  User root\n",
    );
    let hints = cfg.inherited_hints("web");
    let expected = Some(("root".to_string(), "Match originalhost web".to_string()));
    assert_eq!(hints.user, expected);
}

#[test]
fn match_block_in_included_file_carries_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let inc = dir.path().join("extra");
    std::fs::write(&inc, "Match all\n  User root\n").expect("write include");
    let main = dir.path().join("config");
    let content = format!("Include {}\nHost a\n  HostName 1\n", inc.display());
    std::fs::write(&main, content).expect("write config");
    let env = crate::runtime::env::Env::for_test(dir.path());
    let cfg = SshConfigFile::parse_with_env(&main, &env).expect("parse");
    let blocks = cfg.match_block_entries();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].source_file.as_deref(), Some(inc.as_path()));
    assert_eq!(cfg.host_entries()[0].user, "root");
}

#[test]
fn match_blocks_round_trip_unchanged() {
    let content = "Host a\n  HostName 1\n\n\
                   Match=host *.prod,!*.lab exec \"test -f ~/.vpn\"\n\
                   \tProxyJump bastion\n  # purple:tags prod\n\n\
                   Match all\n  ServerAliveInterval 60\n";
    let cfg = parse_str(content);
    assert_eq!(cfg.serialize(), content);
}

#[test]
fn add_host_never_lands_inside_a_match_block_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    let inc = dir.path().join("extra");
    std::fs::write(&inc, "Host from-include\n  HostName 9\n").expect("write include");
    let main = dir.path().join("config");
    let content = format!(
        "Host a\n  HostName 1\n\nMatch all\n  Include {}\n  User root\n\nHost *\n  ServerAliveInterval 30\n",
        inc.display()
    );
    std::fs::write(&main, &content).expect("write config");
    let env = crate::runtime::env::Env::for_test(dir.path());
    let mut cfg = SshConfigFile::parse_with_env(&main, &env).expect("parse");
    cfg.add_host(&HostEntry {
        alias: "new".to_string(),
        hostname: "2".to_string(),
        ..Default::default()
    });
    let out = cfg.serialize();
    let expected = format!(
        "Host a\n  HostName 1\n\nMatch all\n  Include {}\n  User root\n\n\
         Host new\n  HostName 2\n\nHost *\n  ServerAliveInterval 30\n",
        inc.display()
    );
    assert_eq!(out, expected, "the Match body must stay whole");
    let reparsed = parse_str(&out);
    let own = reparsed.own_host_entries();
    let new_host = own.iter().find(|h| h.alias == "new").expect("new host");
    assert_eq!(
        new_host.user, "",
        "the Match body must not move into the new host"
    );
    assert_eq!(
        reparsed.host_entries()[0].user,
        "root",
        "Match all still applies to a"
    );
}

#[test]
fn add_host_after_an_include_in_a_match_block_keeps_comments_with_the_pattern() {
    let dir = tempfile::tempdir().expect("tempdir");
    let inc = dir.path().join("extra");
    std::fs::write(&inc, "Host from-include\n  HostName 9\n").expect("write include");
    let main = dir.path().join("config");
    let content = format!(
        "Host a\n  HostName 1\n\nMatch all\n  Include {}\n\n# defaults\nHost *\n  User x\n",
        inc.display()
    );
    std::fs::write(&main, &content).expect("write config");
    let env = crate::runtime::env::Env::for_test(dir.path());
    let mut cfg = SshConfigFile::parse_with_env(&main, &env).expect("parse");
    cfg.add_host(&HostEntry {
        alias: "new".to_string(),
        hostname: "2".to_string(),
        ..Default::default()
    });
    let out = cfg.serialize();
    assert!(
        out.contains("# defaults\nHost *"),
        "the comment stays with Host *:\n{out}"
    );
    assert!(out.contains("Host new\n  HostName 2\n"), "{out}");
}

/// Parse `main` with `files` written next to it, Include lines resolved.
fn parse_with_files(main: &str, files: &[(&str, &str)]) -> (tempfile::TempDir, SshConfigFile) {
    let dir = tempfile::tempdir().expect("tempdir");
    let dir_text = dir.path().display().to_string();
    for (name, text) in files {
        let text = text.replace("$DIR", &dir_text);
        std::fs::write(dir.path().join(name), text).expect("write include");
    }
    let path = dir.path().join("config");
    let main = main.replace("$DIR", &dir_text);
    std::fs::write(&path, main).expect("write config");
    let env = crate::runtime::env::Env::for_test(dir.path());
    let cfg = SshConfigFile::parse_with_env(&path, &env).expect("parse");
    (dir, cfg)
}

#[test]
fn nested_match_block_is_skipped_while_its_parent_does_not_apply() {
    // ssh -G b does not apply Port 2 here: the include is never read for b.
    let (_dir, cfg) = parse_with_files(
        "Host a\n\nHost b\n\nMatch originalhost a\n  Include $DIR/f\n  User z\n",
        &[("f", "Match originalhost b\n  Port 2\n  User fromb\n")],
    );
    assert!(cfg.matching_match_blocks("b").is_empty());
    let hosts = cfg.host_entries();
    let b = hosts.iter().find(|h| h.alias == "b").expect("b");
    assert_eq!(b.user, "", "nothing from the nested block");
    let a = hosts.iter().find(|h| h.alias == "a").expect("a");
    assert_eq!(a.user, "z", "the parent resumes after its Include");
}

#[test]
fn nested_match_block_under_an_undecided_parent_is_undecided() {
    let (_dir, cfg) = parse_with_files(
        "Host a\n\nMatch originalhost a localuser x\n  Include $DIR/f\n",
        &[("f", "Match originalhost a\n  User q\n")],
    );
    let m = cfg.matching_match_blocks("a");
    assert_eq!(m.len(), 2);
    assert_eq!(m[1].entry.parent, Some(m[0].entry.order));
    assert_eq!(m[1].status, BlockStatus::Undecided);
    assert_eq!(cfg.host_entries()[0].user, "");
}

#[test]
fn match_body_reads_lines_after_a_nested_include() {
    let (_dir, cfg) = parse_with_files(
        "Host a\n\nMatch originalhost a\n  Include $DIR/f\n",
        &[("f", "Include $DIR/g\nUser zz\n"), ("g", "Port 9\n")],
    );
    let blocks = cfg.match_block_entries();
    let keys: Vec<&str> = blocks[0]
        .directives
        .iter()
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(keys, vec!["Port", "User"]);
    assert_eq!(cfg.host_entries()[0].user, "zz");
}

#[test]
fn section_index_match_statuses_agree_with_resolve() {
    let cfg = parse_str(
        "Host a\n  HostName 1\n\nHost b\n  HostName 2\n\n\
         Match originalhost a\n  User root\n\n\
         Match originalhost b localuser x\n  User x\n",
    );
    let index = cfg.section_index();
    for alias in ["a", "b"] {
        let full: Vec<(usize, BlockStatus)> = index
            .resolve(alias)
            .match_blocks
            .iter()
            .map(|m| (m.entry.order, m.status))
            .collect();
        assert_eq!(index.match_statuses(alias), full, "{alias}");
    }
}

#[test]
fn undecided_block_leaves_the_fallback_value_below_it() {
    let cfg = parse_str(
        "Host a\n  HostName 1\n\n\
         Match originalhost a exec \"test -f /vpn\"\n  ProxyJump none\n\n\
         Host *\n  ProxyJump bastion\n",
    );
    let h = &cfg.host_entries()[0];
    assert_eq!(
        h.proxy_jump, "bastion",
        "the value ssh uses when the exec fails"
    );
    let m = cfg.matching_match_blocks("a");
    assert_eq!(m[0].status, BlockStatus::Undecided);
}

#[test]
fn add_host_before_a_match_line_or_pattern_keeps_its_old_position() {
    let content = "Host a\n  HostName 1\n\nMatch all\n  User root\n\n# Defaults\nHost *\n  ServerAliveInterval 30\n";
    let mut cfg = parse_str(content);
    cfg.add_host(&HostEntry {
        alias: "new".to_string(),
        hostname: "2".to_string(),
        ..Default::default()
    });
    let out = cfg.serialize();
    assert!(
        out.starts_with("Host a\n  HostName 1\n\nHost new\n  HostName 2\n\nMatch all\n"),
        "without an Include the host still goes above the Match block:\n{out}"
    );
    assert!(
        out.contains("Match all\n  User root\n\n# Defaults\nHost *"),
        "{out}"
    );
}

#[test]
fn matching_match_blocks_overrides_own_lists_first_wins_keys_set_by_both() {
    let cfg = parse_str(
        "Match all\n  User root\n  IdentityFile ~/.ssh/id_all\n  Port 2222\n\n\
         Host db\n  User dba\n  IdentityFile ~/.ssh/id_db\n\n\
         Match all\n  HostName late\n",
    );
    let m = cfg.matching_match_blocks("db");
    assert_eq!(
        m[0].overrides_own,
        vec!["User"],
        "IdentityFile is cumulative"
    );
    assert!(
        m[1].overrides_own.is_empty(),
        "the block sits below the host"
    );
}

#[test]
fn matching_match_blocks_overrides_own_ignores_inherited_values() {
    let cfg = parse_str(
        "Match all\n  User root\n\n\
         Host db\n  HostName 1\n\n\
         Host *\n  User other\n",
    );
    assert_eq!(cfg.host_entries()[0].user, "root");
    assert!(cfg.matching_match_blocks("db")[0].overrides_own.is_empty());
}

#[test]
fn matching_match_blocks_overrides_own_needs_an_applying_block() {
    let cfg = parse_str("Match all localuser x\n  User root\n\nHost db\n  User dba\n");
    let m = cfg.matching_match_blocks("db");
    assert_eq!(m[0].status, BlockStatus::Undecided);
    assert!(m[0].overrides_own.is_empty());
}
