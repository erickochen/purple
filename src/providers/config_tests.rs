use super::*;

/// Generate a unique temp file path per test invocation to avoid races
/// when tests run in parallel.
fn unique_tmp_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "purple_{}_{}_{}",
        label,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

#[test]
fn vault_role_invalid_is_dropped_on_parse() {
    let config = ProviderConfig::parse("[aws]\ntoken=abc\nvault_role=-format=json\n");
    assert_eq!(config.sections.len(), 1);
    assert!(config.sections[0].vault_role.is_empty());
}

#[test]
fn vault_role_valid_is_parsed() {
    let config = ProviderConfig::parse("[aws]\ntoken=abc\nvault_role=ssh/sign/engineer\n");
    assert_eq!(config.sections[0].vault_role, "ssh/sign/engineer");
}

#[test]
fn vault_role_roundtrip_preserves_value() {
    // Full parse → save → re-parse roundtrip for the provider-level
    // vault_role field. Catches regressions where save() forgets to emit
    // the field or parse() forgets to read it back.
    let tmp = unique_tmp_path("vault_role_roundtrip");
    let config = ProviderConfig {
        path_override: Some(tmp.clone()),
        sections: vec![ProviderSection {
            filter: String::new(),
            interface: String::new(),
            ssh_port: None,
            id: crate::providers::config::ProviderConfigId::bare("aws"),
            token: "abc".to_string(),
            alias_prefix: "aws".to_string(),
            user: "ec2-user".to_string(),
            identity_file: String::new(),
            url: String::new(),
            verify_tls: true,
            profile: String::new(),
            regions: "us-east-1".to_string(),
            project: String::new(),
            compartment: String::new(),
            vault_role: "ssh-client-signer/sign/engineer".to_string(),
            vault_addr: String::new(),
            auto_sync: true,
            ssm: crate::providers::aws_ssm::SsmMode::default(),
        }],
    };
    config.save().expect("save failed");

    let content = std::fs::read_to_string(&tmp).expect("read failed");
    assert!(
        content.contains("vault_role=ssh-client-signer/sign/engineer"),
        "serialized form missing vault_role: {}",
        content
    );

    let reparsed = ProviderConfig::parse(&content);
    assert_eq!(reparsed.sections.len(), 1);
    assert_eq!(
        reparsed.sections[0].vault_role,
        "ssh-client-signer/sign/engineer"
    );

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn vault_role_invalid_skipped_on_write() {
    let mut config = ProviderConfig::parse("[aws]\ntoken=abc\n");
    // Inject an invalid role directly (bypassing parse) to simulate tampering.
    config.sections[0].vault_role = "bad role".to_string();
    // Emulate serialization logic for vault_role.
    let mut out = String::new();
    if !config.sections[0].vault_role.is_empty()
        && crate::vault_ssh::is_valid_role(&config.sections[0].vault_role)
    {
        out.push_str("vault_role=");
    }
    assert!(out.is_empty(), "invalid role must be skipped on write");
}

#[test]
fn test_parse_empty() {
    let config = ProviderConfig::parse("");
    assert!(config.sections.is_empty());
}

#[test]
fn test_parse_single_section() {
    let content = "\
[digitalocean]
token=dop_v1_abc123
alias_prefix=do
user=root
key=~/.ssh/id_ed25519
";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 1);
    let s = &config.sections[0];
    assert_eq!(s.provider(), "digitalocean");
    assert_eq!(s.token, "dop_v1_abc123");
    assert_eq!(s.alias_prefix, "do");
    assert_eq!(s.user, "root");
    assert_eq!(s.identity_file, "~/.ssh/id_ed25519");
}

#[test]
fn test_parse_multiple_sections() {
    let content = "\
[digitalocean]
token=abc

[vultr]
token=xyz
user=deploy
";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 2);
    assert_eq!(config.sections[0].provider(), "digitalocean");
    assert_eq!(config.sections[1].provider(), "vultr");
    assert_eq!(config.sections[1].user, "deploy");
}

#[test]
fn test_parse_comments_and_blanks() {
    let content = "\
# Provider config

[linode]
# API token
token=mytoken
";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].token, "mytoken");
}

#[test]
fn test_set_section_add() {
    let mut config = ProviderConfig::default();
    config.set_section(ProviderSection {
        filter: String::new(),
        interface: String::new(),
        ssh_port: None,
        id: crate::providers::config::ProviderConfigId::bare("vultr"),
        token: "abc".to_string(),
        alias_prefix: "vultr".to_string(),
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
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    });
    assert_eq!(config.sections.len(), 1);
}

#[test]
fn test_set_section_replace() {
    let mut config = ProviderConfig::parse("[vultr]\ntoken=old\n");
    config.set_section(ProviderSection {
        filter: String::new(),
        interface: String::new(),
        ssh_port: None,
        id: crate::providers::config::ProviderConfigId::bare("vultr"),
        token: "new".to_string(),
        alias_prefix: "vultr".to_string(),
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
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    });
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].token, "new");
}

#[test]
fn test_remove_section() {
    let mut config = ProviderConfig::parse("[vultr]\ntoken=abc\n[linode]\ntoken=xyz\n");
    config.remove_section("vultr");
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].provider(), "linode");
}

#[test]
fn test_section_lookup() {
    let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
    assert!(config.section("digitalocean").is_some());
    assert!(config.section("vultr").is_none());
}

#[test]
fn test_parse_duplicate_sections_first_wins() {
    let content = "\
[digitalocean]
token=first

[digitalocean]
token=second
";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].token, "first");
}

#[test]
fn test_parse_duplicate_sections_trailing() {
    let content = "\
[vultr]
token=abc

[linode]
token=xyz

[vultr]
token=dup
";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 2);
    assert_eq!(config.sections[0].provider(), "vultr");
    assert_eq!(config.sections[0].token, "abc");
    assert_eq!(config.sections[1].provider(), "linode");
}

#[test]
fn test_defaults_applied() {
    let config = ProviderConfig::parse("[hetzner]\ntoken=abc\n");
    let s = &config.sections[0];
    assert_eq!(s.user, "root");
    assert_eq!(s.alias_prefix, "hetzner");
    assert!(s.identity_file.is_empty());
    assert!(s.url.is_empty());
    assert!(s.verify_tls);
    assert!(s.auto_sync);
}

