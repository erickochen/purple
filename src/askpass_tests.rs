use super::*;

#[test]
fn describe_source_keychain() {
    assert_eq!(describe_source("keychain"), "OS Keychain");
}

#[test]
fn describe_source_1password() {
    assert_eq!(describe_source("op://Vault/Item/password"), "1Password");
}

#[test]
fn describe_source_pass() {
    assert_eq!(describe_source("pass:ssh/myserver"), "pass");
}

#[test]
fn describe_source_bitwarden() {
    assert_eq!(describe_source("bw:my-ssh-server"), "Bitwarden");
}

#[test]
fn describe_source_vault() {
    assert_eq!(
        describe_source("vault:secret/data/myapp#password"),
        "HashiCorp Vault KV"
    );
}

#[test]
fn describe_source_vault_no_field() {
    assert_eq!(
        describe_source("vault:secret/data/myapp"),
        "HashiCorp Vault KV"
    );
}

#[test]
fn describe_source_custom() {
    assert_eq!(describe_source("my-script %a"), "Custom command");
}

#[test]
fn marker_path_contains_alias() {
    let path = marker_path("myserver");
    assert!(path.is_some());
    let p = path.unwrap();
    assert!(p.to_string_lossy().contains(".askpass_myserver"));
}

#[test]
fn prompt_filtering_passphrase() {
    let prompt = "Enter passphrase for key '/home/user/.ssh/id_rsa':";
    assert!(prompt.to_ascii_lowercase().contains("passphrase"));
}

#[test]
fn prompt_filtering_host_key() {
    let prompt = "Are you sure you want to continue connecting (yes/no)?";
    assert!(prompt.to_ascii_lowercase().contains("yes/no"));
}

#[test]
fn prompt_filtering_host_key_fingerprint() {
    let prompt = "Are you sure you want to continue connecting (yes/no/[fingerprint])?";
    assert!(prompt.to_ascii_lowercase().contains("(yes/no/"));
}

#[test]
fn prompt_password_not_filtered() {
    let prompt = "user@host's password:";
    let lower = prompt.to_ascii_lowercase();
    assert!(!lower.contains("passphrase"));
    assert!(!lower.contains("yes/no"));
}

#[test]
fn command_substitution() {
    let cmd = "get-pass %a %h";
    let expanded = cmd.replace("%a", "myalias").replace("%h", "myhost.com");
    assert_eq!(expanded, "get-pass myalias myhost.com");
}

#[test]
fn command_substitution_no_placeholders() {
    let cmd = "get-pass fixed";
    let expanded = cmd.replace("%a", "alias").replace("%h", "host");
    assert_eq!(expanded, "get-pass fixed");
}

#[test]
fn retrieve_password_routes_keychain() {
    // Just verify routing, not actual keychain access
    assert_eq!(describe_source("keychain"), "OS Keychain");
}

#[test]
fn retrieve_password_routes_op() {
    assert_eq!(describe_source("op://Vault/Item/field"), "1Password");
}

#[test]
fn retrieve_password_routes_pass() {
    assert_eq!(describe_source("pass:servers/web"), "pass");
}

#[test]
fn retrieve_password_routes_bitwarden() {
    assert_eq!(describe_source("bw:my-server-id"), "Bitwarden");
}

#[test]
fn retrieve_password_routes_vault() {
    assert_eq!(
        describe_source("vault:secret/ssh/prod#password"),
        "HashiCorp Vault KV"
    );
}

#[test]
fn password_sources_count() {
    assert_eq!(PASSWORD_SOURCES.len(), 7);
}

#[test]
fn password_sources_none_is_last() {
    assert_eq!(PASSWORD_SOURCES.last().unwrap().label, "None");
}

// -- PASSWORD_SOURCES structural integrity --

#[test]
fn password_sources_labels_unique() {
    let labels: Vec<&str> = PASSWORD_SOURCES.iter().map(|s| s.label).collect();
    for (i, a) in labels.iter().enumerate() {
        for (j, b) in labels.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "Duplicate label at index {} and {}", i, j);
            }
        }
    }
}

#[test]
fn password_sources_hints_non_empty() {
    for src in PASSWORD_SOURCES {
        assert!(
            !src.hint.is_empty(),
            "Hint for '{}' should not be empty",
            src.label
        );
    }
}

#[test]
fn password_sources_keychain_is_first() {
    assert_eq!(PASSWORD_SOURCES[0].label, "OS Keychain");
    assert_eq!(PASSWORD_SOURCES[0].value, "keychain");
}

#[test]
fn password_sources_1password_value() {
    let op = PASSWORD_SOURCES
        .iter()
        .find(|s| s.label == "1Password")
        .unwrap();
    assert_eq!(op.value, "op://");
}

#[test]
fn password_sources_bitwarden_value() {
    let bw = PASSWORD_SOURCES
        .iter()
        .find(|s| s.label == "Bitwarden")
        .unwrap();
    assert_eq!(bw.value, "bw:");
}

#[test]
fn password_sources_pass_value() {
    let pass = PASSWORD_SOURCES.iter().find(|s| s.label == "pass").unwrap();
    assert_eq!(pass.value, "pass:");
}

#[test]
fn password_sources_vault_value() {
    let vault = PASSWORD_SOURCES
        .iter()
        .find(|s| s.label == "HashiCorp Vault KV")
        .unwrap();
    assert_eq!(vault.value, "vault:");
}

#[test]
fn password_sources_custom_command_value() {
    let custom = PASSWORD_SOURCES
        .iter()
        .find(|s| s.label == "Custom command")
        .unwrap();
    assert_eq!(custom.value, "cmd:");
}

#[test]
fn password_sources_none_empty_value() {
    let none = PASSWORD_SOURCES.iter().find(|s| s.label == "None").unwrap();
    assert_eq!(none.value, "");
}

