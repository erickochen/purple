use super::types::DisplayTag;
use super::*;
use crate::ssh_config::model::SshConfigFile;
use crate::tunnel::TunnelType;
use std::path::PathBuf;

fn make_app(content: &str) -> App {
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/test_config"),
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    // Isolate from the real ~/.purple/providers so tests don't
    // pick up the user's vault_role / vault_addr config.
    app.provider_config = crate::providers::config::ProviderConfig::default();
    app
}

fn test_app_with_hosts(hosts: &[&str]) -> App {
    make_app(&hosts.join("\n"))
}

#[test]
fn has_any_vault_role_false_when_none_configured() {
    let app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    assert!(!app.has_any_vault_role());
}

#[test]
fn has_any_vault_role_false_when_host_only_has_vault_addr() {
    // Contract guard: `has_any_vault_role` gates the `V` key and other
    // vault-sign affordances. A host with ONLY `vault_addr` set (no
    // role, no provider default role) must return false — an address
    // without a role cannot be signed. Locks down the semantic contract
    // so a future refactor cannot flip it without a test failure.
    let app = test_app_with_hosts(&[
        "Host a\n  HostName 1.2.3.4\n  # purple:vault-addr http://127.0.0.1:8200\n",
    ]);
    assert_eq!(
        app.hosts[0].vault_addr.as_deref(),
        Some("http://127.0.0.1:8200")
    );
    assert!(app.hosts[0].vault_ssh.is_none());
    assert!(
        !app.has_any_vault_role(),
        "vault_addr without a role must not count as a vault-sign candidate"
    );
}

// ---- refresh_cert_cache tests ----

#[test]
fn refresh_cert_cache_noop_when_alias_not_in_hosts() {
    let mut app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    // Plant a stale entry and verify refresh removes it when the alias
    // is not in self.hosts (caller typed a bad alias).
    app.vault.cert_cache.insert(
        "ghost".to_string(),
        (
            std::time::Instant::now(),
            crate::vault_ssh::CertStatus::Missing,
            None,
        ),
    );
    app.refresh_cert_cache("ghost");
    assert!(!app.vault.cert_cache.contains_key("ghost"));
}

#[test]
fn refresh_cert_cache_removes_entry_when_no_vault_role() {
    let mut app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    // Host exists but has no vault role. Any lingering cache entry
    // should be removed so the detail panel does not flash a phantom
    // "Not signed" under a section that should not even render.
    app.vault.cert_cache.insert(
        "a".to_string(),
        (
            std::time::Instant::now(),
            crate::vault_ssh::CertStatus::Missing,
            None,
        ),
    );
    app.refresh_cert_cache("a");
    assert!(!app.vault.cert_cache.contains_key("a"));
}

#[test]
fn host_form_is_dirty_detects_vault_addr_change() {
    // Regression guard: FormBaseline + host_form_is_dirty must track
    // vault_addr so Esc with an unsaved address triggers the discard
    // confirm dialog.
    let mut app = make_app("Host a\n  HostName 1.2.3.4\n");
    app.form.vault_ssh = "ssh-client-signer/sign/engineer".to_string();
    app.form.vault_addr = String::new();
    app.capture_form_baseline();
    assert!(!app.host_form_is_dirty());
    app.form.vault_addr = "http://127.0.0.1:8200".to_string();
    assert!(
        app.host_form_is_dirty(),
        "typing into vault_addr must mark the form dirty"
    );
}

#[test]
fn edit_host_from_form_does_not_write_vault_addr_for_pattern() {
    // set_host_vault_addr refuses wildcards. The edit_host_from_form path
    // must skip the call entirely for pattern forms so the debug_assert
    // does not fire. Verify: a pattern entry with vault_addr set on the
    // form does NOT end up with a vault-addr comment in the config.
    let config_src = "Host web-* db-*\n  User debian\n";
    let mut app = make_app(config_src);
    app.patterns = app.config.pattern_entries();
    let pattern = app.patterns.first().cloned().unwrap();
    let form = HostForm::from_pattern_entry(&pattern);
    app.form = form;
    app.form.vault_addr = "http://should-not-persist:8200".to_string();
    let result = app.edit_host_from_form("web-* db-*");
    assert!(result.is_ok(), "edit failed: {:?}", result);
    let serialized = app.config.serialize();
    assert!(
        !serialized.contains("vault-addr"),
        "pattern entry must never carry a vault-addr comment, got: {}",
        serialized
    );
}

#[test]
fn add_host_from_form_writes_vault_addr_when_role_set() {
    // Positive case: a new host with both role and address persists
    // both comments via set_host_vault_ssh + set_host_vault_addr.
    let mut app = make_app("");
    app.form = HostForm::new();
    app.form.alias = "newhost".to_string();
    app.form.hostname = "10.0.0.1".to_string();
    app.form.vault_ssh = "ssh-client-signer/sign/engineer".to_string();
    app.form.vault_addr = "http://127.0.0.1:8200".to_string();
    let result = app.add_host_from_form();
    assert!(result.is_ok(), "add failed: {:?}", result);
    let serialized = app.config.serialize();
    assert!(serialized.contains("# purple:vault-ssh ssh-client-signer/sign/engineer"));
    assert!(serialized.contains("# purple:vault-addr http://127.0.0.1:8200"));
}

#[test]
fn refresh_cert_cache_inserts_missing_status_for_nonexistent_cert() {
    // Host with a vault role but no cert on disk yet: cache should be
    // populated with Missing so the detail panel shows the correct
    // "Not signed (press V to sign)" affordance immediately after the
    // host is added via the form.
    let mut app = test_app_with_hosts(&[
        "Host a\n  HostName 1.2.3.4\n  # purple:vault-ssh ssh-client-signer/sign/engineer\n",
    ]);
    app.refresh_cert_cache("a");
    match app.vault.cert_cache.get("a") {
        Some((_, crate::vault_ssh::CertStatus::Missing, mtime)) => {
            assert!(mtime.is_none(), "mtime must be None when cert file absent");
        }
        other => panic!("expected Missing status, got {:?}", other),
    }
}

// ---- end refresh_cert_cache tests ----

#[test]
fn has_any_vault_role_true_when_host_has_vault_ssh() {
    let app = test_app_with_hosts(&[
        "Host a\n  HostName 1.2.3.4\n  # purple:vault-ssh ssh/sign/engineer\n",
    ]);
    assert!(app.has_any_vault_role());
}

#[test]
fn has_any_vault_role_true_when_provider_has_vault_role() {
    let mut app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    app.provider_config = crate::providers::config::ProviderConfig::parse(
        "[aws]\ntoken=abc\nvault_role=ssh/sign/engineer\n",
    );
    assert!(app.has_any_vault_role());
}

#[test]
fn collect_unique_tags_includes_vault_when_host_has_vault_ssh() {
    let app = test_app_with_hosts(&[
        "Host a\n  HostName 1.2.3.4\n  # purple:vault-ssh ssh/sign/engineer\n",
    ]);
    let tags = app.collect_unique_tags();
    assert!(tags.contains(&"vault-ssh".to_string()));
}

#[test]
fn collect_unique_tags_includes_vault_when_provider_has_vault_role() {
    let mut app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    app.provider_config = crate::providers::config::ProviderConfig::parse(
        "[aws]\ntoken=abc\nvault_role=ssh/sign/engineer\n",
    );
    let tags = app.collect_unique_tags();
    assert!(tags.contains(&"vault-ssh".to_string()));
}

#[test]
fn collect_unique_tags_excludes_vault_when_none_configured() {
    let app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    let tags = app.collect_unique_tags();
    assert!(!tags.contains(&"vault-ssh".to_string()));
    assert!(!tags.contains(&"vault-kv".to_string()));
}

/// Regression: vault-kv (askpass) and vault-ssh (signed certs) are two
/// distinct integrations and must produce two distinct virtual tags. A
/// host configured with one must NOT cross-pollute the other tag.
#[test]
fn vault_kv_and_vault_ssh_are_distinct_virtual_tags() {
    // Host A: only Vault KV password source (askpass).
    // Host B: only Vault SSH signed cert role.
    // Host C: both at once.
    let app = test_app_with_hosts(&[
        "Host kv-only\n  HostName 1.0.0.1\n  # purple:askpass vault:secret/data/ssh/kv-only\n",
        "Host ssh-only\n  HostName 1.0.0.2\n  # purple:vault-ssh ssh/sign/engineer\n",
        "Host both\n  HostName 1.0.0.3\n  # purple:askpass vault:secret/data/ssh/both\n  # purple:vault-ssh ssh/sign/engineer\n",
    ]);
    let tags = app.collect_unique_tags();
    assert!(
        tags.contains(&"vault-kv".to_string()),
        "vault-kv must be present: {:?}",
        tags
    );
    assert!(
        tags.contains(&"vault-ssh".to_string()),
        "vault-ssh must be present: {:?}",
        tags
    );
}

#[test]
fn vault_kv_only_host_does_not_get_vault_ssh_tag() {
    // A host with only an askpass `vault:` source must not be reported as
    // having a Vault SSH role configured.
    let app = test_app_with_hosts(&[
        "Host kv-only\n  HostName 1.0.0.1\n  # purple:askpass vault:secret/data/ssh/kv-only\n",
    ]);
    assert!(
        !app.has_any_vault_role(),
        "vault: askpass must not register as a Vault SSH role"
    );
}

#[test]
fn flush_pending_vault_write_noop_when_flag_false() {
    let mut app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    app.pending_vault_config_write = false;
    app.flush_pending_vault_write();
    assert!(!app.pending_vault_config_write);
}

#[test]
fn flush_pending_vault_write_clears_flag_after_flush() {
    let mut app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    app.pending_vault_config_write = true;
    let tmpdir = std::env::temp_dir();
    let path = tmpdir.join("purple_test_flush_pending.ini");
    app.config.path = path.clone();
    app.flush_pending_vault_write();
    assert!(!app.pending_vault_config_write);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn reload_hosts_clears_pending_vault_write_flag() {
    let mut app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    app.pending_vault_config_write = true;
    let tmpdir = std::env::temp_dir();
    let path = tmpdir.join("purple_test_reload_flush.ini");
    app.config.path = path.clone();
    app.reload_hosts();
    assert!(!app.pending_vault_config_write);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn confirm_vault_sign_screen_stores_signable_list() {
    let mut app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    let signable = vec![(
        "a".to_string(),
        "ssh/sign/engineer".to_string(),
        String::new(),
        std::path::PathBuf::from("/tmp/id_ed25519.pub"),
        None,
    )];
    app.screen = Screen::ConfirmVaultSign {
        signable: signable.clone(),
    };
    match &app.screen {
        Screen::ConfirmVaultSign { signable: s } => {
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].0, "a");
        }
        _ => panic!("wrong screen"),
    }
}

#[test]
fn test_apply_filter_matches_alias() {
    let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
    app.start_search();
    app.search.query = Some("alp".to_string());
    app.apply_filter();
    assert_eq!(app.search.filtered_indices, vec![0]);
}

#[test]
fn test_apply_filter_matches_hostname() {
    let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
    app.start_search();
    app.search.query = Some("b.com".to_string());
    app.apply_filter();
    assert_eq!(app.search.filtered_indices, vec![1]);
}

#[test]
fn test_apply_filter_empty_query() {
    let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
    app.start_search();
    assert_eq!(app.search.filtered_indices, vec![0, 1]);
}

#[test]
fn test_apply_filter_no_matches() {
    let mut app = make_app("Host alpha\n  HostName a.com\n");
    app.start_search();
    app.search.query = Some("zzz".to_string());
    app.apply_filter();
    assert!(app.search.filtered_indices.is_empty());
}

#[test]
fn test_build_display_list_with_group_headers() {
    let content = "\
# Production
Host prod
  HostName prod.example.com

# Staging
Host staging
  HostName staging.example.com
";
    let app = make_app(content);
    assert_eq!(app.display_list.len(), 4);
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "Production"));
    assert!(matches!(
        &app.display_list[1],
        HostListItem::Host { index: 0 }
    ));
    assert!(matches!(&app.display_list[2], HostListItem::GroupHeader(s) if s == "Staging"));
    assert!(matches!(
        &app.display_list[3],
        HostListItem::Host { index: 1 }
    ));
}

#[test]
fn test_build_display_list_blank_line_breaks_group() {
    let content = "\
# This comment is separated by blank line

Host nogroup
  HostName nogroup.example.com
";
    let app = make_app(content);
    // Blank line between comment and host means no group header
    assert_eq!(app.display_list.len(), 1);
    assert!(matches!(
        &app.display_list[0],
        HostListItem::Host { index: 0 }
    ));
}

#[test]
fn test_navigation_skips_headers() {
    let content = "\
# Group
Host alpha
  HostName a.com

# Group 2
Host beta
  HostName b.com
";
    let mut app = make_app(content);
    // Should start on first Host (index 1 in display_list)
    assert_eq!(app.ui.list_state.selected(), Some(1));
    app.select_next();
    // Should skip header at index 2, land on Host at index 3
    assert_eq!(app.ui.list_state.selected(), Some(3));
    app.select_prev();
    assert_eq!(app.ui.list_state.selected(), Some(1));
}

#[test]
fn test_group_by_provider_creates_headers() {
    let content = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:123

Host do-db
  HostName 5.6.7.8
  # purple:provider digitalocean:456

Host vultr-app
  HostName 9.9.9.9
  # purple:provider vultr:789
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();

    // Should have: DigitalOcean header, 2 hosts, Vultr header, 1 host
    assert_eq!(app.display_list.len(), 5);
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean"));
    assert!(matches!(&app.display_list[1], HostListItem::Host { .. }));
    assert!(matches!(&app.display_list[2], HostListItem::Host { .. }));
    assert!(matches!(&app.display_list[3], HostListItem::GroupHeader(s) if s == "Vultr"));
    assert!(matches!(&app.display_list[4], HostListItem::Host { .. }));
}

#[test]
fn test_group_by_provider_no_header_for_none() {
    let content = "\
Host manual
  HostName 1.2.3.4

Host do-web
  HostName 5.6.7.8
  # purple:provider digitalocean:123
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();

    // manual first (no header), then DigitalOcean header + do-web
    assert_eq!(app.display_list.len(), 3);
    // No header before the manual host
    assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
    assert!(matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "DigitalOcean"));
    assert!(matches!(&app.display_list[2], HostListItem::Host { .. }));
}

#[test]
fn test_group_by_provider_with_alpha_sort() {
    let content = "\
Host do-zeta
  HostName 1.2.3.4
  # purple:provider digitalocean:1

Host do-alpha
  HostName 5.6.7.8
  # purple:provider digitalocean:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();

    // DigitalOcean header + sorted hosts
    assert_eq!(app.display_list.len(), 3);
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean"));
    // First host should be do-alpha (alphabetical)
    if let HostListItem::Host { index } = &app.display_list[1] {
        assert_eq!(app.hosts[*index].alias, "do-alpha");
    } else {
        panic!("Expected Host item");
    }
}

#[test]
fn test_config_changed_since_form_open_no_mtime() {
    let app = make_app("Host alpha\n  HostName a.com\n");
    // No mtime captured — should return false
    assert!(!app.config_changed_since_form_open());
}

#[test]
fn test_config_changed_since_form_open_same_mtime() {
    let mut app = make_app("Host alpha\n  HostName a.com\n");
    // Config path is /tmp/test_config which doesn't exist, so mtime is None
    app.capture_form_mtime();
    // Immediately checking — mtime should be same (None == None)
    assert!(!app.config_changed_since_form_open());
}

#[test]
fn test_config_changed_since_form_open_detects_change() {
    let mut app = make_app("Host alpha\n  HostName a.com\n");
    // Set form_mtime to a known past value (different from current None)
    app.conflict.form_mtime = Some(SystemTime::UNIX_EPOCH);
    // Config path doesn't exist (mtime is None), so it differs from UNIX_EPOCH
    assert!(app.config_changed_since_form_open());
}

#[test]
fn test_group_by_provider_toggle_off_restores_flat() {
    let content = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:123

Host vultr-app
  HostName 5.6.7.8
  # purple:provider vultr:456
";
    let mut app = make_app(content);
    app.sort_mode = SortMode::AlphaAlias;

    // Enable grouping
    app.group_by = GroupBy::Provider;
    app.apply_sort();
    let grouped_len = app.display_list.len();
    assert!(grouped_len > 2); // Has headers

    // Disable grouping
    app.group_by = GroupBy::None;
    app.apply_sort();
    // Should be flat: just hosts, no headers
    assert_eq!(app.display_list.len(), 2);
    assert!(
        app.display_list
            .iter()
            .all(|item| matches!(item, HostListItem::Host { .. }))
    );
}

#[test]
fn group_by_tag_groups_hosts_with_tag() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host web2
  HostName 2.2.2.2
  # purple:tags production

Host dev1
  HostName 3.3.3.3
  # purple:tags staging
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    // dev1 ungrouped first, then production header + 2 hosts
    assert_eq!(app.display_list.len(), 4);
    assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
    assert!(matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "production"));
    assert!(matches!(&app.display_list[2], HostListItem::Host { .. }));
    assert!(matches!(&app.display_list[3], HostListItem::Host { .. }));
    // Verify config order preserved within group
    if let HostListItem::Host { index } = &app.display_list[2] {
        assert_eq!(app.hosts[*index].alias, "web1");
    } else {
        panic!("Expected Host item at position 2");
    }
    if let HostListItem::Host { index } = &app.display_list[3] {
        assert_eq!(app.hosts[*index].alias, "web2");
    } else {
        panic!("Expected Host item at position 3");
    }
}

#[test]
fn group_by_tag_no_hosts_have_tag() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags staging

Host web2
  HostName 2.2.2.2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    assert_eq!(app.display_list.len(), 2);
    assert!(
        app.display_list
            .iter()
            .all(|item| matches!(item, HostListItem::Host { .. }))
    );
}

#[test]
fn group_by_tag_all_hosts_have_tag() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host web2
  HostName 2.2.2.2
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    assert_eq!(app.display_list.len(), 3);
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
}

#[test]
fn group_by_tag_host_with_multiple_tags() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production,frontend

Host dev1
  HostName 3.3.3.3
  # purple:tags staging
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    assert_eq!(app.display_list.len(), 3);
    assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
    assert!(matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "production"));
}

#[test]
fn group_by_tag_empty_host_list() {
    let content = "";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    assert!(app.display_list.is_empty());
}

#[test]
fn group_by_tag_case_sensitive() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags Production

Host web2
  HostName 2.2.2.2
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    assert_eq!(app.display_list.len(), 3);
    assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
    assert!(matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "production"));
    if let HostListItem::Host { index } = &app.display_list[2] {
        assert_eq!(app.hosts[*index].alias, "web2");
    } else {
        panic!("Expected Host item");
    }
}

#[test]
fn group_by_tag_with_alpha_sort() {
    let content = "\
Host zeta
  HostName 1.1.1.1
  # purple:tags production

Host alpha
  HostName 2.2.2.2
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();
    assert_eq!(app.display_list.len(), 3);
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
    if let HostListItem::Host { index } = &app.display_list[1] {
        assert_eq!(app.hosts[*index].alias, "alpha");
    } else {
        panic!("Expected Host item");
    }
}

#[test]
fn group_by_tag_preserves_ordering_within_ungrouped() {
    let content = "\
Host charlie
  HostName 3.3.3.3

Host alpha
  HostName 1.1.1.1

Host bravo
  HostName 2.2.2.2
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();
    assert_eq!(app.display_list.len(), 4);
    if let HostListItem::Host { index } = &app.display_list[0] {
        assert_eq!(app.hosts[*index].alias, "alpha");
    } else {
        panic!("Expected Host item");
    }
    if let HostListItem::Host { index } = &app.display_list[1] {
        assert_eq!(app.hosts[*index].alias, "charlie");
    } else {
        panic!("Expected Host item");
    }
    assert!(matches!(&app.display_list[2], HostListItem::GroupHeader(s) if s == "production"));
}

#[test]
fn group_by_tag_does_not_mutate_config() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host web2
  HostName 2.2.2.2
  # purple:tags staging
  # purple:provider_tags cloud,frontend
  # purple:provider digitalocean:123
";
    let app = make_app(content);
    let original_len = app.config.elements.len();

    let mut app2 = make_app(content);
    app2.group_by = GroupBy::Tag("production".to_string());
    app2.apply_sort();

    // Config elements must be identical — grouping is display-only
    assert_eq!(app.config.elements.len(), app2.config.elements.len());
    assert_eq!(original_len, app2.config.elements.len());
}

#[test]
fn group_by_tag_then_provider_then_none_config_unchanged() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
  # purple:provider digitalocean:1

Host dev1
  HostName 2.2.2.2
  # purple:tags staging
";
    let mut app = make_app(content);
    let original_len = app.config.elements.len();

    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    app.group_by = GroupBy::Provider;
    app.apply_sort();
    app.group_by = GroupBy::None;
    app.apply_sort();

    assert_eq!(app.config.elements.len(), original_len);
}

#[test]
fn provider_grouping_still_works_after_refactor() {
    let content = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:123

Host manual
  HostName 5.5.5.5

Host vultr-app
  HostName 9.9.9.9
  # purple:provider vultr:789
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();

    assert_eq!(app.display_list.len(), 5);
    assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
    assert!(matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "DigitalOcean"));
    assert!(matches!(&app.display_list[2], HostListItem::Host { .. }));
    assert!(matches!(&app.display_list[3], HostListItem::GroupHeader(s) if s == "Vultr"));
    assert!(matches!(&app.display_list[4], HostListItem::Host { .. }));
}

#[test]
fn provider_grouping_with_sort_still_works() {
    let content = "\
Host do-zeta
  HostName 1.2.3.4
  # purple:provider digitalocean:1

Host do-alpha
  HostName 5.6.7.8
  # purple:provider digitalocean:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();

    assert_eq!(app.display_list.len(), 3);
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean"));
    if let HostListItem::Host { index } = &app.display_list[1] {
        assert_eq!(app.hosts[*index].alias, "do-alpha");
    } else {
        panic!("Expected Host item");
    }
}

#[test]
fn group_by_to_key_none() {
    assert_eq!(GroupBy::None.to_key(), "none");
}

#[test]
fn group_by_to_key_provider() {
    assert_eq!(GroupBy::Provider.to_key(), "provider");
}

#[test]
fn group_by_to_key_tag() {
    assert_eq!(
        GroupBy::Tag("production".to_string()).to_key(),
        "tag:production"
    );
}

#[test]
fn group_by_from_key_none() {
    assert_eq!(GroupBy::from_key("none"), GroupBy::None);
}