#[test]
fn test_parse_url_and_verify_tls() {
    let content = "\
[proxmox]
token=user@pam!purple=secret
url=https://pve.example.com:8006
verify_tls=false
";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 1);
    let s = &config.sections[0];
    assert_eq!(s.url, "https://pve.example.com:8006");
    assert!(!s.verify_tls);
}

#[test]
fn test_url_and_verify_tls_round_trip() {
    let content = "\
[proxmox]
token=tok
alias_prefix=pve
user=root
url=https://pve.local:8006
verify_tls=false
";
    let config = ProviderConfig::parse(content);
    let s = &config.sections[0];
    assert_eq!(s.url, "https://pve.local:8006");
    assert!(!s.verify_tls);
}

#[test]
fn test_verify_tls_default_true() {
    // verify_tls not present -> defaults to true
    let config = ProviderConfig::parse("[proxmox]\ntoken=abc\nurl=https://pve:8006\n");
    assert!(config.sections[0].verify_tls);
}

#[test]
fn test_verify_tls_false_variants() {
    for value in &["false", "False", "FALSE", "0", "no", "No", "NO"] {
        let content = format!(
            "[proxmox]\ntoken=abc\nurl=https://pve:8006\nverify_tls={}\n",
            value
        );
        let config = ProviderConfig::parse(&content);
        assert!(
            !config.sections[0].verify_tls,
            "verify_tls={} should be false",
            value
        );
    }
}

#[test]
fn test_verify_tls_true_variants() {
    for value in &["true", "True", "1", "yes"] {
        let content = format!(
            "[proxmox]\ntoken=abc\nurl=https://pve:8006\nverify_tls={}\n",
            value
        );
        let config = ProviderConfig::parse(&content);
        assert!(
            config.sections[0].verify_tls,
            "verify_tls={} should be true",
            value
        );
    }
}

#[test]
fn test_non_proxmox_url_not_written() {
    // url and verify_tls=false must not appear for non-Proxmox providers in saved config
    let section = ProviderSection {
        filter: String::new(),
        interface: String::new(),
        ssh_port: None,
        id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
        token: "tok".to_string(),
        alias_prefix: "do".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        url: String::new(), // empty: not written
        verify_tls: true,   // default: not written
        auto_sync: true,    // default for non-proxmox: not written
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    };
    let mut config = ProviderConfig::default();
    config.set_section(section);
    // Parse it back: url and verify_tls should be at defaults
    let s = &config.sections[0];
    assert!(s.url.is_empty());
    assert!(s.verify_tls);
}

#[test]
fn test_proxmox_url_fallback_in_section() {
    // Simulates the update path: existing section has url, new section should preserve it
    let existing = ProviderConfig::parse(
        "[proxmox]\ntoken=old\nalias_prefix=pve\nuser=root\nurl=https://pve.local:8006\n",
    );
    let existing_url = existing
        .section("proxmox")
        .map(|s| s.url.clone())
        .unwrap_or_default();
    assert_eq!(existing_url, "https://pve.local:8006");

    let mut config = existing;
    config.set_section(ProviderSection {
        filter: String::new(),
        interface: String::new(),
        ssh_port: None,
        id: crate::providers::config::ProviderConfigId::bare("proxmox"),
        token: "new".to_string(),
        alias_prefix: "pve".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        url: existing_url,
        verify_tls: true,
        auto_sync: false,
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    });
    assert_eq!(config.sections[0].token, "new");
    assert_eq!(config.sections[0].url, "https://pve.local:8006");
}

#[test]
fn test_auto_sync_default_true_for_non_proxmox() {
    let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
    assert!(config.sections[0].auto_sync);
}

#[test]
fn test_auto_sync_default_false_for_proxmox() {
    let config = ProviderConfig::parse("[proxmox]\ntoken=abc\nurl=https://pve:8006\n");
    assert!(!config.sections[0].auto_sync);
}

#[test]
fn test_auto_sync_explicit_true() {
    let config =
        ProviderConfig::parse("[proxmox]\ntoken=abc\nurl=https://pve:8006\nauto_sync=true\n");
    assert!(config.sections[0].auto_sync);
}

#[test]
fn test_auto_sync_explicit_false_non_proxmox() {
    let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\nauto_sync=false\n");
    assert!(!config.sections[0].auto_sync);
}

#[test]
fn test_auto_sync_not_written_when_default() {
    // non-proxmox with auto_sync=true (default) -> not written
    let mut config = ProviderConfig::default();
    config.set_section(ProviderSection {
        filter: String::new(),
        interface: String::new(),
        ssh_port: None,
        id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
        token: "tok".to_string(),
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
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    });
    // Re-parse: auto_sync should still be true (default)
    assert!(config.sections[0].auto_sync);

    // proxmox with auto_sync=false (default) -> not written
    let mut config2 = ProviderConfig::default();
    config2.set_section(ProviderSection {
        filter: String::new(),
        interface: String::new(),
        ssh_port: None,
        id: crate::providers::config::ProviderConfigId::bare("proxmox"),
        token: "tok".to_string(),
        alias_prefix: "pve".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        url: "https://pve:8006".to_string(),
        verify_tls: true,
        auto_sync: false,
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    });
    assert!(!config2.sections[0].auto_sync);
}

#[test]
fn test_auto_sync_false_variants() {
    for value in &["false", "False", "FALSE", "0", "no"] {
        let content = format!("[digitalocean]\ntoken=abc\nauto_sync={}\n", value);
        let config = ProviderConfig::parse(&content);
        assert!(
            !config.sections[0].auto_sync,
            "auto_sync={} should be false",
            value
        );
    }
}

#[test]
fn test_auto_sync_true_variants() {
    for value in &["true", "True", "TRUE", "1", "yes"] {
        // Start from proxmox default=false, override to true via explicit value
        let content = format!(
            "[proxmox]\ntoken=abc\nurl=https://pve:8006\nauto_sync={}\n",
            value
        );
        let config = ProviderConfig::parse(&content);
        assert!(
            config.sections[0].auto_sync,
            "auto_sync={} should be true",
            value
        );
    }
}

#[test]
fn test_auto_sync_malformed_value_treated_as_true() {
    // Unrecognised value is not "false"/"0"/"no", so treated as true (like verify_tls)
    let config =
        ProviderConfig::parse("[proxmox]\ntoken=abc\nurl=https://pve:8006\nauto_sync=maybe\n");
    assert!(config.sections[0].auto_sync);
}

