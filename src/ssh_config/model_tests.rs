use super::*;

fn parse_str(content: &str) -> SshConfigFile {
    SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
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
    // Extra space in config value vs single space in query — should still match
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
    // Now check if the target value already exists — should detect duplicate
    assert!(config.has_forward("myserver", "LocalForward", "9090 localhost:90"));
}

#[test]
fn has_forward_tab_whitespace_normalized() {
    let config = parse_str("Host myserver\n  LocalForward 8080\tlocalhost:80\n");
    // Tab in config value vs space in query — should match via values_match
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
    config.set_host_askpass("myserver", "keychain");
    assert_eq!(first_block(&config).askpass(), Some("keychain".to_string()));
}

#[test]
fn set_askpass_replaces_existing() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    config.set_host_askpass("myserver", "op://V/I/p");
    assert_eq!(
        first_block(&config).askpass(),
        Some("op://V/I/p".to_string())
    );
}

#[test]
fn set_askpass_empty_removes_comment() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    config.set_host_askpass("myserver", "");
    assert_eq!(first_block(&config).askpass(), None);
}

#[test]
fn set_askpass_preserves_other_directives() {
    let mut config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  User admin\n  # purple:tags prod\n");
    config.set_host_askpass("myserver", "vault:secret/ssh");
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
    config.set_host_askpass("myserver", "keychain");
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
    config.set_host_askpass("nohost", "keychain");
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
    config.set_host_askpass("myserver", "vault:secret/data/team#api_key");
    assert_eq!(
        first_block(&config).askpass(),
        Some("vault:secret/data/team#api_key".to_string())
    );
}

#[test]
fn set_askpass_custom_command_with_percent() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    config.set_host_askpass("myserver", "get-pass %a %h");
    assert_eq!(
        first_block(&config).askpass(),
        Some("get-pass %a %h".to_string())
    );
}

#[test]
fn multiple_hosts_independent_askpass() {
    let mut config = parse_str("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
    config.set_host_askpass("alpha", "keychain");
    config.set_host_askpass("beta", "vault:secret/ssh");
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
    config.set_host_askpass("myserver", "keychain");
    assert_eq!(first_block(&config).askpass(), Some("keychain".to_string()));
    config.set_host_askpass("myserver", "");
    assert_eq!(first_block(&config).askpass(), None);
    config.set_host_askpass("myserver", "op://V/I/p");
    assert_eq!(
        first_block(&config).askpass(),
        Some("op://V/I/p".to_string())
    );
}

#[test]
fn askpass_tab_indent_preserved() {
    let mut config = parse_str("Host myserver\n\tHostName 10.0.0.1\n");
    config.set_host_askpass("myserver", "pass:ssh/prod");
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
    config.set_host_askpass("myserver", "keychain");
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.askpass, Some("keychain".to_string()));
    assert!(entry.tags.contains(&"prod".to_string()));
    assert!(entry.tags.contains(&"staging".to_string()));
}

#[test]
fn askpass_idempotent_set_same_value() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    config.set_host_askpass("myserver", "keychain");
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
    config.set_host_askpass("myserver", "cmd --opt=val %h");
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
    config.set_host_askpass("myserver", uri);
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
    config.set_host_askpass("myserver", "pass:ssh/prod");
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
    config.set_host_askpass("myserver", "new-source");
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
    config.set_host_askpass("myserver", "keychain");
    let output = config.serialize();
    // Should still have exactly one askpass comment
    assert_eq!(output.matches("purple:askpass").count(), 1);
    assert!(output.contains("# purple:askpass keychain"));
}

#[test]
fn set_askpass_with_equals_in_value() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    config.set_host_askpass("myserver", "cmd --opt=val");
    let entries = config.host_entries();
    assert_eq!(entries[0].askpass, Some("cmd --opt=val".to_string()));
}

#[test]
fn set_askpass_with_hash_in_value() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    config.set_host_askpass("myserver", "vault:secret/data#field");
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
    config.set_host_askpass("myserver", long_uri);
    assert_eq!(config.host_entries()[0].askpass, Some(long_uri.to_string()));
}

#[test]
fn askpass_host_with_multi_pattern_is_skipped() {
    // Multi-pattern host blocks ("Host prod staging") are treated as patterns
    // and are not included in host_entries(), so set_askpass is a no-op
    let config_str = "Host prod staging\n  HostName 10.0.0.1\n";
    let mut config = parse_str(config_str);
    config.set_host_askpass("prod", "keychain");
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
    config.set_host_meta("myhost", &meta);
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
    config.set_host_meta("myhost", &[("region".to_string(), "new".to_string())]);
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
    config.set_host_meta("myhost", &[]);
    let output = config.serialize();
    assert!(!output.contains("purple:meta"));
}

#[test]
fn meta_sanitizes_commas_in_values() {
    let config_str = "Host myhost\n  HostName 1.2.3.4\n";
    let mut config = parse_str(config_str);
    let meta = vec![("plan".to_string(), "s-1vcpu,1gb".to_string())];
    config.set_host_meta("myhost", &meta);
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
                    raw_line: "# purple:group Production".to_string(),
                    is_non_directive: true,
                },
            ],
        })],
        path: PathBuf::from("/tmp/test_config"),
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
        assert_eq!(line, "# purple:group Production");
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
                    raw_line: "# purple:group Staging".to_string(),
                    is_non_directive: true,
                },
            ],
        })],
        path: PathBuf::from("/tmp/test_config"),
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
    let mut config = parse_str("# purple:group Production\nHost myserver\n  HostName 10.0.0.1\n");
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
                        raw_line: "# purple:group Staging".to_string(),
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
        path: PathBuf::from("/tmp/test_config"),
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