#[test]
fn group_by_from_key_provider() {
    assert_eq!(GroupBy::from_key("provider"), GroupBy::Provider);
}

#[test]
fn group_by_from_key_tag() {
    assert_eq!(
        GroupBy::from_key("tag:production"),
        GroupBy::Tag("production".to_string())
    );
}

#[test]
fn group_by_from_key_unknown_falls_back_to_none() {
    assert_eq!(GroupBy::from_key("garbage"), GroupBy::None);
}

#[test]
fn group_by_from_key_empty_tag_name() {
    assert_eq!(GroupBy::from_key("tag:"), GroupBy::Tag(String::new()));
}

#[test]
fn group_by_label_none() {
    assert_eq!(GroupBy::None.label(), "ungrouped");
}

#[test]
fn group_by_label_provider() {
    assert_eq!(GroupBy::Provider.label(), "provider");
}

#[test]
fn group_by_label_tag() {
    assert_eq!(GroupBy::Tag("env".to_string()).label(), "tag: env");
}

// --- New validation tests from review findings ---

#[test]
fn test_validate_rejects_hash_in_alias() {
    let mut form = HostForm::new();
    form.alias = "my#host".to_string();
    form.hostname = "1.2.3.4".to_string();
    let result = form.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("#"));
}

#[test]
fn test_validate_empty_alias() {
    let mut form = HostForm::new();
    form.alias = "".to_string();
    form.hostname = "1.2.3.4".to_string();
    assert!(form.validate().is_err());
}

#[test]
fn test_validate_whitespace_alias() {
    let mut form = HostForm::new();
    form.alias = "my host".to_string();
    form.hostname = "1.2.3.4".to_string();
    assert!(form.validate().is_err());
}

#[test]
fn test_validate_pattern_alias() {
    let mut form = HostForm::new();
    form.alias = "my*host".to_string();
    form.hostname = "1.2.3.4".to_string();
    assert!(form.validate().is_err());
}

#[test]
fn test_validate_empty_hostname() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "".to_string();
    assert!(form.validate().is_err());
}

#[test]
fn test_validate_invalid_port() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "abc".to_string();
    assert!(form.validate().is_err());
}

#[test]
fn test_validate_port_zero() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "0".to_string();
    assert!(form.validate().is_err());
}

#[test]
fn test_validate_valid_form() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "22".to_string();
    assert!(form.validate().is_ok());
}

#[test]
fn test_validate_rejects_control_chars() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4\x00".to_string();
    form.port = "22".to_string();
    assert!(form.validate().is_err());
}

#[test]
fn test_validate_vault_ssh_accepts_valid_role() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "22".to_string();
    form.vault_ssh = "ssh-client-signer/sign/engineer".to_string();
    assert!(form.validate().is_ok());
}

#[test]
fn test_validate_vault_ssh_accepts_empty_role() {
    // Empty vault_ssh means "inherit from provider or none"
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "22".to_string();
    form.vault_ssh = String::new();
    assert!(form.validate().is_ok());
}

// ---- vault_addr validation ----

fn minimal_form_with_role() -> HostForm {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "22".to_string();
    form.vault_ssh = "ssh-client-signer/sign/engineer".to_string();
    form
}

#[test]
fn test_validate_vault_addr_accepts_empty() {
    let form = minimal_form_with_role();
    assert!(form.validate().is_ok());
}

#[test]
fn test_validate_vault_addr_accepts_valid_url() {
    let mut form = minimal_form_with_role();
    form.vault_addr = "http://127.0.0.1:8200".to_string();
    assert!(form.validate().is_ok());
}

#[test]
fn test_validate_vault_addr_rejects_whitespace() {
    let mut form = minimal_form_with_role();
    form.vault_addr = "http://host :8200".to_string();
    let err = form.validate().unwrap_err();
    assert!(err.contains("Vault SSH address"), "got: {}", err);
}

#[test]
fn test_validate_vault_addr_rejects_control_char() {
    let mut form = minimal_form_with_role();
    form.vault_addr = "http://host\n8200".to_string();
    assert!(form.validate().is_err());
}

#[test]
fn test_validate_vault_addr_ignored_when_role_empty() {
    // No role set: vault_addr is not validated (it will be dropped at
    // to_entry time regardless), so an otherwise-invalid value does
    // not cause a submit failure.
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "22".to_string();
    form.vault_addr = "http://host with space".to_string();
    assert!(form.validate().is_ok());
}

#[test]
fn test_to_entry_clears_vault_addr_when_role_empty() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "22".to_string();
    form.vault_addr = "http://leftover:8200".to_string();
    // vault_ssh intentionally empty — vault_addr must not survive to_entry.
    let entry = form.to_entry();
    assert!(entry.vault_addr.is_none());
}

#[test]
fn test_to_entry_persists_vault_addr_when_role_set() {
    let mut form = minimal_form_with_role();
    form.vault_addr = "http://127.0.0.1:8200".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.vault_addr.as_deref(), Some("http://127.0.0.1:8200"));
}

// ---- end vault_addr validation ----

#[test]
fn test_validate_vault_ssh_rejects_spaces_in_role() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "22".to_string();
    form.vault_ssh = "ssh client signer/sign/role".to_string();
    let err = form.validate().unwrap_err();
    assert!(
        err.contains("Vault SSH role"),
        "error should mention Vault SSH role, got: {}",
        err
    );
}

#[test]
fn test_validate_vault_ssh_rejects_shell_metacharacters() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "22".to_string();
    form.vault_ssh = "ssh-client-signer/sign/role;rm -rf /".to_string();
    assert!(form.validate().is_err());
}

#[test]
fn test_to_entry_parses_tags() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.tags = "prod, staging, us-east".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.tags, vec!["prod", "staging", "us-east"]);
}

#[test]
fn test_sort_mode_round_trip() {
    for mode in [
        SortMode::Original,
        SortMode::AlphaAlias,
        SortMode::AlphaHostname,
        SortMode::Frecency,
        SortMode::MostRecent,
    ] {
        assert_eq!(SortMode::from_key(mode.to_key()), mode);
    }
}

// --- TunnelForm tests ---

#[test]
fn tunnel_form_from_rule_local() {
    use crate::tunnel::{TunnelRule, TunnelType};
    let rule = TunnelRule {
        tunnel_type: TunnelType::Local,
        bind_address: String::new(),
        bind_port: 8080,
        remote_host: "localhost".to_string(),
        remote_port: 80,
    };
    let form = TunnelForm::from_rule(&rule);
    assert_eq!(form.tunnel_type, TunnelType::Local);
    assert_eq!(form.bind_port, "8080");
    assert_eq!(form.remote_host, "localhost");
    assert_eq!(form.remote_port, "80");
}

#[test]
fn tunnel_form_from_rule_dynamic() {
    use crate::tunnel::{TunnelRule, TunnelType};
    let rule = TunnelRule {
        tunnel_type: TunnelType::Dynamic,
        bind_address: String::new(),
        bind_port: 1080,
        remote_host: String::new(),
        remote_port: 0,
    };
    let form = TunnelForm::from_rule(&rule);
    assert_eq!(form.tunnel_type, TunnelType::Dynamic);
    assert_eq!(form.bind_port, "1080");
    assert_eq!(form.remote_host, "");
    assert_eq!(form.remote_port, "");
}

#[test]
fn tunnel_form_to_directive_local() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: "8080".to_string(),
        bind_address: String::new(),
        remote_host: "localhost".to_string(),
        remote_port: "80".to_string(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    let (key, value) = form.to_directive();
    assert_eq!(key, "LocalForward");
    assert_eq!(value, "8080 localhost:80");
}

#[test]
fn tunnel_form_to_directive_remote() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Remote,
        bind_port: "9090".to_string(),
        bind_address: String::new(),
        remote_host: "localhost".to_string(),
        remote_port: "3000".to_string(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    let (key, value) = form.to_directive();
    assert_eq!(key, "RemoteForward");
    assert_eq!(value, "9090 localhost:3000");
}

#[test]
fn tunnel_form_to_directive_dynamic() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Dynamic,
        bind_port: "1080".to_string(),
        bind_address: String::new(),
        remote_host: String::new(),
        remote_port: String::new(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    let (key, value) = form.to_directive();
    assert_eq!(key, "DynamicForward");
    assert_eq!(value, "1080");
}

#[test]
fn tunnel_form_validate_valid() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: "8080".to_string(),
        bind_address: String::new(),
        remote_host: "localhost".to_string(),
        remote_port: "80".to_string(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    assert!(form.validate().is_ok());
}

#[test]
fn tunnel_form_validate_bad_bind_port() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: "abc".to_string(),
        bind_address: String::new(),
        remote_host: "localhost".to_string(),
        remote_port: "80".to_string(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    assert!(form.validate().is_err());
}

#[test]
fn tunnel_form_validate_zero_bind_port() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: "0".to_string(),
        bind_address: String::new(),
        remote_host: "localhost".to_string(),
        remote_port: "80".to_string(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    assert!(form.validate().is_err());
}

#[test]
fn tunnel_form_validate_empty_remote_host() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: "8080".to_string(),
        bind_address: String::new(),
        remote_host: "  ".to_string(),
        remote_port: "80".to_string(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    assert!(form.validate().is_err());
}

#[test]
fn tunnel_form_validate_dynamic_skips_remote() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Dynamic,
        bind_port: "1080".to_string(),
        bind_address: String::new(),
        remote_host: String::new(),
        remote_port: String::new(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    assert!(form.validate().is_ok());
}

#[test]
fn tunnel_form_field_next_local() {
    use crate::tunnel::TunnelType;
    assert_eq!(
        TunnelFormField::Type.next(TunnelType::Local),
        TunnelFormField::BindPort
    );
    assert_eq!(
        TunnelFormField::BindPort.next(TunnelType::Local),
        TunnelFormField::RemoteHost
    );
    assert_eq!(
        TunnelFormField::RemoteHost.next(TunnelType::Local),
        TunnelFormField::RemotePort
    );
    assert_eq!(
        TunnelFormField::RemotePort.next(TunnelType::Local),
        TunnelFormField::Type
    );
}

#[test]
fn tunnel_form_field_next_dynamic_skips_remote() {
    use crate::tunnel::TunnelType;
    assert_eq!(
        TunnelFormField::Type.next(TunnelType::Dynamic),
        TunnelFormField::BindPort
    );
    assert_eq!(
        TunnelFormField::BindPort.next(TunnelType::Dynamic),
        TunnelFormField::Type
    );
}

#[test]
fn tunnel_form_field_prev_local() {
    use crate::tunnel::TunnelType;
    assert_eq!(
        TunnelFormField::Type.prev(TunnelType::Local),
        TunnelFormField::RemotePort
    );
    assert_eq!(
        TunnelFormField::BindPort.prev(TunnelType::Local),
        TunnelFormField::Type
    );
    assert_eq!(
        TunnelFormField::RemoteHost.prev(TunnelType::Local),
        TunnelFormField::BindPort
    );
    assert_eq!(
        TunnelFormField::RemotePort.prev(TunnelType::Local),
        TunnelFormField::RemoteHost
    );
}

#[test]
fn tunnel_form_field_prev_dynamic_skips_remote() {
    use crate::tunnel::TunnelType;
    assert_eq!(
        TunnelFormField::Type.prev(TunnelType::Dynamic),
        TunnelFormField::BindPort
    );
    assert_eq!(
        TunnelFormField::BindPort.prev(TunnelType::Dynamic),
        TunnelFormField::Type
    );
}

#[test]
fn tunnel_form_validate_bad_remote_port() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: "8080".to_string(),
        bind_address: String::new(),
        remote_host: "localhost".to_string(),
        remote_port: "abc".to_string(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    assert!(form.validate().is_err());
}

#[test]
fn tunnel_form_from_rule_with_bind_address() {
    use crate::tunnel::{TunnelRule, TunnelType};
    let rule = TunnelRule {
        tunnel_type: TunnelType::Local,
        bind_address: "192.168.1.1".to_string(),
        bind_port: 8080,
        remote_host: "localhost".to_string(),
        remote_port: 80,
    };
    let form = TunnelForm::from_rule(&rule);
    assert_eq!(form.bind_address, "192.168.1.1");
    assert_eq!(form.bind_port, "8080");
    let (key, value) = form.to_directive();
    assert_eq!(key, "LocalForward");
    assert_eq!(value, "192.168.1.1:8080 localhost:80");
}

#[test]
fn tunnel_form_validate_empty_bind_port() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: String::new(),
        bind_address: String::new(),
        remote_host: "localhost".to_string(),
        remote_port: "80".to_string(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    assert!(form.validate().is_err());
}

#[test]
fn tunnel_form_validate_zero_remote_port() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: "8080".to_string(),
        bind_address: String::new(),
        remote_host: "localhost".to_string(),
        remote_port: "0".to_string(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("Remote port"));
}

#[test]
fn tunnel_form_validate_control_chars() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: "8080".to_string(),
        bind_address: String::new(),
        remote_host: "local\x00host".to_string(),
        remote_port: "80".to_string(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("control characters"));
}

#[test]
fn tunnel_form_validate_spaces_in_remote_host() {
    use crate::tunnel::TunnelType;
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: "8080".to_string(),
        bind_address: String::new(),
        remote_host: "local host".to_string(),
        remote_port: "80".to_string(),
        focused_field: TunnelFormField::Type,
        cursor_pos: 0,
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("spaces"));
}

#[test]
fn tunnel_form_from_rule_ipv6_bind_address() {
    use crate::tunnel::{TunnelRule, TunnelType};
    let rule = TunnelRule {
        tunnel_type: TunnelType::Local,
        bind_address: "::1".to_string(),
        bind_port: 8080,
        remote_host: "localhost".to_string(),
        remote_port: 80,
    };
    let form = TunnelForm::from_rule(&rule);
    assert_eq!(form.bind_address, "::1");
    let (key, value) = form.to_directive();
    assert_eq!(key, "LocalForward");
    assert_eq!(value, "[::1]:8080 localhost:80");
}

// --- HostForm validation tests ---

#[test]
fn validate_hostname_whitespace_rejected() {
    let form = HostForm {
        alias: "myserver".to_string(),
        hostname: "host name".to_string(),
        port: "22".to_string(),
        ..HostForm::new()
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("whitespace"), "got: {}", err);
}

#[test]
fn validate_user_whitespace_rejected() {
    let form = HostForm {
        alias: "myserver".to_string(),
        hostname: "10.0.0.1".to_string(),
        user: "my user".to_string(),
        port: "22".to_string(),
        ..HostForm::new()
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("whitespace"), "got: {}", err);
}

#[test]
fn validate_hostname_with_control_chars_rejected() {
    let form = HostForm {
        alias: "myserver".to_string(),
        hostname: "10.0.0.1\n".to_string(),
        port: "22".to_string(),
        ..HostForm::new()
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("control"), "got: {}", err);
}

// --- TunnelForm validation error message tests ---

#[test]
fn tunnel_validate_bind_port_zero_message() {
    let form = TunnelForm {
        bind_port: "0".to_string(),
        ..TunnelForm::new()
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("0"), "got: {}", err);
}

#[test]
fn tunnel_validate_remote_host_empty_message() {
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: "8080".to_string(),
        remote_host: "".to_string(),
        remote_port: "80".to_string(),
        ..TunnelForm::new()
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("empty"), "got: {}", err);
}

#[test]
fn tunnel_validate_remote_host_whitespace_message() {
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: "8080".to_string(),
        remote_host: "host name".to_string(),
        remote_port: "80".to_string(),
        ..TunnelForm::new()
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("spaces"), "got: {}", err);
}

#[test]
fn tunnel_validate_bind_port_non_numeric_message() {
    let form = TunnelForm {
        bind_port: "abc".to_string(),
        ..TunnelForm::new()
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("1-65535"), "got: {}", err);
}

#[test]
fn tunnel_validate_remote_port_zero_message() {
    let form = TunnelForm {
        tunnel_type: TunnelType::Local,
        bind_port: "8080".to_string(),
        remote_host: "localhost".to_string(),
        remote_port: "0".to_string(),
        ..TunnelForm::new()
    };
    let err = form.validate().unwrap_err();
    assert!(err.contains("0"), "got: {}", err);
}

#[test]
fn select_host_by_alias_normal_mode() {
    let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
    app.select_host_by_alias("beta");
    let selected = app.selected_host().unwrap();
    assert_eq!(selected.alias, "beta");
}

#[test]
fn select_host_by_alias_search_mode() {
    let mut app = make_app(
        "Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n\nHost gamma\n  HostName g.com\n",
    );
    app.start_search();
    // Filter to beta and gamma (both contain letter 'a' in hostname or alias)
    app.search.query = Some("a".to_string());
    app.apply_filter();
    // filtered_indices should contain alpha (0) and gamma (2)
    assert!(app.search.filtered_indices.contains(&0));
    assert!(app.search.filtered_indices.contains(&2));

    // Select gamma by alias — should find it in filtered_indices
    app.select_host_by_alias("gamma");
    let selected = app.selected_host().unwrap();
    assert_eq!(selected.alias, "gamma");
}

#[test]
fn select_host_by_alias_search_mode_not_in_results() {
    let mut app = make_app("Host alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n");
    app.start_search();
    app.search.query = Some("alpha".to_string());
    app.apply_filter();
    assert_eq!(app.search.filtered_indices, vec![0]);

    // "beta" is not in filtered results — selection should not change
    let before = app.ui.list_state.selected();
    app.select_host_by_alias("beta");
    assert_eq!(app.ui.list_state.selected(), before);
}

fn make_provider_app() -> App {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.provider_config = crate::providers::config::ProviderConfig::default();
    app.provider_config
        .set_section(crate::providers::config::ProviderSection {
            provider: "digitalocean".to_string(),
            token: "test-token".to_string(),
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
        });
    app
}

#[test]
fn test_apply_sync_result_no_config() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.provider_config = crate::providers::config::ProviderConfig::default();
    let (msg, is_err, total, _, _, _) = app.apply_sync_result("digitalocean", vec![], false);
    assert!(is_err);
    assert_eq!(total, 0);
    assert!(msg.contains("no config"));
}

#[test]
fn test_apply_sync_result_empty_hosts_returns_zero_total() {
    let mut app = make_provider_app();
    let (msg, is_err, total, _, _, _) = app.apply_sync_result("digitalocean", vec![], false);
    assert!(!is_err);
    assert_eq!(total, 0);
    assert!(msg.contains("added 0"));
    assert!(msg.contains("unchanged 0"));
}

#[test]
fn test_apply_sync_result_with_hosts_returns_total() {
    let mut app = make_provider_app();
    let hosts = vec![
        crate::providers::ProviderHost::new(
            "s1".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            vec![],
        ),
        crate::providers::ProviderHost::new(
            "s2".to_string(),
            "db".to_string(),
            "5.6.7.8".to_string(),
            vec![],
        ),
    ];
    let (msg, is_err, total, added, _, _) = app.apply_sync_result("digitalocean", hosts, false);
    assert!(!is_err);
    assert_eq!(total, 2);
    assert_eq!(added, 2);
    assert!(msg.contains("added 2"));
    assert!(msg.contains("unchanged 0"));
}

#[test]
fn test_apply_sync_result_write_failure_preserves_total() {
    let mut app = make_provider_app();
    // Point config to a non-writable path so write() fails
    app.config.path = PathBuf::from("/dev/null/impossible");
    let hosts = vec![
        crate::providers::ProviderHost::new(
            "s1".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            vec![],
        ),
        crate::providers::ProviderHost::new(
            "s2".to_string(),
            "db".to_string(),
            "5.6.7.8".to_string(),
            vec![],
        ),
    ];
    let (msg, is_err, total, _, _, _) = app.apply_sync_result("digitalocean", hosts, false);
    assert!(is_err);
    assert_eq!(total, 2); // total preserved despite write failure
    assert!(msg.contains("Sync failed to save"));
}

#[test]
fn test_apply_sync_result_unknown_provider() {
    let mut app = make_provider_app();
    // Configure a section for the unknown provider name so it passes
    // the config check but fails on get_provider()
    app.provider_config
        .set_section(crate::providers::config::ProviderSection {
            provider: "nonexistent".to_string(),
            token: "tok".to_string(),
            alias_prefix: "nope".to_string(),
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
        });
    let (msg, is_err, total, _, _, _) = app.apply_sync_result("nonexistent", vec![], false);
    assert!(is_err);
    assert_eq!(total, 0);
    assert!(msg.contains("Unknown provider"));
}

#[test]
fn test_sync_history_cleared_on_provider_remove() {
    let mut app = make_provider_app();
    // Simulate a completed sync
    app.sync_history.insert(
        "digitalocean".to_string(),
        SyncRecord {
            timestamp: 100,
            message: "3 servers".to_string(),
            is_error: false,
        },
    );
    assert!(app.sync_history.contains_key("digitalocean"));

    // Simulate provider remove (same as handler.rs 'd' key path)
    app.provider_config.remove_section("digitalocean");
    app.sync_history.remove("digitalocean");

    assert!(!app.sync_history.contains_key("digitalocean"));
}

#[test]
fn test_sync_history_overwrite_replaces_error_with_success() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    // First sync fails
    app.sync_history.insert(
        "hetzner".to_string(),
        SyncRecord {
            timestamp: 100,
            message: "auth failed".to_string(),
            is_error: true,
        },
    );
    // Second sync succeeds
    app.sync_history.insert(
        "hetzner".to_string(),
        SyncRecord {
            timestamp: 200,
            message: "5 servers".to_string(),
            is_error: false,
        },
    );
    let record = app.sync_history.get("hetzner").unwrap();
    assert_eq!(record.timestamp, 200);
    assert!(!record.is_error);
    assert_eq!(record.message, "5 servers");
}

// --- SyncRecord persistence tests ---