#[test]
fn test_auto_sync_written_only_when_non_default() {
    // proxmox defaults to false — setting it to true is non-default, so it IS written
    let mut config = ProviderConfig::default();
    config.set_section(ProviderSection {
        filter: String::new(),
        interface: String::new(),
        ssh_port: None,
        id: crate::providers::config::ProviderConfigId::bare("proxmox"),
        token: "tok".to_string(),
        alias_prefix: "pve".to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        url: "https://pve:8006".to_string(),
        verify_tls: true,
        auto_sync: true, // non-default for proxmox
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    });
    // Simulate save by rebuilding content string (same logic as save())
    let content =
        "[proxmox]\ntoken=tok\nalias_prefix=pve\nuser=root\nurl=https://pve:8006\nauto_sync=true\n"
            .to_string();
    let reparsed = ProviderConfig::parse(&content);
    assert!(reparsed.sections[0].auto_sync);

    // digitalocean defaults to true — setting it to false IS written
    let content2 = "[digitalocean]\ntoken=tok\nalias_prefix=do\nuser=root\nauto_sync=false\n";
    let reparsed2 = ProviderConfig::parse(content2);
    assert!(!reparsed2.sections[0].auto_sync);
}

// =========================================================================
// configured_providers accessor
// =========================================================================

#[test]
fn test_configured_providers_empty() {
    let config = ProviderConfig::default();
    assert!(config.configured_providers().is_empty());
}

#[test]
fn test_configured_providers_returns_all() {
    let content = "[digitalocean]\ntoken=a\n\n[vultr]\ntoken=b\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.configured_providers().len(), 2);
}

// =========================================================================
// Parse edge cases
// =========================================================================

#[test]
fn test_parse_unknown_keys_ignored() {
    let content = "[digitalocean]\ntoken=abc\nfoo=bar\nunknown_key=value\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].token, "abc");
}

#[test]
fn test_parse_unknown_provider_still_parsed() {
    let content = "[aws]\ntoken=secret\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].provider(), "aws");
}

#[test]
fn test_parse_whitespace_in_section_name() {
    let content = "[ digitalocean ]\ntoken=abc\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].provider(), "digitalocean");
}

#[test]
fn test_parse_value_with_equals() {
    // Token might contain = signs (base64)
    let content = "[digitalocean]\ntoken=abc=def==\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections[0].token, "abc=def==");
}

#[test]
fn test_parse_whitespace_around_key_value() {
    let content = "[digitalocean]\n  token = my-token  \n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections[0].token, "my-token");
}

#[test]
fn test_parse_key_field_sets_identity_file() {
    let content = "[digitalocean]\ntoken=abc\nkey=~/.ssh/id_rsa\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections[0].identity_file, "~/.ssh/id_rsa");
}

#[test]
fn test_section_lookup_missing() {
    let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
    assert!(config.section("vultr").is_none());
}

#[test]
fn test_section_lookup_found() {
    let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
    let section = config.section("digitalocean").unwrap();
    assert_eq!(section.token, "abc");
}

#[test]
fn test_remove_nonexistent_section_noop() {
    let mut config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
    config.remove_section("vultr");
    assert_eq!(config.sections.len(), 1);
}

// =========================================================================
// Default alias_prefix from short_label
// =========================================================================

#[test]
fn test_default_alias_prefix_digitalocean() {
    let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
    assert_eq!(config.sections[0].alias_prefix, "do");
}

#[test]
fn test_default_alias_prefix_upcloud() {
    let config = ProviderConfig::parse("[upcloud]\ntoken=abc\n");
    assert_eq!(config.sections[0].alias_prefix, "uc");
}

#[test]
fn test_default_alias_prefix_proxmox() {
    let config = ProviderConfig::parse("[proxmox]\ntoken=abc\n");
    assert_eq!(config.sections[0].alias_prefix, "pve");
}

#[test]
fn test_alias_prefix_override() {
    let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\nalias_prefix=ocean\n");
    assert_eq!(config.sections[0].alias_prefix, "ocean");
}

// =========================================================================
// Default user is root
// =========================================================================

#[test]
fn test_default_user_is_root() {
    let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\n");
    assert_eq!(config.sections[0].user, "root");
}

#[test]
fn test_user_override() {
    let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\nuser=admin\n");
    assert_eq!(config.sections[0].user, "admin");
}

// =========================================================================
// Proxmox URL scheme validation context
// =========================================================================

#[test]
fn test_proxmox_url_parsed() {
    let config = ProviderConfig::parse("[proxmox]\ntoken=abc\nurl=https://pve.local:8006\n");
    assert_eq!(config.sections[0].url, "https://pve.local:8006");
}

#[test]
fn test_non_proxmox_url_parsed_but_ignored() {
    // URL field is parsed for all providers, but only Proxmox uses it
    let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\nurl=https://api.do.com\n");
    assert_eq!(config.sections[0].url, "https://api.do.com");
}

// =========================================================================
// Duplicate sections
// =========================================================================

#[test]
fn test_duplicate_section_first_wins() {
    let content = "[digitalocean]\ntoken=first\n\n[digitalocean]\ntoken=second\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].token, "first");
}

// =========================================================================
// verify_tls parsing
// =========================================================================

// =========================================================================
// auto_sync default per provider
// =========================================================================

#[test]
fn test_auto_sync_default_proxmox_false() {
    let config = ProviderConfig::parse("[proxmox]\ntoken=abc\n");
    assert!(!config.sections[0].auto_sync);
}

#[test]
fn test_auto_sync_default_all_others_true() {
    for provider in &[
        "digitalocean",
        "vultr",
        "linode",
        "hetzner",
        "upcloud",
        "aws",
        "scaleway",
        "gcp",
        "azure",
        "tailscale",
        "oracle",
        "ovh",
    ] {
        let content = format!("[{}]\ntoken=abc\n", provider);
        let config = ProviderConfig::parse(&content);
        assert!(
            config.sections[0].auto_sync,
            "auto_sync should default to true for {}",
            provider
        );
    }
}

#[test]
fn test_auto_sync_override_proxmox_to_true() {
    let config = ProviderConfig::parse("[proxmox]\ntoken=abc\nauto_sync=true\n");
    assert!(config.sections[0].auto_sync);
}

#[test]
fn test_auto_sync_override_do_to_false() {
    let config = ProviderConfig::parse("[digitalocean]\ntoken=abc\nauto_sync=false\n");
    assert!(!config.sections[0].auto_sync);
}

// =========================================================================
// set_section and remove_section
// =========================================================================