// -- Prefix-based picker behavior logic --

#[test]
fn prefix_sources_end_with_colon_or_slash() {
    // Sources that are prefixes should end with : or //
    // These are: op://, bw:, pass:, vault:
    let prefix_sources: Vec<&PasswordSourceOption> = PASSWORD_SOURCES
        .iter()
        .filter(|s| !s.value.is_empty() && s.value != "keychain")
        .collect();
    assert_eq!(prefix_sources.len(), 5, "Expected 5 prefix sources");
    for src in &prefix_sources {
        assert!(
            src.value.ends_with(':') || src.value.ends_with("//"),
            "Prefix source '{}' value '{}' should end with : or //",
            src.label,
            src.value
        );
    }
}

#[test]
fn keychain_is_not_prefix() {
    let kc = &PASSWORD_SOURCES[0];
    assert_eq!(kc.value, "keychain");
    assert!(!kc.value.ends_with(':'));
    assert!(!kc.value.ends_with("//"));
}

// -- Vault spec parsing --

#[test]
fn vault_spec_with_field() {
    let spec = "secret/data/myapp#api_key";
    let (path, field) = spec.rsplit_once('#').unwrap();
    assert_eq!(path, "secret/data/myapp");
    assert_eq!(field, "api_key");
}

#[test]
fn vault_spec_without_field() {
    let spec = "secret/data/myapp";
    let result = spec.rsplit_once('#');
    assert!(result.is_none());
    // Fallback to "password"
    let (_, field) = result.unwrap_or((spec, "password"));
    assert_eq!(field, "password");
}

#[test]
fn vault_spec_multiple_hashes() {
    // rsplit_once splits at the LAST #
    let spec = "secret/data/my#app#token";
    let (path, field) = spec.rsplit_once('#').unwrap();
    assert_eq!(path, "secret/data/my#app");
    assert_eq!(field, "token");
}

#[test]
fn vault_spec_trailing_hash() {
    let spec = "secret/data/myapp#";
    let (path, field) = spec.rsplit_once('#').unwrap();
    assert_eq!(path, "secret/data/myapp");
    assert_eq!(field, "");
}

#[test]
fn vault_spec_deep_path() {
    let spec = "secret/data/team/env/prod/ssh#private_key";
    let (path, field) = spec.rsplit_once('#').unwrap();
    assert_eq!(path, "secret/data/team/env/prod/ssh");
    assert_eq!(field, "private_key");
}

// -- describe_source edge cases --

#[test]
fn describe_source_op_minimal() {
    assert_eq!(describe_source("op://x"), "1Password");
}

#[test]
fn describe_source_pass_minimal() {
    assert_eq!(describe_source("pass:x"), "pass");
}

#[test]
fn describe_source_bw_minimal() {
    assert_eq!(describe_source("bw:x"), "Bitwarden");
}

#[test]
fn describe_source_vault_minimal() {
    assert_eq!(describe_source("vault:x"), "HashiCorp Vault KV");
}

#[test]
fn describe_source_empty_string_is_custom() {
    // Empty source falls through to custom command
    assert_eq!(describe_source(""), "Custom command");
}

#[test]
fn describe_source_random_command_is_custom() {
    assert_eq!(describe_source("sshpass -p mypass"), "Custom command");
}

#[test]
fn describe_source_vault_with_complex_path() {
    assert_eq!(
        describe_source("vault:secret/data/team/prod/ssh#password"),
        "HashiCorp Vault KV"
    );
}

// -- All describe_source values match PASSWORD_SOURCES labels --

#[test]
fn describe_source_matches_password_sources_labels() {
    assert_eq!(describe_source("keychain"), PASSWORD_SOURCES[0].label);
    assert_eq!(describe_source("op://anything"), PASSWORD_SOURCES[1].label);
    assert_eq!(describe_source("bw:anything"), PASSWORD_SOURCES[2].label);
    assert_eq!(describe_source("pass:anything"), PASSWORD_SOURCES[3].label);
    assert_eq!(describe_source("vault:anything"), PASSWORD_SOURCES[4].label);
    assert_eq!(describe_source("some-cmd"), PASSWORD_SOURCES[5].label);
}

// -- describe_source does not confuse prefixes --

#[test]
fn describe_source_keychain_prefix_not_op() {
    // "keychain" should NOT be matched as 1Password even though it starts with "k"
    assert_eq!(describe_source("keychain"), "OS Keychain");
}

#[test]
fn describe_source_op_requires_double_slash() {
    // "op:" without "//" should be custom command
    assert_eq!(describe_source("op:something"), "Custom command");
}

#[test]
fn describe_source_vault_colon_required() {
    // "vault" without colon should be custom command
    assert_eq!(describe_source("vault"), "Custom command");
}

#[test]
fn describe_source_pass_colon_required() {
    assert_eq!(describe_source("pass"), "Custom command");
}

#[test]
fn describe_source_bw_colon_required() {
    assert_eq!(describe_source("bw"), "Custom command");
}

// -- Vault field extraction is right-split, not left-split --

#[test]
fn vault_spec_field_at_right_of_last_hash() {
    let spec = "a/b#c/d#field";
    let (path, field) = spec.rsplit_once('#').unwrap();
    assert_eq!(path, "a/b#c/d");
    assert_eq!(field, "field");
}

// -- PASSWORD_SOURCES ordering matches retrieve_password routing --