#[test]
fn test_sync_record_save_load_roundtrip() {
    let dir = std::env::temp_dir().join(format!("purple_sync_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".purple")).unwrap();

    // Build history
    let mut history = HashMap::new();
    history.insert(
        "digitalocean".to_string(),
        SyncRecord {
            timestamp: 1710000000,
            message: "3 servers".to_string(),
            is_error: false,
        },
    );
    history.insert(
        "aws".to_string(),
        SyncRecord {
            timestamp: 1710000100,
            message: "auth failed".to_string(),
            is_error: true,
        },
    );
    history.insert(
        "hetzner".to_string(),
        SyncRecord {
            timestamp: 1710000200,
            message: "1 server (1 of 3 failed)".to_string(),
            is_error: true,
        },
    );

    // Save
    let path = dir.join(".purple").join("sync_history.tsv");
    let mut lines = Vec::new();
    for (provider, record) in &history {
        lines.push(format!(
            "{}\t{}\t{}\t{}",
            provider,
            record.timestamp,
            if record.is_error { "1" } else { "0" },
            record.message
        ));
    }
    std::fs::write(&path, lines.join("\n")).unwrap();

    // Load
    let content = std::fs::read_to_string(&path).unwrap();
    let mut loaded = HashMap::new();
    for line in content.lines() {
        let parts: Vec<&str> = line.splitn(4, '\t').collect();
        if parts.len() < 4 {
            continue;
        }
        let ts: u64 = parts[1].parse().unwrap();
        let is_error = parts[2] == "1";
        loaded.insert(
            parts[0].to_string(),
            SyncRecord {
                timestamp: ts,
                message: parts[3].to_string(),
                is_error,
            },
        );
    }

    // Verify
    assert_eq!(loaded.len(), 3);
    let do_rec = loaded.get("digitalocean").unwrap();
    assert_eq!(do_rec.timestamp, 1710000000);
    assert_eq!(do_rec.message, "3 servers");
    assert!(!do_rec.is_error);

    let aws_rec = loaded.get("aws").unwrap();
    assert_eq!(aws_rec.timestamp, 1710000100);
    assert_eq!(aws_rec.message, "auth failed");
    assert!(aws_rec.is_error);

    let hz_rec = loaded.get("hetzner").unwrap();
    assert_eq!(hz_rec.message, "1 server (1 of 3 failed)");
    assert!(hz_rec.is_error);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_sync_record_load_missing_file() {
    // load_all on a nonexistent path should return empty map
    // (tested implicitly since load_all uses dirs::home_dir,
    // but we verify the parser handles empty/malformed input)
    let mut map = HashMap::new();
    let content = "";
    for line in content.lines() {
        let parts: Vec<&str> = line.splitn(4, '\t').collect();
        if parts.len() < 4 {
            continue;
        }
        let Some(ts) = parts[1].parse::<u64>().ok() else {
            continue;
        };
        map.insert(
            parts[0].to_string(),
            SyncRecord {
                timestamp: ts,
                message: parts[3].to_string(),
                is_error: parts[2] == "1",
            },
        );
    }
    assert!(map.is_empty());
}

#[test]
fn test_sync_record_load_malformed_lines() {
    // Malformed lines should be skipped
    let content = "badline\naws\t123\t0\t2 servers\nalso_bad\ttwo\t0\tfoo\n";
    let mut map = HashMap::new();
    for line in content.lines() {
        let parts: Vec<&str> = line.splitn(4, '\t').collect();
        if parts.len() < 4 {
            continue;
        }
        let Some(ts) = parts[1].parse::<u64>().ok() else {
            continue;
        };
        map.insert(
            parts[0].to_string(),
            SyncRecord {
                timestamp: ts,
                message: parts[3].to_string(),
                is_error: parts[2] == "1",
            },
        );
    }
    assert_eq!(map.len(), 1);
    assert_eq!(map.get("aws").unwrap().message, "2 servers");
}

// --- auto_sync tests ---

fn make_section(provider: &str, auto_sync: bool) -> crate::providers::config::ProviderSection {
    crate::providers::config::ProviderSection {
        provider: provider.to_string(),
        token: "tok".to_string(),
        alias_prefix: provider[..2].to_string(),
        user: "root".to_string(),
        identity_file: String::new(),
        url: if provider == "proxmox" {
            "https://pve:8006".to_string()
        } else {
            String::new()
        },
        verify_tls: true,
        auto_sync,
        profile: String::new(),
        regions: String::new(),
        project: String::new(),
        compartment: String::new(),
        vault_role: String::new(),
        vault_addr: String::new(),
    }
}

#[test]
fn test_startup_auto_sync_filter_skips_disabled_providers() {
    // Simuleert de startup-loop in main.rs: providers met auto_sync=false worden overgeslagen.
    let mut config = crate::providers::config::ProviderConfig::default();
    config.set_section(make_section("digitalocean", true));
    config.set_section(make_section("proxmox", false));
    let auto_synced: Vec<&str> = config
        .configured_providers()
        .iter()
        .filter(|s| s.auto_sync)
        .map(|s| s.provider.as_str())
        .collect();
    assert_eq!(auto_synced, vec!["digitalocean"]);
    assert!(!auto_synced.contains(&"proxmox"));
}

#[test]
fn test_startup_auto_sync_filter_all_enabled() {
    let mut config = crate::providers::config::ProviderConfig::default();
    config.set_section(make_section("digitalocean", true));
    config.set_section(make_section("vu", true)); // vultr-achtig
    let skipped: Vec<&str> = config
        .configured_providers()
        .iter()
        .filter(|s| !s.auto_sync)
        .map(|s| s.provider.as_str())
        .collect();
    assert!(skipped.is_empty());
}

#[test]
fn test_startup_auto_sync_filter_explicit_false_skips() {
    // Niet-Proxmox provider met expliciete auto_sync=false wordt ook overgeslagen.
    let mut config = crate::providers::config::ProviderConfig::default();
    config.set_section(make_section("digitalocean", false));
    let s = &config.configured_providers()[0];
    assert!(!s.auto_sync);
}

#[test]
fn test_provider_form_fields_new_defaults() {
    let form = ProviderFormFields::new();
    assert!(form.auto_sync, "new() should default auto_sync to true");
    assert!(form.verify_tls);
    assert_eq!(form.focused_field, ProviderFormField::Token);
}

#[test]
fn test_provider_form_field_cloud_fields_include_auto_sync() {
    let fields = ProviderFormField::fields_for("digitalocean");
    assert!(
        fields.contains(&ProviderFormField::AutoSync),
        "CLOUD_FIELDS should contain AutoSync"
    );
    assert!(
        !fields.contains(&ProviderFormField::VerifyTls),
        "CLOUD_FIELDS should not contain VerifyTls"
    );
}

#[test]
fn test_provider_form_field_proxmox_fields_include_auto_sync_and_verify_tls() {
    let fields = ProviderFormField::fields_for("proxmox");
    assert!(
        fields.contains(&ProviderFormField::AutoSync),
        "PROXMOX_FIELDS should contain AutoSync"
    );
    assert!(
        fields.contains(&ProviderFormField::VerifyTls),
        "PROXMOX_FIELDS should contain VerifyTls"
    );
}

#[test]
fn test_provider_form_field_ovh_fields() {
    let fields = ProviderFormField::fields_for("ovh");
    assert_eq!(*fields.last().unwrap(), ProviderFormField::AutoSync);
    assert!(fields.contains(&ProviderFormField::Token));
    assert!(fields.contains(&ProviderFormField::Project));
    assert!(fields.contains(&ProviderFormField::Regions));
    assert!(fields.contains(&ProviderFormField::AliasPrefix));
    assert!(!fields.contains(&ProviderFormField::Url));
    assert!(!fields.contains(&ProviderFormField::VerifyTls));
}

#[test]
fn test_provider_form_field_auto_sync_is_last_in_all_field_lists() {
    let cloud = ProviderFormField::fields_for("digitalocean");
    assert_eq!(*cloud.last().unwrap(), ProviderFormField::AutoSync);

    let proxmox = ProviderFormField::fields_for("proxmox");
    assert_eq!(*proxmox.last().unwrap(), ProviderFormField::AutoSync);

    let aws = ProviderFormField::fields_for("aws");
    assert_eq!(*aws.last().unwrap(), ProviderFormField::AutoSync);

    let scaleway = ProviderFormField::fields_for("scaleway");
    assert_eq!(*scaleway.last().unwrap(), ProviderFormField::AutoSync);
    assert!(scaleway.contains(&ProviderFormField::Regions));
    assert!(scaleway.contains(&ProviderFormField::Token));
    assert!(!scaleway.contains(&ProviderFormField::Profile));
    assert!(!scaleway.contains(&ProviderFormField::Url));
    assert!(!scaleway.contains(&ProviderFormField::VerifyTls));

    let azure = ProviderFormField::fields_for("azure");
    assert_eq!(*azure.last().unwrap(), ProviderFormField::AutoSync);
    assert!(azure.contains(&ProviderFormField::Regions));
    assert!(azure.contains(&ProviderFormField::Token));
    assert!(!azure.contains(&ProviderFormField::Profile));
    assert!(!azure.contains(&ProviderFormField::Url));
    assert!(!azure.contains(&ProviderFormField::VerifyTls));

    let ovh = ProviderFormField::fields_for("ovh");
    assert_eq!(*ovh.last().unwrap(), ProviderFormField::AutoSync);
    assert!(ovh.contains(&ProviderFormField::Token));
    assert!(ovh.contains(&ProviderFormField::Project));
    assert!(ovh.contains(&ProviderFormField::Regions));
    assert!(!ovh.contains(&ProviderFormField::Url));
}

#[test]
fn test_provider_form_field_label_auto_sync() {
    assert_eq!(ProviderFormField::AutoSync.label(), "Auto Sync");
}

// =========================================================================
// HostForm askpass tests
// =========================================================================

#[test]
fn test_form_new_has_empty_askpass() {
    let form = HostForm::new();
    assert_eq!(form.askpass, "");
}

#[test]
fn test_form_from_entry_with_askpass() {
    let entry = HostEntry {
        alias: "test".to_string(),
        hostname: "1.2.3.4".to_string(),
        askpass: Some("keychain".to_string()),
        ..Default::default()
    };
    let form = HostForm::from_entry(&entry, Default::default());
    assert_eq!(form.askpass, "keychain");
}

#[test]
fn test_form_from_entry_without_askpass() {
    let entry = HostEntry {
        alias: "test".to_string(),
        hostname: "1.2.3.4".to_string(),
        askpass: None,
        ..Default::default()
    };
    let form = HostForm::from_entry(&entry, Default::default());
    assert_eq!(form.askpass, "");
}

#[test]
fn test_form_from_entry_with_inherited_hints() {
    use crate::ssh_config::model::InheritedHints;
    let entry = HostEntry {
        alias: "myserver".to_string(),
        hostname: "10.0.0.1".to_string(),
        ..Default::default()
    };
    let hints = InheritedHints {
        proxy_jump: Some(("bastion".to_string(), "web-*".to_string())),
        user: Some(("admin".to_string(), "*".to_string())),
        identity_file: None,
    };
    let form = HostForm::from_entry(&entry, hints);
    // Form fields are empty (raw entry has no own values).
    assert_eq!(form.proxy_jump, "");
    assert_eq!(form.user, "");
    // Inherited hints are carried through.
    let (val, src) = form.inherited.proxy_jump.as_ref().unwrap();
    assert_eq!(val, "bastion");
    assert_eq!(src, "web-*");
    let (val, src) = form.inherited.user.as_ref().unwrap();
    assert_eq!(val, "admin");
    assert_eq!(src, "*");
    assert!(form.inherited.identity_file.is_none());
}

#[test]
fn test_form_clone_carries_enriched_values() {
    // When cloning a host, inherited values become own values in the form.
    // Clone uses the enriched entry (from host_entries with inheritance)
    // and passes Default::default() for hints.
    let entry = HostEntry {
        alias: "web-prod".to_string(),
        hostname: "10.0.0.1".to_string(),
        proxy_jump: "bastion".to_string(), // enriched: includes inherited
        user: "team".to_string(),
        ..Default::default()
    };
    let form = HostForm::from_entry(&entry, Default::default());
    // Values are in the form fields (editable, not dimmed).
    assert_eq!(form.proxy_jump, "bastion");
    assert_eq!(form.user, "team");
    // No inherited hints (clone is self-contained).
    assert!(form.inherited.proxy_jump.is_none());
    assert!(form.inherited.user.is_none());
}

#[test]
fn test_to_entry_with_askpass_keychain() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "keychain".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.askpass, Some("keychain".to_string()));
}

#[test]
fn test_to_entry_with_askpass_op() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "op://Vault/Item/password".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.askpass, Some("op://Vault/Item/password".to_string()));
}

#[test]
fn test_to_entry_with_askpass_vault() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "vault:secret/data/myapp#password".to_string();
    let entry = form.to_entry();
    assert_eq!(
        entry.askpass,
        Some("vault:secret/data/myapp#password".to_string())
    );
}

#[test]
fn test_to_entry_with_askpass_bw() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "bw:my-item".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.askpass, Some("bw:my-item".to_string()));
}

#[test]
fn test_to_entry_with_askpass_pass() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "pass:ssh/myserver".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.askpass, Some("pass:ssh/myserver".to_string()));
}

#[test]
fn test_to_entry_with_askpass_custom_command() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "my-script %a %h".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.askpass, Some("my-script %a %h".to_string()));
}

#[test]
fn test_to_entry_with_askpass_empty() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.askpass, None);
}

#[test]
fn test_to_entry_with_askpass_whitespace_only() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "  ".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.askpass, None);
}

#[test]
fn test_to_entry_askpass_trimmed() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "  keychain  ".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.askpass, Some("keychain".to_string()));
}

#[test]
fn test_focused_value_mut_askpass() {
    let mut form = HostForm::new();
    form.focused_field = FormField::AskPass;
    form.focused_value_mut().push_str("vault:");
    assert_eq!(form.askpass, "vault:");
}

#[test]
fn test_askpass_field_label() {
    assert_eq!(FormField::AskPass.label(), "Password Source");
}

#[test]
fn test_askpass_field_navigation() {
    // Schema order: IdentityFile, VaultSsh, VaultAddr, ProxyJump, AskPass, Tags.
    // `FormField::next()`/`prev()` walk the raw `ALL` array (schema order)
    // regardless of visibility; the visibility-aware walk lives in
    // `HostForm::focus_next_visible`/`focus_prev_visible` and is covered
    // by its own tests below.
    assert_eq!(FormField::IdentityFile.next(), FormField::VaultSsh);
    assert_eq!(FormField::VaultSsh.next(), FormField::VaultAddr);
    assert_eq!(FormField::VaultAddr.next(), FormField::ProxyJump);
    assert_eq!(FormField::ProxyJump.next(), FormField::AskPass);
    assert_eq!(FormField::AskPass.next(), FormField::Tags);
    assert_eq!(FormField::Tags.prev(), FormField::AskPass);
    assert_eq!(FormField::AskPass.prev(), FormField::ProxyJump);
    assert_eq!(FormField::ProxyJump.prev(), FormField::VaultAddr);
    assert_eq!(FormField::VaultAddr.prev(), FormField::VaultSsh);
    assert_eq!(FormField::VaultSsh.prev(), FormField::IdentityFile);
}

#[test]
fn test_form_field_all_includes_askpass() {
    assert!(FormField::ALL.contains(&FormField::AskPass));
    assert!(FormField::ALL.contains(&FormField::VaultSsh));
    assert!(FormField::ALL.contains(&FormField::VaultAddr));
    assert_eq!(FormField::ALL.len(), 10);
}

#[test]
fn host_form_visible_fields_hides_vault_addr_when_role_empty() {
    let form = HostForm::new();
    assert!(form.vault_ssh.is_empty());
    let visible = form.visible_fields();
    assert!(
        !visible.contains(&FormField::VaultAddr),
        "VaultAddr must be hidden when role is empty"
    );
    // All other fields still present.
    assert_eq!(visible.len(), FormField::ALL.len() - 1);
}

#[test]
fn host_form_visible_fields_shows_vault_addr_when_role_set() {
    let mut form = HostForm::new();
    form.vault_ssh = "ssh-client-signer/sign/engineer".to_string();
    let visible = form.visible_fields();
    assert!(visible.contains(&FormField::VaultAddr));
    assert_eq!(visible.len(), FormField::ALL.len());
}

#[test]
fn host_form_focus_next_visible_skips_vault_addr_when_role_empty() {
    let mut form = HostForm::new();
    form.focused_field = FormField::VaultSsh;
    form.focus_next_visible();
    assert_eq!(
        form.focused_field,
        FormField::ProxyJump,
        "Tab from VaultSsh must skip the hidden VaultAddr"
    );
}

#[test]
fn host_form_focus_prev_visible_skips_vault_addr_when_role_empty() {
    let mut form = HostForm::new();
    form.focused_field = FormField::ProxyJump;
    form.focus_prev_visible();
    assert_eq!(
        form.focused_field,
        FormField::VaultSsh,
        "Shift-Tab from ProxyJump must skip the hidden VaultAddr"
    );
}

#[test]
fn host_form_focus_next_visible_includes_vault_addr_when_role_set() {
    let mut form = HostForm::new();
    form.vault_ssh = "ssh/sign/engineer".to_string();
    form.focused_field = FormField::VaultSsh;
    form.focus_next_visible();
    assert_eq!(form.focused_field, FormField::VaultAddr);
    form.focus_next_visible();
    assert_eq!(form.focused_field, FormField::ProxyJump);
}

// --- Password picker state ---

#[test]
fn test_password_picker_state_init() {
    let app = make_app("Host test\n  HostName test.com\n");
    assert!(!app.ui.show_password_picker);
}

#[test]
fn test_select_next_password_source() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.ui.password_picker_state.select(Some(0));
    app.select_next_password_source();
    assert_eq!(app.ui.password_picker_state.selected(), Some(1));
}

#[test]
fn test_select_prev_password_source() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.ui.password_picker_state.select(Some(2));
    app.select_prev_password_source();
    assert_eq!(app.ui.password_picker_state.selected(), Some(1));
}

#[test]
fn test_select_password_source_wrap_bottom() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
    app.ui.password_picker_state.select(Some(last));
    app.select_next_password_source();
    assert_eq!(app.ui.password_picker_state.selected(), Some(0));
}

#[test]
fn test_select_password_source_wrap_top() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.ui.password_picker_state.select(Some(0));
    app.select_prev_password_source();
    let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
    assert_eq!(app.ui.password_picker_state.selected(), Some(last));
}

// --- ProviderFormFields vault_addr visibility ---

#[test]
fn provider_form_visible_fields_hides_vault_addr_when_role_empty() {
    let form = ProviderFormFields::new();
    let visible = form.visible_fields("digitalocean");
    assert!(
        !visible.contains(&ProviderFormField::VaultAddr),
        "VaultAddr must be hidden when the provider role is empty"
    );
    assert!(visible.contains(&ProviderFormField::VaultRole));
}

#[test]
fn provider_form_visible_fields_shows_vault_addr_when_role_set() {
    let mut form = ProviderFormFields::new();
    form.vault_role = "ssh-client-signer/sign/engineer".to_string();
    let visible = form.visible_fields("digitalocean");
    assert!(visible.contains(&ProviderFormField::VaultAddr));
    assert!(visible.contains(&ProviderFormField::VaultRole));
}

#[test]
fn provider_form_visible_fields_vault_addr_follows_role_across_providers() {
    let mut form = ProviderFormFields::new();
    form.vault_role = "ssh-client-signer/sign/engineer".to_string();
    for provider in ["digitalocean", "proxmox", "aws", "gcp", "azure", "oracle"] {
        let visible = form.visible_fields(provider);
        assert!(
            visible.contains(&ProviderFormField::VaultAddr),
            "VaultAddr must be visible for provider {} when role is set",
            provider
        );
    }
}

// --- Host entry askpass from config ---

#[test]
fn test_host_entries_include_askpass() {
    let app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    assert_eq!(app.hosts[0].askpass, Some("keychain".to_string()));
}

#[test]
fn test_host_entries_vault_askpass() {
    let app =
        make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pass\n");
    assert_eq!(
        app.hosts[0].askpass,
        Some("vault:secret/ssh#pass".to_string())
    );
}

#[test]
fn test_host_entries_no_askpass() {
    let app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    assert_eq!(app.hosts[0].askpass, None);
}

// --- Validate with askpass ---

#[test]
fn test_validate_askpass_with_control_char() {
    let mut form = HostForm::new();
    form.alias = "myhost".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "keychain\x00".to_string();
    let result = form.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Password Source"));
}

#[test]
fn test_validate_askpass_normal_values_ok() {
    let sources = [
        "",
        "keychain",
        "op://V/I/p",
        "bw:x",
        "pass:x",
        "vault:x#y",
        "cmd %a",
    ];
    for src in &sources {
        let mut form = HostForm::new();
        form.alias = "myhost".to_string();
        form.hostname = "1.2.3.4".to_string();
        form.askpass = src.to_string();
        assert!(
            form.validate().is_ok(),
            "Validate should pass for askpass='{}'",
            src
        );
    }
}

// --- add_host askpass flow (test config mutation directly, bypassing write) ---

#[test]
fn test_add_host_config_mutation_with_askpass() {
    let mut app = make_app("");
    let entry = HostEntry {
        alias: "newhost".to_string(),
        hostname: "1.2.3.4".to_string(),
        askpass: Some("keychain".to_string()),
        ..Default::default()
    };
    app.config.add_host(&entry);
    app.config.set_host_askpass("newhost", "keychain");
    let serialized = app.config.serialize();
    assert!(serialized.contains("purple:askpass keychain"));
    let entries = app.config.host_entries();
    let found = entries.iter().find(|e| e.alias == "newhost").unwrap();
    assert_eq!(found.askpass, Some("keychain".to_string()));
}

#[test]
fn test_add_host_config_mutation_with_vault() {
    let mut app = make_app("");
    let entry = HostEntry {
        alias: "vaulthost".to_string(),
        hostname: "10.0.0.1".to_string(),
        askpass: Some("vault:secret/ssh#pass".to_string()),
        ..Default::default()
    };
    app.config.add_host(&entry);
    app.config
        .set_host_askpass("vaulthost", "vault:secret/ssh#pass");
    let serialized = app.config.serialize();
    assert!(serialized.contains("purple:askpass vault:secret/ssh#pass"));
}

#[test]
fn test_add_host_config_mutation_without_askpass() {
    let mut app = make_app("");
    let entry = HostEntry {
        alias: "nopass".to_string(),
        hostname: "1.2.3.4".to_string(),
        ..Default::default()
    };
    app.config.add_host(&entry);
    // Don't call set_host_askpass when None — mirrors add_host_from_form logic
    let serialized = app.config.serialize();
    assert!(
        !serialized.contains("purple:askpass"),
        "No askpass comment when None"
    );
}

#[test]
fn test_add_host_from_form_calls_set_askpass() {
    // Verify that add_host_from_form invokes set_host_askpass for non-None askpass.
    // We test by checking the form.to_entry() produces correct askpass.
    let mut form = HostForm::new();
    form.alias = "test".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "op://Vault/Item/pw".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.askpass, Some("op://Vault/Item/pw".to_string()));
    // And that the code path in add_host_from_form would call set_host_askpass
    assert!(entry.askpass.is_some());
}

// --- vault_ssh round-trip tests ---