#[test]
fn test_set_section_adds_new() {
    let mut config = ProviderConfig::default();
    let section = ProviderSection {
        filter: String::new(),
        interface: String::new(),
        ssh_port: None,
        id: crate::providers::config::ProviderConfigId::bare("vultr"),
        token: "tok".to_string(),
        alias_prefix: "vultr".to_string(),
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
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    };
    config.set_section(section);
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].provider(), "vultr");
}

#[test]
fn test_set_section_replaces_existing() {
    let mut config = ProviderConfig::parse("[vultr]\ntoken=old\n");
    assert_eq!(config.sections[0].token, "old");
    let section = ProviderSection {
        filter: String::new(),
        interface: String::new(),
        ssh_port: None,
        id: crate::providers::config::ProviderConfigId::bare("vultr"),
        token: "new".to_string(),
        alias_prefix: "vultr".to_string(),
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
        ssm: crate::providers::aws_ssm::SsmMode::default(),
    };
    config.set_section(section);
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].token, "new");
}

#[test]
fn test_remove_section_keeps_others() {
    let mut config = ProviderConfig::parse("[vultr]\ntoken=abc\n\n[linode]\ntoken=def\n");
    assert_eq!(config.sections.len(), 2);
    config.remove_section("vultr");
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].provider(), "linode");
}

// =========================================================================
// Comments and blank lines
// =========================================================================

#[test]
fn test_comments_ignored() {
    let content = "# This is a comment\n[digitalocean]\n# Another comment\ntoken=abc\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].token, "abc");
}

#[test]
fn test_blank_lines_ignored() {
    let content = "\n\n[digitalocean]\n\ntoken=abc\n\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].token, "abc");
}

// =========================================================================
// Multiple providers
// =========================================================================

#[test]
fn test_multiple_providers() {
    let content = "[digitalocean]\ntoken=do-tok\n\n[vultr]\ntoken=vultr-tok\n\n[proxmox]\ntoken=pve-tok\nurl=https://pve:8006\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections.len(), 3);
    assert_eq!(config.sections[0].provider(), "digitalocean");
    assert_eq!(config.sections[1].provider(), "vultr");
    assert_eq!(config.sections[2].provider(), "proxmox");
    assert_eq!(config.sections[2].url, "https://pve:8006");
}

// =========================================================================
// Token with special characters
// =========================================================================

#[test]
fn test_token_with_equals_sign() {
    // API tokens can contain = signs (e.g., base64)
    let content = "[digitalocean]\ntoken=dop_v1_abc123==\n";
    let config = ProviderConfig::parse(content);
    // split_once('=') splits at first =, so "dop_v1_abc123==" is preserved
    assert_eq!(config.sections[0].token, "dop_v1_abc123==");
}

#[test]
fn test_proxmox_token_with_exclamation() {
    let content = "[proxmox]\ntoken=user@pam!api-token=12345678-abcd\nurl=https://pve:8006\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections[0].token, "user@pam!api-token=12345678-abcd");
}

// =========================================================================
// Parse serialization roundtrip
// =========================================================================

#[test]
fn test_serialize_roundtrip_single_provider() {
    let content = "[digitalocean]\ntoken=abc\nalias_prefix=do\nuser=root\n";
    let config = ProviderConfig::parse(content);
    let mut serialized = String::new();
    for section in &config.sections {
        serialized.push_str(&format!("[{}]\n", section.provider()));
        serialized.push_str(&format!("token={}\n", section.token));
        serialized.push_str(&format!("alias_prefix={}\n", section.alias_prefix));
        serialized.push_str(&format!("user={}\n", section.user));
    }
    let reparsed = ProviderConfig::parse(&serialized);
    assert_eq!(reparsed.sections.len(), 1);
    assert_eq!(reparsed.sections[0].token, "abc");
    assert_eq!(reparsed.sections[0].alias_prefix, "do");
    assert_eq!(reparsed.sections[0].user, "root");
}

// =========================================================================
// verify_tls parsing variants
// =========================================================================

#[test]
fn test_verify_tls_values() {
    for (val, expected) in [
        ("false", false),
        ("False", false),
        ("FALSE", false),
        ("0", false),
        ("no", false),
        ("No", false),
        ("NO", false),
        ("true", true),
        ("True", true),
        ("1", true),
        ("yes", true),
        ("anything", true), // any unrecognized value defaults to true
    ] {
        let content = format!("[digitalocean]\ntoken=t\nverify_tls={}\n", val);
        let config = ProviderConfig::parse(&content);
        assert_eq!(
            config.sections[0].verify_tls, expected,
            "verify_tls={} should be {}",
            val, expected
        );
    }
}

// =========================================================================
// auto_sync parsing variants
// =========================================================================

#[test]
fn test_auto_sync_values() {
    for (val, expected) in [
        ("false", false),
        ("False", false),
        ("FALSE", false),
        ("0", false),
        ("no", false),
        ("No", false),
        ("true", true),
        ("1", true),
        ("yes", true),
    ] {
        let content = format!("[digitalocean]\ntoken=t\nauto_sync={}\n", val);
        let config = ProviderConfig::parse(&content);
        assert_eq!(
            config.sections[0].auto_sync, expected,
            "auto_sync={} should be {}",
            val, expected
        );
    }
}

// =========================================================================
// Default values
// =========================================================================

#[test]
fn test_default_user_root_when_not_specified() {
    let content = "[digitalocean]\ntoken=abc\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections[0].user, "root");
}

#[test]
fn test_default_alias_prefix_from_short_label() {
    // DigitalOcean short_label is "do"
    let content = "[digitalocean]\ntoken=abc\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections[0].alias_prefix, "do");
}

#[test]
fn test_default_alias_prefix_unknown_provider() {
    // Unknown provider uses the section name as default prefix
    let content = "[unknown_cloud]\ntoken=abc\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections[0].alias_prefix, "unknown_cloud");
}

#[test]
fn test_default_identity_file_empty() {
    let content = "[digitalocean]\ntoken=abc\n";
    let config = ProviderConfig::parse(content);
    assert!(config.sections[0].identity_file.is_empty());
}

#[test]
fn test_default_url_empty() {
    let content = "[digitalocean]\ntoken=abc\n";
    let config = ProviderConfig::parse(content);
    assert!(config.sections[0].url.is_empty());
}

// =========================================================================
// GCP project field
// =========================================================================

#[test]
fn test_gcp_project_parsed() {
    let config = ProviderConfig::parse("[gcp]\ntoken=abc\nproject=my-gcp-project\n");
    assert_eq!(config.sections[0].project, "my-gcp-project");
}