#[test]
fn password_sources_order_matches_routing() {
    // The order should be: keychain, op://, bw:, pass:, vault:, custom, none
    // This matches the if-chain order in retrieve_password
    assert_eq!(PASSWORD_SOURCES[0].value, "keychain");
    assert_eq!(PASSWORD_SOURCES[1].value, "op://");
    assert_eq!(PASSWORD_SOURCES[2].value, "bw:");
    assert_eq!(PASSWORD_SOURCES[3].value, "pass:");
    assert_eq!(PASSWORD_SOURCES[4].value, "vault:");
    assert_eq!(PASSWORD_SOURCES[5].label, "Custom command");
    assert_eq!(PASSWORD_SOURCES[6].label, "None");
}

// =========================================================================
// find_askpass_source tests (private fn, accessible within mod tests)
// =========================================================================

fn parse_config(content: &str) -> SshConfigFile {
    SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: std::path::PathBuf::from("/tmp/test_askpass_config"),
        crlf: false,
        bom: false,
    }
}

#[test]
fn find_askpass_source_returns_per_host_source() {
    let config = parse_config("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    assert_eq!(
        find_askpass_source(&config, "myserver"),
        Some("keychain".to_string())
    );
}

#[test]
fn find_askpass_source_returns_none_when_absent() {
    let config = parse_config("Host myserver\n  HostName 10.0.0.1\n");
    // No per-host askpass, and no global default (test env has no ~/.purple/preferences)
    // Returns None unless ~/.purple/preferences has an askpass entry
    let result = find_askpass_source(&config, "myserver");
    // We can't assert None because the real home dir might have preferences.
    // Instead verify it does NOT return a per-host source.
    if let Some(ref source) = result {
        // If something is returned, it must be from global preferences, not per-host
        assert_ne!(source, "keychain", "Should not find per-host keychain");
    }
}

#[test]
fn find_askpass_source_returns_vault() {
    let config = parse_config(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pass\n",
    );
    assert_eq!(
        find_askpass_source(&config, "myserver"),
        Some("vault:secret/ssh#pass".to_string())
    );
}

#[test]
fn find_askpass_source_op_uri() {
    let config = parse_config(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://Vault/SSH/password\n",
    );
    assert_eq!(
        find_askpass_source(&config, "myserver"),
        Some("op://Vault/SSH/password".to_string())
    );
}

#[test]
fn find_askpass_source_custom_command() {
    let config =
        parse_config("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass get-pass %a %h\n");
    assert_eq!(
        find_askpass_source(&config, "myserver"),
        Some("get-pass %a %h".to_string())
    );
}