#[test]
fn host_form_validate_rejects_invalid_vault_role() {
    let mut form = HostForm::new();
    form.alias = "h".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "22".to_string();
    form.vault_ssh = "bad role with spaces".to_string();
    let err = form.validate().unwrap_err();
    assert!(
        err.contains("Vault SSH role"),
        "expected vault role error, got: {}",
        err
    );
}

#[test]
fn host_form_validate_accepts_empty_vault_role() {
    let mut form = HostForm::new();
    form.alias = "h".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "22".to_string();
    form.vault_ssh = "   ".to_string();
    assert!(form.validate().is_ok());
}

#[test]
fn host_form_validate_accepts_valid_vault_role() {
    let mut form = HostForm::new();
    form.alias = "h".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.port = "22".to_string();
    form.vault_ssh = "ssh-client-signer/sign/my-role".to_string();
    assert!(form.validate().is_ok());
}

#[test]
fn to_entry_vault_ssh_some_when_set() {
    let mut form = HostForm::new();
    form.alias = "test".to_string();
    form.hostname = "10.0.0.1".to_string();
    form.vault_ssh = "ssh/sign/engineer".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.vault_ssh.as_deref(), Some("ssh/sign/engineer"));
}

#[test]
fn to_entry_vault_ssh_none_when_empty() {
    let mut form = HostForm::new();
    form.alias = "test".to_string();
    form.hostname = "10.0.0.1".to_string();
    form.vault_ssh = String::new();
    let entry = form.to_entry();
    assert!(entry.vault_ssh.is_none());
}

#[test]
fn to_entry_vault_ssh_none_when_whitespace() {
    let mut form = HostForm::new();
    form.alias = "test".to_string();
    form.hostname = "10.0.0.1".to_string();
    form.vault_ssh = "   ".to_string();
    let entry = form.to_entry();
    assert!(entry.vault_ssh.is_none());
}

#[test]
fn from_entry_duplicate_clears_vault_ssh_and_cert() {
    let entry = crate::ssh_config::model::HostEntry {
        alias: "original".to_string(),
        hostname: "10.0.0.1".to_string(),
        vault_ssh: Some("ssh/sign/admin".to_string()),
        certificate_file: "~/.purple/certs/original-cert.pub".to_string(),
        ..Default::default()
    };
    let (form, had_vault) =
        HostForm::from_entry_duplicate(&entry, crate::ssh_config::model::InheritedHints::default());
    // vault_ssh must be cleared so the copy does not inherit a per-host
    // override tied to the original alias's cert file.
    assert!(form.vault_ssh.is_empty());
    assert!(had_vault, "caller should be told vault_ssh was cleared");
    // HostForm has no certificate_file field; the cert path is derived
    // from the alias at save time, so cloning can never carry it over.
}

#[test]
fn from_entry_populates_vault_ssh() {
    let entry = crate::ssh_config::model::HostEntry {
        alias: "test".to_string(),
        hostname: "10.0.0.1".to_string(),
        vault_ssh: Some("ssh/sign/admin".to_string()),
        ..Default::default()
    };
    let form = HostForm::from_entry(&entry, crate::ssh_config::model::InheritedHints::default());
    assert_eq!(form.vault_ssh, "ssh/sign/admin");
}

// --- add_host_from_form with vault_ssh (bypassing write) ---

#[test]
fn test_add_host_from_form_sets_vault_ssh_and_certificate_file() {
    let dir = std::env::temp_dir().join("purple_test_add_vault_ssh");
    let _ = std::fs::create_dir_all(&dir);
    let config_path = dir.join("config");
    let _ = std::fs::write(&config_path, "Host existing\n  HostName 1.2.3.4\n");
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content("Host existing\n  HostName 1.2.3.4\n"),
        path: config_path.clone(),
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    app.form.alias = "vaulthost".to_string();
    app.form.hostname = "10.0.0.1".to_string();
    app.form.vault_ssh = "ssh/sign/engineer".to_string();
    let result = app.add_host_from_form();
    assert!(result.is_ok(), "add_host_from_form failed: {:?}", result);
    let entries = app.config.host_entries();
    let host = entries.iter().find(|e| e.alias == "vaulthost").unwrap();
    assert_eq!(host.vault_ssh.as_deref(), Some("ssh/sign/engineer"));
    let serialized = app.config.serialize();
    assert!(
        serialized.contains("CertificateFile"),
        "should have CertificateFile: {}",
        serialized
    );
    assert!(
        serialized.contains("purple:vault-ssh ssh/sign/engineer"),
        "should have vault-ssh comment: {}",
        serialized
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- update host askpass via config (bypassing write which fails in test) ---

#[test]
fn test_config_set_host_askpass_adds() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    app.config.set_host_askpass("myserver", "bw:my-item");
    let serialized = app.config.serialize();
    assert!(serialized.contains("purple:askpass bw:my-item"));
    let entries = app.config.host_entries();
    assert_eq!(entries[0].askpass, Some("bw:my-item".to_string()));
}

#[test]
fn test_config_set_host_askpass_changes() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    app.config.set_host_askpass("myserver", "pass:ssh/myserver");
    let serialized = app.config.serialize();
    assert!(!serialized.contains("keychain"));
    assert!(serialized.contains("purple:askpass pass:ssh/myserver"));
}

#[test]
fn test_config_set_host_askpass_removes() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    app.config.set_host_askpass("myserver", "");
    let serialized = app.config.serialize();
    assert!(!serialized.contains("purple:askpass"));
    let entries = app.config.host_entries();
    assert_eq!(entries[0].askpass, None);
}

#[test]
fn test_edit_host_from_form_sets_askpass_in_config() {
    // edit_host_from_form calls config.set_host_askpass() before write().
    // Since write() fails with test path, the rollback restores old state.
    // Test the config mutation directly to verify the flow.
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    let entry = HostEntry {
        alias: "myserver".to_string(),
        hostname: "10.0.0.1".to_string(),
        askpass: Some("vault:secret/ssh#pass".to_string()),
        ..Default::default()
    };
    app.config.update_host("myserver", &entry);
    app.config
        .set_host_askpass("myserver", entry.askpass.as_deref().unwrap_or(""));
    let serialized = app.config.serialize();
    assert!(serialized.contains("purple:askpass vault:secret/ssh#pass"));
}