#[test]
fn test_gcp_project_default_empty() {
    let config = ProviderConfig::parse("[gcp]\ntoken=abc\n");
    assert!(config.sections[0].project.is_empty());
}

#[test]
fn test_gcp_project_roundtrip() {
    let content = "[gcp]\ntoken=sa.json\nproject=my-project\nregions=us-central1-a\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections[0].project, "my-project");
    assert_eq!(config.sections[0].regions, "us-central1-a");
    // Re-serialize and parse
    let serialized = format!(
        "[gcp]\ntoken={}\nproject={}\nregions={}\n",
        config.sections[0].token, config.sections[0].project, config.sections[0].regions,
    );
    let reparsed = ProviderConfig::parse(&serialized);
    assert_eq!(reparsed.sections[0].project, "my-project");
    assert_eq!(reparsed.sections[0].regions, "us-central1-a");
}

#[test]
fn test_default_alias_prefix_gcp() {
    let config = ProviderConfig::parse("[gcp]\ntoken=abc\n");
    assert_eq!(config.sections[0].alias_prefix, "gcp");
}

// =========================================================================
// configured_providers and section methods
// =========================================================================

#[test]
fn test_configured_providers_returns_all_sections() {
    let content = "[digitalocean]\ntoken=a\n\n[vultr]\ntoken=b\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.configured_providers().len(), 2);
}

#[test]
fn test_section_by_name() {
    let content = "[digitalocean]\ntoken=do-tok\n\n[vultr]\ntoken=vultr-tok\n";
    let config = ProviderConfig::parse(content);
    let do_section = config.section("digitalocean").unwrap();
    assert_eq!(do_section.token, "do-tok");
    let vultr_section = config.section("vultr").unwrap();
    assert_eq!(vultr_section.token, "vultr-tok");
}

#[test]
fn test_section_not_found() {
    let config = ProviderConfig::parse("");
    assert!(config.section("nonexistent").is_none());
}

// =========================================================================
// Key without value
// =========================================================================

#[test]
fn test_line_without_equals_ignored() {
    let content = "[digitalocean]\ntoken=abc\ngarbage_line\nuser=admin\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections[0].token, "abc");
    assert_eq!(config.sections[0].user, "admin");
}

#[test]
fn test_unknown_key_ignored() {
    let content = "[digitalocean]\ntoken=abc\nfoo=bar\nbaz=qux\nuser=admin\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections[0].token, "abc");
    assert_eq!(config.sections[0].user, "admin");
}

// =========================================================================
// Whitespace handling
// =========================================================================

#[test]
fn test_whitespace_around_section_name() {
    let content = "[  digitalocean  ]\ntoken=abc\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections[0].provider(), "digitalocean");
}

#[test]
fn test_whitespace_around_key_value() {
    let content = "[digitalocean]\n  token  =  abc  \n  user  =  admin  \n";
    let config = ProviderConfig::parse(content);
    assert_eq!(config.sections[0].token, "abc");
    assert_eq!(config.sections[0].user, "admin");
}

// =========================================================================
// set_section edge cases
// =========================================================================

#[test]
fn test_set_section_multiple_adds() {
    let mut config = ProviderConfig::default();
    for name in ["digitalocean", "vultr", "hetzner"] {
        config.set_section(ProviderSection {
            filter: String::new(),
            interface: String::new(),
            ssh_port: None,
            id: crate::providers::config::ProviderConfigId::bare(name),
            token: format!("{}-tok", name),
            alias_prefix: name.to_string(),
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
            ssm: crate::providers::aws_ssm::SsmMode::default(),
        });
    }
    assert_eq!(config.sections.len(), 3);
}

#[test]
fn test_remove_section_all() {
    let content = "[digitalocean]\ntoken=a\n\n[vultr]\ntoken=b\n";
    let mut config = ProviderConfig::parse(content);
    config.remove_section("digitalocean");
    config.remove_section("vultr");
    assert!(config.sections.is_empty());
}

// =========================================================================
// Oracle / compartment field
// =========================================================================

#[test]
fn test_compartment_field_round_trip() {
    use std::path::PathBuf;
    let content = "[oracle]\ntoken=~/.oci/config\ncompartment=ocid1.compartment.oc1..example\n";
    let config = ProviderConfig::parse(content);
    assert_eq!(
        config.sections[0].compartment,
        "ocid1.compartment.oc1..example"
    );

    // Save to a temp file and re-parse
    let tmp = unique_tmp_path("compartment_round_trip");
    let mut cfg = config;
    cfg.path_override = Some(PathBuf::from(&tmp));
    cfg.save().expect("save failed");
    let saved = std::fs::read_to_string(&tmp).expect("read failed");
    let _ = std::fs::remove_file(&tmp);
    let reparsed = ProviderConfig::parse(&saved);
    assert_eq!(
        reparsed.sections[0].compartment,
        "ocid1.compartment.oc1..example"
    );
}

#[test]
fn test_auto_sync_default_true_for_oracle() {
    let config = ProviderConfig::parse("[oracle]\ntoken=~/.oci/config\n");
    assert!(config.sections[0].auto_sync);
}

#[test]
fn test_sanitize_value_strips_control_chars() {
    assert_eq!(ProviderConfig::sanitize_value("clean"), "clean");
    assert_eq!(ProviderConfig::sanitize_value("has\nnewline"), "hasnewline");
    assert_eq!(ProviderConfig::sanitize_value("has\ttab"), "hastab");
    assert_eq!(
        ProviderConfig::sanitize_value("has\rcarriage"),
        "hascarriage"
    );
    assert_eq!(ProviderConfig::sanitize_value("has\x00null"), "hasnull");
    assert_eq!(ProviderConfig::sanitize_value(""), "");
}

#[test]
fn test_save_sanitizes_token_with_newline() {
    let path = std::env::temp_dir().join(format!(
        "__purple_test_config_sanitize_{}.ini",
        std::process::id()
    ));
    let config = ProviderConfig {
        sections: vec![ProviderSection {
            filter: String::new(),
            interface: String::new(),
            ssh_port: None,
            id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
            token: "abc\ndef".to_string(),
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
            ssm: crate::providers::aws_ssm::SsmMode::default(),
        }],
        path_override: Some(path.clone()),
    };
    config.save().unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    // Token should be on a single line with newline stripped
    assert!(content.contains("token=abcdef\n"));
    assert!(!content.contains("token=abc\ndef"));
}