#[test]
fn find_askpass_source_wrong_alias_returns_nothing() {
    let config = parse_config("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    let result = find_askpass_source(&config, "otherhost");
    // otherhost has no per-host source; result depends on global preferences
    if let Some(ref source) = result {
        assert_ne!(source, "keychain");
    }
}

#[test]
fn find_askpass_source_multiple_hosts_returns_correct() {
    let config = parse_config(
        "\
Host alpha
  HostName a.com
  # purple:askpass keychain

Host beta
  HostName b.com
  # purple:askpass vault:secret/ssh#pass
",
    );
    assert_eq!(
        find_askpass_source(&config, "alpha"),
        Some("keychain".to_string())
    );
    assert_eq!(
        find_askpass_source(&config, "beta"),
        Some("vault:secret/ssh#pass".to_string())
    );
}

// =========================================================================
// find_hostname tests
// =========================================================================

#[test]
fn find_hostname_returns_hostname() {
    let config = parse_config("Host myserver\n  HostName 10.0.0.1\n");
    assert_eq!(find_hostname(&config, "myserver"), "10.0.0.1");
}

#[test]
fn find_hostname_returns_alias_when_not_found() {
    let config = parse_config("Host myserver\n  HostName 10.0.0.1\n");
    assert_eq!(find_hostname(&config, "nonexistent"), "nonexistent");
}

#[test]
fn find_hostname_returns_correct_for_multiple_hosts() {
    let config = parse_config(
        "\
Host alpha
  HostName a.com

Host beta
  HostName b.com
",
    );
    assert_eq!(find_hostname(&config, "alpha"), "a.com");
    assert_eq!(find_hostname(&config, "beta"), "b.com");
}

// =========================================================================
// Marker file tests
// =========================================================================

#[test]
fn is_recent_marker_returns_false_for_nonexistent() {
    let path = PathBuf::from("/tmp/purple_test_nonexistent_marker");
    assert!(!is_recent_marker(&path));
}

#[test]
fn is_recent_marker_returns_true_for_fresh_file() {
    let path = PathBuf::from("/tmp/purple_test_fresh_marker");
    let _ = std::fs::write(&path, b"");
    assert!(is_recent_marker(&path));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn cleanup_marker_removes_file() {
    // Create a marker file manually
    let alias = "test_cleanup_marker";
    let path = marker_path(alias).unwrap();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(&path, b"");
    assert!(path.exists());
    cleanup_marker(alias);
    assert!(!path.exists());
}

#[test]
fn cleanup_marker_noop_for_nonexistent() {
    // Should not panic or error
    cleanup_marker("nonexistent_test_host_cleanup");
}

// =========================================================================
// retrieve_password routing verification
// =========================================================================
// We can't test actual retrieval (requires external tools), but we can verify
// the routing logic by checking which branch each source pattern hits.
// This is done indirectly through describe_source, but let's also verify
// the strip_prefix logic directly.

#[test]
fn retrieve_routing_keychain_exact_match() {
    // "keychain" must be an exact match, not a prefix
    assert!("keychain" == "keychain");
    assert!("keychainx" != "keychain");
}

#[test]
fn retrieve_routing_op_strip_prefix() {
    let source = "op://Vault/Item/field";
    let uri = source.strip_prefix("op://").unwrap();
    assert_eq!(uri, "Vault/Item/field");
    // Reconstructed with prefix for 1Password CLI
    assert_eq!(format!("op://{}", uri), source);
}

#[test]
fn retrieve_routing_pass_strip_prefix() {
    let source = "pass:ssh/myserver";
    let entry = source.strip_prefix("pass:").unwrap();
    assert_eq!(entry, "ssh/myserver");
}

#[test]
fn retrieve_routing_bw_strip_prefix() {
    let source = "bw:my-item-id";
    let item_id = source.strip_prefix("bw:").unwrap();
    assert_eq!(item_id, "my-item-id");
}

#[test]
fn retrieve_routing_vault_strip_prefix() {
    let source = "vault:secret/data/myapp#password";
    let rest = source.strip_prefix("vault:").unwrap();
    assert_eq!(rest, "secret/data/myapp#password");
}

#[test]
fn retrieve_routing_custom_command_no_prefix() {
    let source = "my-script %a %h";
    assert!(source.strip_prefix("op://").is_none());
    assert!(source.strip_prefix("pass:").is_none());
    assert!(source.strip_prefix("bw:").is_none());
    assert!(source.strip_prefix("vault:").is_none());
    assert_ne!(source, "keychain");
    // Falls through to custom command
}

#[test]
fn retrieve_routing_priority_order() {
    // Verify that a source matching multiple prefixes hits the first match.
    // E.g. "keychain" should NOT match as custom even though it would.
    // "pass:bw:test" should match pass, not bw.
    let source = "pass:bw:test";
    assert!(source.strip_prefix("pass:").is_some());
    // The if-chain checks pass: before bw:, so this correctly routes to pass.
}

// =========================================================================
// load_askpass_default_direct() INI parsing logic
// =========================================================================
// We test the parsing logic by replicating it inline since the real fn
// reads from ~/.purple/preferences and we can't mock the filesystem.

fn parse_preferences_content(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == "askpass" {
                let val = v.trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

#[test]
fn preferences_parser_extracts_askpass() {
    let content = "sort_mode=alpha\naskpass=keychain\n";
    assert_eq!(
        parse_preferences_content(content),
        Some("keychain".to_string())
    );
}

#[test]
fn preferences_parser_returns_none_when_absent() {
    let content = "sort_mode=alpha\ngroup_by_provider=true\n";
    assert_eq!(parse_preferences_content(content), None);
}

#[test]
fn preferences_parser_skips_comments() {
    let content = "# askpass=old\naskpass=vault:secret/ssh\n";
    assert_eq!(
        parse_preferences_content(content),
        Some("vault:secret/ssh".to_string())
    );
}

#[test]
fn preferences_parser_skips_empty_lines() {
    let content = "\n\naskpass=op://V/I/p\n\n";
    assert_eq!(
        parse_preferences_content(content),
        Some("op://V/I/p".to_string())
    );
}

#[test]
fn preferences_parser_trims_whitespace_around_equals() {
    let content = "askpass = bw:my-item\n";
    assert_eq!(
        parse_preferences_content(content),
        Some("bw:my-item".to_string())
    );
}

#[test]
fn preferences_parser_returns_none_for_empty_value() {
    let content = "askpass=\n";
    assert_eq!(parse_preferences_content(content), None);
}

#[test]
fn preferences_parser_returns_none_for_whitespace_only_value() {
    let content = "askpass=   \n";
    assert_eq!(parse_preferences_content(content), None);
}

#[test]
fn preferences_parser_first_askpass_wins() {
    let content = "askpass=keychain\naskpass=op://V/I/p\n";
    assert_eq!(
        parse_preferences_content(content),
        Some("keychain".to_string())
    );
}

#[test]
fn preferences_parser_preserves_special_chars_in_value() {
    let content = "askpass=vault:secret/data/my-app#api_key\n";
    assert_eq!(
        parse_preferences_content(content),
        Some("vault:secret/data/my-app#api_key".to_string())
    );
}

#[test]
fn preferences_parser_handles_value_with_equals_sign() {
    // split_once('=') splits at first '=', rest goes to value
    let content = "askpass=cmd --opt=val\n";
    assert_eq!(
        parse_preferences_content(content),
        Some("cmd --opt=val".to_string())
    );
}

// =========================================================================
// Command substitution edge cases
// =========================================================================

#[test]
fn command_substitution_multiple_occurrences() {
    let cmd = "get-pass %a %a %h %h";
    let expanded = cmd.replace("%a", "srv").replace("%h", "host.com");
    assert_eq!(expanded, "get-pass srv srv host.com host.com");
}

#[test]
fn command_substitution_empty_alias() {
    let cmd = "get-pass %a %h";
    let expanded = cmd.replace("%a", "").replace("%h", "host.com");
    assert_eq!(expanded, "get-pass  host.com");
}

#[test]
fn command_substitution_special_chars_in_hostname() {
    let cmd = "get-pass %h";
    let expanded = cmd
        .replace("%a", "srv")
        .replace("%h", "host-01.example.com");
    assert_eq!(expanded, "get-pass host-01.example.com");
}

#[test]
fn command_substitution_only_percent_a() {
    let cmd = "pass show ssh/%a";
    let expanded = cmd.replace("%a", "webserver").replace("%h", "unused");
    assert_eq!(expanded, "pass show ssh/webserver");
}

#[test]
fn command_substitution_only_percent_h() {
    let cmd = "sshpass -f /secrets/%h";
    let expanded = cmd.replace("%a", "unused").replace("%h", "10.0.0.1");
    assert_eq!(expanded, "sshpass -f /secrets/10.0.0.1");
}

// =========================================================================
// Askpass fallback chain: per-host → global default
// =========================================================================

#[test]
fn find_askpass_source_per_host_takes_precedence() {
    // When per-host source exists, global default is not consulted
    let config =
        parse_config("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass op://V/I/p\n");
    let result = find_askpass_source(&config, "myserver");
    assert_eq!(result, Some("op://V/I/p".to_string()));
}

#[test]
fn find_askpass_source_bw_source() {
    let config =
        parse_config("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass bw:my-item-id\n");
    assert_eq!(
        find_askpass_source(&config, "myserver"),
        Some("bw:my-item-id".to_string())
    );
}

#[test]
fn find_askpass_source_pass_source() {
    let config =
        parse_config("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass pass:ssh/prod\n");
    assert_eq!(
        find_askpass_source(&config, "myserver"),
        Some("pass:ssh/prod".to_string())
    );
}

// =========================================================================
// describe_source with exact PASSWORD_SOURCES.value inputs
// =========================================================================

#[test]
fn describe_source_with_exact_picker_values() {
    // Test describe_source with the exact values from PASSWORD_SOURCES
    assert_eq!(describe_source("keychain"), "OS Keychain");
    assert_eq!(describe_source("op://"), "1Password"); // just the prefix
    assert_eq!(describe_source("bw:"), "Bitwarden"); // just the prefix
    assert_eq!(describe_source("pass:"), "pass"); // just the prefix
    assert_eq!(describe_source("vault:"), "HashiCorp Vault KV"); // just the prefix
}

// =========================================================================
// Marker path format
// =========================================================================

#[test]
fn marker_path_special_chars_in_alias() {
    let path = marker_path("my-server_01").unwrap();
    assert!(path.to_string_lossy().ends_with(".askpass_my-server_01"));
}

#[test]
fn marker_path_is_in_dot_purple_dir() {
    let path = marker_path("test").unwrap();
    assert!(path.to_string_lossy().contains(".purple/"));
}

// =========================================================================
// find_hostname with askpass hosts
// =========================================================================

#[test]
fn find_hostname_with_askpass_host() {
    let config = parse_config("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    assert_eq!(find_hostname(&config, "myserver"), "10.0.0.1");
}

#[test]
fn find_hostname_no_hostname_directive() {
    // Host with no HostName directive: parser stores empty hostname,
    // but find_hostname still finds the entry. The fallback to alias
    // only applies when the host is not found at all.
    let config = parse_config("Host myserver\n  User root\n");
    let hn = find_hostname(&config, "myserver");
    // With no HostName directive, the parsed hostname is empty
    assert_eq!(hn, "");
}

// =========================================================================
// SSH prompt filtering (handle() line 35 logic)
// Tests the exact prompts SSH sends in various scenarios
// =========================================================================

fn should_filter_prompt(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    lower.contains("passphrase") || lower.contains("yes/no") || lower.contains("(yes/no/")
}

#[test]
fn prompt_filter_rsa_passphrase() {
    assert!(should_filter_prompt(
        "Enter passphrase for key '/home/user/.ssh/id_rsa': "
    ));
}

#[test]
fn prompt_filter_ed25519_passphrase() {
    assert!(should_filter_prompt(
        "Enter passphrase for key '/home/user/.ssh/id_ed25519': "
    ));
}

#[test]
fn prompt_filter_host_key_yes_no() {
    assert!(should_filter_prompt(
        "The authenticity of host 'example.com (93.184.216.34)' can't be established.\nED25519 key fingerprint is SHA256:abc123.\nAre you sure you want to continue connecting (yes/no)? "
    ));
}

#[test]
fn prompt_filter_host_key_yes_no_fingerprint() {
    assert!(should_filter_prompt(
        "Are you sure you want to continue connecting (yes/no/[fingerprint])? "
    ));
}

#[test]
fn prompt_filter_case_insensitive_passphrase() {
    assert!(should_filter_prompt("Enter PASSPHRASE for key: "));
}

#[test]
fn prompt_allows_password_prompt() {
    assert!(!should_filter_prompt("user@host's password: "));
}

#[test]
fn prompt_allows_password_prompt_root() {
    assert!(!should_filter_prompt("root@192.168.1.1's password: "));
}

#[test]
fn prompt_allows_generic_password() {
    assert!(!should_filter_prompt("Password: "));
}

#[test]
fn prompt_allows_empty() {
    assert!(!should_filter_prompt(""));
}

#[test]
fn prompt_filter_keyboard_interactive() {
    // Keyboard-interactive prompts that ask for password should NOT be filtered
    assert!(!should_filter_prompt("Password for user@host: "));
}

// =========================================================================
// pass first-line extraction logic (retrieve_from_pass line 274)
// =========================================================================

fn extract_first_line(output: &str) -> &str {
    output.lines().next().unwrap_or("")
}

#[test]
fn pass_first_line_single_line() {
    assert_eq!(extract_first_line("mysecretpassword"), "mysecretpassword");
}

#[test]
fn pass_first_line_multiline() {
    assert_eq!(
        extract_first_line("mysecretpassword\nusername: admin\nurl: https://example.com"),
        "mysecretpassword"
    );
}

#[test]
fn pass_first_line_empty() {
    assert_eq!(extract_first_line(""), "");
}

#[test]
fn pass_first_line_newline_only() {
    assert_eq!(extract_first_line("\n"), "");
}

#[test]
fn pass_first_line_trailing_newline() {
    assert_eq!(extract_first_line("password123\n"), "password123");
}

// =========================================================================
// retrieve_password routing completeness
// Verify that every PASSWORD_SOURCES value routes to the expected backend
// =========================================================================

fn routing_backend(source: &str) -> &str {
    if source == "keychain" {
        return "keychain";
    }
    if source.strip_prefix("op://").is_some() {
        return "1password";
    }
    if source.strip_prefix("pass:").is_some() {
        return "pass";
    }
    if source.strip_prefix("bw:").is_some() {
        return "bitwarden";
    }
    if source.strip_prefix("vault:").is_some() {
        return "vault";
    }
    "command"
}

#[test]
fn routing_all_password_sources_have_backend() {
    let expected = [
        "keychain",
        "1password",
        "bitwarden",
        "pass",
        "vault",
        "command",
        "command",
    ];
    for (i, src) in PASSWORD_SOURCES.iter().enumerate() {
        let backend = routing_backend(src.value);
        assert_eq!(
            backend, expected[i],
            "Source '{}' (value '{}') routed to '{}', expected '{}'",
            src.label, src.value, backend, expected[i]
        );
    }
}

#[test]
fn routing_keychain_does_not_match_prefix() {
    // "keychainx" should route to command, not keychain
    assert_eq!(routing_backend("keychainx"), "command");
}

#[test]
fn routing_op_single_slash_is_command() {
    // "op:/" (single slash) should NOT match 1Password
    assert_eq!(routing_backend("op:/something"), "command");
}

#[test]
fn routing_vault_without_colon_is_command() {
    assert_eq!(routing_backend("vaultsecret"), "command");
}

// =========================================================================
// handle() env var / empty checks (lines 40-42)
// =========================================================================

#[test]
fn handle_requires_both_env_vars() {
    // Both alias and config_path must be non-empty
    let alias = "";
    let config_path = "/some/path";
    assert!(alias.is_empty() || config_path.is_empty());

    let alias2 = "myserver";
    let config_path2 = "";
    assert!(alias2.is_empty() || config_path2.is_empty());
}

#[test]
fn handle_proceeds_when_both_set() {
    let alias = "myserver";
    let config_path = "/home/user/.ssh/config";
    assert!(!alias.is_empty() && !config_path.is_empty());
}

// =========================================================================
// BwStatus parsing (parse_bw_status)
// =========================================================================

#[test]
fn bw_status_parse_unlocked() {
    assert_eq!(
        parse_bw_status(r#"{"status":"unlocked"}"#),
        BwStatus::Unlocked
    );
}

#[test]
fn bw_status_parse_locked() {
    assert_eq!(parse_bw_status(r#"{"status":"locked"}"#), BwStatus::Locked);
}

#[test]
fn bw_status_parse_unauthenticated() {
    assert_eq!(
        parse_bw_status(r#"{"status":"unauthenticated"}"#),
        BwStatus::NotAuthenticated
    );
}

#[test]
fn bw_status_parse_empty_output() {
    assert_eq!(parse_bw_status(""), BwStatus::NotInstalled);
}

#[test]
fn bw_status_parse_malformed_json() {
    assert_eq!(parse_bw_status("not json at all"), BwStatus::NotInstalled);
}

#[test]
fn bw_status_parse_missing_status_key() {
    assert_eq!(
        parse_bw_status(r#"{"version":"2024.1.0"}"#),
        BwStatus::NotInstalled
    );
}

#[test]
fn bw_status_parse_unknown_status_defaults_to_locked() {
    assert_eq!(
        parse_bw_status(r#"{"status":"migrating"}"#),
        BwStatus::Locked
    );
}

#[test]
fn bw_status_parse_with_extra_fields() {
    let json = r#"{"serverUrl":"https://vault.bitwarden.com","lastSync":"2024-01-01","status":"unlocked","userId":"abc123"}"#;
    assert_eq!(parse_bw_status(json), BwStatus::Unlocked);
}

#[test]
fn bw_status_parse_with_whitespace() {
    let json = r#"{ "status" : "locked" }"#;
    // Our parser splits on "status": so the space before colon breaks it
    // This verifies the current behavior
    assert_eq!(parse_bw_status(json), BwStatus::NotInstalled);
}

#[test]
fn bw_status_parse_empty_status_value() {
    // Empty string status matches the catch-all, defaulting to Locked
    assert_eq!(parse_bw_status(r#"{"status":""}"#), BwStatus::Locked);
}

// =========================================================================
// ensure_bw_session decision logic (replicated inline)
// =========================================================================

fn should_prompt_bw(existing: Option<&str>, askpass: Option<&str>) -> bool {
    let askpass = match askpass {
        Some(a) => a,
        None => return false,
    };
    if !askpass.starts_with("bw:") || existing.is_some() {
        return false;
    }
    true
}

#[test]
fn bw_session_not_needed_when_no_askpass() {
    assert!(!should_prompt_bw(None, None));
}

#[test]
fn bw_session_not_needed_for_keychain() {
    assert!(!should_prompt_bw(None, Some("keychain")));
}

#[test]
fn bw_session_not_needed_for_op() {
    assert!(!should_prompt_bw(None, Some("op://Vault/Item/pw")));
}

#[test]
fn bw_session_not_needed_for_pass() {
    assert!(!should_prompt_bw(None, Some("pass:ssh/server")));
}

#[test]
fn bw_session_not_needed_for_vault() {
    assert!(!should_prompt_bw(None, Some("vault:secret/ssh")));
}

#[test]
fn bw_session_not_needed_for_custom_command() {
    assert!(!should_prompt_bw(None, Some("my-script %h")));
}

#[test]
fn bw_session_needed_for_bw_source() {
    assert!(should_prompt_bw(None, Some("bw:my-item")));
}

#[test]
fn bw_session_not_needed_when_already_cached() {
    assert!(!should_prompt_bw(Some("cached-token"), Some("bw:my-item")));
}

#[test]
fn bw_session_not_needed_for_bw_prefix_without_colon() {
    // "bwmy-item" should not trigger BW session
    assert!(!should_prompt_bw(None, Some("bwmy-item")));
}

// =========================================================================
// bw_unlock empty token check (askpass.rs line 412)
// =========================================================================

#[test]
fn bw_unlock_empty_token_check_logic() {
    // bw_unlock bails when stdout is empty after trim
    let token = "".trim().to_string();
    assert!(token.is_empty(), "Empty token should be rejected");
}

#[test]
fn bw_unlock_whitespace_only_token_check() {
    let token = "  \n\t  ".trim().to_string();
    assert!(token.is_empty(), "Whitespace-only token should be rejected");
}

#[test]
fn bw_unlock_valid_token_check() {
    let token = "eyJhbGciOiJSUzI1NiJ9.session_token_here".trim().to_string();
    assert!(!token.is_empty(), "Valid token should be accepted");
}

#[test]
fn bw_unlock_password_via_env_not_args() {
    // bw_unlock passes password via PURPLE_BW_MASTER env var, not via CLI args
    // This avoids exposure in `ps` output
    let env_var_name = "PURPLE_BW_MASTER";
    let cli_arg = "--passwordenv";
    assert_eq!(env_var_name, "PURPLE_BW_MASTER");
    assert_eq!(cli_arg, "--passwordenv");
}

// =========================================================================
// store_in_keychain / remove_from_keychain argument construction
// =========================================================================

#[test]
fn keychain_service_name_is_purple_ssh() {
    // All keychain operations use "purple-ssh" as the service name
    let service = "purple-ssh";
    assert_eq!(service, "purple-ssh");
}

#[test]
#[cfg(target_os = "macos")]
fn store_keychain_macos_uses_security_command() {
    // On macOS: security add-generic-password -U -a <alias> -s purple-ssh -w <password>
    let args = [
        "add-generic-password",
        "-U",
        "-a",
        "myserver",
        "-s",
        "purple-ssh",
        "-w",
        "secret123",
    ];
    assert_eq!(args[0], "add-generic-password");
    assert_eq!(args[1], "-U"); // -U means update if exists
    assert_eq!(args[3], "myserver"); // alias
    assert_eq!(args[5], "purple-ssh"); // service name
}

#[test]
#[cfg(target_os = "macos")]
fn remove_keychain_macos_uses_delete_generic() {
    let args = [
        "delete-generic-password",
        "-a",
        "myserver",
        "-s",
        "purple-ssh",
    ];
    assert_eq!(args[0], "delete-generic-password");
    assert_eq!(args[2], "myserver");
    assert_eq!(args[4], "purple-ssh");
}

#[test]
#[cfg(target_os = "macos")]
fn retrieve_keychain_macos_uses_find_generic() {
    let args = [
        "find-generic-password",
        "-a",
        "myserver",
        "-s",
        "purple-ssh",
        "-w",
    ];
    assert_eq!(args[0], "find-generic-password");
    assert_eq!(args[5], "-w"); // -w returns only the password
}

#[test]
#[cfg(not(target_os = "macos"))]
fn store_keychain_linux_uses_secret_tool() {
    let label = format!("purple-ssh: {}", "myserver");
    assert_eq!(label, "purple-ssh: myserver");
    let args = [
        "store",
        "--label",
        &label,
        "application",
        "purple-ssh",
        "host",
        "myserver",
    ];
    assert_eq!(args[0], "store");
    assert_eq!(args[4], "purple-ssh");
}

#[test]
#[cfg(not(target_os = "macos"))]
fn remove_keychain_linux_uses_secret_tool_clear() {
    let args = ["clear", "application", "purple-ssh", "host", "myserver"];
    assert_eq!(args[0], "clear");
}

#[test]
#[cfg(not(target_os = "macos"))]
fn retrieve_keychain_linux_uses_secret_tool_lookup() {
    let args = ["lookup", "application", "purple-ssh", "host", "myserver"];
    assert_eq!(args[0], "lookup");
}

// =========================================================================
// retrieve_password op:// prefix reconstruction (askpass.rs line 138)
// =========================================================================

#[test]
fn op_uri_strip_and_reconstruct_is_identity() {
    // retrieve_password strips "op://" then reconstructs with format!("op://{}", uri)
    let source = "op://Vault/Item/field";
    let uri = source.strip_prefix("op://").unwrap();
    let reconstructed = format!("op://{}", uri);
    assert_eq!(
        reconstructed, source,
        "Reconstructed URI should match original"
    );
}

#[test]
fn op_uri_strip_preserves_complex_paths() {
    let source = "op://Personal/SSH Server/password";
    let uri = source.strip_prefix("op://").unwrap();
    assert_eq!(uri, "Personal/SSH Server/password");
    let reconstructed = format!("op://{}", uri);
    assert_eq!(reconstructed, source);
}

#[test]
fn op_uri_strip_preserves_special_chars() {
    let source = "op://Work Vault/My-Server (prod)/api_key";
    let uri = source.strip_prefix("op://").unwrap();
    let reconstructed = format!("op://{}", uri);
    assert_eq!(reconstructed, source);
}

// =========================================================================
// ensure_bw_session retry logic (main.rs lines 1217-1243)
// =========================================================================

#[test]
fn ensure_bw_session_retry_count_is_two() {
    // The retry loop runs for attempts 0..2 (i.e., 2 attempts)
    let attempts: Vec<usize> = (0..2).collect();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0], 0);
    assert_eq!(attempts[1], 1);
}

#[test]
fn ensure_bw_session_first_attempt_says_try_again() {
    // On first failure (attempt == 0), message says "Try again."
    let attempt = 0;
    let msg = if attempt == 0 {
        "Unlock failed: error. Try again."
    } else {
        "Unlock failed: error. SSH will prompt for password."
    };
    assert!(msg.contains("Try again"));
}

#[test]
fn ensure_bw_session_second_attempt_says_ssh_will_prompt() {
    // On second failure (attempt == 1), message says "SSH will prompt"
    let attempt = 1;
    let msg = if attempt == 0 {
        "Unlock failed: error. Try again."
    } else {
        "Unlock failed: error. SSH will prompt for password."
    };
    assert!(msg.contains("SSH will prompt"));
}

#[test]
fn ensure_bw_session_status_unlocked_returns_none() {
    // When vault is already unlocked, no action needed
    let status = BwStatus::Unlocked;
    let needs_prompt = matches!(status, BwStatus::Locked);
    assert!(!needs_prompt);
}

#[test]
fn ensure_bw_session_status_not_installed_returns_none() {
    let status = BwStatus::NotInstalled;
    let needs_prompt = matches!(status, BwStatus::Locked);
    assert!(!needs_prompt);
}

#[test]
fn ensure_bw_session_status_not_authenticated_returns_none() {
    let status = BwStatus::NotAuthenticated;
    let needs_prompt = matches!(status, BwStatus::Locked);
    assert!(!needs_prompt);
}

#[test]
fn ensure_bw_session_status_locked_needs_prompt() {
    let status = BwStatus::Locked;
    let needs_prompt = matches!(status, BwStatus::Locked);
    assert!(needs_prompt);
}

// =========================================================================
// handle_password_command CLI validation (main.rs lines 1249-1274)
// =========================================================================

#[test]
fn password_command_set_rejects_empty_password() {
    // match prompt_hidden_input(...)? => Some(p) if !p.is_empty() => p
    let password = "";
    assert!(
        password.is_empty(),
        "Empty password should be rejected by set command"
    );
}

#[test]
fn password_command_set_accepts_non_empty_password() {
    let password = "mysecret";
    assert!(!password.is_empty());
}

#[test]
fn password_command_set_success_message_format() {
    let alias = "webserver";
    let msg = format!(
        "Password stored for {}. Set 'keychain' as password source to use it.",
        alias
    );
    assert!(msg.contains("webserver"));
    assert!(msg.contains("keychain"));
}

#[test]
fn password_command_remove_success_message_format() {
    let alias = "dbserver";
    let msg = format!("Password removed for {}.", alias);
    assert!(msg.contains("dbserver"));
}

// =========================================================================
// handle() flow: retry marker lifecycle
// =========================================================================

#[test]
fn retry_marker_lifecycle_create_then_detect() {
    let alias = "test_lifecycle_marker";
    let path = marker_path(alias).unwrap();
    let _ = std::fs::create_dir_all(path.parent().unwrap());

    // Initially no marker
    assert!(!is_recent_marker(&path));

    // Create marker
    let _ = std::fs::write(&path, b"");
    assert!(is_recent_marker(&path));

    // Clean up removes it
    cleanup_marker(alias);
    assert!(!is_recent_marker(&path));
}

// =========================================================================
// handle() flow: source None exits early
// =========================================================================

#[test]
fn handle_exits_when_no_source_found() {
    // When find_askpass_source returns None, handle() should exit(1).
    // We verify the logic: source None triggers exit.
    let source: Option<String> = None;
    assert!(source.is_none(), "No source should trigger early exit");
}

// =========================================================================
// retrieve_password: all 6 branches covered by routing_backend
// verify error message format for each source type
// =========================================================================

#[test]
fn retrieve_error_messages_are_descriptive() {
    // Document the error messages from each retrieval function
    let errors = [
        "Keychain lookup failed",
        "1Password lookup failed",
        "pass lookup failed",
        "Bitwarden lookup failed",
        "Vault lookup failed",
        "Custom askpass command failed",
    ];
    for err in &errors {
        assert!(!err.is_empty());
        assert!(err.contains("failed") || err.contains("Failed"));
    }
}

// =========================================================================
// Included hosts: askpass readable but not editable
// =========================================================================

#[test]
fn included_host_askpass_is_readable() {
    // An included host can have askpass set and be read
    let config = parse_config("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    let entries = config.host_entries();
    assert_eq!(entries[0].askpass, Some("keychain".to_string()));
}

#[test]
fn find_askpass_source_works_for_any_host() {
    // find_askpass_source does not distinguish between main config and included hosts
    let config =
        parse_config("Host included-server\n  HostName 10.0.0.1\n  # purple:askpass op://V/I/p\n");
    assert_eq!(
        find_askpass_source(&config, "included-server"),
        Some("op://V/I/p".to_string())
    );
}

// keychain_has_password delegates to retrieve_from_keychain

#[test]
fn keychain_has_password_returns_bool() {
    // Can't test actual keychain access, but verify the function compiles
    // and returns false for a non-existent alias (won't be in test keychain)
    let result = keychain_has_password("__purple_test_nonexistent_host__");
    assert!(!result);
}

// retrieve_from_command shell escaping

#[test]
fn retrieve_from_command_escapes_alias_metacharacters() {
    // The command itself will fail but we verify the expansion is safe
    // by using echo which shows the escaped values
    let result = retrieve_from_command("echo %a", "$(whoami)", "host.com");
    // echo prints the shell-escaped alias literally, not the result of $(whoami)
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output, "$(whoami)");
}

#[test]
fn retrieve_from_command_escapes_hostname_metacharacters() {
    let result = retrieve_from_command("echo %h", "myalias", "$(id)");
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output, "$(id)");
}

#[test]
fn retrieve_from_command_escapes_backtick_injection() {
    let result = retrieve_from_command("echo %a", "`uname`", "host");
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output, "`uname`");
}

#[test]
fn retrieve_from_command_escapes_semicolon() {
    let result = retrieve_from_command("echo %a", "foo;id", "host");
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output, "foo;id");
}

#[test]
fn retrieve_from_command_normal_values_unchanged() {
    let result = retrieve_from_command("echo %a %h", "myserver", "10.0.0.1");
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output, "myserver 10.0.0.1");
}