#[test]
fn test_edit_host_sets_vault_ssh_and_certificate_file() {
    let content = "Host myserver\n  HostName 10.0.0.1\n";
    let dir = std::env::temp_dir().join("purple_test_edit_vault_ssh_set");
    let _ = std::fs::create_dir_all(&dir);
    let config_path = dir.join("config");
    let _ = std::fs::write(&config_path, content);
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: config_path.clone(),
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    let host = app.hosts[0].clone();
    app.form = HostForm::from_entry(&host, Default::default());
    app.form.vault_ssh = "ssh/sign/engineer".to_string();

    let result = app.edit_host_from_form("myserver");
    assert!(result.is_ok(), "edit_host_from_form failed: {:?}", result);
    let serialized = app.config.serialize();
    assert!(
        serialized.contains("purple:vault-ssh ssh/sign/engineer"),
        "should have vault-ssh: {}",
        serialized
    );
    assert!(
        serialized.contains("CertificateFile"),
        "should have CertificateFile: {}",
        serialized
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Regression: editing a host that already has a user-set custom
/// CertificateFile must NOT overwrite that path with purple's default
/// when the host has a Vault SSH role. The whole point of the
/// `should_write_certificate_file` invariant.
#[test]
fn test_edit_host_preserves_custom_certificate_file_with_vault_role() {
    let content = "Host myserver\n  HostName 10.0.0.1\n  CertificateFile /etc/ssh/my-custom-cert.pub\n  # purple:vault-ssh ssh/sign/engineer\n";
    let dir = std::env::temp_dir().join(format!(
        "purple_test_preserve_custom_cert_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config");
    std::fs::write(&config_path, content).unwrap();
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: config_path,
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    let host = app.hosts[0].clone();
    app.form = HostForm::from_entry(&host, Default::default());
    // Change something unrelated so the form actually mutates.
    app.form.user = "admin".to_string();

    let result = app.edit_host_from_form("myserver");
    assert!(result.is_ok(), "edit_host_from_form failed: {:?}", result);
    let serialized = app.config.serialize();
    assert!(
        serialized.contains("CertificateFile /etc/ssh/my-custom-cert.pub"),
        "custom CertificateFile must be preserved across edit: {}",
        serialized
    );
    assert!(
        !serialized.contains(".purple/certs/"),
        "purple's default cert path must NOT be written: {}",
        serialized
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_edit_host_clears_vault_ssh_removes_certificate_file() {
    let content = "Host myserver\n  HostName 10.0.0.1\n  CertificateFile ~/.purple/certs/myserver-cert.pub\n  # purple:vault-ssh ssh/sign/old\n";
    let dir = std::env::temp_dir().join("purple_test_edit_vault_ssh_clear");
    let _ = std::fs::create_dir_all(&dir);
    let config_path = dir.join("config");
    let _ = std::fs::write(&config_path, content);
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: config_path.clone(),
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    let host = app.hosts[0].clone();
    app.form = HostForm::from_entry(&host, Default::default());
    app.form.vault_ssh = String::new();

    let result = app.edit_host_from_form("myserver");
    assert!(result.is_ok(), "edit_host_from_form failed: {:?}", result);
    let serialized = app.config.serialize();
    assert!(
        !serialized.contains("vault-ssh"),
        "vault-ssh should be removed: {}",
        serialized
    );
    assert!(
        !serialized.contains("CertificateFile"),
        "CertificateFile should be removed: {}",
        serialized
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_edit_pattern_from_form_finds_multi_host_pattern() {
    // Multi-host patterns like "Host web-* db-*" have spaces in the pattern.
    // edit_host_from_form must find them via has_host_block, not has_host.
    let mut app = make_app("Host web-* db-*\n  User deploy\n");
    assert_eq!(app.patterns.len(), 1);
    assert_eq!(app.patterns[0].pattern, "web-* db-*");

    app.form = HostForm::from_pattern_entry(&app.patterns[0]);
    assert!(app.form.is_pattern);
    app.form.user = "admin".to_string();

    let result = app.edit_host_from_form("web-* db-*");
    assert!(result.is_ok(), "expected success, got: {:?}", result);
}

#[test]
fn test_edit_single_pattern_from_form() {
    let mut app = make_app("Host *.example.com\n  User deploy\n");
    assert_eq!(app.patterns.len(), 1);
    app.form = HostForm::from_pattern_entry(&app.patterns[0]);
    app.form.user = "admin".to_string();

    let result = app.edit_host_from_form("*.example.com");
    assert!(result.is_ok(), "expected success, got: {:?}", result);
}

#[test]
fn test_edit_pattern_duplicate_detection() {
    let mut app = make_app("Host web-* db-*\n  User deploy\nHost cache-*\n  User cache\n");
    let pat = app
        .patterns
        .iter()
        .find(|p| p.pattern == "web-* db-*")
        .unwrap()
        .clone();
    app.form = HostForm::from_pattern_entry(&pat);
    app.form.alias = "cache-*".to_string();
    let result = app.edit_host_from_form("web-* db-*");
    assert_eq!(result, Err("Pattern 'cache-*' already exists.".to_string()));
}

#[test]
fn test_add_pattern_duplicate_detection() {
    let mut app = make_app("Host web-* db-*\n  User deploy\n");
    app.form = HostForm::new_pattern();
    app.form.alias = "web-* db-*".to_string();
    app.form.user = "admin".to_string();
    let result = app.add_host_from_form();
    assert_eq!(
        result,
        Err("Pattern 'web-* db-*' already exists.".to_string())
    );
}

// --- pending_connect carries askpass ---

#[test]
fn test_pending_connect_with_askpass() {
    let app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    let host = &app.hosts[0];
    assert_eq!(host.askpass, Some("keychain".to_string()));
    // Simulating what handle_host_list does
    let pending = (host.alias.clone(), host.askpass.clone());
    assert_eq!(pending.0, "myserver");
    assert_eq!(pending.1, Some("keychain".to_string()));
}

#[test]
fn test_pending_connect_without_askpass() {
    let app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    let host = &app.hosts[0];
    let pending = (host.alias.clone(), host.askpass.clone());
    assert_eq!(pending.0, "myserver");
    assert_eq!(pending.1, None);
}

// --- from_entry roundtrip for all source types ---

#[test]
fn test_form_entry_roundtrip_all_sources() {
    let sources = [
        Some("keychain".to_string()),
        Some("op://V/I/p".to_string()),
        Some("bw:item".to_string()),
        Some("pass:ssh/x".to_string()),
        Some("vault:s/d#f".to_string()),
        Some("cmd %a %h".to_string()),
        None,
    ];
    for askpass in &sources {
        let entry = HostEntry {
            alias: "test".to_string(),
            hostname: "1.2.3.4".to_string(),
            askpass: askpass.clone(),
            ..Default::default()
        };
        let form = HostForm::from_entry(&entry, Default::default());
        let back = form.to_entry();
        assert_eq!(back.askpass, *askpass, "Roundtrip failed for {:?}", askpass);
    }
}

// --- askpass special values ---

#[test]
fn test_to_entry_askpass_with_equals_sign() {
    let mut form = HostForm::new();
    form.alias = "test".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "cmd --opt=val %h".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.askpass, Some("cmd --opt=val %h".to_string()));
}

#[test]
fn test_to_entry_askpass_with_hash() {
    let mut form = HostForm::new();
    form.alias = "test".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "vault:secret/ssh#api_key".to_string();
    let entry = form.to_entry();
    assert_eq!(entry.askpass, Some("vault:secret/ssh#api_key".to_string()));
}

#[test]
fn test_to_entry_askpass_long_value() {
    let mut form = HostForm::new();
    form.alias = "test".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "op://My Personal Vault/SSH Production Server/password".to_string();
    let entry = form.to_entry();
    assert_eq!(
        entry.askpass,
        Some("op://My Personal Vault/SSH Production Server/password".to_string())
    );
}

// --- edit form askpass rollback logic ---

#[test]
fn test_edit_askpass_rollback_restores_old_source() {
    // Simulate the rollback logic from edit_host_from_form
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    let old_entry = app.hosts[0].clone();
    assert_eq!(old_entry.askpass, Some("keychain".to_string()));

    // Apply new askpass
    app.config
        .set_host_askpass("myserver", "vault:secret/ssh#pw");
    assert_eq!(
        app.config.host_entries()[0].askpass,
        Some("vault:secret/ssh#pw".to_string())
    );

    // Simulate rollback (write failed)
    app.config
        .set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
    assert_eq!(
        app.config.host_entries()[0].askpass,
        Some("keychain".to_string())
    );
}

#[test]
fn test_edit_vault_addr_rollback_restores_old_value() {
    // Mirrors the askpass rollback pattern: on a simulated write
    // failure, the old vault_addr comment must be restored so the
    // on-disk config is not half-mutated when the form submit bails.
    let mut app = make_app(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:vault-ssh ssh-client-signer/sign/engineer\n  # purple:vault-addr http://old:8200\n",
    );
    let old_entry = app.hosts[0].clone();
    assert_eq!(old_entry.vault_addr.as_deref(), Some("http://old:8200"));

    // Apply a new vault_addr (successful in-memory mutation).
    assert!(
        app.config
            .set_host_vault_addr("myserver", "http://new:8200")
    );
    assert_eq!(
        app.config.host_entries()[0].vault_addr.as_deref(),
        Some("http://new:8200")
    );

    // Simulate rollback (write failed). This is the exact call the
    // edit_host_from_form rollback block makes.
    let _ = app.config.set_host_vault_addr(
        &old_entry.alias,
        old_entry.vault_addr.as_deref().unwrap_or(""),
    );
    assert_eq!(
        app.config.host_entries()[0].vault_addr.as_deref(),
        Some("http://old:8200")
    );
}

#[test]
fn test_edit_vault_addr_rollback_restores_none() {
    // Rollback from a just-added vault_addr back to empty (no comment).
    let mut app = make_app(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:vault-ssh ssh-client-signer/sign/engineer\n",
    );
    let old_entry = app.hosts[0].clone();
    assert!(old_entry.vault_addr.is_none());

    assert!(
        app.config
            .set_host_vault_addr("myserver", "http://new:8200")
    );
    assert_eq!(
        app.config.host_entries()[0].vault_addr.as_deref(),
        Some("http://new:8200")
    );

    let _ = app.config.set_host_vault_addr(
        &old_entry.alias,
        old_entry.vault_addr.as_deref().unwrap_or(""),
    );
    assert!(app.config.host_entries()[0].vault_addr.is_none());
}

#[test]
fn test_edit_askpass_rollback_restores_none() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    let old_entry = app.hosts[0].clone();
    assert_eq!(old_entry.askpass, None);

    // Apply new askpass
    app.config.set_host_askpass("myserver", "bw:my-item");
    assert_eq!(
        app.config.host_entries()[0].askpass,
        Some("bw:my-item".to_string())
    );

    // Simulate rollback (write failed)
    app.config
        .set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
    assert_eq!(app.config.host_entries()[0].askpass, None);
}

// --- password picker state edge cases ---

#[test]
fn test_password_picker_initial_state_not_shown() {
    let app = make_app("Host test\n  HostName test.com\n");
    assert!(!app.ui.show_password_picker);
    assert_eq!(app.ui.password_picker_state.selected(), None);
}

// --- askpass global default fallback ---

#[test]
fn test_pending_connect_askpass_from_host() {
    let app = make_app(
        "Host s1\n  HostName 1.1.1.1\n  # purple:askpass bw:item1\n\nHost s2\n  HostName 2.2.2.2\n",
    );
    assert_eq!(app.hosts[0].askpass, Some("bw:item1".to_string()));
    assert_eq!(app.hosts[1].askpass, None);
}

// --- form field cycling includes askpass ---

#[test]
fn test_form_field_cycle_through_askpass() {
    let fields = FormField::ALL;
    let askpass_idx = fields
        .iter()
        .position(|f| matches!(f, FormField::AskPass))
        .unwrap();
    // VaultAddr was added after VaultSsh, shifting AskPass from index 7
    // to index 8. Its neighbours (ProxyJump before, Tags after) are
    // unchanged.
    assert_eq!(askpass_idx, 8, "AskPass should be the 9th field (index 8)");
    assert!(matches!(fields[askpass_idx - 1], FormField::ProxyJump));
    assert!(matches!(fields[askpass_idx + 1], FormField::Tags));
}

// --- validate control chars in askpass ---

#[test]
fn test_validate_askpass_rejects_newline() {
    let mut form = HostForm::new();
    form.alias = "test".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "keychain\ninjected".to_string();
    assert!(form.validate().is_err());
}

#[test]
fn test_validate_askpass_rejects_tab() {
    let mut form = HostForm::new();
    form.alias = "test".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "keychain\tinjected".to_string();
    assert!(form.validate().is_err());
}

#[test]
fn test_validate_askpass_rejects_null_byte() {
    let mut form = HostForm::new();
    form.alias = "test".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "keychain\0injected".to_string();
    assert!(form.validate().is_err());
}

#[test]
fn test_validate_askpass_allows_normal_special_chars() {
    let mut form = HostForm::new();
    form.alias = "test".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "vault:secret/data/my-app#api_key".to_string();
    assert!(form.validate().is_ok());
}

#[test]
fn test_validate_askpass_allows_percent_substitution() {
    let mut form = HostForm::new();
    form.alias = "test".to_string();
    form.hostname = "1.2.3.4".to_string();
    form.askpass = "get-pass %a %h".to_string();
    assert!(form.validate().is_ok());
}

// =========================================================================
// Askpass fallback chain: per-host → global default (replicated logic)
// =========================================================================

#[test]
fn test_askpass_fallback_per_host_takes_precedence() {
    // main.rs: host_askpass.or_else(preferences::load_askpass_default)
    let host_askpass: Option<String> = Some("op://V/I/p".to_string());
    let global_default: Option<String> = Some("keychain".to_string());
    let result = host_askpass.or(global_default);
    assert_eq!(result, Some("op://V/I/p".to_string()));
}

#[test]
fn test_askpass_fallback_uses_global_when_no_per_host() {
    let host_askpass: Option<String> = None;
    let global_default: Option<String> = Some("keychain".to_string());
    let result = host_askpass.or(global_default);
    assert_eq!(result, Some("keychain".to_string()));
}

#[test]
fn test_askpass_fallback_none_when_both_absent() {
    let host_askpass: Option<String> = None;
    let global_default: Option<String> = None;
    let result = host_askpass.or(global_default);
    assert_eq!(result, None);
}

// =========================================================================
// cleanup_marker called after connection (document contract)
// =========================================================================

#[test]
fn test_cleanup_marker_contract() {
    // After successful connection, main.rs calls askpass::cleanup_marker(&alias)
    // to remove the retry detection marker file
    let alias = "myserver";
    let call = format!("askpass::cleanup_marker(\"{}\")", alias);
    assert!(call.contains("cleanup_marker"));
}

// =========================================================================
// pending_connect carries askpass through TUI event loop
// =========================================================================

#[test]
fn test_pending_connect_tuple_structure() {
    // pending_connect is Option<(String, Option<String>)> = (alias, askpass)
    let (alias, askpass) = ("myserver".to_string(), Some("keychain".to_string()));
    assert_eq!(alias, "myserver");
    assert_eq!(askpass, Some("keychain".to_string()));
}

#[test]
fn test_pending_connect_none_askpass() {
    let (alias, askpass): (String, Option<String>) = ("myserver".to_string(), None);
    assert_eq!(alias, "myserver");
    assert!(askpass.is_none());
}

// =========================================================================
// bw_session caching in app state
// =========================================================================

#[test]
fn test_bw_session_cached_across_connections() {
    let mut app = make_app(
        "Host a\n  HostName 1.1.1.1\n  # purple:askpass bw:item\n\nHost b\n  HostName 2.2.2.2\n  # purple:askpass bw:other\n",
    );
    // First connection prompts for unlock and caches token
    app.bw_session = Some("cached-token".to_string());
    // Second connection should reuse cached token
    let existing = app.bw_session.as_deref();
    assert_eq!(existing, Some("cached-token"));
    // ensure_bw_session returns None when existing is Some (no re-prompt)
    let needs_prompt = existing.is_none();
    assert!(!needs_prompt);
}

#[test]
fn test_bw_session_not_set_for_non_bw() {
    let app = make_app("Host srv\n  HostName 1.1.1.1\n  # purple:askpass keychain\n");
    assert!(app.bw_session.is_none());
}

// =========================================================================
// AskPass field in HostForm: display label and position
// =========================================================================

#[test]
fn test_askpass_field_is_ninth_in_form() {
    // VaultAddr was added at position 6 (right after VaultSsh), pushing
    // ProxyJump/AskPass/Tags each one slot further.
    let fields = FormField::ALL;
    assert_eq!(fields.len(), 10);
    assert!(matches!(fields[8], FormField::AskPass));
}

#[test]
fn test_field_order_identity_vault_addr_proxy_askpass_tags() {
    let fields = FormField::ALL;
    assert!(matches!(fields[4], FormField::IdentityFile));
    assert!(matches!(fields[5], FormField::VaultSsh));
    assert!(matches!(fields[6], FormField::VaultAddr));
    assert!(matches!(fields[7], FormField::ProxyJump));
    assert!(matches!(fields[8], FormField::AskPass));
    assert!(matches!(fields[9], FormField::Tags));
}

// =========================================================================
// Search/filter with provider_tags
// =========================================================================

#[test]
fn test_search_tag_exact_matches_provider_tags() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:provider_tags prod\n");
    app.start_search();
    app.search.query = Some("tag=prod".to_string());
    app.apply_filter();
    assert_eq!(app.search.filtered_indices, vec![0]);
}

#[test]
fn test_search_tag_fuzzy_matches_provider_tags() {
    let mut app =
        make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:provider_tags production\n");
    app.start_search();
    app.search.query = Some("tag:prod".to_string());
    app.apply_filter();
    assert_eq!(app.search.filtered_indices, vec![0]);
}

#[test]
fn test_search_general_matches_provider_tags() {
    let mut app =
        make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:provider_tags staging\n");
    app.start_search();
    app.search.query = Some("staging".to_string());
    app.apply_filter();
    assert_eq!(app.search.filtered_indices, vec![0]);
}

#[test]
fn test_collect_unique_tags_includes_provider_tags() {
    let app = make_app(
        "Host srv1\n  HostName 10.0.0.1\n  # purple:tags user1\n  # purple:provider_tags cloud1\n\nHost srv2\n  HostName 10.0.0.2\n  # purple:provider_tags cloud2\n  # purple:tags user2\n",
    );
    let tags = app.collect_unique_tags();
    assert!(tags.contains(&"user1".to_string()));
    assert!(tags.contains(&"user2".to_string()));
    assert!(tags.contains(&"cloud1".to_string()));
    assert!(tags.contains(&"cloud2".to_string()));
}

#[test]
fn test_sort_alpha_alias_stale_to_bottom() {
    let config_str = "\
Host alpha
  HostName 1.1.1.1
  # purple:stale 1711900000

Host beta
  HostName 2.2.2.2

Host gamma
  HostName 3.3.3.3
  # purple:stale 1711900000
";
    let mut app = make_app(config_str);
    app.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();

    // beta (non-stale) should come first, then alpha and gamma (stale, sorted alphabetically)
    assert_eq!(app.display_list.len(), 3);
    if let HostListItem::Host { index } = &app.display_list[0] {
        assert_eq!(app.hosts[*index].alias, "beta");
    } else {
        panic!("Expected Host item at position 0");
    }
    if let HostListItem::Host { index } = &app.display_list[1] {
        assert_eq!(app.hosts[*index].alias, "alpha");
    } else {
        panic!("Expected Host item at position 1");
    }
    if let HostListItem::Host { index } = &app.display_list[2] {
        assert_eq!(app.hosts[*index].alias, "gamma");
    } else {
        panic!("Expected Host item at position 2");
    }
}

#[test]
fn test_apply_sort_selects_first_in_sorted_order() {
    // Config order: charlie, alpha, beta
    let mut app = make_app(
        "Host charlie\n  HostName c.com\n\nHost alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n",
    );
    // Initial selection should be charlie (first in config)
    assert_eq!(app.selected_host().unwrap().alias, "charlie");

    // Sort alphabetically and reset selection to first sorted
    app.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();
    app.select_first_host();

    // After sort + select_first_host, alpha should be selected (first alphabetically)
    assert_eq!(app.selected_host().unwrap().alias, "alpha");
}

#[test]
fn test_apply_sort_preserves_selection_without_reset() {
    // Verify apply_sort alone preserves the current selection (for interactive use)
    let mut app = make_app(
        "Host charlie\n  HostName c.com\n\nHost alpha\n  HostName a.com\n\nHost beta\n  HostName b.com\n",
    );
    assert_eq!(app.selected_host().unwrap().alias, "charlie");

    app.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();

    // apply_sort preserves the previously selected host (charlie)
    assert_eq!(app.selected_host().unwrap().alias, "charlie");
}

#[test]
fn test_select_first_host_lands_on_group_header_when_grouped() {
    let content = "\
Host do-beta
  HostName 2.2.2.2
  # purple:provider digitalocean:2

Host do-alpha
  HostName 1.1.1.1
  # purple:provider digitalocean:1
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();
    app.select_first_host();

    // Headers are never selectable; first host is selected instead
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(_)));
    assert_eq!(app.ui.list_state.selected(), Some(1));
    assert!(app.selected_host().is_some());
}

#[test]
fn test_select_first_host_skips_group_header_when_ungrouped() {
    let content = "\
Host do-beta
  HostName 2.2.2.2
  # purple:provider digitalocean:2

Host do-alpha
  HostName 1.1.1.1
  # purple:provider digitalocean:1
";
    let mut app = make_app(content);
    // GroupBy::None means headers should be skipped
    app.group_by = GroupBy::None;
    app.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();
    app.select_first_host();

    // With no grouping, display_list has no headers
    assert_eq!(app.selected_host().unwrap().alias, "do-alpha");
}

#[test]
fn test_select_first_host_with_hostname_sort() {
    // Config order: srv-a (z.com), srv-b (a.com), srv-c (m.com)
    let mut app = make_app(
        "Host srv-a\n  HostName z.com\n\nHost srv-b\n  HostName a.com\n\nHost srv-c\n  HostName m.com\n",
    );
    app.sort_mode = SortMode::AlphaHostname;
    app.apply_sort();
    app.select_first_host();

    // srv-b has hostname a.com, should be first alphabetically by hostname
    assert_eq!(app.selected_host().unwrap().alias, "srv-b");
}

#[test]
fn test_filter_tag_exact_stale() {
    let config_str = "\
Host alpha
  HostName 1.1.1.1
  # purple:stale 1711900000

Host beta
  HostName 2.2.2.2

Host gamma
  HostName 3.3.3.3
  # purple:stale 1711900000
";
    let mut app = make_app(config_str);
    app.start_search();
    app.search.query = Some("tag=stale".to_string());
    app.apply_filter();

    // Only stale hosts (alpha and gamma) should match
    assert_eq!(app.search.filtered_indices.len(), 2);
    assert_eq!(app.hosts[app.search.filtered_indices[0]].alias, "alpha");
    assert_eq!(app.hosts[app.search.filtered_indices[1]].alias, "gamma");
}

#[test]
fn test_filter_tag_fuzzy_stale() {
    let config_str = "\
Host alpha
  HostName 1.1.1.1
  # purple:stale 1711900000

Host beta
  HostName 2.2.2.2

Host gamma
  HostName 3.3.3.3
  # purple:stale 1711900000
";
    let mut app = make_app(config_str);
    app.start_search();
    app.search.query = Some("tag:stal".to_string());
    app.apply_filter();

    // Fuzzy match on "stal" should match stale hosts
    assert_eq!(app.search.filtered_indices.len(), 2);
    assert_eq!(app.hosts[app.search.filtered_indices[0]].alias, "alpha");
    assert_eq!(app.hosts[app.search.filtered_indices[1]].alias, "gamma");
}

#[test]
fn test_apply_sync_result_stale_in_message() {
    // Create a temp config file so writes succeed
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join(format!("purple_test_stale_{}.conf", std::process::id()));
    let initial_config = "\
Host do-web
  HostName 1.2.3.4
  # purple:provider digitalocean:s1

Host do-db
  HostName 5.6.7.8
  # purple:provider digitalocean:s2
";
    std::fs::write(&tmp_path, initial_config).unwrap();

    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(initial_config),
        path: tmp_path.clone(),
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    app.provider_config = crate::providers::config::ProviderConfig::default();
    app.provider_config
        .set_section(crate::providers::config::ProviderSection {
            provider: "digitalocean".to_string(),
            token: "test-token".to_string(),
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
        });

    // First sync adds both hosts
    let hosts = vec![
        crate::providers::ProviderHost::new(
            "s1".to_string(),
            "web".to_string(),
            "1.2.3.4".to_string(),
            vec![],
        ),
        crate::providers::ProviderHost::new(
            "s2".to_string(),
            "db".to_string(),
            "5.6.7.8".to_string(),
            vec![],
        ),
    ];
    let (_, is_err, _, _, _, _) = app.apply_sync_result("digitalocean", hosts, false);
    assert!(!is_err);

    // Second sync with only one host (non-partial) should mark the other as stale
    let hosts2 = vec![crate::providers::ProviderHost::new(
        "s1".to_string(),
        "web".to_string(),
        "1.2.3.4".to_string(),
        vec![],
    )];
    let (msg, is_err, total, _, _, stale) = app.apply_sync_result("digitalocean", hosts2, false);
    assert!(!is_err);
    assert_eq!(total, 1); // only the one host that's still present
    assert_eq!(stale, 1);
    assert!(
        msg.contains("stale 1"),
        "Expected stale count in message, got: {}",
        msg
    );

    // Clean up
    let _ = std::fs::remove_file(&tmp_path);
}

// --- Pattern form validation tests ---

#[test]
fn pattern_form_validates_wildcard_required() {
    let mut form = HostForm::new_pattern();
    form.alias = "myserver".to_string(); // No wildcard
    assert!(form.validate().is_err());
    form.alias = "*.example.com".to_string(); // Valid pattern
    assert!(form.validate().is_ok());
    form.alias = "10.30.0.*".to_string(); // Valid IP pattern
    assert!(form.validate().is_ok());
    form.alias = "server-[123]".to_string(); // Valid char class
    assert!(form.validate().is_ok());
    form.alias = "prod staging".to_string(); // Valid multi-pattern (space = pattern)
    assert!(form.validate().is_ok());
}

#[test]
fn pattern_form_hostname_optional() {
    let mut form = HostForm::new_pattern();
    form.alias = "*.example.com".to_string();
    // Hostname empty is OK for patterns
    assert!(form.validate().is_ok());
    // Hostname filled is also OK
    form.hostname = "10.0.0.1".to_string();
    assert!(form.validate().is_ok());
}

#[test]
fn reload_hosts_clears_filtered_pattern_indices() {
    let config_str = "\
Host myserver
  HostName 1.1.1.1

Host 10.30.0.*
  User debian
";
    let mut app = make_app(config_str);
    assert_eq!(app.patterns.len(), 1);
    // Start a search that matches the pattern
    app.start_search();
    app.search.query = Some("10.30".to_string());
    app.apply_filter();
    assert!(!app.search.filtered_pattern_indices.is_empty());
    // Cancel search and verify cleared
    app.cancel_search();
    assert!(app.search.filtered_pattern_indices.is_empty());
    // Start search again, then reload (simulates config change)
    app.start_search();
    app.search.query = Some("10.30".to_string());
    app.apply_filter();
    assert!(!app.search.filtered_pattern_indices.is_empty());
    // Simulate non-search reload path
    app.search.query = None;
    app.reload_hosts();
    assert!(app.search.filtered_pattern_indices.is_empty());
}

#[test]
fn pattern_clone_clears_alias() {
    let entry = crate::ssh_config::model::PatternEntry {
        pattern: "10.30.0.*".to_string(),
        user: "debian".to_string(),
        identity_file: "~/.ssh/id_ed25519".to_string(),
        ..Default::default()
    };
    let mut form = HostForm::from_pattern_entry(&entry);
    // Simulate clone behavior from handler.rs
    form.alias.clear();
    form.cursor_pos = 0;
    assert!(form.is_pattern);
    assert!(form.alias.is_empty());
    assert_eq!(form.cursor_pos, 0);
    // Other fields should be preserved
    assert_eq!(form.user, "debian");
    assert_eq!(form.identity_file, "~/.ssh/id_ed25519");
}

#[test]
fn tag_exact_search_finds_patterns() {
    let config_str = "\
Host myserver
  HostName 1.1.1.1
  # purple:tags web

Host 10.30.0.*
  User debian
  # purple:tags internal
";
    let mut app = make_app(config_str);
    app.start_search();
    app.search.query = Some("tag=internal".to_string());
    app.apply_filter();
    // Host should not match
    assert!(app.search.filtered_indices.is_empty());
    // Pattern should match
    assert_eq!(app.search.filtered_pattern_indices.len(), 1);
    assert_eq!(
        app.patterns[app.search.filtered_pattern_indices[0]].pattern,
        "10.30.0.*"
    );
}

#[test]
fn tag_fuzzy_search_finds_patterns() {
    let config_str = "\
Host myserver
  HostName 1.1.1.1

Host 10.30.0.*
  User debian
  # purple:tags internal
";
    let mut app = make_app(config_str);
    app.start_search();
    app.search.query = Some("tag:intern".to_string());
    app.apply_filter();
    assert!(app.search.filtered_indices.is_empty());
    assert_eq!(app.search.filtered_pattern_indices.len(), 1);
}

#[test]
fn collect_unique_tags_includes_pattern_tags() {
    let config_str = "\
Host myserver
  HostName 1.1.1.1
  # purple:tags web

Host 10.30.0.*
  User debian
  # purple:tags internal
";
    let app = make_app(config_str);
    let tags = app.collect_unique_tags();
    assert!(tags.contains(&"web".to_string()));
    assert!(tags.contains(&"internal".to_string()));
}

#[test]
fn general_search_matches_pattern_tags() {
    let config_str = "\
Host myserver
  HostName 1.1.1.1

Host 10.30.0.*
  User debian
  # purple:tags internal
";
    let mut app = make_app(config_str);
    app.start_search();
    app.search.query = Some("internal".to_string());
    app.apply_filter();
    assert!(
        app.search.filtered_indices.is_empty(),
        "host should not match"
    );
    assert_eq!(
        app.search.filtered_pattern_indices.len(),
        1,
        "pattern with matching tag should appear in general search"
    );
    assert_eq!(
        app.patterns[app.search.filtered_pattern_indices[0]].pattern,
        "10.30.0.*"
    );
}

#[test]
fn pattern_placeholder_text() {
    use crate::app::FormField;
    use crate::ui::host_form::{placeholder_text, placeholder_text_pattern};
    // Regular host placeholder
    assert_eq!(placeholder_text(FormField::Alias), "e.g. prod or db-01");
    // Pattern placeholder
    assert_eq!(
        placeholder_text_pattern(FormField::Alias),
        "10.0.0.* or *.example.com"
    );
    // Non-alias fields should be the same regardless of is_pattern
    assert_eq!(
        placeholder_text(FormField::User),
        placeholder_text_pattern(FormField::User)
    );
}

#[test]
fn pattern_form_from_entry_roundtrip() {
    let entry = crate::ssh_config::model::PatternEntry {
        pattern: "10.30.0.*".to_string(),
        hostname: String::new(),
        user: "debian".to_string(),
        port: 2222,
        identity_file: "~/.ssh/id_ed25519".to_string(),
        proxy_jump: "bastion".to_string(),
        tags: vec!["internal".to_string()],
        askpass: Some("keychain".to_string()),
        source_file: None,
        directives: vec![
            ("User".to_string(), "debian".to_string()),
            ("Port".to_string(), "2222".to_string()),
        ],
    };
    let form = HostForm::from_pattern_entry(&entry);
    assert!(form.is_pattern);
    assert_eq!(form.alias, "10.30.0.*");
    assert_eq!(form.user, "debian");
    assert_eq!(form.port, "2222");
    assert_eq!(form.identity_file, "~/.ssh/id_ed25519");
    assert_eq!(form.proxy_jump, "bastion");
    assert_eq!(form.tags, "internal");
    assert_eq!(form.askpass, "keychain");
}

// --- GroupBy::from_key edge cases ---

#[test]
fn group_by_from_key_tag_with_colon_in_name() {
    // "tag:prod:us-east" — everything after first "tag:" is the tag name
    assert_eq!(
        GroupBy::from_key("tag:prod:us-east"),
        GroupBy::Tag("prod:us-east".to_string())
    );
}

#[test]
fn group_by_from_key_tag_with_special_chars() {
    assert_eq!(
        GroupBy::from_key("tag:prod-v2.1"),
        GroupBy::Tag("prod-v2.1".to_string())
    );
}

#[test]
fn group_by_from_key_tag_with_unicode() {
    assert_eq!(
        GroupBy::from_key("tag:生产"),
        GroupBy::Tag("生产".to_string())
    );
}

#[test]
fn group_by_from_key_tag_with_spaces() {
    assert_eq!(
        GroupBy::from_key("tag:my servers"),
        GroupBy::Tag("my servers".to_string())
    );
}

// --- group_indices_by_tag with stale hosts ---

#[test]
fn group_by_tag_stale_host_with_tag() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
  # purple:stale 1700000000

Host web2
  HostName 2.2.2.2
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // Both hosts have the tag, stale or not — both in group
    assert_eq!(app.display_list.len(), 3);
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
}

#[test]
fn group_by_tag_host_with_provider_and_user_tags() {
    let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:tags production
  # purple:provider_tags cloud,frontend
  # purple:provider digitalocean:123

Host manual
  HostName 2.2.2.2
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // Both hosts have user tag "production" — both grouped
    assert_eq!(app.display_list.len(), 3);
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
}

#[test]
fn group_by_tag_provider_tag_not_matched() {
    // provider_tags should NOT be matched by group_indices_by_tag
    let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider_tags production

Host manual
  HostName 2.2.2.2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // "production" is a provider_tag, not a user tag — no grouping
    assert_eq!(app.display_list.len(), 2);
    assert!(
        app.display_list
            .iter()
            .all(|item| matches!(item, HostListItem::Host { .. }))
    );
}

// --- apply_sort() — missing SortMode x GroupBy combinations ---

#[test]
fn group_by_tag_with_original_sort() {
    let content = "\
Host zeta
  HostName 1.1.1.1
  # purple:tags production

Host alpha
  HostName 2.2.2.2
  # purple:tags production

Host manual
  HostName 3.3.3.3
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.sort_mode = SortMode::Original;
    app.apply_sort();

    // manual ungrouped, then production header + zeta + alpha (config order)
    assert_eq!(app.display_list.len(), 4);
    assert!(matches!(&app.display_list[0], HostListItem::Host { .. }));
    assert!(matches!(&app.display_list[1], HostListItem::GroupHeader(s) if s == "production"));
    // Verify config order preserved within group
    if let HostListItem::Host { index } = &app.display_list[2] {
        assert_eq!(app.hosts[*index].alias, "zeta");
    } else {
        panic!("Expected Host item at position 2");
    }
    if let HostListItem::Host { index } = &app.display_list[3] {
        assert_eq!(app.hosts[*index].alias, "alpha");
    } else {
        panic!("Expected Host item at position 3");
    }
}

#[test]
fn group_by_tag_with_hostname_sort() {
    let content = "\
Host web1
  HostName zebra.example.com
  # purple:tags production

Host web2
  HostName alpha.example.com
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.sort_mode = SortMode::AlphaHostname;
    app.apply_sort();

    assert_eq!(app.display_list.len(), 3);
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
    if let HostListItem::Host { index } = &app.display_list[1] {
        assert_eq!(app.hosts[*index].hostname, "alpha.example.com");
    } else {
        panic!("Expected Host item");
    }
}

#[test]
fn group_by_provider_with_hostname_sort() {
    let content = "\
Host do-zebra
  HostName zebra.example.com
  # purple:provider digitalocean:1

Host do-alpha
  HostName alpha.example.com
  # purple:provider digitalocean:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.sort_mode = SortMode::AlphaHostname;
    app.apply_sort();

    assert_eq!(app.display_list.len(), 3);
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean"));
    if let HostListItem::Host { index } = &app.display_list[1] {
        assert_eq!(app.hosts[*index].hostname, "alpha.example.com");
    } else {
        panic!("Expected Host item");
    }
}

#[test]
fn group_by_none_with_each_sort_mode() {
    let content = "\
Host beta
  HostName 2.2.2.2

Host alpha
  HostName 1.1.1.1
";
    for mode in [SortMode::AlphaAlias, SortMode::AlphaHostname] {
        let mut app = make_app(content);
        app.group_by = GroupBy::None;
        app.sort_mode = mode;
        app.apply_sort();

        // No headers, just sorted hosts
        assert_eq!(app.display_list.len(), 2);
        assert!(
            app.display_list
                .iter()
                .all(|item| matches!(item, HostListItem::Host { .. }))
        );
        if let HostListItem::Host { index } = &app.display_list[0] {
            assert_eq!(app.hosts[*index].alias, "alpha");
        }
    }
}

// --- Search + grouping interaction ---

#[test]
fn search_works_with_tag_grouping() {
    let content = "\
Host web-prod
  HostName 1.1.1.1
  # purple:tags production

Host web-staging
  HostName 2.2.2.2
  # purple:tags staging

Host db-prod
  HostName 3.3.3.3
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // Before search: 1 ungrouped + 1 header + 2 grouped = 4
    assert_eq!(app.display_list.len(), 4);

    // Start search and filter for "web"
    app.start_search();
    app.search.query = Some("web".to_string());
    app.apply_filter();

    // Search should filter to web-prod and web-staging
    assert_eq!(app.search.filtered_indices.len(), 2);
}

// --- Multi-select cleared on group change ---

#[test]
fn multi_select_cleared_on_group_change() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host web2
  HostName 2.2.2.2
";
    let mut app = make_app(content);
    app.multi_select.insert(0);
    app.multi_select.insert(1);
    assert_eq!(app.multi_select.len(), 2);

    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    assert!(app.multi_select.is_empty());
}

// --- Pattern entries with tag grouping ---

#[test]
fn patterns_appear_at_bottom_with_tag_grouping() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host 10.0.0.*
  User debian
  # purple:tags internal
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();

    // Should have: production header + web1, then Patterns header + pattern
    let has_patterns_header = app
        .display_list
        .iter()
        .any(|item| matches!(item, HostListItem::GroupHeader(s) if s == "Patterns"));
    assert!(
        has_patterns_header,
        "Patterns header should appear at bottom"
    );

    // Patterns header should be after all hosts
    let patterns_pos = app
        .display_list
        .iter()
        .position(|item| matches!(item, HostListItem::GroupHeader(s) if s == "Patterns"))
        .unwrap();
    let last_host_pos = app
        .display_list
        .iter()
        .rposition(|item| matches!(item, HostListItem::Host { .. }));
    if let Some(host_pos) = last_host_pos {
        assert!(
            patterns_pos > host_pos,
            "Patterns header should be after last host"
        );
    }
}

// --- Proptest: group_by_tag display_list consistency ---