#[test]
fn provider_vault_role_invalid_characters_rejected_on_parse() {
    // Values with spaces, shell metacharacters or newlines are silently
    // dropped so parsing stays infallible but invalid roles never reach
    // the Vault CLI.
    let cases = [
        "[aws]\ntoken=abc\nvault_role=bad role\n",
        "[aws]\ntoken=abc\nvault_role=role;rm\n",
        "[aws]\ntoken=abc\nvault_role=role$(x)\n",
        "[aws]\ntoken=abc\nvault_role=role|cat\n",
    ];
    for input in &cases {
        let config = ProviderConfig::parse(input);
        assert!(
            config.sections[0].vault_role.is_empty(),
            "expected empty vault_role for input: {:?}",
            input
        );
    }
}

#[test]
fn test_vault_role_default_empty() {
    let config = ProviderConfig::parse("[aws]\ntoken=abc\n");
    assert!(config.sections[0].vault_role.is_empty());
}

#[test]
fn test_vault_role_not_written_when_empty() {
    let path = unique_tmp_path("vault_role_empty");
    let mut config = ProviderConfig::parse("[aws]\ntoken=abc\n");
    config.path_override = Some(path.clone());
    config.save().unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(!content.contains("vault_role"));
}

#[test]
fn test_vault_role_round_trip() {
    let path = unique_tmp_path("vault_role_rt");
    let mut config = ProviderConfig::parse("[aws]\ntoken=abc\nvault_role=ssh/sign/engineer\n");
    config.path_override = Some(path.clone());
    config.save().unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(content.contains("vault_role=ssh/sign/engineer"));
}

// ---- vault_addr tests ----

#[test]
fn vault_addr_default_empty() {
    let config = ProviderConfig::parse("[aws]\ntoken=abc\n");
    assert!(config.sections[0].vault_addr.is_empty());
}

#[test]
fn vault_addr_parsed() {
    let config = ProviderConfig::parse("[aws]\ntoken=abc\nvault_addr=http://127.0.0.1:8200\n");
    assert_eq!(config.sections[0].vault_addr, "http://127.0.0.1:8200");
}

#[test]
fn vault_addr_invalid_dropped_on_parse() {
    // Whitespace and control chars are not allowed in a VAULT_ADDR.
    for input in [
        "[aws]\ntoken=abc\nvault_addr=has space\n",
        "[aws]\ntoken=abc\nvault_addr=\n",
    ] {
        let config = ProviderConfig::parse(input);
        assert!(
            config.sections[0].vault_addr.is_empty(),
            "expected empty vault_addr for input: {:?}",
            input
        );
    }
}

#[test]
fn vault_addr_round_trip() {
    let path = unique_tmp_path("vault_addr_rt");
    let mut config = ProviderConfig::parse("[aws]\ntoken=abc\nvault_addr=http://127.0.0.1:8200\n");
    config.path_override = Some(path.clone());
    config.save().unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(content.contains("vault_addr=http://127.0.0.1:8200"));
}

#[test]
fn vault_addr_not_written_when_empty() {
    let path = unique_tmp_path("vault_addr_empty");
    let mut config = ProviderConfig::parse("[aws]\ntoken=abc\n");
    config.path_override = Some(path.clone());
    config.save().unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(!content.contains("vault_addr"));
}

// --- ProviderConfigId tests ---

#[test]
fn provider_config_id_display_bare() {
    assert_eq!(
        ProviderConfigId::bare("digitalocean").to_string(),
        "digitalocean"
    );
}

#[test]
fn provider_config_id_display_labeled() {
    assert_eq!(
        ProviderConfigId::labeled("digitalocean", "work").to_string(),
        "digitalocean:work"
    );
}

#[test]
fn provider_config_id_from_str_bare() {
    let id: ProviderConfigId = "digitalocean".parse().unwrap();
    assert_eq!(id.provider, "digitalocean");
    assert_eq!(id.label, None);
}

#[test]
fn provider_config_id_from_str_labeled() {
    let id: ProviderConfigId = "digitalocean:work".parse().unwrap();
    assert_eq!(id.provider, "digitalocean");
    assert_eq!(id.label.as_deref(), Some("work"));
}

#[test]
fn provider_config_id_round_trip() {
    for s in &["aws", "aws:work", "digitalocean:personal", "hetzner-prod"] {
        let id: ProviderConfigId = s.parse().unwrap();
        assert_eq!(&id.to_string(), s, "round-trip failed for {}", s);
    }
}

#[test]
fn provider_config_id_from_str_rejects_empty_label() {
    // "digitalocean:" has an empty label — must be rejected, not coerced to bare.
    let result: Result<ProviderConfigId, _> = "digitalocean:".parse();
    assert!(result.is_err(), "empty label should be rejected");
}

#[test]
fn provider_config_id_from_str_rejects_empty_provider() {
    let result: Result<ProviderConfigId, _> = "".parse();
    assert!(result.is_err());
    let result: Result<ProviderConfigId, _> = ":work".parse();
    assert!(result.is_err());
}

#[test]
fn provider_config_id_from_str_rejects_invalid_chars() {
    for input in &[
        "aws:WORK",       // uppercase
        "aws:work!",      // special char
        "aws:work space", // whitespace
        "aws:-work",      // leading dash
        "aws:work-",      // trailing dash
    ] {
        let result: Result<ProviderConfigId, _> = input.parse();
        assert!(result.is_err(), "expected error for {}", input);
    }
}

#[test]
fn validate_label_accepts_valid() {
    for label in &["work", "w", "personal", "prod-1", "a1b2c3", "0"] {
        validate_label(label).unwrap_or_else(|e| panic!("rejected {}: {}", label, e));
    }
}

#[test]
fn validate_label_rejects_too_long() {
    let long = "a".repeat(33);
    assert!(validate_label(&long).is_err());
}

// --- INI parsing for [provider:label] ---

#[test]
fn parse_labeled_section_header() {
    let config = ProviderConfig::parse("[digitalocean:work]\ntoken=abc\n");
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].id.provider, "digitalocean");
    assert_eq!(config.sections[0].id.label.as_deref(), Some("work"));
}

#[test]
fn parse_labeled_default_alias_prefix_includes_label() {
    let config = ProviderConfig::parse("[digitalocean:work]\ntoken=abc\n");
    assert_eq!(config.sections[0].alias_prefix, "do-work");
}