# purple:group Staging
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
    config.set_host_tags("myserver", &["newuser".to_string()]);
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.tags, vec!["newuser"]);
    assert_eq!(entry.provider_tags, vec!["cloud1", "cloud2"]);
}

#[test]
fn test_set_provider_tags_does_not_delete_user_tags() {
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:tags user1,user2\n  # purple:provider_tags old\n",
    );
    config.set_host_provider_tags("myserver", &["new1".to_string(), "new2".to_string()]);
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.tags, vec!["user1", "user2"]);
    assert_eq!(entry.provider_tags, vec!["new1", "new2"]);
}

#[test]
fn test_set_askpass_does_not_delete_similar_comments() {
    // A hypothetical "# purple:askpass_backup test" should NOT be deleted by set_askpass
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n  # purple:askpass_backup test\n",
    );
    config.set_host_askpass("myserver", "op://vault/item/pass");
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
    config.set_host_meta("myserver", &[("region".to_string(), "eu-west".to_string())]);
    let serialized = config.serialize();
    assert!(serialized.contains("purple:meta region=eu-west"));
    assert!(serialized.contains("purple:metadata foo"));
}

#[test]
fn test_set_meta_sanitizes_control_chars() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    config.set_host_meta(
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
    config.set_host_stale("db", 1234567890);
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
    config.clear_host_stale("web");
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
    config.set_host_stale("nonexistent", 12345);
    assert_eq!(config.serialize(), before);
}

#[test]
fn clear_host_stale_nonexistent_alias_is_noop() {
    let config_str = "Host web\n  HostName 1.2.3.4\n";
    let mut config = parse_str(config_str);
    let before = config.serialize();
    config.clear_host_stale("nonexistent");
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
    config.clear_host_stale("web");
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
    config.set_host_stale("web", 1711900000);
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
    config.set_host_stale("web", 1711900000);
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
    config.set_host_stale("do-web", 1711900000);
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
    config.set_host_stale("do-db", 1711900000);
    let after_stale = config.serialize();
    assert_ne!(after_stale, original, "stale should change the config");

    // Clear stale
    config.clear_host_stale("do-db");
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
        config.set_host_stale("web", 1711900000);
        config.clear_host_stale("web");
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

    config.set_host_stale("complex", 1711900000);
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
    config.clear_host_stale("complex");
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
    config.set_host_stale("web", 1711900000);
    let output = config.serialize();
    assert!(output.ends_with('\n'), "config must end with newline");

    config.clear_host_stale("web");
    let output2 = config.serialize();
    assert_eq!(output2, config_str);
}

#[test]
fn stale_with_crlf_preserves_line_endings() {
    let config_str = "Host web\r\n  HostName 1.2.3.4\r\n";
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(config_str),
        path: std::path::PathBuf::from("/tmp/test"),
        crlf: true,
        bom: false,
    };
    let mut config = config;
    config.set_host_stale("web", 1711900000);
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

    config.clear_host_stale("web");
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
    assert_eq!(hosts[0].proxy_jump, ""); // no match — alias doesn't match pattern
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
    // inherited_hints is independent of the host's own values — it reports
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
        path: PathBuf::from("/tmp/test_config"),
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
        path: PathBuf::from("/tmp/test_config"),
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
    config.set_host_vault_ssh("myserver", "ssh/sign/engineer");
    assert_eq!(
        first_block(&config).vault_ssh(),
        Some("ssh/sign/engineer".to_string())
    );
}

#[test]
fn set_vault_ssh_replaces_existing() {
    let mut config =
        parse_str("Host myserver\n  HostName 10.0.0.1\n  # purple:vault-ssh ssh/sign/old\n");
    config.set_host_vault_ssh("myserver", "ssh/sign/new");
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
    config.set_host_vault_ssh("myserver", "");
    assert!(first_block(&config).vault_ssh().is_none());
    assert!(!config.serialize().contains("vault-ssh"));
}

#[test]
fn set_vault_ssh_preserves_other_comments() {
    let mut config = parse_str(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n  # purple:tags prod\n",
    );
    config.set_host_vault_ssh("myserver", "ssh/sign/engineer");
    let entry = first_block(&config).to_host_entry();
    assert_eq!(entry.askpass, Some("keychain".to_string()));
    assert!(entry.tags.contains(&"prod".to_string()));
    assert_eq!(entry.vault_ssh.as_deref(), Some("ssh/sign/engineer"));
}

#[test]
fn set_vault_ssh_preserves_indent() {
    let mut config = parse_str("Host myserver\n    HostName 10.0.0.1\n");
    config.set_host_vault_ssh("myserver", "ssh/sign/engineer");
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
    let input = "\
Host alpha
  HostName 10.0.0.1

Match host alpha
  CertificateFile /user/set/match-cert.pub
";
    let mut config = parse_str(input);
    assert!(config.set_host_certificate_file("alpha", "/purple/managed.pub"));
    let out = config.serialize();
    // Top-level alpha block got the directive
    assert!(out.contains("Host alpha\n  HostName 10.0.0.1\n  CertificateFile /purple/managed.pub"));
    // Match block's own CertificateFile is untouched
    assert!(out.contains("Match host alpha\n  CertificateFile /user/set/match-cert.pub"));
}

#[test]
fn set_vault_ssh_twice_replaces_not_appends() {
    let mut config = parse_str("Host myserver\n  HostName 10.0.0.1\n");
    config.set_host_vault_ssh("myserver", "ssh/sign/one");
    config.set_host_vault_ssh("myserver", "ssh/sign/two");
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
    config.set_host_vault_ssh("myserver", "ssh/sign/engineer");
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
    config.set_host_vault_ssh("myserver", "");
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
    config.set_host_vault_ssh("myserver", "ssh/sign/new");
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