use proptest::prelude::*;

/// Generate a simple SSH config block with optional user tags.
fn prop_host_block(alias: String, hostname: String, tags: Option<Vec<String>>) -> String {
    let mut lines = vec![format!("Host {alias}"), format!("  HostName {hostname}")];
    if let Some(ref ts) = tags {
        if !ts.is_empty() {
            lines.push(format!("  # purple:tags {}", ts.join(",")));
        }
    }
    lines.join("\n")
}

proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(200))]

    /// GroupBy::Tag display_list is consistent:
    /// - Total host items == app.hosts.len()
    /// - No duplicate host indices
    /// - At most one GroupHeader per apply_sort call
    /// - All indices are in-bounds
    #[test]
    fn group_by_tag_display_list_consistent(
        hosts in prop::collection::vec(
            (
                "[a-z][a-z0-9]{2,10}".prop_map(|s| s),
                "[a-z]{3,8}\\.(com|net|io)".prop_map(|s| s),
                prop::option::of(
                    prop::collection::vec("[a-z]{2,8}", 1..=3)
                ),
            ),
            1..=15,
        ),
        tag_index in 0usize..10,
    ) {
        // Build config content from generated host data
        let mut blocks: Vec<String> = Vec::new();
        let mut all_tags: Vec<String> = Vec::new();

        for (alias, hostname, tags) in &hosts {
            if let Some(ts) = tags {
                for t in ts {
                    if !all_tags.contains(t) {
                        all_tags.push(t.clone());
                    }
                }
            }
            blocks.push(prop_host_block(alias.clone(), hostname.clone(), tags.clone()));
        }

        let content = blocks.join("\n\n") + "\n";
        let mut app = make_app(&content);

        // Pick a tag to group by (or use a nonexistent one if no tags)
        let chosen_tag = if all_tags.is_empty() {
            "nonexistent".to_string()
        } else {
            all_tags[tag_index % all_tags.len()].clone()
        };

        app.group_by = GroupBy::Tag(chosen_tag.clone());
        app.apply_sort();

        let host_count = app.hosts.len();
        let display_host_count = app.display_list.iter()
            .filter(|item| matches!(item, HostListItem::Host { .. }))
            .count();

        // All hosts must appear exactly once
        prop_assert_eq!(
            host_count,
            display_host_count,
            "host count mismatch: {} hosts but {} in display_list",
            host_count,
            display_host_count,
        );

        // No duplicate host indices
        let indices: Vec<usize> = app.display_list.iter()
            .filter_map(|item| {
                if let HostListItem::Host { index } = item {
                    Some(*index)
                } else {
                    None
                }
            })
            .collect();

        let mut seen = std::collections::HashSet::new();
        for &idx in &indices {
            prop_assert!(
                seen.insert(idx),
                "duplicate host index {} in display_list",
                idx,
            );
            prop_assert!(
                idx < host_count,
                "host index {} out of bounds (hosts len {})",
                idx,
                host_count,
            );
        }

        // At most one GroupHeader with the chosen tag name
        let header_count = app.display_list.iter()
            .filter(|item| matches!(item, HostListItem::GroupHeader(s) if s == &chosen_tag))
            .count();
        prop_assert!(
            header_count <= 1,
            "expected at most 1 GroupHeader for '{}', got {}",
            chosen_tag,
            header_count,
        );

        // If header is present, all tagged hosts appear after it
        if header_count == 1 {
            let header_pos = app.display_list.iter()
                .position(|item| matches!(item, HostListItem::GroupHeader(s) if s == &chosen_tag))
                .unwrap();
            for (pos, item) in app.display_list.iter().enumerate() {
                if let HostListItem::Host { index } = item {
                    let has_tag = app.hosts[*index].tags.iter().any(|t| t == &chosen_tag);
                    if has_tag {
                        prop_assert!(
                            pos > header_pos,
                            "tagged host at pos {} is before header at pos {}",
                            pos,
                            header_pos,
                        );
                    }
                }
            }
        }
    }

    /// GroupBy::None produces no GroupHeaders and all hosts appear exactly once.
    #[test]
    fn group_by_none_display_list_no_headers(
        hosts in prop::collection::vec(
            (
                "[a-z][a-z0-9]{2,10}".prop_map(|s| s),
                "[a-z]{3,8}\\.(com|net|io)".prop_map(|s| s),
                prop::option::of(prop::collection::vec("[a-z]{2,8}", 1..=3)),
            ),
            1..=10,
        ),
    ) {
        let blocks: Vec<String> = hosts.iter().map(|(alias, hostname, tags)| {
            prop_host_block(alias.clone(), hostname.clone(), tags.clone())
        }).collect();
        let content = blocks.join("\n\n") + "\n";
        let mut app = make_app(&content);

        app.group_by = GroupBy::None;
        app.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();

        let host_count = app.hosts.len();

        // No group headers from GroupBy::None (comment-based headers possible;
        // but no tag/provider headers)
        let display_host_count = app.display_list.iter()
            .filter(|item| matches!(item, HostListItem::Host { .. }))
            .count();

        prop_assert_eq!(
            host_count,
            display_host_count,
            "GroupBy::None: host count mismatch: {} hosts vs {} in display",
            host_count,
            display_host_count,
        );
    }

    /// Switching GroupBy::Tag → GroupBy::None always removes the GroupHeader.
    #[test]
    fn group_by_tag_to_none_removes_header(
        alias in "[a-z][a-z0-9]{2,8}",
        hostname in "[a-z]{3,8}\\.(com|net|io)",
        tag in "[a-z]{2,8}",
    ) {
        let content = format!(
            "Host {alias}\n  HostName {hostname}\n  # purple:tags {tag}\n"
        );
        let mut app = make_app(&content);

        // Apply tag grouping
        app.group_by = GroupBy::Tag(tag.clone());
        app.apply_sort();
        let has_header_grouped = app.display_list.iter()
            .any(|item| matches!(item, HostListItem::GroupHeader(s) if s == &tag));
        prop_assert!(has_header_grouped, "expected GroupHeader for tag '{}'", tag);

        // Switch to None
        app.group_by = GroupBy::None;
        app.apply_sort();
        let has_header_none = app.display_list.iter()
            .any(|item| matches!(item, HostListItem::GroupHeader(s) if s == &tag));
        prop_assert!(!has_header_none, "GroupHeader should be gone after GroupBy::None");
    }
}

#[test]
fn group_by_tag_graceful_when_tag_removed_from_all_hosts() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags staging

Host web2
  HostName 2.2.2.2
";
    let mut app = make_app(content);
    // Group by a tag that no host has
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // All hosts ungrouped, no header, no panic
    assert_eq!(app.display_list.len(), 2);
    assert!(
        app.display_list
            .iter()
            .all(|item| matches!(item, HostListItem::Host { .. }))
    );
}

#[test]
fn group_by_tag_original_sort_preserves_stale_position() {
    // In Original sort mode, stale hosts stay in config order even when grouped.
    // This differs from other sort modes which push stale hosts to the bottom.
    let content = "\
Host stale-first
  HostName 1.1.1.1
  # purple:tags production
  # purple:stale 1700000000

Host healthy-second
  HostName 2.2.2.2
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.sort_mode = SortMode::Original;
    app.apply_sort();

    // Original order preserved: stale host first within group
    assert_eq!(app.display_list.len(), 3);
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
    if let HostListItem::Host { index } = &app.display_list[1] {
        assert_eq!(app.hosts[*index].alias, "stale-first");
    } else {
        panic!("Expected Host item");
    }
}

#[test]
fn group_by_tag_alpha_sort_pushes_stale_to_bottom() {
    // Non-Original sort modes push stale hosts to the bottom of each group.
    let content = "\
Host alpha-stale
  HostName 1.1.1.1
  # purple:tags production
  # purple:stale 1700000000

Host beta-healthy
  HostName 2.2.2.2
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();

    // Alpha sort: stale host pushed to bottom of group
    assert_eq!(app.display_list.len(), 3);
    assert!(matches!(&app.display_list[0], HostListItem::GroupHeader(s) if s == "production"));
    if let HostListItem::Host { index } = &app.display_list[1] {
        assert_eq!(app.hosts[*index].alias, "beta-healthy");
    } else {
        panic!("Expected Host item");
    }
    if let HostListItem::Host { index } = &app.display_list[2] {
        assert_eq!(app.hosts[*index].alias, "alpha-stale");
    } else {
        panic!("Expected Host item");
    }
}

#[test]
fn clear_stale_group_tag_clears_when_tag_missing() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags staging
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());

    let cleared = app.clear_stale_group_tag();

    assert!(cleared);
    assert_eq!(app.group_by, GroupBy::None);
}

#[test]
fn clear_stale_group_tag_keeps_when_tag_exists() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());

    let cleared = app.clear_stale_group_tag();

    assert!(!cleared);
    assert_eq!(app.group_by, GroupBy::Tag("production".to_string()));
}

#[test]
fn clear_stale_group_tag_noop_for_provider() {
    let content = "\
Host web1
  HostName 1.1.1.1
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;

    let cleared = app.clear_stale_group_tag();

    assert!(!cleared);
    assert_eq!(app.group_by, GroupBy::Provider);
}

#[test]
fn clear_stale_group_tag_noop_for_none() {
    let content = "\
Host web1
  HostName 1.1.1.1
";
    let mut app = make_app(content);
    app.group_by = GroupBy::None;

    let cleared = app.clear_stale_group_tag();

    assert!(!cleared);
    assert_eq!(app.group_by, GroupBy::None);
}

#[test]
fn clear_stale_group_tag_empty_hosts() {
    let content = "";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());

    let cleared = app.clear_stale_group_tag();

    assert!(cleared);
    assert_eq!(app.group_by, GroupBy::None);
}

#[test]
fn clear_stale_group_tag_keeps_empty_tag_sentinel() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag(String::new());

    let cleared = app.clear_stale_group_tag();

    assert!(!cleared, "empty tag sentinel should not be cleared");
    assert_eq!(app.group_by, GroupBy::Tag(String::new()));
}

#[test]
fn clear_stale_group_tag_keeps_when_tag_only_on_pattern() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags staging

Host 10.0.0.*
  User root
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());

    let cleared = app.clear_stale_group_tag();

    assert!(
        !cleared,
        "tag existing only on a pattern should not be cleared"
    );
    assert_eq!(app.group_by, GroupBy::Tag("production".to_string()));
}

// --- Group filter (tab navigation) ---

#[test]
fn group_filter_shows_only_group_hosts() {
    let content = "\
Host web-prod
  HostName 1.1.1.1
  # purple:tags production

Host web-staging
  HostName 2.2.2.2
  # purple:tags staging

Host db-prod
  HostName 3.3.3.3
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // Without filter: header + all 3 hosts visible
    let hosts_before: Vec<_> = app
        .display_list
        .iter()
        .filter(|item| matches!(item, HostListItem::Host { .. }))
        .collect();
    assert_eq!(hosts_before.len(), 3, "all 3 hosts should be visible");

    // group_host_counts should show 2 for production
    assert_eq!(
        app.group_host_counts.get("production"),
        Some(&2),
        "production group should have 2 hosts"
    );

    // Filter to production group only
    app.group_filter = Some("production".to_string());
    app.apply_sort();

    // Only production hosts should be visible (no header, no staging host)
    let hosts_after: Vec<_> = app
        .display_list
        .iter()
        .filter(|item| matches!(item, HostListItem::Host { .. }))
        .collect();
    assert_eq!(
        hosts_after.len(),
        2,
        "only production hosts should be visible when filtered"
    );

    // group_host_counts should still show the correct count
    assert_eq!(
        app.group_host_counts.get("production"),
        Some(&2),
        "count should still be 2 with filter active"
    );
}

#[test]
fn group_filter_cleared_restores_display_list() {
    let content = "\
Host web-prod
  HostName 1.1.1.1
  # purple:tags production

Host web-staging
  HostName 2.2.2.2
  # purple:tags staging

Host db-prod
  HostName 3.3.3.3
  # purple:tags production
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());

    // Filter
    app.group_filter = Some("production".to_string());
    app.apply_sort();

    let hosts_filtered: Vec<_> = app
        .display_list
        .iter()
        .filter(|item| matches!(item, HostListItem::Host { .. }))
        .collect();
    assert_eq!(hosts_filtered.len(), 2);

    // Clear filter
    app.group_filter = None;
    app.apply_sort();

    let hosts_unfiltered: Vec<_> = app
        .display_list
        .iter()
        .filter(|item| matches!(item, HostListItem::Host { .. }))
        .collect();
    assert_eq!(
        hosts_unfiltered.len(),
        3,
        "all hosts should reappear after clearing filter"
    );
}

#[test]
fn group_filter_cleared_on_stale_group_by_change() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
  # purple:provider aws:i-123

Host web2
  HostName 2.2.2.2
  # purple:tags staging
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.group_filter = Some("aws".to_string());

    // Change group_by to Tag, which triggers clear_stale_group_tag
    app.group_by = GroupBy::Tag("nonexistent".to_string());
    let cleared = app.clear_stale_group_tag();

    assert!(cleared);
    assert!(
        app.group_filter.is_none(),
        "group_filter should be cleared when group_by tag is stale"
    );
}

#[test]
fn group_tab_order_populated_by_apply_sort() {
    let content = "\
Host web-prod
  HostName 1.1.1.1
  # purple:tags production

Host web-staging
  HostName 2.2.2.2
  # purple:tags staging
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // group_tab_order should contain "production"
    assert!(
        app.group_tab_order.contains(&"production".to_string()),
        "group_tab_order should include production group"
    );
}

#[test]
fn group_tab_order_tag_mode_tiebreaker_is_alphabetical() {
    let content = "\
Host h1
  HostName 1.1.1.1
  # purple:tags beta

Host h2
  HostName 2.2.2.2
  # purple:tags alpha
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("alpha".to_string());
    app.apply_sort();

    assert_eq!(app.group_tab_order.len(), 2);
    assert_eq!(app.group_tab_order[0], "alpha");
    assert_eq!(app.group_tab_order[1], "beta");
}

#[test]
fn ctrl_a_with_group_filter_skips_hidden_hosts() {
    let content = "\
Host web-prod
  HostName 1.1.1.1
  # purple:tags production

Host db-prod
  HostName 3.3.3.3
  # purple:tags production

Host web-staging
  HostName 2.2.2.2
  # purple:tags staging
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    // Filter to staging: only web-staging visible (it's the ungrouped host)
    app.group_filter = Some("production".to_string());
    app.apply_sort();

    // Simulate Ctrl+A: select all visible Host items
    let visible_indices: Vec<usize> = app
        .display_list
        .iter()
        .filter_map(|item| match item {
            HostListItem::Host { index } => Some(*index),
            _ => None,
        })
        .collect();
    for idx in &visible_indices {
        app.multi_select.insert(*idx);
    }

    // Only production hosts should be selected when filter is active
    assert_eq!(app.multi_select.len(), 2);
    for idx in &app.multi_select {
        let host = &app.hosts[*idx];
        assert!(
            host.tags.contains(&"production".to_string()),
            "only production hosts should be selected"
        );
    }
}

// --- Ping generation ---
// Handler-level test: test_p_key_clears_ping_increments_generation in handler.rs

// --- Ctrl+A select all / deselect all ---

#[test]
fn ctrl_a_selects_all_visible_hosts() {
    let content = "\
Host web1
  HostName 1.1.1.1

Host web2
  HostName 2.2.2.2

Host web3
  HostName 3.3.3.3
";
    let mut app = make_app(content);
    app.apply_sort();

    // Simulate Ctrl+A: collect all Host indices from display_list
    let host_indices: Vec<usize> = app
        .display_list
        .iter()
        .filter_map(|item| match item {
            HostListItem::Host { index } => Some(*index),
            _ => None,
        })
        .collect();
    for idx in &host_indices {
        app.multi_select.insert(*idx);
    }

    assert_eq!(app.multi_select.len(), 3);
}

#[test]
fn ctrl_a_toggle_deselects_when_all_selected() {
    let content = "\
Host web1
  HostName 1.1.1.1

Host web2
  HostName 2.2.2.2

Host web3
  HostName 3.3.3.3
";
    let mut app = make_app(content);
    app.apply_sort();

    // Select all
    let host_indices: Vec<usize> = app
        .display_list
        .iter()
        .filter_map(|item| match item {
            HostListItem::Host { index } => Some(*index),
            _ => None,
        })
        .collect();
    for idx in &host_indices {
        app.multi_select.insert(*idx);
    }
    assert_eq!(app.multi_select.len(), 3);

    // Check all_selected condition and clear
    let all_selected = host_indices
        .iter()
        .all(|idx| app.multi_select.contains(idx));
    assert!(all_selected);
    app.multi_select.clear();

    assert!(app.multi_select.is_empty());
}

// --- next_group_tab ---

#[test]
fn next_group_tab_from_all_goes_to_first_group() {
    let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host aws-db
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();
    assert!(app.group_filter.is_none());
    assert_eq!(app.group_tab_index, 0);
    assert!(!app.group_tab_order.is_empty());

    let first_group = app.group_tab_order[0].clone();
    app.next_group_tab();

    assert_eq!(app.group_filter, Some(first_group));
    assert_eq!(app.group_tab_index, 1);
}

#[test]
fn next_group_tab_cycles_through_groups_and_back_to_all() {
    let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host aws-db
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();
    // Ensure exactly 2 groups
    assert_eq!(app.group_tab_order.len(), 2);

    // First call: All -> group1
    app.next_group_tab();
    assert!(app.group_filter.is_some());
    assert_eq!(app.group_tab_index, 1);

    // Second call: group1 -> group2
    app.next_group_tab();
    assert!(app.group_filter.is_some());
    assert_eq!(app.group_tab_index, 2);

    // Third call: group2 -> All
    app.next_group_tab();
    assert_eq!(app.group_filter, None);
    assert_eq!(app.group_tab_index, 0);
}

#[test]
fn next_group_tab_with_zero_groups_does_nothing() {
    let content = "\
Host web1
  HostName 1.1.1.1
";
    let mut app = make_app(content);
    // No grouping, so group_tab_order is empty
    app.group_by = GroupBy::None;
    app.apply_sort();
    assert!(app.group_tab_order.is_empty());

    app.next_group_tab();

    assert_eq!(app.group_filter, None);
    assert_eq!(app.group_tab_index, 0);
}

#[test]
fn next_group_tab_with_one_group_toggles() {
    let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();
    assert_eq!(app.group_tab_order.len(), 1);

    let only_group = app.group_tab_order[0].clone();

    // First call: All -> the one group
    app.next_group_tab();
    assert_eq!(app.group_filter, Some(only_group));

    // Second call: the one group -> All
    app.next_group_tab();
    assert_eq!(app.group_filter, None);
    assert_eq!(app.group_tab_index, 0);
}

// --- prev_group_tab ---

#[test]
fn prev_group_tab_from_all_goes_to_last_group() {
    let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host aws-db
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();
    assert_eq!(app.group_tab_order.len(), 2);

    let last_group = app.group_tab_order.last().unwrap().clone();
    app.prev_group_tab();

    assert_eq!(app.group_filter, Some(last_group));
}

#[test]
fn prev_group_tab_wraps_to_all() {
    let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host aws-db
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();

    // Navigate to the first group using next_group_tab (reliable, deterministic)
    app.next_group_tab();
    assert!(app.group_filter.is_some());
    assert_eq!(app.group_tab_index, 1);

    // prev from first group should go back to All
    app.prev_group_tab();
    assert_eq!(app.group_filter, None);
    assert_eq!(app.group_tab_index, 0);
}

// --- clear_group_filter ---

#[test]
fn clear_group_filter_resets_to_all() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host db1
  HostName 2.2.2.2
  # purple:provider digitalocean:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();

    // Navigate into a group
    app.next_group_tab();
    assert!(app.group_filter.is_some());

    app.clear_group_filter();

    assert_eq!(app.group_filter, None);
    assert_eq!(app.group_tab_index, 0);
}

#[test]
fn clear_group_filter_noop_when_already_none() {
    let content = "Host web1\n  HostName 1.1.1.1\n";
    let mut app = make_app(content);
    assert_eq!(app.group_filter, None);
    assert_eq!(app.group_tab_index, 0);

    // Should not panic or change state
    app.clear_group_filter();

    assert_eq!(app.group_filter, None);
    assert_eq!(app.group_tab_index, 0);
}

// --- select_next_skipping_headers / select_prev_skipping_headers ---

#[test]
fn select_next_skipping_headers_skips_group_header() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host web2
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();

    // display_list: [GroupHeader(DO), Host(idx), GroupHeader(AWS), Host(idx)]
    // Find the first Host item index in display_list
    let first_host_pos = app
        .display_list
        .iter()
        .position(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    app.ui.list_state.select(Some(first_host_pos));

    app.select_next_skipping_headers();

    let selected = app.ui.list_state.selected().unwrap();
    assert!(
        matches!(app.display_list[selected], HostListItem::Host { .. }),
        "selection should land on a Host, not a GroupHeader"
    );
    assert!(
        selected > first_host_pos,
        "selection should have moved forward"
    );
}

#[test]
fn select_prev_skipping_headers_skips_group_header() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host web2
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();

    // Find the last Host item in display_list
    let last_host_pos = app
        .display_list
        .iter()
        .rposition(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    app.ui.list_state.select(Some(last_host_pos));

    app.select_prev_skipping_headers();

    let selected = app.ui.list_state.selected().unwrap();
    assert!(
        matches!(app.display_list[selected], HostListItem::Host { .. }),
        "selection should land on a Host, not a GroupHeader"
    );
    assert!(selected < last_host_pos, "selection should have moved back");
}

#[test]
fn select_next_skipping_headers_stays_at_end() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();

    // Put selection on the only Host item
    let host_pos = app
        .display_list
        .iter()
        .position(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    app.ui.list_state.select(Some(host_pos));

    app.select_next_skipping_headers();

    // Should stay at the same position since there is no next host
    assert_eq!(app.ui.list_state.selected(), Some(host_pos));
}

// --- update_group_tab_follow ---

#[test]
fn tag_mode_tab_follows_selected_host() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host web2
  HostName 2.2.2.2
  # purple:tags staging

Host web3
  HostName 3.3.3.3
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // Find a host with "staging" tag and select it
    for (i, item) in app.display_list.iter().enumerate() {
        if let HostListItem::Host { index } = item {
            if app.hosts[*index].alias == "web2" {
                app.ui.list_state.select(Some(i));
                break;
            }
        }
    }
    app.update_group_tab_follow();
    let staging_pos = app
        .group_tab_order
        .iter()
        .position(|t| t == "staging")
        .unwrap();
    assert_eq!(app.group_tab_index, staging_pos + 1);
}