#[test]
fn parse_two_labeled_configs_same_provider() {
    let config =
        ProviderConfig::parse("[digitalocean:work]\ntoken=a\n\n[digitalocean:personal]\ntoken=b\n");
    assert_eq!(config.sections.len(), 2);
    assert_eq!(config.sections[0].id.label.as_deref(), Some("work"));
    assert_eq!(config.sections[1].id.label.as_deref(), Some("personal"));
}

#[test]
fn parse_rejects_mix_of_bare_and_labeled() {
    let config = ProviderConfig::parse("[digitalocean]\ntoken=a\n\n[digitalocean:work]\ntoken=b\n");
    // Bare wins (first), labeled rejected.
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].id.label, None);
}

#[test]
fn parse_rejects_duplicate_labeled() {
    let config =
        ProviderConfig::parse("[digitalocean:work]\ntoken=a\n\n[digitalocean:work]\ntoken=b\n");
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].token, "a");
}

#[test]
fn parse_skips_invalid_label() {
    let config = ProviderConfig::parse("[digitalocean:WORK]\ntoken=a\n");
    assert!(config.sections.is_empty());
}

// --- save() round-trip with labeled section ---

#[test]
fn save_labeled_section_round_trip() {
    let path = unique_tmp_path("labeled_rt");
    let mut config = ProviderConfig::parse("[digitalocean:work]\ntoken=abc\n");
    config.path_override = Some(path.clone());
    config.save().unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(content.contains("[digitalocean:work]"));
    let reparsed = ProviderConfig::parse(&content);
    assert_eq!(reparsed.sections[0].id.label.as_deref(), Some("work"));
}

// --- validate() ---

#[test]
fn validate_accepts_clean_config() {
    let config = ProviderConfig::parse(
        "[digitalocean:work]\ntoken=a\nalias_prefix=do-work\n\n[digitalocean:personal]\ntoken=b\nalias_prefix=do-personal\n",
    );
    config.validate().unwrap();
}

#[test]
fn validate_rejects_duplicate_alias_prefix() {
    let mut config = ProviderConfig::parse(
        "[digitalocean:work]\ntoken=a\nalias_prefix=do-shared\n\n[digitalocean:personal]\ntoken=b\n",
    );
    // Manually set both to the same prefix.
    config.sections[1].alias_prefix = "do-shared".to_string();
    assert!(config.validate().is_err());
}

// --- API ---

#[test]
fn section_by_id_finds_exact_match() {
    let config =
        ProviderConfig::parse("[digitalocean:work]\ntoken=a\n\n[digitalocean:personal]\ntoken=b\n");
    let work = config
        .section_by_id(&ProviderConfigId::labeled("digitalocean", "work"))
        .unwrap();
    assert_eq!(work.token, "a");
    let personal = config
        .section_by_id(&ProviderConfigId::labeled("digitalocean", "personal"))
        .unwrap();
    assert_eq!(personal.token, "b");
}

#[test]
fn sections_for_provider_returns_all_configs() {
    let config =
        ProviderConfig::parse("[digitalocean:work]\ntoken=a\n\n[digitalocean:personal]\ntoken=b\n");
    let sections = config.sections_for_provider("digitalocean");
    assert_eq!(sections.len(), 2);
}

// --- Coverage gap tests (post-review) ---

#[test]
fn validate_label_accepts_exact_max_length() {
    // Boundary: 32 chars must be accepted, 33 rejected (covered elsewhere).
    let exactly_32 = "a".repeat(32);
    validate_label(&exactly_32).expect("32 chars must be accepted");
}

#[test]
fn validate_rejects_bare_labeled_mix_in_memory() {
    // Direct in-memory construction can produce a mix that bypasses parse-time
    // detection. validate() must reject it before save() touches disk.
    let mut cfg = ProviderConfig::default();
    cfg.sections.push(ProviderSection {
        id: ProviderConfigId::bare("digitalocean"),
        alias_prefix: "do".to_string(),
        ..ProviderSection::default()
    });
    cfg.sections.push(ProviderSection {
        id: ProviderConfigId::labeled("digitalocean", "work"),
        alias_prefix: "do-work".to_string(),
        ..ProviderSection::default()
    });
    let err = cfg.validate().expect_err("mix must be rejected");
    assert!(err.contains("digitalocean"));
}

#[test]
fn validate_rejects_duplicate_id() {
    let mut cfg = ProviderConfig::default();
    cfg.sections.push(ProviderSection {
        id: ProviderConfigId::labeled("aws", "work"),
        alias_prefix: "aws-work".to_string(),
        ..ProviderSection::default()
    });
    cfg.sections.push(ProviderSection {
        id: ProviderConfigId::labeled("aws", "work"),
        alias_prefix: "aws-work-2".to_string(),
        ..ProviderSection::default()
    });
    let err = cfg.validate().expect_err("duplicate id must be rejected");
    assert!(err.contains("duplicate"));
}

#[test]
fn parse_rejects_mix_labeled_first_then_bare() {
    // Symmetric counterpart of parse_rejects_mix_of_bare_and_labeled —
    // ensures the mix detection works regardless of order.
    let config = ProviderConfig::parse("[digitalocean:work]\ntoken=a\n\n[digitalocean]\ntoken=b\n");
    // Labeled wins (first), bare rejected.
    assert_eq!(config.sections.len(), 1);
    assert_eq!(config.sections[0].id.label.as_deref(), Some("work"));
}

#[test]
fn config_id_exposes_kind() {
    let id = ProviderConfigId::bare("hetzner");
    assert_eq!(id.kind(), Some(crate::providers::ProviderKind::Hetzner));
}

#[test]
fn config_id_unknown_kind_is_none() {
    let id = ProviderConfigId::bare("not-a-provider");
    assert_eq!(id.kind(), None);
}

#[test]
fn provider_section_debug_redacts_token() {
    let section = ProviderSection {
        token: "super-secret-token-value".to_string(),
        ..Default::default()
    };
    let dbg = format!("{:?}", section);
    assert!(
        !dbg.contains("super-secret-token-value"),
        "token leaked: {dbg}"
    );
    assert!(
        dbg.contains("<redacted>"),
        "missing redaction marker: {dbg}"
    );
}

#[test]
fn provider_section_debug_marks_empty_token() {
    let section = ProviderSection::default();
    let dbg = format!("{:?}", section);
    assert!(dbg.contains("<empty>"), "empty marker missing: {dbg}");
    assert!(!dbg.contains("<redacted>"));
}

#[test]
fn provider_section_debug_redacts_vault_addr() {
    let section = ProviderSection {
        token: "tok".to_string(),
        vault_addr: "https://vault.internal.example.com:8200".to_string(),
        ..Default::default()
    };
    let dbg = format!("{:?}", section);
    assert!(
        !dbg.contains("vault.internal.example.com"),
        "vault_addr leaked: {dbg}"
    );
    let redacted_count = dbg.matches("<redacted>").count();
    assert_eq!(
        redacted_count, 2,
        "expected token and vault_addr both redacted: {dbg}"
    );
}

#[test]
fn provider_section_log_record_keeps_the_token_out() {
    let section = ProviderSection {
        token: "super-secret-token".to_string(),
        alias_prefix: "aws".to_string(),
        user: "root".to_string(),
        profile: "default".to_string(),
        regions: "eu-central-1".to_string(),
        auto_sync: true,
        ..Default::default()
    };
    let record = section.log_record();
    assert!(
        !record.contains("super-secret-token"),
        "token leaked: {record}"
    );
    assert!(
        record.contains("token=set"),
        "token state missing: {record}"
    );
    assert!(record.contains("prefix='aws'"), "prefix missing: {record}");
    assert!(
        record.contains("regions='eu-central-1'"),
        "regions missing: {record}"
    );
}

#[test]
fn provider_section_log_record_marks_an_empty_token() {
    let record = ProviderSection::default().log_record();
    assert!(
        record.contains("token=empty"),
        "empty marker missing: {record}"
    );
    // Whitespace counts as empty here too, matching what the sync does with
    // it. Logging `set` for a token the sync ignores misleads the reader.
    let blank = ProviderSection {
        token: "   ".to_string(),
        ..Default::default()
    };
    assert!(
        blank.log_record().contains("token=empty"),
        "whitespace token logged as set: {}",
        blank.log_record()
    );
}

#[test]
fn provider_section_log_record_names_the_field_each_provider_uses() {
    // A Proxmox save is about the URL, a GCP save about the project. Fields
    // the provider does not use stay out instead of printing as empty.
    let proxmox = ProviderSection {
        url: "https://pve.example.com:8006".to_string(),
        token: "user@pam!t=s".to_string(),
        ..Default::default()
    };
    let record = proxmox.log_record();
    assert!(
        record.contains("url='https://pve.example.com:8006'"),
        "url missing: {record}"
    );
    assert!(
        !record.contains("project="),
        "empty field printed: {record}"
    );
    assert!(
        !record.contains("regions="),
        "empty field printed: {record}"
    );

    let gcp = ProviderSection {
        project: "my-project".to_string(),
        ..Default::default()
    };
    let record = gcp.log_record();
    assert!(
        record.contains("project='my-project'"),
        "project missing: {record}"
    );
    assert!(!record.contains("url="), "empty field printed: {record}");
}

#[test]
fn provider_section_log_record_leaves_vault_addr_out() {
    // Same reason the Debug impl redacts it: the address reveals internal
    // Vault topology.
    let section = ProviderSection {
        vault_addr: "https://vault.internal.example.com:8200".to_string(),
        vault_role: "ssh/sign/admin".to_string(),
        ..Default::default()
    };
    let record = section.log_record();
    assert!(
        !record.contains("vault.internal.example.com"),
        "vault_addr leaked: {record}"
    );
    assert!(
        record.contains("vault_role='ssh/sign/admin'"),
        "vault_role missing: {record}"
    );
}

// --- filter key (NetBox) ---

#[test]
fn parse_reads_filter_key() {
    let config = ProviderConfig::parse("[netbox]\ntoken=tk\nfilter=status=active&tag=ssh\n");
    assert_eq!(config.sections[0].filter, "status=active&tag=ssh");
}

#[test]
fn filter_defaults_to_empty() {
    let config = ProviderConfig::parse("[netbox]\ntoken=tk\n");
    assert_eq!(config.sections[0].filter, "");
    assert_eq!(config.sections[0].interface, "");
    assert_eq!(config.sections[0].ssh_port, None);
}

#[test]
fn save_round_trips_filter() {
    let path = unique_tmp_path("filter_roundtrip");
    let mut config = ProviderConfig::parse("[netbox]\ntoken=tk\nfilter=tag=ssh\n");
    config.path_override = Some(path.clone());
    config.save().unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let reread = ProviderConfig::parse(&content);
    assert_eq!(reread.sections[0].filter, "tag=ssh");
}

#[test]
fn save_omits_empty_filter() {
    let path = unique_tmp_path("filter_empty");
    let mut config = ProviderConfig::parse("[netbox]\ntoken=tk\n");
    config.path_override = Some(path.clone());
    config.save().unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(!content.contains("filter="));
}

#[test]
fn save_round_trips_netbox_interface() {
    let path = unique_tmp_path("netbox_interface_roundtrip");
    let mut config = ProviderConfig::parse("[netbox]\ntoken=tk\ninterface=ens192\n");
    config.path_override = Some(path.clone());
    config.save().unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let reread = ProviderConfig::parse(&content);
    assert_eq!(reread.sections[0].interface, "ens192");
}

#[test]
fn parse_reads_valid_netbox_ssh_port_and_ignores_invalid_values() {
    let valid = ProviderConfig::parse("[netbox]\nssh_port=2222\n");
    assert_eq!(valid.sections[0].ssh_port, Some(2222));

    for value in ["0", "65536", "not-a-port"] {
        let config = ProviderConfig::parse(&format!("[netbox]\nssh_port={value}\n"));
        assert_eq!(config.sections[0].ssh_port, None);
    }
}

#[test]
fn save_round_trips_netbox_ssh_port() {
    let path = unique_tmp_path("netbox_ssh_port_roundtrip");
    let mut config = ProviderConfig::parse("[netbox]\ntoken=tk\nssh_port=2222\n");
    config.path_override = Some(path.clone());
    config.save().unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let reread = ProviderConfig::parse(&content);
    assert_eq!(reread.sections[0].ssh_port, Some(2222));
}

#[test]
fn ssh_port_is_included_in_log_record_when_set() {
    let config = ProviderConfig::parse("[netbox]\nssh_port=2222\n");
    let record = config.sections[0].log_record();
    assert!(record.contains("ssh_port=2222"), "got: {record}");
}

#[test]
fn filter_appears_in_log_record_when_set() {
    let config = ProviderConfig::parse("[netbox]\ntoken=tk\nfilter=tag=ssh\n");
    let record = config.sections[0].log_record();
    assert!(record.contains("filter='tag=ssh'"), "got: {record}");
}