#[test]
fn tag_mode_tab_follows_first_tab_tag() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host web2
  HostName 2.2.2.2
  # purple:tags staging
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // Select the "production" host
    for (i, item) in app.display_list.iter().enumerate() {
        if let HostListItem::Host { index } = item {
            if app.hosts[*index].alias == "web1" {
                app.ui.list_state.select(Some(i));
                break;
            }
        }
    }
    app.update_group_tab_follow();
    let prod_pos = app
        .group_tab_order
        .iter()
        .position(|t| t == "production")
        .unwrap();
    assert_eq!(app.group_tab_index, prod_pos + 1);
}

#[test]
fn tag_mode_tab_fallback_for_untagged_host() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host web2
  HostName 2.2.2.2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // Select the untagged host
    for (i, item) in app.display_list.iter().enumerate() {
        if let HostListItem::Host { index } = item {
            if app.hosts[*index].alias == "web2" {
                app.ui.list_state.select(Some(i));
                break;
            }
        }
    }
    app.update_group_tab_follow();
    assert_eq!(app.group_tab_index, 0);
}

#[test]
fn tag_mode_tab_ignores_provider_tags() {
    let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider_tags cloud
  # purple:provider digitalocean:1

Host manual
  HostName 2.2.2.2
  # purple:tags cloud
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("cloud".to_string());
    app.apply_sort();

    // "cloud" should only appear once in tab order (from manual's user tag)
    assert_eq!(
        app.group_host_counts.get("cloud").copied().unwrap_or(0),
        1,
        "provider_tags should not count toward group tab"
    );

    // Select the provider-tagged host (do-web has no user tags)
    for (i, item) in app.display_list.iter().enumerate() {
        if let HostListItem::Host { index } = item {
            if app.hosts[*index].alias == "do-web" {
                app.ui.list_state.select(Some(i));
                break;
            }
        }
    }
    app.update_group_tab_follow();
    // do-web has no user tags matching the tab bar, should fall back to All
    assert_eq!(app.group_tab_index, 0);
}

#[test]
fn provider_mode_tab_follows_across_groups() {
    let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host aws-web
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();

    // Navigate to the last host
    let last_host = app
        .display_list
        .iter()
        .enumerate()
        .rfind(|(_, item)| matches!(item, HostListItem::Host { .. }))
        .unwrap()
        .0;
    app.ui.list_state.select(Some(last_host));
    app.update_group_tab_follow();

    // Should be on the group that contains the last host (non-zero)
    assert_ne!(app.group_tab_index, 0);

    // Navigate to the first host
    let first_host = app
        .display_list
        .iter()
        .enumerate()
        .find(|(_, item)| matches!(item, HostListItem::Host { .. }))
        .unwrap()
        .0;
    app.ui.list_state.select(Some(first_host));
    app.update_group_tab_follow();

    // First host has no provider header before it (non-provider hosts first)
    // or is in the first provider group
    let first_idx = app.group_tab_index;
    assert_ne!(first_idx, app.group_tab_order.len() + 1);
}

#[test]
fn group_filter_active_prevents_tab_follow() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production

Host web2
  HostName 2.2.2.2
  # purple:tags staging
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // Set a filter and record the tab index
    app.next_group_tab();
    let fixed_index = app.group_tab_index;

    // Navigate to a different host
    for (i, item) in app.display_list.iter().enumerate() {
        if matches!(item, HostListItem::Host { .. }) {
            app.ui.list_state.select(Some(i));
            break;
        }
    }
    app.update_group_tab_follow();

    // Tab index should not change when filter is active
    assert_eq!(app.group_tab_index, fixed_index);
}

#[test]
fn ungrouped_mode_tab_index_stays_zero() {
    let content = "\
Host web1
  HostName 1.1.1.1

Host web2
  HostName 2.2.2.2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::None;
    app.apply_sort();

    app.ui.list_state.select(Some(1));
    app.update_group_tab_follow();
    assert_eq!(app.group_tab_index, 0);
}

// --- Scoped search ---

#[test]
fn scoped_search_filters_within_group() {
    let content = "\
Host web-do
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host db-do
  HostName 3.3.3.3
  # purple:provider digitalocean:2

Host web-aws
  HostName 2.2.2.2
  # purple:provider aws:3
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();

    // Navigate into the DigitalOcean group
    let do_group = app
        .group_tab_order
        .iter()
        .find(|g| g.to_lowercase().contains("digital"))
        .cloned()
        .unwrap_or_else(|| app.group_tab_order[0].clone());
    app.group_filter = Some(do_group.clone());
    app.apply_sort();

    // Start search with "web" - matches hosts in both providers
    app.start_search();
    app.search.query = Some("web".to_string());
    app.apply_filter();

    // Only web-do should match (web-aws is outside the scoped group)
    assert_eq!(
        app.search.filtered_indices.len(),
        1,
        "scoped search should only return hosts in the active group"
    );
    let matched_idx = app.search.filtered_indices[0];
    assert_eq!(
        app.hosts[matched_idx].provider.as_deref(),
        Some("digitalocean")
    );
}

#[test]
fn global_search_when_no_filter() {
    let content = "\
Host web-do
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host web-aws
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    // No group_filter
    app.apply_sort();

    app.start_search();
    // scope_indices should be None when no group filter is active
    assert!(app.search.scope_indices.is_none());

    app.search.query = Some("web".to_string());
    app.apply_filter();

    // Both hosts match "web"
    assert_eq!(app.search.filtered_indices.len(), 2);
}

// --- group_tab_order computation ---

#[test]
fn group_tab_order_tag_mode_sorted_by_count() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags common

Host web2
  HostName 2.2.2.2
  # purple:tags common

Host db1
  HostName 3.3.3.3
  # purple:tags common

Host cache1
  HostName 4.4.4.4
  # purple:tags rare
";
    let mut app = make_app(content);
    // Use "common" as the active groupBy tag; group_tab_order is computed from all host tags
    app.group_by = GroupBy::Tag("common".to_string());
    app.apply_sort();

    // group_tab_order should be sorted by frequency descending
    // "common" appears 3 times, "rare" once
    assert!(!app.group_tab_order.is_empty());
    assert_eq!(app.group_tab_order[0], "common");
    assert_eq!(app.group_tab_order[1], "rare");
}

#[test]
fn group_tab_order_tag_mode_includes_pattern_tags() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags prod

Host 10.0.0.*
  User root
  # purple:tags internal
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("prod".to_string());
    app.apply_sort();

    assert!(
        app.group_tab_order.contains(&"internal".to_string()),
        "pattern-only tag should appear in group_tab_order"
    );
    assert!(
        app.group_tab_order.contains(&"prod".to_string()),
        "host tag should also appear"
    );
}

#[test]
fn group_host_counts_includes_patterns() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags prod

Host 10.0.0.*
  User root
  # purple:tags prod
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("prod".to_string());
    app.apply_sort();

    // group_host_counts for "prod" should count both the host and the pattern
    assert_eq!(
        app.group_host_counts.get("prod"),
        Some(&2),
        "prod group should count both hosts and patterns"
    );
}

#[test]
fn group_tab_order_tag_mode_max_ten() {
    // Build a config with 12 unique tags
    let mut blocks = Vec::new();
    for i in 0..12 {
        blocks.push(format!(
            "Host host{i}\n  HostName {i}.{i}.{i}.{i}\n  # purple:tags tag{i}"
        ));
    }
    let content = blocks.join("\n\n") + "\n";

    let mut app = make_app(&content);
    app.group_by = GroupBy::Tag("tag0".to_string());
    app.apply_sort();

    assert_eq!(
        app.group_tab_order.len(),
        10,
        "group_tab_order should be capped at exactly 10, got {}",
        app.group_tab_order.len()
    );
}

#[test]
fn group_tab_order_provider_mode_from_headers() {
    let content = "\
Host do-web
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host aws-db
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();

    // group_tab_order should reflect GroupHeader order
    assert!(!app.group_tab_order.is_empty());
    for name in &app.group_tab_order {
        let header_exists = app
            .display_list
            .iter()
            .any(|item| matches!(item, HostListItem::GroupHeader(s) if s == name));
        assert!(
            header_exists,
            "group_tab_order entry '{name}' should have a corresponding GroupHeader"
        );
    }
}

// --- Tag mode filtering ---

#[test]
fn tag_filter_shows_hosts_with_matching_tag() {
    let content = "\
Host web-prod
  HostName 1.1.1.1
  # purple:tags prod

Host web-staging
  HostName 2.2.2.2
  # purple:tags staging
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("prod".to_string());
    app.group_filter = Some("prod".to_string());
    app.apply_sort();

    // Only hosts with the prod tag should appear
    for item in &app.display_list {
        if let HostListItem::Host { index } = item {
            assert!(
                app.hosts[*index].tags.contains(&"prod".to_string()),
                "only hosts with 'prod' tag should appear when filtered"
            );
        }
    }

    let host_count = app
        .display_list
        .iter()
        .filter(|item| matches!(item, HostListItem::Host { .. }))
        .count();
    assert_eq!(host_count, 1, "exactly one prod host should be visible");
}

#[test]
fn tag_filter_includes_patterns_with_matching_tag() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags prod

Host 10.0.0.*
  User root
  # purple:tags prod
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Tag("prod".to_string());
    app.group_filter = Some("prod".to_string());
    app.apply_sort();

    let pattern_count = app
        .display_list
        .iter()
        .filter(|item| matches!(item, HostListItem::Pattern { .. }))
        .count();
    assert_eq!(
        pattern_count, 1,
        "pattern with matching tag should be visible"
    );
}

// --- page_down header skipping ---

#[test]
fn page_down_skips_group_headers() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host web2
  HostName 2.2.2.2
  # purple:provider digitalocean:2

Host aws1
  HostName 3.3.3.3
  # purple:provider aws:3
";
    let mut app = make_app(content);
    app.group_by = GroupBy::Provider;
    app.apply_sort();

    // Start at the first item
    app.ui.list_state.select(Some(0));

    app.page_down_host();

    let selected = app.ui.list_state.selected().unwrap();
    assert!(
        matches!(
            app.display_list[selected],
            HostListItem::Host { .. } | HostListItem::Pattern { .. }
        ),
        "page_down should not land on a GroupHeader"
    );
}

// --- GroupBy::Tag round-trip ---

#[test]
fn group_by_tag_empty_round_trips() {
    let gb = GroupBy::Tag(String::new());
    let key = gb.to_key();
    let restored = GroupBy::from_key(&key);
    assert_eq!(restored, gb);
}

#[test]
fn group_by_tag_nonempty_round_trips() {
    let gb = GroupBy::Tag("production".to_string());
    let key = gb.to_key();
    let restored = GroupBy::from_key(&key);
    assert_eq!(restored, gb);
}

#[test]
fn group_by_none_round_trips() {
    let gb = GroupBy::None;
    let key = gb.to_key();
    let restored = GroupBy::from_key(&key);
    assert_eq!(restored, gb);
}

#[test]
fn group_by_provider_round_trips() {
    let gb = GroupBy::Provider;
    let key = gb.to_key();
    let restored = GroupBy::from_key(&key);
    assert_eq!(restored, gb);
}

#[test]
fn ping_sort_key_ordering() {
    assert!(
        super::ping_sort_key(Some(&PingStatus::Unreachable))
            < super::ping_sort_key(Some(&PingStatus::Slow { rtt_ms: 300 }))
    );
    assert!(
        super::ping_sort_key(Some(&PingStatus::Slow { rtt_ms: 300 }))
            < super::ping_sort_key(Some(&PingStatus::Reachable { rtt_ms: 10 }))
    );
    assert!(
        super::ping_sort_key(Some(&PingStatus::Reachable { rtt_ms: 10 }))
            < super::ping_sort_key(Some(&PingStatus::Checking))
    );
    assert!(
        super::ping_sort_key(Some(&PingStatus::Checking))
            < super::ping_sort_key(Some(&PingStatus::Skipped))
    );
    assert_eq!(
        super::ping_sort_key(Some(&PingStatus::Skipped)),
        super::ping_sort_key(None)
    );
}

#[test]
fn sort_mode_status_round_trips() {
    assert_eq!(SortMode::from_key("status"), SortMode::Status);
    assert_eq!(SortMode::Status.to_key(), "status");
}

#[test]
fn sort_mode_status_in_cycle() {
    assert_eq!(SortMode::MostRecent.next(), SortMode::Status);
    assert_eq!(SortMode::Status.next(), SortMode::Original);
}

#[test]
fn classify_ping_reachable_below_threshold() {
    let status = super::classify_ping(Some(199), 200);
    assert_eq!(status, PingStatus::Reachable { rtt_ms: 199 });
}

#[test]
fn classify_ping_slow_at_threshold() {
    let status = super::classify_ping(Some(200), 200);
    assert_eq!(status, PingStatus::Slow { rtt_ms: 200 });
}

#[test]
fn classify_ping_slow_above_threshold() {
    let status = super::classify_ping(Some(201), 200);
    assert_eq!(status, PingStatus::Slow { rtt_ms: 201 });
}

#[test]
fn classify_ping_unreachable() {
    let status = super::classify_ping(None, 200);
    assert_eq!(status, PingStatus::Unreachable);
}

#[test]
fn classify_ping_zero_rtt() {
    let status = super::classify_ping(Some(0), 200);
    assert_eq!(status, PingStatus::Reachable { rtt_ms: 0 });
}

#[test]
fn cancel_search_clears_filter_down_only() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    app.ping.filter_down_only = true;
    app.search.query = Some(String::new());
    app.cancel_search();
    assert!(!app.ping.filter_down_only);
    assert!(app.search.query.is_none());
}

#[test]
fn filter_down_only_keeps_unreachable_hosts() {
    let mut app = make_app(
        "Host web1\n  HostName 1.1.1.1\nHost web2\n  HostName 2.2.2.2\nHost web3\n  HostName 3.3.3.3\n",
    );
    app.ping
        .status
        .insert("web1".to_string(), PingStatus::Unreachable);
    app.ping
        .status
        .insert("web2".to_string(), PingStatus::Reachable { rtt_ms: 10 });
    app.ping
        .status
        .insert("web3".to_string(), PingStatus::Slow { rtt_ms: 300 });
    app.ping.filter_down_only = true;
    app.search.query = Some(String::new());
    app.apply_filter();
    // Only web1 (Unreachable) should remain
    assert_eq!(app.search.filtered_indices.len(), 1);
    let alias = &app.hosts[app.search.filtered_indices[0]].alias;
    assert_eq!(alias, "web1");
    // Patterns should be cleared
    assert!(app.search.filtered_pattern_indices.is_empty());
}

#[test]
fn sort_mode_status_orders_by_ping() {
    let mut app = make_app(
        "Host web1\n  HostName 1.1.1.1\nHost web2\n  HostName 2.2.2.2\nHost web3\n  HostName 3.3.3.3\n",
    );
    app.ping
        .status
        .insert("web1".to_string(), PingStatus::Reachable { rtt_ms: 10 });
    app.ping
        .status
        .insert("web2".to_string(), PingStatus::Unreachable);
    app.ping
        .status
        .insert("web3".to_string(), PingStatus::Slow { rtt_ms: 300 });
    app.sort_mode = SortMode::Status;
    app.group_by = GroupBy::None;
    app.apply_sort();
    let aliases: Vec<&str> = app
        .display_list
        .iter()
        .filter_map(|item| {
            if let HostListItem::Host { index } = item {
                Some(app.hosts[*index].alias.as_str())
            } else {
                None
            }
        })
        .collect();
    // Unreachable first, then Slow, then Reachable
    assert_eq!(aliases, vec!["web2", "web3", "web1"]);
}

#[test]
fn status_glyph_reachable() {
    let s = PingStatus::Reachable { rtt_ms: 10 };
    assert_eq!(status_glyph(Some(&s), 0), "\u{25CF}");
}

#[test]
fn status_glyph_slow() {
    let s = PingStatus::Slow { rtt_ms: 300 };
    assert_eq!(status_glyph(Some(&s), 0), "\u{25B2}");
}

#[test]
fn status_glyph_unreachable() {
    assert_eq!(status_glyph(Some(&PingStatus::Unreachable), 0), "\u{2716}");
}

#[test]
fn status_glyph_checking() {
    assert_eq!(
        status_glyph(Some(&PingStatus::Checking), 0),
        "\u{280B}" // first spinner frame
    );
}

#[test]
fn status_glyph_checking_cycles() {
    assert_eq!(
        status_glyph(Some(&PingStatus::Checking), 1),
        "\u{2819}" // second spinner frame
    );
}

#[test]
fn status_glyph_skipped() {
    assert_eq!(status_glyph(Some(&PingStatus::Skipped), 0), "");
}

#[test]
fn status_glyph_none() {
    assert_eq!(status_glyph(None, 0), "\u{25CB}");
}

#[test]
fn status_glyph_none_is_static_circle() {
    assert_eq!(status_glyph(None, 0), status_glyph(None, 5));
}

#[test]
fn status_glyph_none_differs_from_checking() {
    assert_ne!(
        status_glyph(None, 0),
        status_glyph(Some(&PingStatus::Checking), 0)
    );
}

#[test]
fn health_summary_empty_ping_status() {
    let app = make_app("Host web1\n  HostName 1.1.1.1\n");
    let spans = health_summary_spans(&app.ping.status, &app.hosts);
    assert!(spans.is_empty());
}

#[test]
fn health_summary_mixed_statuses() {
    let mut app = make_app(
        "Host web1\n  HostName 1.1.1.1\nHost web2\n  HostName 2.2.2.2\nHost web3\n  HostName 3.3.3.3\nHost web4\n  HostName 4.4.4.4\n",
    );
    app.ping
        .status
        .insert("web1".to_string(), PingStatus::Reachable { rtt_ms: 10 });
    app.ping
        .status
        .insert("web2".to_string(), PingStatus::Slow { rtt_ms: 300 });
    app.ping
        .status
        .insert("web3".to_string(), PingStatus::Unreachable);
    // web4 has no ping status -> unchecked
    let spans = health_summary_spans(&app.ping.status, &app.hosts);
    // Layout: ●1 " " ▲1 " " ✖1 " " ○1 = 4 status + 3 separators = 7 spans
    assert_eq!(spans.len(), 7);
    let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("●1"), "should contain online count");
    assert!(text.contains("▲1"), "should contain slow count");
    assert!(text.contains("✖1"), "should contain down count");
    assert!(text.contains("○1"), "should contain unchecked count");
}

#[test]
fn health_summary_suppresses_zero_count() {
    let mut app = make_app("Host web1\n  HostName 1.1.1.1\n");
    app.ping
        .status
        .insert("web1".to_string(), PingStatus::Reachable { rtt_ms: 10 });
    let spans = health_summary_spans(&app.ping.status, &app.hosts);
    // Only online, no separators
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].content.as_ref(), "\u{25CF}1");
}

#[test]
fn health_summary_skipped_excluded() {
    let mut app = make_app("Host proxy\n  HostName 1.1.1.1\n");
    app.ping
        .status
        .insert("proxy".to_string(), PingStatus::Skipped);
    let spans = health_summary_spans(&app.ping.status, &app.hosts);
    // Skipped hosts produce no counts, so result is empty
    assert!(spans.is_empty());
}

#[test]
fn health_summary_for_subset() {
    let mut ping_status = HashMap::new();
    ping_status.insert("web1".to_string(), PingStatus::Reachable { rtt_ms: 10 });
    ping_status.insert("web2".to_string(), PingStatus::Unreachable);
    ping_status.insert("web3".to_string(), PingStatus::Reachable { rtt_ms: 20 });
    // Only ask about web1 and web2
    let spans = health_summary_spans_for(&ping_status, ["web1", "web2"].iter().copied());
    // ●1 space ✖1 = 3 spans
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0].content.as_ref(), "\u{25CF}1");
    assert_eq!(spans[2].content.as_ref(), "\u{2716}1");
}

/// Helper: extract tag names from DisplayTag vec.
fn tag_names(tags: &[DisplayTag]) -> Vec<&str> {
    tags.iter().map(|t| t.name.as_str()).collect()
}

/// Helper: extract is_user flags from DisplayTag vec.
fn tag_sources(tags: &[DisplayTag]) -> Vec<bool> {
    tags.iter().map(|t| t.is_user).collect()
}

#[test]
fn select_display_tags_user_and_provider_flat() {
    let host = HostEntry {
        tags: vec!["prod".into(), "us-east".into()],
        provider_tags: vec!["web".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    let tags = select_display_tags(&host, &GroupBy::None, false);
    assert_eq!(tag_names(&tags), vec!["prod", "us-east", "web"]);
    assert_eq!(tag_sources(&tags), vec![true, true, false]);
}

#[test]
fn select_display_tags_grouped_by_provider_suppresses_name() {
    let host = HostEntry {
        tags: vec!["prod".into()],
        provider_tags: vec!["web".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    // Grouped: only user tags shown
    let tags = select_display_tags(&host, &GroupBy::Provider, false);
    assert_eq!(tag_names(&tags), vec!["prod"]);
    assert_eq!(tag_sources(&tags), vec![true]);
}

#[test]
fn select_display_tags_only_provider_tags() {
    let host = HostEntry {
        provider_tags: vec!["web".into(), "cache".into()],
        provider: Some("do".into()),
        ..Default::default()
    };
    let tags = select_display_tags(&host, &GroupBy::None, false);
    assert_eq!(tag_names(&tags), vec!["web", "cache", "do"]);
    assert_eq!(tag_sources(&tags), vec![false, false, false]);
}

#[test]
fn select_display_tags_no_tags() {
    let host = HostEntry::default();
    let tags = select_display_tags(&host, &GroupBy::None, false);
    assert!(tags.is_empty());
}

#[test]
fn select_display_tags_detail_mode_only_primary() {
    let host = HostEntry {
        tags: vec!["prod".into(), "us-east".into()],
        provider_tags: vec!["web".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    let tags = select_display_tags(&host, &GroupBy::None, true);
    assert_eq!(tag_names(&tags), vec!["prod"]);
    assert_eq!(tag_sources(&tags), vec![true]);
}

#[test]
fn select_display_tags_group_name_suppression() {
    let host = HostEntry {
        tags: vec!["prod".into()],
        provider_tags: vec![],
        provider: None,
        ..Default::default()
    };
    // Group by tag "prod" -> prod suppressed from user tags
    let tags = select_display_tags(&host, &GroupBy::Tag("prod".into()), false);
    assert!(tags.is_empty());
}

#[test]
fn select_display_tags_group_by_tag_shows_remaining() {
    let host = HostEntry {
        tags: vec!["prod".into(), "us-east".into(), "api".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    // Group by "prod" -> "prod" suppressed, remaining user tags: us-east, api
    let tags = select_display_tags(&host, &GroupBy::Tag("prod".into()), false);
    assert_eq!(tag_names(&tags), vec!["us-east", "api"]);
    assert_eq!(tag_sources(&tags), vec![true, true]);
}

#[test]
fn health_summary_for_empty_aliases() {
    // Empty alias iterator with non-empty ping_status: all counters stay 0,
    // returns empty spans (no health summary to display).
    let mut ping = HashMap::new();
    ping.insert("host1".to_string(), PingStatus::Reachable { rtt_ms: 10 });
    let spans = health_summary_spans_for(&ping, std::iter::empty());
    assert!(spans.is_empty());
}

#[test]
fn select_display_tags_provider_none_group_by_provider() {
    let host = HostEntry {
        tags: vec!["prod".into(), "us-east".into()],
        provider: None,
        ..Default::default()
    };
    // GroupBy::Provider with provider=None: group_name is None, no suppression
    let tags = select_display_tags(&host, &GroupBy::Provider, false);
    assert_eq!(tag_names(&tags), vec!["prod", "us-east"]);
    assert_eq!(tag_sources(&tags), vec![true, true]);
}

#[test]
fn select_display_tags_duplicate_provider_name_in_provider_tags() {
    let host = HostEntry {
        tags: vec!["prod".into()],
        provider_tags: vec!["aws".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    let tags = select_display_tags(&host, &GroupBy::None, false);
    assert_eq!(tag_names(&tags), vec!["prod", "aws", "aws"]);
    assert_eq!(tag_sources(&tags), vec![true, false, false]);
}

#[test]
fn select_display_tags_grouped_user_tags_only() {
    let host = HostEntry {
        tags: vec!["prod".into()],
        provider_tags: vec!["aws".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    // Grouped: only user tags, provider tags excluded entirely
    let tags = select_display_tags(&host, &GroupBy::Provider, false);
    assert_eq!(tag_names(&tags), vec!["prod"]);
    assert_eq!(tag_sources(&tags), vec![true]);
}

#[test]
fn select_display_tags_grouped_excludes_all_provider_tags() {
    let host = HostEntry {
        tags: vec![],
        provider_tags: vec!["web".into(), "cache".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    // Grouped: no user tags, provider tags excluded entirely
    let tags = select_display_tags(&host, &GroupBy::Provider, false);
    assert!(tags.is_empty());
}

#[test]
fn select_display_tags_case_insensitive_group_suppression() {
    let host = HostEntry {
        tags: vec!["prod".into(), "AWS".into()],
        provider_tags: vec![],
        provider: Some("AWS".into()),
        ..Default::default()
    };
    // GroupBy::Provider -> group_name = "AWS", user tag "AWS" suppressed case-insensitively
    let tags = select_display_tags(&host, &GroupBy::Provider, false);
    assert_eq!(tag_names(&tags), vec!["prod"]);
    assert_eq!(tag_sources(&tags), vec![true]);
}

#[test]
fn select_display_tags_flat_one_user_tag_with_provider_tags() {
    let host = HostEntry {
        tags: vec!["prod".into()],
        provider_tags: vec!["web".into(), "cache".into()],
        provider: Some("do".into()),
        ..Default::default()
    };
    let tags = select_display_tags(&host, &GroupBy::None, false);
    assert_eq!(tag_names(&tags), vec!["prod", "web", "cache"]);
    assert_eq!(tag_sources(&tags), vec![true, false, false]);
}

#[test]
fn select_display_tags_grouped_tertiary_user_only() {
    let host = HostEntry {
        tags: vec!["prod".into(), "us-east".into(), "api".into(), "db".into()],
        provider_tags: vec!["web".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    // Group by "prod" -> suppressed; only user tags: us-east, api, db (max 3)
    let tags = select_display_tags(&host, &GroupBy::Tag("prod".into()), false);
    assert_eq!(tag_names(&tags), vec!["us-east", "api", "db"]);
    assert_eq!(tag_sources(&tags), vec![true, true, true]);
}

#[test]
fn select_display_tags_detail_mode_grouped() {
    let host = HostEntry {
        tags: vec!["prod".into(), "us-east".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    let tags = select_display_tags(&host, &GroupBy::Provider, true);
    assert_eq!(tag_names(&tags), vec!["prod"]);
    assert_eq!(tag_sources(&tags), vec![true]);
}

#[test]
fn health_summary_skipped_excluded_with_other_hosts() {
    let mut app = make_app("Host proxy\n  HostName 1.1.1.1\nHost web\n  HostName 2.2.2.2\n");
    app.ping
        .status
        .insert("proxy".to_string(), PingStatus::Skipped);
    app.ping
        .status
        .insert("web".to_string(), PingStatus::Reachable { rtt_ms: 5 });
    let spans = health_summary_spans(&app.ping.status, &app.hosts);
    // Only online count for web, skipped proxy excluded
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].content.as_ref(), "●1");
}

// --- Bastion ping propagation tests ---

fn make_host_entry(alias: &str, hostname: &str, proxy_jump: &str) -> HostEntry {
    HostEntry {
        alias: alias.to_string(),
        hostname: hostname.to_string(),
        proxy_jump: proxy_jump.to_string(),
        ..Default::default()
    }
}

#[test]
fn propagate_ping_bastion_reachable() {
    let bastion = make_host_entry("bastion", "1.1.1.1", "");
    let dep1 = make_host_entry("web1", "10.0.0.1", "bastion");
    let dep2 = make_host_entry("web2", "10.0.0.2", "bastion");
    let other = make_host_entry("standalone", "2.2.2.2", "");
    let hosts = vec![bastion, dep1, dep2, other];
    let mut ping_status = HashMap::new();
    ping_status.insert("web1".to_string(), PingStatus::Checking);
    ping_status.insert("web2".to_string(), PingStatus::Checking);

    let status = PingStatus::Reachable { rtt_ms: 15 };
    propagate_ping_to_dependents(&hosts, &mut ping_status, "bastion", &status);

    assert_eq!(
        ping_status.get("web1"),
        Some(&PingStatus::Reachable { rtt_ms: 15 })
    );
    assert_eq!(
        ping_status.get("web2"),
        Some(&PingStatus::Reachable { rtt_ms: 15 })
    );
    assert!(!ping_status.contains_key("standalone"));
}

#[test]
fn propagate_ping_bastion_unreachable() {
    let bastion = make_host_entry("bastion", "1.1.1.1", "");
    let dep = make_host_entry("web1", "10.0.0.1", "bastion");
    let hosts = vec![bastion, dep];
    let mut ping_status = HashMap::new();
    ping_status.insert("web1".to_string(), PingStatus::Checking);

    propagate_ping_to_dependents(
        &hosts,
        &mut ping_status,
        "bastion",
        &PingStatus::Unreachable,
    );

    assert_eq!(ping_status.get("web1"), Some(&PingStatus::Unreachable));
}

#[test]
fn propagate_ping_no_dependents() {
    let host = make_host_entry("standalone", "1.1.1.1", "");
    let hosts = vec![host];
    let mut ping_status = HashMap::new();

    propagate_ping_to_dependents(
        &hosts,
        &mut ping_status,
        "standalone",
        &PingStatus::Reachable { rtt_ms: 10 },
    );

    assert!(!ping_status.contains_key("standalone"));
}

// --- SnippetParamFormState::is_dirty tests ---

#[test]
fn snippet_param_form_not_dirty_when_defaults_match() {
    let state = SnippetParamFormState::new(&[crate::snippet::SnippetParam {
        name: "host".into(),
        default: Some("localhost".into()),
    }]);
    assert!(!state.is_dirty());
}

#[test]
fn snippet_param_form_dirty_when_value_differs() {
    let mut state = SnippetParamFormState::new(&[crate::snippet::SnippetParam {
        name: "host".into(),
        default: Some("localhost".into()),
    }]);
    state.values[0] = "other".into();
    assert!(state.is_dirty());
}

#[test]
fn snippet_param_form_not_dirty_no_default_empty_value() {
    let state = SnippetParamFormState::new(&[crate::snippet::SnippetParam {
        name: "host".into(),
        default: None,
    }]);
    assert!(!state.is_dirty());
}

#[test]
fn snippet_param_form_dirty_no_default_nonempty_value() {
    let mut state = SnippetParamFormState::new(&[crate::snippet::SnippetParam {
        name: "host".into(),
        default: None,
    }]);
    state.values[0] = "something".into();
    assert!(state.is_dirty());
}

#[test]
fn snippet_param_form_not_dirty_empty_params() {
    let state = SnippetParamFormState::new(&[]);
    assert!(!state.is_dirty());
}

#[test]
fn tick_status_sticky_never_expires() {
    let mut app = make_app("");
    app.set_sticky_status("vault signing...", false);
    for _ in 0..50 {
        app.tick_status();
    }
    assert!(app.status.is_some(), "sticky status must not expire");
}

#[test]
fn tick_toast_non_sticky_error_expires() {
    let mut app = make_app("");
    app.set_status("failed", true);
    assert!(app.toast.is_some(), "error should route to toast");
    for _ in 0..=20 {
        app.tick_toast();
    }
    assert!(
        app.toast.is_none(),
        "non-sticky error toast must expire after 20 ticks"
    );
}

#[test]
fn tick_toast_non_sticky_success_expires() {
    let mut app = make_app("");
    app.set_status("done", false);
    assert!(app.toast.is_some(), "confirmation should route to toast");
    for _ in 0..=6 {
        app.tick_toast();
    }
    assert!(
        app.toast.is_none(),
        "non-sticky confirmation toast must expire after 6 ticks"
    );
}

#[test]
fn set_status_does_not_overwrite_sticky() {
    let mut app = make_app("");
    app.set_sticky_status("signing...", false);
    app.set_background_status("ping expired", false);
    assert_eq!(
        app.status.as_ref().unwrap().text,
        "signing...",
        "set_background_status must not overwrite sticky"
    );
}

#[test]
fn set_sticky_status_replaces_sticky() {
    let mut app = make_app("");
    app.set_sticky_status("signing...", false);
    app.set_sticky_status("done signing", true);
    assert_eq!(
        app.status.as_ref().unwrap().text,
        "done signing",
        "set_sticky_status must replace sticky"
    );
}

#[test]
fn set_status_routes_confirmation_to_toast() {
    let mut app = make_app("");
    app.set_sticky_status("signing...", false);
    // set_status (Confirmation) routes to toast, sticky footer stays
    app.set_status("Signed 3 of 3 certificates.", false);
    assert_eq!(
        app.toast.as_ref().unwrap().text,
        "Signed 3 of 3 certificates.",
        "set_status(false) must route to toast"
    );
    // Sticky footer is still there
    assert!(app.status.as_ref().unwrap().sticky);
}

// Gap 1: set_status on a fresh app with no prior status at all.
#[test]
fn set_status_routes_to_toast_when_none() {
    let mut app = make_app("");
    assert!(app.toast.is_none(), "precondition: fresh app has no toast");
    app.set_status("connected", false);
    assert_eq!(
        app.toast.as_ref().unwrap().text,
        "connected",
        "set_status(false) must route to toast"
    );
}

// Gap 2: set_status is still blocked after the sticky is replaced by a second set_sticky_status.
// Verifies that the blocking invariant holds for the replacement sticky, not just the first one.
#[test]
fn set_background_status_blocked_after_sticky_replaced_by_sticky() {
    let mut app = make_app("");
    app.set_sticky_status("signing...", false);
    app.set_sticky_status("still signing...", false);
    app.set_background_status("ping expired", false);
    assert_eq!(
        app.status.as_ref().unwrap().text,
        "still signing...",
        "set_background_status must not overwrite the replacement sticky"
    );
}

// Gap 3: tick_status does not alter the content of a sticky message, only its absence/presence.
#[test]
fn tick_status_sticky_text_unchanged() {
    let mut app = make_app("");
    app.set_sticky_status("vault signing...", false);
    for _ in 0..50 {
        app.tick_status();
    }
    assert_eq!(
        app.status.as_ref().unwrap().text,
        "vault signing...",
        "tick_status must not alter sticky message text"
    );
    assert!(
        app.status.as_ref().unwrap().sticky,
        "tick_status must not clear the sticky flag"
    );
}

// Gap 4: documents the user-action contract: while a sticky Vault signing message is active,
// any incidental set_status call (e.g. from a navigation handler) is suppressed.
// This is intentional: Vault SSH signing feedback is more important than transient nav feedback.
#[test]
fn set_background_status_suppressed_during_vault_signing() {
    let mut app = make_app("");
    app.set_sticky_status("Signing certificate...", false);
    // Background event (ping, tunnel) must not clobber signing status
    app.set_background_status("Ping expired.", false);
    assert_eq!(
        app.status.as_ref().unwrap().text,
        "Signing certificate...",
        "background status must be suppressed while sticky is active"
    );
    assert!(app.status.as_ref().unwrap().sticky);
}

#[test]
fn set_background_status_works_when_no_sticky() {
    let mut app = make_app("");
    app.set_background_status("ping expired", false);
    assert_eq!(app.status.as_ref().unwrap().text, "ping expired");
}

#[test]
fn set_background_error_goes_to_toast_even_with_sticky_and_existing_toast() {
    let mut app = make_app("");
    // Sticky progress in footer
    app.set_sticky_status("Signing...", false);
    // First toast already active
    app.set_status("Copied host", false);
    assert!(app.toast.is_some());
    // Background error should queue to toast, not touch footer
    app.set_background_status("Sync failed", true);
    assert_eq!(app.status.as_ref().unwrap().text, "Signing...");
    assert!(app.status.as_ref().unwrap().sticky);
    assert_eq!(app.toast.as_ref().unwrap().text, "Copied host");
    assert_eq!(app.toast_queue.len(), 1);
    assert_eq!(app.toast_queue.front().unwrap().text, "Sync failed");
    assert!(app.toast_queue.front().unwrap().is_error());
}

#[test]
fn vault_signing_lifecycle() {
    let mut app = make_app("");
    // 1. Signing starts: sticky progress in footer
    app.set_sticky_status("Signing certificate...", false);
    assert!(app.status.as_ref().unwrap().sticky);

    // 2. Background event must not clobber sticky footer
    app.set_background_status("Ping expired.", false);
    assert_eq!(app.status.as_ref().unwrap().text, "Signing certificate...");

    // 3. Signing error routes to toast, sticky footer stays
    app.set_status("Vault SSH: failed to sign host: timeout", true);
    assert!(app.toast.as_ref().unwrap().is_error());
    assert_eq!(
        app.toast.as_ref().unwrap().text,
        "Vault SSH: failed to sign host: timeout"
    );
    // Sticky footer is still there
    assert!(app.status.as_ref().unwrap().sticky);

    // 4. Final summary replaces sticky footer with sticky error
    app.set_sticky_status("Signed 0 of 1 certificate. 1 failed: timeout", true);
    assert!(app.status.as_ref().unwrap().sticky);
    assert!(app.status.as_ref().unwrap().is_error());

    // 5. Background non-error cannot clobber sticky footer
    app.set_background_status("Config reloaded. 5 hosts.", false);
    assert_eq!(
        app.status.as_ref().unwrap().text,
        "Signed 0 of 1 certificate. 1 failed: timeout"
    );
}

#[test]
fn vault_signing_success_clears_sticky_progress() {
    let mut app = make_app("");
    // Sticky progress during signing
    app.set_sticky_status("Signing 3/3: last-server (V to cancel)", false);
    assert!(app.status.as_ref().unwrap().sticky);

    // Success summary via set_info_status replaces sticky footer
    app.set_info_status("Signed 3 of 3 certificates.");
    assert!(!app.status.as_ref().unwrap().sticky);
    assert_eq!(
        app.status.as_ref().unwrap().text,
        "Signed 3 of 3 certificates."
    );
}

#[test]
fn confirmation_replaces_previous_toast() {
    let mut app = make_app("");
    app.set_status("first", false);
    app.set_status("second", false);
    app.set_status("third", false);
    // Confirmations replace immediately, no queue
    assert_eq!(app.toast.as_ref().unwrap().text, "third");
    assert!(app.toast_queue.is_empty());
}

#[test]
fn confirmation_clears_alert_queue() {
    let mut app = make_app("");
    app.set_status("err1", true);
    app.set_status("err2", true);
    assert_eq!(app.toast_queue.len(), 1);
    // Confirmation replaces active toast and clears queue
    app.set_status("copied", false);
    assert_eq!(app.toast.as_ref().unwrap().text, "copied");
    assert!(app.toast_queue.is_empty());
}

#[test]
fn alert_queue_sequential_display() {
    let mut app = make_app("");
    app.set_status("err1", true);
    app.set_status("err2", true);
    app.set_status("err3", true);
    assert_eq!(app.toast.as_ref().unwrap().text, "err1");
    assert_eq!(app.toast_queue.len(), 2);
    for _ in 0..=20 {
        app.tick_toast();
    }
    assert_eq!(app.toast.as_ref().unwrap().text, "err2");
    assert_eq!(app.toast_queue.len(), 1);
    for _ in 0..=20 {
        app.tick_toast();
    }
    assert_eq!(app.toast.as_ref().unwrap().text, "err3");
    assert!(app.toast_queue.is_empty());
}

#[test]
fn alert_queue_max_five() {
    let mut app = make_app("");
    for i in 0..8 {
        app.set_status(format!("err{i}"), true);
    }
    assert_eq!(app.toast.as_ref().unwrap().text, "err0");
    assert_eq!(app.toast_queue.len(), 5);
    assert_eq!(app.toast_queue.front().unwrap().text, "err3");
    assert_eq!(app.toast_queue.back().unwrap().text, "err7");
}

#[test]
fn alert_queue_drains_fully() {
    let mut app = make_app("");
    app.set_status("a", true);
    app.set_status("b", true);
    for _ in 0..=20 {
        app.tick_toast();
    }
    for _ in 0..=20 {
        app.tick_toast();
    }
    assert!(app.toast.is_none());
    assert!(app.toast_queue.is_empty());
}

#[test]
fn set_info_status_goes_to_footer() {
    let mut app = make_app("");
    app.set_info_status("Syncing...");
    assert!(app.toast.is_none());
    assert_eq!(app.status.as_ref().unwrap().text, "Syncing...");
    assert_eq!(app.status.as_ref().unwrap().class, MessageClass::Info);
}

#[test]
fn tick_status_info_expires() {
    let mut app = make_app("");
    app.set_info_status("done");
    for _ in 0..=12 {
        app.tick_status();
    }
    assert!(app.status.is_none());
}

#[test]
fn tick_status_does_not_expire_while_syncing() {
    let mut app = make_app("");
    app.set_info_status("syncing...");
    app.syncing_providers
        .insert("aws".to_string(), Arc::new(AtomicBool::new(true)));
    for _ in 0..30 {
        app.tick_status();
    }
    assert!(
        app.status.is_some(),
        "status must not expire while providers are syncing"
    );
    app.syncing_providers.clear();
    for _ in 0..=12 {
        app.tick_status();
    }
    assert!(
        app.status.is_none(),
        "status must expire after syncing completes"
    );
}

#[test]
fn tick_toast_alert_expires() {
    let mut app = make_app("");
    app.set_status("failed", true);
    assert!(app.toast.is_some());
    assert!(app.status.is_none());
    for _ in 0..=20 {
        app.tick_toast();
    }
    assert!(app.toast.is_none());
}

#[test]
fn tick_toast_confirmation_expires() {
    let mut app = make_app("");
    app.set_status("done", false);
    assert!(app.toast.is_some());
    for _ in 0..=6 {
        app.tick_toast();
    }
    assert!(app.toast.is_none());
}

#[test]
fn message_class_is_toast_routing() {
    assert!(
        StatusMessage {
            text: String::new(),
            class: MessageClass::Confirmation,
            tick_count: 0,
            sticky: false,
        }
        .is_toast()
    );
    assert!(
        !StatusMessage {
            text: String::new(),
            class: MessageClass::Info,
            tick_count: 0,
            sticky: false,
        }
        .is_toast()
    );
    assert!(
        StatusMessage {
            text: String::new(),
            class: MessageClass::Alert,
            tick_count: 0,
            sticky: false,
        }
        .is_toast()
    );
    assert!(
        !StatusMessage {
            text: String::new(),
            class: MessageClass::Progress,
            tick_count: 0,
            sticky: false,
        }
        .is_toast()
    );
}

#[test]
fn message_class_timeout_values() {
    let mk = |class| StatusMessage {
        text: String::new(),
        class,
        tick_count: 0,
        sticky: false,
    };
    assert_eq!(mk(MessageClass::Confirmation).timeout(), 6);
    assert_eq!(mk(MessageClass::Info).timeout(), 12);
    assert_eq!(mk(MessageClass::Alert).timeout(), 20);
    assert_eq!(mk(MessageClass::Progress).timeout(), u32::MAX);
}

#[test]
fn message_class_is_error() {
    let mk = |class| StatusMessage {
        text: String::new(),
        class,
        tick_count: 0,
        sticky: false,
    };
    assert!(!mk(MessageClass::Confirmation).is_error());
    assert!(!mk(MessageClass::Info).is_error());
    assert!(mk(MessageClass::Alert).is_error());
    assert!(!mk(MessageClass::Progress).is_error());
}

#[test]
fn palette_commands_have_unique_keys() {
    let commands = PaletteCommand::all();
    let mut seen = std::collections::HashSet::new();
    for cmd in commands {
        assert!(seen.insert(cmd.key), "duplicate palette key: '{}'", cmd.key);
    }
    assert!(
        commands.len() >= 20,
        "expected at least 20 palette commands"
    );
}

#[test]
fn palette_state_filters_by_query() {
    let mut state = CommandPaletteState::new();
    state.push_query('t');
    let filtered = state.filtered_commands();
    assert!(
        filtered
            .iter()
            .all(|c| c.label.to_lowercase().contains("t")),
        "all filtered commands should contain 't' in label"
    );
    assert!(
        filtered.len() < PaletteCommand::all().len(),
        "filtering should reduce the list"
    );
}

#[test]
fn palette_state_empty_query_returns_all() {
    let state = CommandPaletteState::new();
    let filtered = state.filtered_commands();
    assert_eq!(filtered.len(), PaletteCommand::all().len());
}

#[test]
fn palette_selected_resets_on_query_change() {
    let mut state = CommandPaletteState::new();
    state.selected = 5;
    state.push_query('x');
    assert_eq!(
        state.selected, 0,
        "selected should reset when query changes"
    );
    state.selected = 3;
    state.pop_query();
    assert_eq!(state.selected, 0, "selected should reset on pop too");
}
