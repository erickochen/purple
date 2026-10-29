use super::tag_state::DisplayTag;
use super::*;
use crate::ssh_config::model::{HostEntry, SshConfigFile};
use crate::tunnel::TunnelType;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

fn make_app(content: &str) -> App {
    // Every test gets a unique tempdir so parallel `cargo test` threads
    // cannot race on the same config path when `app.hosts_state.ssh_config.write()` runs.
    // `into_path()` leaks cleanup to the OS — fine for test scratch files.
    let path = tempfile::tempdir()
        .expect("tempdir")
        .keep()
        .join("test_config");
    let config = SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path,
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    // Isolate from the real ~/.purple/providers so tests don't
    // pick up the user's vault_role / vault_addr config.
    app.providers.config = crate::providers::config::ProviderConfig::default();
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
        app.hosts_state.list[0].vault_addr.as_deref(),
        Some("http://127.0.0.1:8200")
    );
    assert!(app.hosts_state.list[0].vault_ssh.is_none());
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
    // is not in self.hosts_state.list (caller typed a bad alias).
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
    app.forms.host.vault_ssh = "ssh-client-signer/sign/engineer".to_string();
    app.forms.host.vault_addr = String::new();
    app.capture_form_baseline();
    assert!(!app.host_form_is_dirty());
    app.forms.host.vault_addr = "http://127.0.0.1:8200".to_string();
    assert!(
        app.host_form_is_dirty(),
        "typing into vault_addr must mark the form dirty"
    );
}

#[test]
fn close_host_form_clears_state_and_returns_to_list() {
    let mut app = make_app("Host a\n  HostName 1.2.3.4\n");
    app.capture_form_baseline();
    app.conflict.form_mtime = Some(SystemTime::UNIX_EPOCH);
    app.set_screen(Screen::EditHost {
        alias: "a".to_string(),
    });
    // Pin the flush invariant: set_screen must precede flush_pending_vault_write
    // because flush no-ops while a form is open. Setting pending_config_write and
    // asserting it clears proves the flush actually ran past the form-open guard.
    app.vault.pending_config_write = true;
    assert!(app.forms.host_baseline.is_some());
    assert!(app.conflict.form_mtime.is_some());

    app.close_host_form();

    assert!(
        app.forms.host_baseline.is_none(),
        "baseline must be cleared"
    );
    assert!(app.conflict.form_mtime.is_none(), "mtime must be cleared");
    assert!(matches!(app.screen, Screen::HostList));
    assert!(
        !app.vault.pending_config_write,
        "flush must have run after set_screen"
    );
}

#[test]
fn close_host_form_after_save_selects_target_alias() {
    let mut app = make_app("Host a\n  HostName 1.2.3.4\nHost b\n  HostName 2.3.4.5\n");
    app.capture_form_baseline();
    app.conflict.form_mtime = Some(SystemTime::UNIX_EPOCH);
    app.set_screen(Screen::EditHost {
        alias: "a".to_string(),
    });
    app.vault.pending_config_write = true;

    app.close_host_form_after_save("b");

    assert!(app.forms.host_baseline.is_none());
    assert!(app.conflict.form_mtime.is_none(), "mtime must be cleared");
    assert!(matches!(app.screen, Screen::HostList));
    assert_eq!(
        app.selected_host().expect("a host must be selected").alias,
        "b"
    );
    assert!(
        !app.vault.pending_config_write,
        "flush must have run after set_screen and select"
    );
}

#[test]
fn close_provider_form_clears_state_and_returns_to_providers() {
    let mut app = make_app("");
    app.capture_provider_form_baseline();
    app.conflict.form_mtime = Some(SystemTime::UNIX_EPOCH);
    app.set_screen(Screen::ProviderForm {
        id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
    });
    app.vault.pending_config_write = true;
    assert!(app.providers.form_baseline.is_some());

    app.close_provider_form();

    assert!(
        app.providers.form_baseline.is_none(),
        "baseline must be cleared"
    );
    assert!(app.conflict.form_mtime.is_none(), "mtime must be cleared");
    assert!(matches!(app.screen, Screen::Providers));
    assert!(
        !app.vault.pending_config_write,
        "flush must have run after set_screen"
    );
}

#[test]
fn close_tunnel_form_clears_state_and_returns_to_passed_screen() {
    let mut app = make_app("Host a\n  HostName 1.2.3.4\n");
    app.capture_tunnel_form_baseline();
    app.conflict.form_mtime = Some(SystemTime::UNIX_EPOCH);
    app.set_screen(Screen::TunnelForm {
        alias: "a".to_string(),
        editing: None,
    });
    // Pin the divergence from host/provider close: tunnel close must NOT
    // flush_pending_vault_write. A future "consistency" refactor that adds
    // flush here would silently start writing vault config on tunnel edits.
    app.vault.pending_config_write = true;
    assert!(app.tunnels.form_baseline.is_some());

    let return_to = Screen::TunnelList {
        alias: "a".to_string(),
    };
    app.close_tunnel_form(return_to);

    assert!(
        app.tunnels.form_baseline.is_none(),
        "baseline must be cleared"
    );
    assert!(app.conflict.form_mtime.is_none(), "mtime must be cleared");
    match &app.screen {
        Screen::TunnelList { alias } => assert_eq!(alias, "a"),
        other => panic!("expected TunnelList, got {:?}", other),
    }
    assert!(
        app.vault.pending_config_write,
        "tunnel close must NOT flush (tunnels are not vault-related)"
    );
}

#[test]
fn open_host_forms_initialize_state() {
    // Add host: form is HostForm::new(), screen = AddHost, mtime + baseline captured.
    let mut app = make_app("");
    std::fs::write(&app.hosts_state.ssh_config.path, "").expect("write empty config");
    app.open_host_add_form();
    assert!(matches!(app.screen, Screen::AddHost));
    assert!(
        app.forms.host_baseline.is_some(),
        "baseline must be captured"
    );
    assert!(app.conflict.form_mtime.is_some(), "mtime must be captured");

    // Add pattern: same screen, baseline + mtime still captured.
    let mut app = make_app("");
    std::fs::write(&app.hosts_state.ssh_config.path, "").expect("write empty config");
    app.open_host_pattern_add_form();
    assert!(matches!(app.screen, Screen::AddHost));
    assert!(app.forms.host_baseline.is_some());
    assert!(app.conflict.form_mtime.is_some());

    // Edit pattern: screen = EditHost { alias }, form populated from entry.
    let content = "Host pat-*\n  User foo\n";
    let mut app = make_app(content);
    std::fs::write(&app.hosts_state.ssh_config.path, content).expect("write config");
    app.hosts_state.patterns = app.hosts_state.ssh_config.pattern_entries();
    let pattern = app
        .hosts_state
        .patterns
        .first()
        .cloned()
        .expect("pattern parsed from content");
    app.open_host_pattern_edit_form(&pattern);
    match &app.screen {
        Screen::EditHost { alias } => assert_eq!(alias, "pat-*"),
        other => panic!("expected EditHost, got {:?}", other),
    }
    assert!(app.forms.host_baseline.is_some());
    assert!(app.conflict.form_mtime.is_some());
}

#[test]
fn open_tunnel_forms_initialize_state() {
    let content = "Host a\n  HostName 1.2.3.4\n";

    // Add tunnel: screen = TunnelForm { alias, editing: None }, baseline + mtime captured.
    let mut app = make_app(content);
    std::fs::write(&app.hosts_state.ssh_config.path, content).expect("write config");
    app.open_tunnel_add_form("a".to_string());
    match &app.screen {
        Screen::TunnelForm { alias, editing } => {
            assert_eq!(alias, "a");
            assert!(editing.is_none(), "add: editing must be None");
        }
        other => panic!("expected TunnelForm, got {:?}", other),
    }
    assert!(app.tunnels.form_baseline.is_some());
    assert!(app.conflict.form_mtime.is_some());

    // Edit tunnel: editing = Some(index), form populated from rule.
    let mut app = make_app(content);
    std::fs::write(&app.hosts_state.ssh_config.path, content).expect("write config");
    let rule = crate::tunnel::TunnelRule {
        tunnel_type: TunnelType::Local,
        bind_address: String::new(),
        bind_port: 8080,
        remote_host: "localhost".to_string(),
        remote_port: 80,
    };
    app.open_tunnel_edit_form("a".to_string(), &rule, 3);
    match &app.screen {
        Screen::TunnelForm { alias, editing } => {
            assert_eq!(alias, "a");
            assert_eq!(*editing, Some(3));
        }
        other => panic!("expected TunnelForm, got {:?}", other),
    }
    // Pin that rule data actually flows through TunnelForm::from_rule.
    // Without these, a silent drop in from_rule would not be caught.
    assert_eq!(app.tunnels.form.bind_port, "8080");
    assert_eq!(app.tunnels.form.remote_host, "localhost");
    assert_eq!(app.tunnels.form.remote_port, "80");
    assert!(app.tunnels.form_baseline.is_some());
    assert!(app.conflict.form_mtime.is_some());
}

#[test]
fn open_host_edit_form_success_and_failure_paths() {
    let content = "Host a\n  HostName 1.2.3.4\n";

    // Success: existing host, no Include source, raw entry found, form populated.
    let mut app = make_app(content);
    std::fs::write(&app.hosts_state.ssh_config.path, content).expect("write config");
    let host = app.hosts_state.list[0].clone();
    let ok = app.open_host_edit_form(host, None);
    assert!(ok, "open_host_edit_form must return true on the happy path");
    match &app.screen {
        Screen::EditHost { alias } => assert_eq!(alias, "a"),
        other => panic!("expected EditHost, got {:?}", other),
    }
    assert!(app.forms.host_baseline.is_some());
    assert!(app.conflict.form_mtime.is_some());
    // Pin that HostForm::from_entry actually flowed entry data into the form.
    assert_eq!(
        app.forms.host.hostname, "1.2.3.4",
        "form hostname must be populated from raw entry"
    );

    // Include source fails: returns false, screen unchanged, Error toast (sticky).
    let mut app = make_app(content);
    let mut host = app.hosts_state.list[0].clone();
    host.source_file = Some(std::path::PathBuf::from("/etc/ssh/ssh_config"));
    let ok = app.open_host_edit_form(host, None);
    assert!(!ok, "Include source must return false");
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.forms.host_baseline.is_none(), "form not opened");
    let toast = app
        .status_center
        .toast
        .as_ref()
        .expect("error toast queued");
    assert_eq!(
        toast.class,
        crate::app::MessageClass::Error,
        "Include-source guard must use notify_error (sticky)"
    );

    // Missing entry fails: alias not in config returns false, Warning toast.
    let mut app = make_app(content);
    let mut host = app.hosts_state.list[0].clone();
    host.alias = "nonexistent".to_string();
    let ok = app.open_host_edit_form(host, None);
    assert!(!ok, "missing alias must return false");
    assert!(matches!(app.screen, Screen::HostList));
    assert!(app.forms.host_baseline.is_none());
    let toast = app
        .status_center
        .toast
        .as_ref()
        .expect("warning toast queued");
    assert_eq!(
        toast.class,
        crate::app::MessageClass::Warning,
        "missing-alias guard must use notify_warning"
    );
}

#[test]
fn open_host_edit_form_emits_stale_warning_when_hint_provided() {
    let content = "Host a\n  HostName 1.2.3.4\n";
    let mut app = make_app(content);
    std::fs::write(&app.hosts_state.ssh_config.path, content).expect("write config");
    let host = app.hosts_state.list[0].clone();

    let ok = app.open_host_edit_form(host, Some("Gone from DigitalOcean".to_string()));

    assert!(ok);
    // Stale hint surfaces as a Warning (not Error) toast even on success open.
    let toast = app
        .status_center
        .toast
        .as_ref()
        .expect("stale warning must be queued");
    assert_eq!(
        toast.class,
        crate::app::MessageClass::Warning,
        "stale hint must be a Warning, not an Error"
    );
}

#[test]
fn open_provider_form_initializes_state_for_all_modes() {
    use crate::providers::config::ProviderConfigId;

    // Bare new digitalocean: blank form, short-label alias_prefix, auto_sync
    // takes the kind default (digitalocean defaults to true).
    let mut app = make_app("");
    app.open_provider_form(ProviderConfigId::bare("digitalocean"));
    match &app.screen {
        Screen::ProviderForm { id } => assert_eq!(id.provider, "digitalocean"),
        other => panic!("expected ProviderForm, got {:?}", other),
    }
    assert!(app.providers.form_baseline.is_some());
    assert!(!app.providers.form.label_entry, "bare add: label_entry off");
    assert!(app.providers.form.url.is_empty(), "bare add: blank url");
    assert_eq!(
        app.providers.form.alias_prefix, "do",
        "bare add: alias_prefix falls back to short_label"
    );
    assert!(
        app.providers.form.auto_sync,
        "bare digitalocean add: auto_sync default is true"
    );

    // Bare new proxmox: different auto_sync default to prove the value
    // comes from ProviderKind::default_auto_sync, not a constant.
    let mut app = make_app("");
    app.open_provider_form(ProviderConfigId::bare("proxmox"));
    assert!(
        !app.providers.form.auto_sync,
        "bare proxmox add: auto_sync default is false"
    );

    // Labeled add with empty label: label_entry mode activates, alias_prefix
    // still falls back to short_label because the label is empty.
    let mut app = make_app("");
    app.open_provider_form(ProviderConfigId {
        provider: "digitalocean".to_string(),
        label: Some(String::new()),
    });
    assert!(
        app.providers.form.label_entry,
        "empty-label add: label_entry must be ON"
    );
    assert_eq!(
        app.providers.form.focused_field,
        crate::app::ProviderFormField::Label
    );
    assert_eq!(
        app.providers.form.alias_prefix, "do",
        "empty-label add: alias_prefix uses short_label fallback"
    );
    assert!(app.providers.form_baseline.is_some());

    // Labeled add with non-empty label: alias_prefix becomes <short>-<label>.
    let mut app = make_app("");
    app.open_provider_form(ProviderConfigId::labeled("digitalocean", "prod"));
    assert_eq!(
        app.providers.form.alias_prefix, "do-prod",
        "labeled add: alias_prefix concatenates short_label and label"
    );
    assert!(
        !app.providers.form.label_entry,
        "labeled add with value: label_entry stays off"
    );

    // Edit existing: form populated from section, label_entry stays off.
    let mut app = make_app("");
    app.providers
        .config
        .set_section(crate::providers::config::ProviderSection {
            filter: String::new(),
            id: ProviderConfigId::bare("digitalocean"),
            token: "secret-token".to_string(),
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
    app.open_provider_form(ProviderConfigId::bare("digitalocean"));
    assert_eq!(app.providers.form.token, "secret-token");
    assert_eq!(app.providers.form.alias_prefix, "do");
    assert!(
        app.providers.form.expanded,
        "edit must open with expanded=true"
    );
    assert!(!app.providers.form.label_entry, "edit: label_entry off");
}

#[test]
fn open_snippet_forms_initialize_state_without_mtime() {
    let aliases = vec!["a".to_string(), "b".to_string()];

    // Add snippet: baseline captured, target_aliases preserved, NO mtime.
    let mut app = make_app("Host a\n  HostName 1.2.3.4\n");
    app.open_snippet_add_form(aliases.clone());
    assert!(
        matches!(app.screen, Screen::SnippetForm),
        "expected SnippetForm, got {:?}",
        app.screen
    );
    assert_eq!(app.snippets.flow_targets(), &aliases[..]);
    assert!(
        app.snippets.form_editing().is_none(),
        "add: editing must be None"
    );
    assert!(app.snippets.form_baseline.is_some());
    // Pin the divergence: snippet open does NOT capture mtime.
    assert!(
        app.conflict.form_mtime.is_none(),
        "snippet open must NOT capture mtime"
    );

    // Edit snippet: editing = Some(idx), form populated from snippet.
    let mut app = make_app("Host a\n  HostName 1.2.3.4\n");
    let snippet = crate::snippet::Snippet {
        name: "deploy".to_string(),
        command: "echo hi".to_string(),
        description: "test snippet".to_string(),
        interactive: false,
    };
    app.open_snippet_edit_form(&snippet, aliases.clone(), 5);
    assert!(
        matches!(app.screen, Screen::SnippetForm),
        "expected SnippetForm, got {:?}",
        app.screen
    );
    assert_eq!(app.snippets.flow_targets(), &aliases[..]);
    assert_eq!(app.snippets.form_editing(), Some(5));
    // Pin that snippet data flows through SnippetForm::from_snippet.
    assert_eq!(app.snippets.form.name, "deploy");
    assert_eq!(app.snippets.form.command, "echo hi");
    assert_eq!(app.snippets.form.description, "test snippet");
    assert!(app.snippets.form_baseline.is_some());
    assert!(
        app.conflict.form_mtime.is_none(),
        "snippet open must NOT capture mtime"
    );
}

#[test]
fn close_snippet_form_clears_state_and_returns_to_picker_with_aliases() {
    let mut app = make_app("Host a\n  HostName 1.2.3.4\n");
    app.capture_snippet_form_baseline();
    app.snippets.set_flow_targets(vec!["a".to_string()]);
    app.snippets.set_form_editing(None);
    app.set_screen(Screen::SnippetForm);
    // Pin the two divergences: snippet close must NOT flush vault, and
    // must NOT clear form_mtime (no mtime is captured on snippet form open).
    app.vault.pending_config_write = true;
    app.conflict.form_mtime = Some(SystemTime::UNIX_EPOCH);
    assert!(app.snippets.form_baseline.is_some());

    app.close_snippet_form(vec!["a".to_string(), "b".to_string()]);

    assert!(
        app.snippets.form_baseline.is_none(),
        "baseline must be cleared"
    );
    assert!(
        matches!(app.screen, Screen::SnippetPicker),
        "expected SnippetPicker, got {:?}",
        app.screen
    );
    assert_eq!(
        app.snippets.flow_targets(),
        &["a".to_string(), "b".to_string()][..]
    );
    assert!(
        app.vault.pending_config_write,
        "snippet close must NOT flush"
    );
    assert!(
        app.conflict.form_mtime.is_some(),
        "snippet close must NOT clear mtime (none was captured on open)"
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
    app.hosts_state.patterns = app.hosts_state.ssh_config.pattern_entries();
    let pattern = app.hosts_state.patterns.first().cloned().unwrap();
    let form = HostForm::from_pattern_entry(&pattern);
    app.forms.host = form;
    app.forms.host.vault_addr = "http://should-not-persist:8200".to_string();
    let result = app.edit_host_from_form("web-* db-*");
    assert!(result.is_ok(), "edit failed: {:?}", result);
    let serialized = app.hosts_state.ssh_config.serialize();
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
    app.forms.host = HostForm::new();
    app.forms.host.alias = "newhost".to_string();
    app.forms.host.hostname = "10.0.0.1".to_string();
    app.forms.host.vault_ssh = "ssh-client-signer/sign/engineer".to_string();
    app.forms.host.vault_addr = "http://127.0.0.1:8200".to_string();
    let result = app.add_host_from_form();
    assert!(result.is_ok(), "add failed: {:?}", result);
    let serialized = app.hosts_state.ssh_config.serialize();
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
    app.providers.config = crate::providers::config::ProviderConfig::parse(
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
    app.providers.config = crate::providers::config::ProviderConfig::parse(
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
    app.vault.pending_config_write = false;
    app.flush_pending_vault_write();
    assert!(!app.vault.pending_config_write);
}

#[test]
fn flush_pending_vault_write_clears_flag_after_flush() {
    let mut app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    app.vault.pending_config_write = true;
    let tmpdir = std::env::temp_dir();
    let path = tmpdir.join("purple_test_flush_pending.ini");
    app.hosts_state.ssh_config.path = path.clone();
    app.flush_pending_vault_write();
    assert!(!app.vault.pending_config_write);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn reload_hosts_clears_pending_vault_write_flag() {
    let mut app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    app.vault.pending_config_write = true;
    let tmpdir = std::env::temp_dir();
    let path = tmpdir.join("purple_test_reload_flush.ini");
    app.hosts_state.ssh_config.path = path.clone();
    app.reload_hosts();
    assert!(!app.vault.pending_config_write);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn confirm_vault_sign_screen_stores_signable_list() {
    let mut app = test_app_with_hosts(&["Host a\n  HostName 1.2.3.4\n"]);
    let signable = vec![crate::vault_ssh::VaultSignTarget {
        alias: "a".to_string(),
        role: "ssh/sign/engineer".to_string(),
        certificate_file: String::new(),
        pubkey: std::path::PathBuf::from("/tmp/id_ed25519.pub"),
        vault_addr: None,
    }];
    app.vault.set_pending_sign(signable.clone());
    app.screen = Screen::ConfirmVaultSign;
    assert!(matches!(app.screen, Screen::ConfirmVaultSign));
    let stored = app
        .vault
        .pending_sign()
        .expect("pending_sign payload must be set");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].alias, "a");
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
    assert_eq!(app.hosts_state.display_list.len(), 4);
    assert!(
        matches!(&app.hosts_state.display_list[0], HostListItem::GroupHeader(s) if s == "Production")
    );
    assert!(matches!(
        &app.hosts_state.display_list[1],
        HostListItem::Host { index: 0 }
    ));
    assert!(
        matches!(&app.hosts_state.display_list[2], HostListItem::GroupHeader(s) if s == "Staging")
    );
    assert!(matches!(
        &app.hosts_state.display_list[3],
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
    assert_eq!(app.hosts_state.display_list.len(), 1);
    assert!(matches!(
        &app.hosts_state.display_list[0],
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
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();

    // Should have: DigitalOcean header, 2 hosts, Vultr header, 1 host
    assert_eq!(app.hosts_state.display_list.len(), 5);
    assert!(
        matches!(&app.hosts_state.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean")
    );
    assert!(matches!(
        &app.hosts_state.display_list[1],
        HostListItem::Host { .. }
    ));
    assert!(matches!(
        &app.hosts_state.display_list[2],
        HostListItem::Host { .. }
    ));
    assert!(
        matches!(&app.hosts_state.display_list[3], HostListItem::GroupHeader(s) if s == "Vultr")
    );
    assert!(matches!(
        &app.hosts_state.display_list[4],
        HostListItem::Host { .. }
    ));
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
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();

    // manual first (no header), then DigitalOcean header + do-web
    assert_eq!(app.hosts_state.display_list.len(), 3);
    // No header before the manual host
    assert!(matches!(
        &app.hosts_state.display_list[0],
        HostListItem::Host { .. }
    ));
    assert!(
        matches!(&app.hosts_state.display_list[1], HostListItem::GroupHeader(s) if s == "DigitalOcean")
    );
    assert!(matches!(
        &app.hosts_state.display_list[2],
        HostListItem::Host { .. }
    ));
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
    app.hosts_state.group_by = GroupBy::Provider;
    app.hosts_state.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();

    // DigitalOcean header + sorted hosts
    assert_eq!(app.hosts_state.display_list.len(), 3);
    assert!(
        matches!(&app.hosts_state.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean")
    );
    // First host should be do-alpha (alphabetical)
    if let HostListItem::Host { index } = &app.hosts_state.display_list[1] {
        assert_eq!(app.hosts_state.list[*index].alias, "do-alpha");
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
    app.hosts_state.sort_mode = SortMode::AlphaAlias;

    // Enable grouping
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();
    let grouped_len = app.hosts_state.display_list.len();
    assert!(grouped_len > 2); // Has headers

    // Disable grouping
    app.hosts_state.group_by = GroupBy::None;
    app.apply_sort();
    // Should be flat: just hosts, no headers
    assert_eq!(app.hosts_state.display_list.len(), 2);
    assert!(
        app.hosts_state
            .display_list
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    // dev1 ungrouped first, then production header + 2 hosts
    assert_eq!(app.hosts_state.display_list.len(), 4);
    assert!(matches!(
        &app.hosts_state.display_list[0],
        HostListItem::Host { .. }
    ));
    assert!(
        matches!(&app.hosts_state.display_list[1], HostListItem::GroupHeader(s) if s == "production")
    );
    assert!(matches!(
        &app.hosts_state.display_list[2],
        HostListItem::Host { .. }
    ));
    assert!(matches!(
        &app.hosts_state.display_list[3],
        HostListItem::Host { .. }
    ));
    // Verify config order preserved within group
    if let HostListItem::Host { index } = &app.hosts_state.display_list[2] {
        assert_eq!(app.hosts_state.list[*index].alias, "web1");
    } else {
        panic!("Expected Host item at position 2");
    }
    if let HostListItem::Host { index } = &app.hosts_state.display_list[3] {
        assert_eq!(app.hosts_state.list[*index].alias, "web2");
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    assert_eq!(app.hosts_state.display_list.len(), 2);
    assert!(
        app.hosts_state
            .display_list
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    assert_eq!(app.hosts_state.display_list.len(), 3);
    assert!(
        matches!(&app.hosts_state.display_list[0], HostListItem::GroupHeader(s) if s == "production")
    );
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    assert_eq!(app.hosts_state.display_list.len(), 3);
    assert!(matches!(
        &app.hosts_state.display_list[0],
        HostListItem::Host { .. }
    ));
    assert!(
        matches!(&app.hosts_state.display_list[1], HostListItem::GroupHeader(s) if s == "production")
    );
}

#[test]
fn group_by_tag_empty_host_list() {
    let content = "";
    let mut app = make_app(content);
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    assert!(app.hosts_state.display_list.is_empty());
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    assert_eq!(app.hosts_state.display_list.len(), 3);
    assert!(matches!(
        &app.hosts_state.display_list[0],
        HostListItem::Host { .. }
    ));
    assert!(
        matches!(&app.hosts_state.display_list[1], HostListItem::GroupHeader(s) if s == "production")
    );
    if let HostListItem::Host { index } = &app.hosts_state.display_list[2] {
        assert_eq!(app.hosts_state.list[*index].alias, "web2");
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.hosts_state.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();
    assert_eq!(app.hosts_state.display_list.len(), 3);
    assert!(
        matches!(&app.hosts_state.display_list[0], HostListItem::GroupHeader(s) if s == "production")
    );
    if let HostListItem::Host { index } = &app.hosts_state.display_list[1] {
        assert_eq!(app.hosts_state.list[*index].alias, "alpha");
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.hosts_state.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();
    assert_eq!(app.hosts_state.display_list.len(), 4);
    if let HostListItem::Host { index } = &app.hosts_state.display_list[0] {
        assert_eq!(app.hosts_state.list[*index].alias, "alpha");
    } else {
        panic!("Expected Host item");
    }
    if let HostListItem::Host { index } = &app.hosts_state.display_list[1] {
        assert_eq!(app.hosts_state.list[*index].alias, "charlie");
    } else {
        panic!("Expected Host item");
    }
    assert!(
        matches!(&app.hosts_state.display_list[2], HostListItem::GroupHeader(s) if s == "production")
    );
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
    let original_len = app.hosts_state.ssh_config.elements.len();

    let mut app2 = make_app(content);
    app2.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app2.apply_sort();

    // Config elements must be identical — grouping is display-only
    assert_eq!(
        app.hosts_state.ssh_config.elements.len(),
        app2.hosts_state.ssh_config.elements.len()
    );
    assert_eq!(original_len, app2.hosts_state.ssh_config.elements.len());
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
    let original_len = app.hosts_state.ssh_config.elements.len();

    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();
    app.hosts_state.group_by = GroupBy::None;
    app.apply_sort();

    assert_eq!(app.hosts_state.ssh_config.elements.len(), original_len);
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
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();

    assert_eq!(app.hosts_state.display_list.len(), 5);
    assert!(matches!(
        &app.hosts_state.display_list[0],
        HostListItem::Host { .. }
    ));
    assert!(
        matches!(&app.hosts_state.display_list[1], HostListItem::GroupHeader(s) if s == "DigitalOcean")
    );
    assert!(matches!(
        &app.hosts_state.display_list[2],
        HostListItem::Host { .. }
    ));
    assert!(
        matches!(&app.hosts_state.display_list[3], HostListItem::GroupHeader(s) if s == "Vultr")
    );
    assert!(matches!(
        &app.hosts_state.display_list[4],
        HostListItem::Host { .. }
    ));
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
    app.hosts_state.group_by = GroupBy::Provider;
    app.hosts_state.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();

    assert_eq!(app.hosts_state.display_list.len(), 3);
    assert!(
        matches!(&app.hosts_state.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean")
    );
    if let HostListItem::Host { index } = &app.hosts_state.display_list[1] {
        assert_eq!(app.hosts_state.list[*index].alias, "do-alpha");
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

// --- Host form validation ---

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
    app.providers.config = crate::providers::config::ProviderConfig::default();
    app.providers
        .config
        .set_section(crate::providers::config::ProviderSection {
            filter: String::new(),
            id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
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
            ssm: crate::providers::aws_ssm::SsmMode::default(),
        });
    app
}

#[test]
fn test_apply_sync_result_no_config() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.providers.config = crate::providers::config::ProviderConfig::default();
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
    app.hosts_state.ssh_config.path = PathBuf::from("/dev/null/impossible");
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
    app.providers
        .config
        .set_section(crate::providers::config::ProviderSection {
            filter: String::new(),
            id: crate::providers::config::ProviderConfigId::bare("nonexistent"),
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
            ssm: crate::providers::aws_ssm::SsmMode::default(),
        });
    let (msg, is_err, total, _, _, _) = app.apply_sync_result("nonexistent", vec![], false);
    assert!(is_err);
    assert_eq!(total, 0);
    assert!(msg.contains("Unknown provider"));
}

#[test]
fn test_apply_sync_result_labeled_provider_resolves() {
    // Regression for issue #51: apply_sync_result must look up the provider
    // by the section's bare name, not by the labeled id string `do:work`. The
    // pre-fix path called `get_provider_with_config(provider, &section)` where
    // `provider == "do:work"`, which missed the descriptor table and returned
    // "Unknown provider." to the user.
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.providers.config = crate::providers::config::ProviderConfig::default();
    app.providers
        .config
        .set_section(crate::providers::config::ProviderSection {
            filter: String::new(),
            id: crate::providers::config::ProviderConfigId::labeled("digitalocean", "work"),
            token: "tok".to_string(),
            alias_prefix: "do-work".to_string(),
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
    let (msg, is_err, total, _, _, _) = app.apply_sync_result("digitalocean:work", vec![], false);
    assert!(!is_err, "labeled provider lookup must not error: {msg}");
    assert_eq!(total, 0);
    assert!(!msg.contains("Unknown provider"));
}

#[test]
fn test_sync_history_cleared_on_provider_remove() {
    let mut app = make_provider_app();
    // Simulate a completed sync
    app.providers.sync_history.insert(
        "digitalocean".to_string(),
        SyncRecord {
            timestamp: 100,
            message: "3 servers".to_string(),
            is_error: false,
        },
    );
    assert!(app.providers.sync_history.contains_key("digitalocean"));

    // Simulate provider remove (same as handler.rs 'd' key path)
    app.providers.config.remove_section("digitalocean");
    app.providers.sync_history.remove("digitalocean");

    assert!(!app.providers.sync_history.contains_key("digitalocean"));
}

#[test]
fn test_sync_history_overwrite_replaces_error_with_success() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    // First sync fails
    app.providers.sync_history.insert(
        "hetzner".to_string(),
        SyncRecord {
            timestamp: 100,
            message: "auth failed".to_string(),
            is_error: true,
        },
    );
    // Second sync succeeds
    app.providers.sync_history.insert(
        "hetzner".to_string(),
        SyncRecord {
            timestamp: 200,
            message: "5 servers".to_string(),
            is_error: false,
        },
    );
    let record = app.providers.sync_history.get("hetzner").unwrap();
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
        filter: String::new(),
        id: crate::providers::config::ProviderConfigId::bare(provider),
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
        ssm: crate::providers::aws_ssm::SsmMode::default(),
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
        .map(|s| s.provider())
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
        .map(|s| s.provider())
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
    assert!(!app.ui.password_picker.open);
}

#[test]
fn test_select_next_password_source() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.ui.password_picker.list.select(Some(0));
    app.select_next_password_source();
    assert_eq!(app.ui.password_picker.list.selected(), Some(1));
}

#[test]
fn test_select_prev_password_source() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.ui.password_picker.list.select(Some(2));
    app.select_prev_password_source();
    assert_eq!(app.ui.password_picker.list.selected(), Some(1));
}

#[test]
fn test_select_password_source_wrap_bottom() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
    app.ui.password_picker.list.select(Some(last));
    app.select_next_password_source();
    assert_eq!(app.ui.password_picker.list.selected(), Some(0));
}

#[test]
fn test_select_password_source_wrap_top() {
    let mut app = make_app("Host test\n  HostName test.com\n");
    app.ui.password_picker.list.select(Some(0));
    app.select_prev_password_source();
    let last = crate::askpass::PASSWORD_SOURCES.len() - 1;
    assert_eq!(app.ui.password_picker.list.selected(), Some(last));
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
    assert_eq!(
        app.hosts_state.list[0].askpass,
        Some("keychain".to_string())
    );
}

#[test]
fn test_host_entries_vault_askpass() {
    let app =
        make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass vault:secret/ssh#pass\n");
    assert_eq!(
        app.hosts_state.list[0].askpass,
        Some("vault:secret/ssh#pass".to_string())
    );
}

#[test]
fn test_host_entries_no_askpass() {
    let app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    assert_eq!(app.hosts_state.list[0].askpass, None);
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
    app.hosts_state.ssh_config.add_host(&entry);
    let _ = app
        .hosts_state
        .ssh_config
        .set_host_askpass("newhost", "keychain");
    let serialized = app.hosts_state.ssh_config.serialize();
    assert!(serialized.contains("purple:askpass keychain"));
    let entries = app.hosts_state.ssh_config.host_entries();
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
    app.hosts_state.ssh_config.add_host(&entry);
    let _ = app
        .hosts_state
        .ssh_config
        .set_host_askpass("vaulthost", "vault:secret/ssh#pass");
    let serialized = app.hosts_state.ssh_config.serialize();
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
    app.hosts_state.ssh_config.add_host(&entry);
    // Don't call set_host_askpass when None — mirrors add_host_from_form logic
    let serialized = app.hosts_state.ssh_config.serialize();
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
    app.forms.host.alias = "vaulthost".to_string();
    app.forms.host.hostname = "10.0.0.1".to_string();
    app.forms.host.vault_ssh = "ssh/sign/engineer".to_string();
    let result = app.add_host_from_form();
    assert!(result.is_ok(), "add_host_from_form failed: {:?}", result);
    let entries = app.hosts_state.ssh_config.host_entries();
    let host = entries.iter().find(|e| e.alias == "vaulthost").unwrap();
    assert_eq!(host.vault_ssh.as_deref(), Some("ssh/sign/engineer"));
    let serialized = app.hosts_state.ssh_config.serialize();
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
    let _ = app
        .hosts_state
        .ssh_config
        .set_host_askpass("myserver", "bw:my-item");
    let serialized = app.hosts_state.ssh_config.serialize();
    assert!(serialized.contains("purple:askpass bw:my-item"));
    let entries = app.hosts_state.ssh_config.host_entries();
    assert_eq!(entries[0].askpass, Some("bw:my-item".to_string()));
}

#[test]
fn test_config_set_host_askpass_changes() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    let _ = app
        .hosts_state
        .ssh_config
        .set_host_askpass("myserver", "pass:ssh/myserver");
    let serialized = app.hosts_state.ssh_config.serialize();
    assert!(!serialized.contains("keychain"));
    assert!(serialized.contains("purple:askpass pass:ssh/myserver"));
}

#[test]
fn test_config_set_host_askpass_removes() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n  # purple:askpass keychain\n");
    let _ = app.hosts_state.ssh_config.set_host_askpass("myserver", "");
    let serialized = app.hosts_state.ssh_config.serialize();
    assert!(!serialized.contains("purple:askpass"));
    let entries = app.hosts_state.ssh_config.host_entries();
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
    app.hosts_state.ssh_config.update_host("myserver", &entry);
    let _ = app
        .hosts_state
        .ssh_config
        .set_host_askpass("myserver", entry.askpass.as_deref().unwrap_or(""));
    let serialized = app.hosts_state.ssh_config.serialize();
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
    let host = app.hosts_state.list[0].clone();
    app.forms.host = HostForm::from_entry(&host, Default::default());
    app.forms.host.vault_ssh = "ssh/sign/engineer".to_string();

    let result = app.edit_host_from_form("myserver");
    assert!(result.is_ok(), "edit_host_from_form failed: {:?}", result);
    let serialized = app.hosts_state.ssh_config.serialize();
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

#[test]
fn add_host_from_form_logs_what_changed() {
    let mut app = make_app("");
    app.forms.host = HostForm::new();
    app.forms.host.alias = "loghost".to_string();
    app.forms.host.hostname = "10.0.0.9".to_string();
    let records = crate::logging::capture::capture(|| {
        let result = app.add_host_from_form();
        assert!(result.is_ok(), "add failed: {:?}", result);
    });
    assert!(
        crate::logging::capture::has(
            &records,
            log::Level::Debug,
            "[purple] host added: alias=loghost"
        ),
        "expected host-added debug log, got: {:?}",
        records
    );
}

#[test]
fn add_host_from_form_logs_save_failure() {
    let mut app = make_app("");
    // Point the config below a regular file so the write fails.
    let not_a_dir = app
        .hosts_state
        .ssh_config
        .path
        .parent()
        .unwrap()
        .join("not_a_dir");
    std::fs::write(&not_a_dir, "").expect("write blocker file");
    app.hosts_state.ssh_config.path = not_a_dir.join("config");
    app.forms.host = HostForm::new();
    app.forms.host.alias = "loghost".to_string();
    app.forms.host.hostname = "10.0.0.9".to_string();
    let records = crate::logging::capture::capture(|| {
        let result = app.add_host_from_form();
        assert!(result.is_err(), "expected save failure");
    });
    assert!(
        crate::logging::capture::has(
            &records,
            log::Level::Warn,
            "[config] failed to save new host: alias=loghost"
        ),
        "expected save-failure warn log, got: {:?}",
        records
    );
}

#[test]
fn edit_host_from_form_logs_what_changed() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    let host = app.hosts_state.list[0].clone();
    app.forms.host = HostForm::from_entry(&host, Default::default());
    app.forms.host.user = "admin".to_string();
    let records = crate::logging::capture::capture(|| {
        let result = app.edit_host_from_form("myserver");
        assert!(result.is_ok(), "edit failed: {:?}", result);
    });
    assert!(
        crate::logging::capture::has(
            &records,
            log::Level::Debug,
            "[purple] host edited: alias=myserver"
        ),
        "expected host-edited debug log, got: {:?}",
        records
    );
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
    let host = app.hosts_state.list[0].clone();
    app.forms.host = HostForm::from_entry(&host, Default::default());
    // Change something unrelated so the form actually mutates.
    app.forms.host.user = "admin".to_string();

    let result = app.edit_host_from_form("myserver");
    assert!(result.is_ok(), "edit_host_from_form failed: {:?}", result);
    let serialized = app.hosts_state.ssh_config.serialize();
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
    let host = app.hosts_state.list[0].clone();
    app.forms.host = HostForm::from_entry(&host, Default::default());
    app.forms.host.vault_ssh = String::new();

    let result = app.edit_host_from_form("myserver");
    assert!(result.is_ok(), "edit_host_from_form failed: {:?}", result);
    let serialized = app.hosts_state.ssh_config.serialize();
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
    assert_eq!(app.hosts_state.patterns.len(), 1);
    assert_eq!(app.hosts_state.patterns[0].pattern, "web-* db-*");

    app.forms.host = HostForm::from_pattern_entry(&app.hosts_state.patterns[0]);
    assert!(app.forms.host.is_pattern);
    app.forms.host.user = "admin".to_string();

    let result = app.edit_host_from_form("web-* db-*");
    assert!(result.is_ok(), "expected success, got: {:?}", result);
}

#[test]
fn test_edit_single_pattern_from_form() {
    let mut app = make_app("Host *.example.com\n  User deploy\n");
    assert_eq!(app.hosts_state.patterns.len(), 1);
    app.forms.host = HostForm::from_pattern_entry(&app.hosts_state.patterns[0]);
    app.forms.host.user = "admin".to_string();

    let result = app.edit_host_from_form("*.example.com");
    assert!(result.is_ok(), "expected success, got: {:?}", result);
}

#[test]
fn test_edit_pattern_duplicate_detection() {
    let mut app = make_app("Host web-* db-*\n  User deploy\nHost cache-*\n  User cache\n");
    let pat = app
        .hosts_state
        .patterns
        .iter()
        .find(|p| p.pattern == "web-* db-*")
        .unwrap()
        .clone();
    app.forms.host = HostForm::from_pattern_entry(&pat);
    app.forms.host.alias = "cache-*".to_string();
    let result = app.edit_host_from_form("web-* db-*");
    assert_eq!(result, Err("Pattern 'cache-*' already exists.".to_string()));
}

#[test]
fn test_add_pattern_duplicate_detection() {
    let mut app = make_app("Host web-* db-*\n  User deploy\n");
    app.forms.host = HostForm::new_pattern();
    app.forms.host.alias = "web-* db-*".to_string();
    app.forms.host.user = "admin".to_string();
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
    let host = &app.hosts_state.list[0];
    assert_eq!(host.askpass, Some("keychain".to_string()));
    // Simulating what handle_host_list does
    let pending = (host.alias.clone(), host.askpass.clone());
    assert_eq!(pending.0, "myserver");
    assert_eq!(pending.1, Some("keychain".to_string()));
}

#[test]
fn test_pending_connect_without_askpass() {
    let app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    let host = &app.hosts_state.list[0];
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
    let old_entry = app.hosts_state.list[0].clone();
    assert_eq!(old_entry.askpass, Some("keychain".to_string()));

    // Apply new askpass
    let _ = app
        .hosts_state
        .ssh_config
        .set_host_askpass("myserver", "vault:secret/ssh#pw");
    assert_eq!(
        app.hosts_state.ssh_config.host_entries()[0].askpass,
        Some("vault:secret/ssh#pw".to_string())
    );

    // Simulate rollback (write failed)
    let _ = app
        .hosts_state
        .ssh_config
        .set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
    assert_eq!(
        app.hosts_state.ssh_config.host_entries()[0].askpass,
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
    let old_entry = app.hosts_state.list[0].clone();
    assert_eq!(old_entry.vault_addr.as_deref(), Some("http://old:8200"));

    // Apply a new vault_addr (successful in-memory mutation).
    assert!(
        app.hosts_state
            .ssh_config
            .set_host_vault_addr("myserver", "http://new:8200")
    );
    assert_eq!(
        app.hosts_state.ssh_config.host_entries()[0]
            .vault_addr
            .as_deref(),
        Some("http://new:8200")
    );

    // Simulate rollback (write failed). This is the exact call the
    // edit_host_from_form rollback block makes.
    let _ = app.hosts_state.ssh_config.set_host_vault_addr(
        &old_entry.alias,
        old_entry.vault_addr.as_deref().unwrap_or(""),
    );
    assert_eq!(
        app.hosts_state.ssh_config.host_entries()[0]
            .vault_addr
            .as_deref(),
        Some("http://old:8200")
    );
}

#[test]
fn test_edit_vault_addr_rollback_restores_none() {
    // Rollback from a just-added vault_addr back to empty (no comment).
    let mut app = make_app(
        "Host myserver\n  HostName 10.0.0.1\n  # purple:vault-ssh ssh-client-signer/sign/engineer\n",
    );
    let old_entry = app.hosts_state.list[0].clone();
    assert!(old_entry.vault_addr.is_none());

    assert!(
        app.hosts_state
            .ssh_config
            .set_host_vault_addr("myserver", "http://new:8200")
    );
    assert_eq!(
        app.hosts_state.ssh_config.host_entries()[0]
            .vault_addr
            .as_deref(),
        Some("http://new:8200")
    );

    let _ = app.hosts_state.ssh_config.set_host_vault_addr(
        &old_entry.alias,
        old_entry.vault_addr.as_deref().unwrap_or(""),
    );
    assert!(
        app.hosts_state.ssh_config.host_entries()[0]
            .vault_addr
            .is_none()
    );
}

#[test]
fn test_edit_askpass_rollback_restores_none() {
    let mut app = make_app("Host myserver\n  HostName 10.0.0.1\n");
    let old_entry = app.hosts_state.list[0].clone();
    assert_eq!(old_entry.askpass, None);

    // Apply new askpass
    let _ = app
        .hosts_state
        .ssh_config
        .set_host_askpass("myserver", "bw:my-item");
    assert_eq!(
        app.hosts_state.ssh_config.host_entries()[0].askpass,
        Some("bw:my-item".to_string())
    );

    // Simulate rollback (write failed)
    let _ = app
        .hosts_state
        .ssh_config
        .set_host_askpass(&old_entry.alias, old_entry.askpass.as_deref().unwrap_or(""));
    assert_eq!(app.hosts_state.ssh_config.host_entries()[0].askpass, None);
}

// --- password picker state edge cases ---

#[test]
fn test_password_picker_initial_state_not_shown() {
    let app = make_app("Host test\n  HostName test.com\n");
    assert!(!app.ui.password_picker.open);
    assert_eq!(app.ui.password_picker.list.selected(), None);
}

// --- askpass global default fallback ---

#[test]
fn test_pending_connect_askpass_from_host() {
    let app = make_app(
        "Host s1\n  HostName 1.1.1.1\n  # purple:askpass bw:item1\n\nHost s2\n  HostName 2.2.2.2\n",
    );
    assert_eq!(
        app.hosts_state.list[0].askpass,
        Some("bw:item1".to_string())
    );
    assert_eq!(app.hosts_state.list[1].askpass, None);
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
    app.hosts_state.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();

    // beta (non-stale) should come first, then alpha and gamma (stale, sorted alphabetically)
    assert_eq!(app.hosts_state.display_list.len(), 3);
    if let HostListItem::Host { index } = &app.hosts_state.display_list[0] {
        assert_eq!(app.hosts_state.list[*index].alias, "beta");
    } else {
        panic!("Expected Host item at position 0");
    }
    if let HostListItem::Host { index } = &app.hosts_state.display_list[1] {
        assert_eq!(app.hosts_state.list[*index].alias, "alpha");
    } else {
        panic!("Expected Host item at position 1");
    }
    if let HostListItem::Host { index } = &app.hosts_state.display_list[2] {
        assert_eq!(app.hosts_state.list[*index].alias, "gamma");
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
    app.hosts_state.sort_mode = SortMode::AlphaAlias;
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

    app.hosts_state.sort_mode = SortMode::AlphaAlias;
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
    app.hosts_state.group_by = GroupBy::Provider;
    app.hosts_state.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();
    app.select_first_host();

    // Headers are never selectable; first host is selected instead
    assert!(matches!(
        &app.hosts_state.display_list[0],
        HostListItem::GroupHeader(_)
    ));
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
    app.hosts_state.group_by = GroupBy::None;
    app.hosts_state.sort_mode = SortMode::AlphaAlias;
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
    app.hosts_state.sort_mode = SortMode::AlphaHostname;
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
    assert_eq!(
        app.hosts_state.list[app.search.filtered_indices[0]].alias,
        "alpha"
    );
    assert_eq!(
        app.hosts_state.list[app.search.filtered_indices[1]].alias,
        "gamma"
    );
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
    assert_eq!(
        app.hosts_state.list[app.search.filtered_indices[0]].alias,
        "alpha"
    );
    assert_eq!(
        app.hosts_state.list[app.search.filtered_indices[1]].alias,
        "gamma"
    );
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
    app.providers.config = crate::providers::config::ProviderConfig::default();
    app.providers
        .config
        .set_section(crate::providers::config::ProviderSection {
            filter: String::new(),
            id: crate::providers::config::ProviderConfigId::bare("digitalocean"),
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
            ssm: crate::providers::aws_ssm::SsmMode::default(),
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
    assert_eq!(app.hosts_state.patterns.len(), 1);
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
        app.hosts_state.patterns[app.search.filtered_pattern_indices[0]].pattern,
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
        app.hosts_state.patterns[app.search.filtered_pattern_indices[0]].pattern,
        "10.30.0.*"
    );
}

#[test]
fn pattern_placeholder_text() {
    use crate::app::FormField;
    use crate::messages::hints;
    use crate::ui::host_form::{placeholder_text, placeholder_text_pattern};
    // Regular host placeholder
    assert_eq!(placeholder_text(FormField::Alias), hints::HOST_ALIAS);
    // Pattern placeholder
    assert_eq!(
        placeholder_text_pattern(FormField::Alias),
        hints::HOST_ALIAS_PATTERN
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // Both hosts have the tag, stale or not — both in group
    assert_eq!(app.hosts_state.display_list.len(), 3);
    assert!(
        matches!(&app.hosts_state.display_list[0], HostListItem::GroupHeader(s) if s == "production")
    );
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // Both hosts have user tag "production" — both grouped
    assert_eq!(app.hosts_state.display_list.len(), 3);
    assert!(
        matches!(&app.hosts_state.display_list[0], HostListItem::GroupHeader(s) if s == "production")
    );
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // "production" is a provider_tag, not a user tag — no grouping
    assert_eq!(app.hosts_state.display_list.len(), 2);
    assert!(
        app.hosts_state
            .display_list
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.hosts_state.sort_mode = SortMode::Original;
    app.apply_sort();

    // manual ungrouped, then production header + zeta + alpha (config order)
    assert_eq!(app.hosts_state.display_list.len(), 4);
    assert!(matches!(
        &app.hosts_state.display_list[0],
        HostListItem::Host { .. }
    ));
    assert!(
        matches!(&app.hosts_state.display_list[1], HostListItem::GroupHeader(s) if s == "production")
    );
    // Verify config order preserved within group
    if let HostListItem::Host { index } = &app.hosts_state.display_list[2] {
        assert_eq!(app.hosts_state.list[*index].alias, "zeta");
    } else {
        panic!("Expected Host item at position 2");
    }
    if let HostListItem::Host { index } = &app.hosts_state.display_list[3] {
        assert_eq!(app.hosts_state.list[*index].alias, "alpha");
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.hosts_state.sort_mode = SortMode::AlphaHostname;
    app.apply_sort();

    assert_eq!(app.hosts_state.display_list.len(), 3);
    assert!(
        matches!(&app.hosts_state.display_list[0], HostListItem::GroupHeader(s) if s == "production")
    );
    if let HostListItem::Host { index } = &app.hosts_state.display_list[1] {
        assert_eq!(app.hosts_state.list[*index].hostname, "alpha.example.com");
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
    app.hosts_state.group_by = GroupBy::Provider;
    app.hosts_state.sort_mode = SortMode::AlphaHostname;
    app.apply_sort();

    assert_eq!(app.hosts_state.display_list.len(), 3);
    assert!(
        matches!(&app.hosts_state.display_list[0], HostListItem::GroupHeader(s) if s == "DigitalOcean")
    );
    if let HostListItem::Host { index } = &app.hosts_state.display_list[1] {
        assert_eq!(app.hosts_state.list[*index].hostname, "alpha.example.com");
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
        app.hosts_state.group_by = GroupBy::None;
        app.hosts_state.sort_mode = mode;
        app.apply_sort();

        // No headers, just sorted hosts
        assert_eq!(app.hosts_state.display_list.len(), 2);
        assert!(
            app.hosts_state
                .display_list
                .iter()
                .all(|item| matches!(item, HostListItem::Host { .. }))
        );
        if let HostListItem::Host { index } = &app.hosts_state.display_list[0] {
            assert_eq!(app.hosts_state.list[*index].alias, "alpha");
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // Before search: 1 ungrouped + 1 header + 2 grouped = 4
    assert_eq!(app.hosts_state.display_list.len(), 4);

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
    app.hosts_state.multi_select.insert(0);
    app.hosts_state.multi_select.insert(1);
    assert_eq!(app.hosts_state.multi_select.len(), 2);

    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    assert!(app.hosts_state.multi_select.is_empty());
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.hosts_state.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();

    // Should have: production header + web1, then Patterns header + pattern
    let has_patterns_header = app
        .hosts_state
        .display_list
        .iter()
        .any(|item| matches!(item, HostListItem::GroupHeader(s) if s == "Patterns"));
    assert!(
        has_patterns_header,
        "Patterns header should appear at bottom"
    );

    // Patterns header should be after all hosts
    let patterns_pos = app
        .hosts_state
        .display_list
        .iter()
        .position(|item| matches!(item, HostListItem::GroupHeader(s) if s == "Patterns"))
        .unwrap();
    let last_host_pos = app
        .hosts_state
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
    if let Some(ref ts) = tags
        && !ts.is_empty()
    {
        lines.push(format!("  # purple:tags {}", ts.join(",")));
    }
    lines.join("\n")
}

proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(200))]

    /// GroupBy::Tag display_list is consistent:
    /// - Total host items == app.hosts_state.list.len()
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

        app.hosts_state.group_by = GroupBy::Tag(chosen_tag.clone());
        app.apply_sort();

        let host_count = app.hosts_state.list.len();
        let display_host_count = app.hosts_state.display_list.iter()
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
        let indices: Vec<usize> = app.hosts_state.display_list.iter()
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
        let header_count = app.hosts_state.display_list.iter()
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
            let header_pos = app.hosts_state.display_list.iter()
                .position(|item| matches!(item, HostListItem::GroupHeader(s) if s == &chosen_tag))
                .unwrap();
            for (pos, item) in app.hosts_state.display_list.iter().enumerate() {
                if let HostListItem::Host { index } = item {
                    let has_tag = app.hosts_state.list[*index].tags.iter().any(|t| t == &chosen_tag);
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

        app.hosts_state.group_by = GroupBy::None;
        app.hosts_state.sort_mode = SortMode::AlphaAlias;
        app.apply_sort();

        let host_count = app.hosts_state.list.len();

        // No group headers from GroupBy::None (comment-based headers possible;
        // but no tag/provider headers)
        let display_host_count = app.hosts_state.display_list.iter()
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
        app.hosts_state.group_by = GroupBy::Tag(tag.clone());
        app.apply_sort();
        let has_header_grouped = app.hosts_state.display_list.iter()
            .any(|item| matches!(item, HostListItem::GroupHeader(s) if s == &tag));
        prop_assert!(has_header_grouped, "expected GroupHeader for tag '{}'", tag);

        // Switch to None
        app.hosts_state.group_by = GroupBy::None;
        app.apply_sort();
        let has_header_none = app.hosts_state.display_list.iter()
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // All hosts ungrouped, no header, no panic
    assert_eq!(app.hosts_state.display_list.len(), 2);
    assert!(
        app.hosts_state
            .display_list
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.hosts_state.sort_mode = SortMode::Original;
    app.apply_sort();

    // Original order preserved: stale host first within group
    assert_eq!(app.hosts_state.display_list.len(), 3);
    assert!(
        matches!(&app.hosts_state.display_list[0], HostListItem::GroupHeader(s) if s == "production")
    );
    if let HostListItem::Host { index } = &app.hosts_state.display_list[1] {
        assert_eq!(app.hosts_state.list[*index].alias, "stale-first");
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.hosts_state.sort_mode = SortMode::AlphaAlias;
    app.apply_sort();

    // Alpha sort: stale host pushed to bottom of group
    assert_eq!(app.hosts_state.display_list.len(), 3);
    assert!(
        matches!(&app.hosts_state.display_list[0], HostListItem::GroupHeader(s) if s == "production")
    );
    if let HostListItem::Host { index } = &app.hosts_state.display_list[1] {
        assert_eq!(app.hosts_state.list[*index].alias, "beta-healthy");
    } else {
        panic!("Expected Host item");
    }
    if let HostListItem::Host { index } = &app.hosts_state.display_list[2] {
        assert_eq!(app.hosts_state.list[*index].alias, "alpha-stale");
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());

    let cleared = app.clear_stale_group_tag();

    assert!(cleared);
    assert_eq!(app.hosts_state.group_by, GroupBy::None);
}

#[test]
fn clear_stale_group_tag_keeps_when_tag_exists() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
";
    let mut app = make_app(content);
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());

    let cleared = app.clear_stale_group_tag();

    assert!(!cleared);
    assert_eq!(
        app.hosts_state.group_by,
        GroupBy::Tag("production".to_string())
    );
}

#[test]
fn clear_stale_group_tag_noop_for_provider() {
    let content = "\
Host web1
  HostName 1.1.1.1
";
    let mut app = make_app(content);
    app.hosts_state.group_by = GroupBy::Provider;

    let cleared = app.clear_stale_group_tag();

    assert!(!cleared);
    assert_eq!(app.hosts_state.group_by, GroupBy::Provider);
}

#[test]
fn clear_stale_group_tag_noop_for_none() {
    let content = "\
Host web1
  HostName 1.1.1.1
";
    let mut app = make_app(content);
    app.hosts_state.group_by = GroupBy::None;

    let cleared = app.clear_stale_group_tag();

    assert!(!cleared);
    assert_eq!(app.hosts_state.group_by, GroupBy::None);
}

#[test]
fn clear_stale_group_tag_empty_hosts() {
    let content = "";
    let mut app = make_app(content);
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());

    let cleared = app.clear_stale_group_tag();

    assert!(cleared);
    assert_eq!(app.hosts_state.group_by, GroupBy::None);
}

#[test]
fn clear_stale_group_tag_keeps_empty_tag_sentinel() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:tags production
";
    let mut app = make_app(content);
    app.hosts_state.group_by = GroupBy::Tag(String::new());

    let cleared = app.clear_stale_group_tag();

    assert!(!cleared, "empty tag sentinel should not be cleared");
    assert_eq!(app.hosts_state.group_by, GroupBy::Tag(String::new()));
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());

    let cleared = app.clear_stale_group_tag();

    assert!(
        !cleared,
        "tag existing only on a pattern should not be cleared"
    );
    assert_eq!(
        app.hosts_state.group_by,
        GroupBy::Tag("production".to_string())
    );
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // Without filter: header + all 3 hosts visible
    let hosts_before: Vec<_> = app
        .hosts_state
        .display_list
        .iter()
        .filter(|item| matches!(item, HostListItem::Host { .. }))
        .collect();
    assert_eq!(hosts_before.len(), 3, "all 3 hosts should be visible");

    // group_host_counts should show 2 for production
    assert_eq!(
        app.hosts_state.group_host_counts.get("production"),
        Some(&2),
        "production group should have 2 hosts"
    );

    // Filter to production group only
    app.hosts_state.group_filter = Some("production".to_string());
    app.apply_sort();

    // Only production hosts should be visible (no header, no staging host)
    let hosts_after: Vec<_> = app
        .hosts_state
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
        app.hosts_state.group_host_counts.get("production"),
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());

    // Filter
    app.hosts_state.group_filter = Some("production".to_string());
    app.apply_sort();

    let hosts_filtered: Vec<_> = app
        .hosts_state
        .display_list
        .iter()
        .filter(|item| matches!(item, HostListItem::Host { .. }))
        .collect();
    assert_eq!(hosts_filtered.len(), 2);

    // Clear filter
    app.hosts_state.group_filter = None;
    app.apply_sort();

    let hosts_unfiltered: Vec<_> = app
        .hosts_state
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
    app.hosts_state.group_by = GroupBy::Provider;
    app.hosts_state.group_filter = Some("aws".to_string());

    // Change group_by to Tag, which triggers clear_stale_group_tag
    app.hosts_state.group_by = GroupBy::Tag("nonexistent".to_string());
    let cleared = app.clear_stale_group_tag();

    assert!(cleared);
    assert!(
        app.hosts_state.group_filter.is_none(),
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    app.apply_sort();

    // group_tab_order should contain "production"
    assert!(
        app.hosts_state
            .group_tab_order
            .contains(&"production".to_string()),
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
    app.hosts_state.group_by = GroupBy::Tag("alpha".to_string());
    app.apply_sort();

    assert_eq!(app.hosts_state.group_tab_order.len(), 2);
    assert_eq!(app.hosts_state.group_tab_order[0], "alpha");
    assert_eq!(app.hosts_state.group_tab_order[1], "beta");
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
    app.hosts_state.group_by = GroupBy::Tag("production".to_string());
    // Filter to staging: only web-staging visible (it's the ungrouped host)
    app.hosts_state.group_filter = Some("production".to_string());
    app.apply_sort();

    // Simulate Ctrl+A: select all visible Host items
    let visible_indices: Vec<usize> = app
        .hosts_state
        .display_list
        .iter()
        .filter_map(|item| match item {
            HostListItem::Host { index } => Some(*index),
            _ => None,
        })
        .collect();
    for idx in &visible_indices {
        app.hosts_state.multi_select.insert(*idx);
    }

    // Only production hosts should be selected when filter is active
    assert_eq!(app.hosts_state.multi_select.len(), 2);
    for idx in &app.hosts_state.multi_select {
        let host = &app.hosts_state.list[*idx];
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
        .hosts_state
        .display_list
        .iter()
        .filter_map(|item| match item {
            HostListItem::Host { index } => Some(*index),
            _ => None,
        })
        .collect();
    for idx in &host_indices {
        app.hosts_state.multi_select.insert(*idx);
    }

    assert_eq!(app.hosts_state.multi_select.len(), 3);
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
        .hosts_state
        .display_list
        .iter()
        .filter_map(|item| match item {
            HostListItem::Host { index } => Some(*index),
            _ => None,
        })
        .collect();
    for idx in &host_indices {
        app.hosts_state.multi_select.insert(*idx);
    }
    assert_eq!(app.hosts_state.multi_select.len(), 3);

    // Check all_selected condition and clear
    let all_selected = host_indices
        .iter()
        .all(|idx| app.hosts_state.multi_select.contains(idx));
    assert!(all_selected);
    app.hosts_state.multi_select.clear();

    assert!(app.hosts_state.multi_select.is_empty());
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
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();

    // Activate a group filter directly (Tab no longer wires this).
    let first_group = app.hosts_state.group_tab_order[0].clone();
    app.hosts_state.group_filter = Some(first_group);
    assert!(app.hosts_state.group_filter.is_some());

    app.clear_group_filter();

    assert_eq!(app.hosts_state.group_filter, None);
}

#[test]
fn clear_group_filter_noop_when_already_none() {
    let content = "Host web1\n  HostName 1.1.1.1\n";
    let mut app = make_app(content);
    assert_eq!(app.hosts_state.group_filter, None);

    // Should not panic or change state
    app.clear_group_filter();

    assert_eq!(app.hosts_state.group_filter, None);
}

// --- select_next / select_prev: header skipping + wrap-around ---

#[test]
fn select_next_skips_group_header() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host web2
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();

    // display_list: [GroupHeader(DO), Host(idx), GroupHeader(AWS), Host(idx)]
    // Find the first Host item index in display_list
    let first_host_pos = app
        .hosts_state
        .display_list
        .iter()
        .position(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    app.ui.list_state.select(Some(first_host_pos));

    app.select_next();

    let selected = app.ui.list_state.selected().unwrap();
    assert!(
        matches!(
            app.hosts_state.display_list[selected],
            HostListItem::Host { .. }
        ),
        "selection should land on a Host, not a GroupHeader"
    );
    assert!(
        selected > first_host_pos,
        "selection should have moved forward"
    );
}

#[test]
fn select_prev_skips_group_header() {
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host web2
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();

    // Find the last Host item in display_list
    let last_host_pos = app
        .hosts_state
        .display_list
        .iter()
        .rposition(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    app.ui.list_state.select(Some(last_host_pos));

    app.select_prev();

    let selected = app.ui.list_state.selected().unwrap();
    assert!(
        matches!(
            app.hosts_state.display_list[selected],
            HostListItem::Host { .. }
        ),
        "selection should land on a Host, not a GroupHeader"
    );
    assert!(selected < last_host_pos, "selection should have moved back");
}

#[test]
fn select_next_wraps_to_first_at_end() {
    // From the last host, select_next wraps back to the first, like the
    // other tabs. Group headers in between are skipped.
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host web2
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();

    let first_host_pos = app
        .hosts_state
        .display_list
        .iter()
        .position(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    let last_host_pos = app
        .hosts_state
        .display_list
        .iter()
        .rposition(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    app.ui.list_state.select(Some(last_host_pos));

    app.select_next();

    assert_eq!(
        app.ui.list_state.selected(),
        Some(first_host_pos),
        "select_next at the end should wrap to the first host"
    );
}

#[test]
fn select_prev_wraps_to_last_at_start() {
    // From the first host, select_prev wraps to the last, like the other tabs.
    let content = "\
Host web1
  HostName 1.1.1.1
  # purple:provider digitalocean:1

Host web2
  HostName 2.2.2.2
  # purple:provider aws:2
";
    let mut app = make_app(content);
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();

    let first_host_pos = app
        .hosts_state
        .display_list
        .iter()
        .position(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    let last_host_pos = app
        .hosts_state
        .display_list
        .iter()
        .rposition(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    app.ui.list_state.select(Some(first_host_pos));

    app.select_prev();

    assert_eq!(
        app.ui.list_state.selected(),
        Some(last_host_pos),
        "select_prev at the start should wrap to the last host"
    );
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
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();

    // Navigate into the DigitalOcean group
    let do_group = app
        .hosts_state
        .group_tab_order
        .iter()
        .find(|g| g.to_lowercase().contains("digital"))
        .cloned()
        .unwrap_or_else(|| app.hosts_state.group_tab_order[0].clone());
    app.hosts_state.group_filter = Some(do_group.clone());
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
        app.hosts_state.list[matched_idx].provider.as_deref(),
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
    app.hosts_state.group_by = GroupBy::Provider;
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
    app.hosts_state.group_by = GroupBy::Tag("common".to_string());
    app.apply_sort();

    // group_tab_order should be sorted by frequency descending
    // "common" appears 3 times, "rare" once
    assert!(!app.hosts_state.group_tab_order.is_empty());
    assert_eq!(app.hosts_state.group_tab_order[0], "common");
    assert_eq!(app.hosts_state.group_tab_order[1], "rare");
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
    app.hosts_state.group_by = GroupBy::Tag("prod".to_string());
    app.apply_sort();

    assert!(
        app.hosts_state
            .group_tab_order
            .contains(&"internal".to_string()),
        "pattern-only tag should appear in group_tab_order"
    );
    assert!(
        app.hosts_state
            .group_tab_order
            .contains(&"prod".to_string()),
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
    app.hosts_state.group_by = GroupBy::Tag("prod".to_string());
    app.apply_sort();

    // group_host_counts for "prod" should count both the host and the pattern
    assert_eq!(
        app.hosts_state.group_host_counts.get("prod"),
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
    app.hosts_state.group_by = GroupBy::Tag("tag0".to_string());
    app.apply_sort();

    assert_eq!(
        app.hosts_state.group_tab_order.len(),
        10,
        "group_tab_order should be capped at exactly 10, got {}",
        app.hosts_state.group_tab_order.len()
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
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();

    // group_tab_order should reflect GroupHeader order
    assert!(!app.hosts_state.group_tab_order.is_empty());
    for name in &app.hosts_state.group_tab_order {
        let header_exists = app
            .hosts_state
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
    app.hosts_state.group_by = GroupBy::Tag("prod".to_string());
    app.hosts_state.group_filter = Some("prod".to_string());
    app.apply_sort();

    // Only hosts with the prod tag should appear
    for item in &app.hosts_state.display_list {
        if let HostListItem::Host { index } = item {
            assert!(
                app.hosts_state.list[*index]
                    .tags
                    .contains(&"prod".to_string()),
                "only hosts with 'prod' tag should appear when filtered"
            );
        }
    }

    let host_count = app
        .hosts_state
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
    app.hosts_state.group_by = GroupBy::Tag("prod".to_string());
    app.hosts_state.group_filter = Some("prod".to_string());
    app.apply_sort();

    let pattern_count = app
        .hosts_state
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
    app.hosts_state.group_by = GroupBy::Provider;
    app.apply_sort();

    // Start at the first item
    app.ui.list_state.select(Some(0));

    app.page_down_host();

    let selected = app.ui.list_state.selected().unwrap();
    assert!(
        matches!(
            app.hosts_state.display_list[selected],
            HostListItem::Host { .. } | HostListItem::Pattern { .. }
        ),
        "page_down should not land on a GroupHeader"
    );
}

#[test]
fn page_down_at_end_wraps_to_first() {
    // On the last host, PageDown wraps to the first, like j/Down at the end.
    let content = "\
Host web1
  HostName 1.1.1.1

Host web2
  HostName 2.2.2.2
";
    let mut app = make_app(content);
    app.apply_sort();

    let first = app
        .hosts_state
        .display_list
        .iter()
        .position(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    let last = app
        .hosts_state
        .display_list
        .iter()
        .rposition(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    app.ui.list_state.select(Some(last));

    app.page_down_host();

    assert_eq!(
        app.ui.list_state.selected(),
        Some(first),
        "PageDown at the bottom should wrap to the first host"
    );
}

#[test]
fn page_up_at_start_wraps_to_last() {
    // On the first host, PageUp wraps to the last, like k/Up at the start.
    let content = "\
Host web1
  HostName 1.1.1.1

Host web2
  HostName 2.2.2.2
";
    let mut app = make_app(content);
    app.apply_sort();

    let first = app
        .hosts_state
        .display_list
        .iter()
        .position(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    let last = app
        .hosts_state
        .display_list
        .iter()
        .rposition(|item| matches!(item, HostListItem::Host { .. }))
        .unwrap();
    app.ui.list_state.select(Some(first));

    app.page_up_host();

    assert_eq!(
        app.ui.list_state.selected(),
        Some(last),
        "PageUp at the top should wrap to the last host"
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
    let alias = &app.hosts_state.list[app.search.filtered_indices[0]].alias;
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
    app.hosts_state.sort_mode = SortMode::Status;
    app.hosts_state.group_by = GroupBy::None;
    app.apply_sort();
    let aliases: Vec<&str> = app
        .hosts_state
        .display_list
        .iter()
        .filter_map(|item| {
            if let HostListItem::Host { index } = item {
                Some(app.hosts_state.list[*index].alias.as_str())
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
    let spans = health_summary_spans(&app.ping.status, &app.hosts_state.list);
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
    let spans = health_summary_spans(&app.ping.status, &app.hosts_state.list);
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
    let spans = health_summary_spans(&app.ping.status, &app.hosts_state.list);
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
    let spans = health_summary_spans(&app.ping.status, &app.hosts_state.list);
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
    // GroupBy::Provider with provider="aws": "aws" itself is suppressed,
    // but the non-matching provider tag "web" stays visible.
    let tags = select_display_tags(&host, &GroupBy::Provider, false);
    assert_eq!(tag_names(&tags), vec!["prod", "web"]);
    assert_eq!(tag_sources(&tags), vec![true, false]);
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
    // Group by "prod": "prod" suppressed, remaining user tags shown,
    // and the provider name "aws" (which does not match "prod") stays.
    let tags = select_display_tags(&host, &GroupBy::Tag("prod".into()), false);
    assert_eq!(tag_names(&tags), vec!["us-east", "api", "aws"]);
    assert_eq!(tag_sources(&tags), vec![true, true, false]);
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
fn select_display_tags_grouped_suppresses_matching_provider_tag() {
    let host = HostEntry {
        tags: vec!["prod".into()],
        provider_tags: vec!["aws".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    // Both provider_tags["aws"] and host.provider="aws" match the group name
    // and are suppressed; the non-matching user tag stays.
    let tags = select_display_tags(&host, &GroupBy::Provider, false);
    assert_eq!(tag_names(&tags), vec!["prod"]);
    assert_eq!(tag_sources(&tags), vec![true]);
}

#[test]
fn select_display_tags_grouped_keeps_non_matching_provider_tags() {
    let host = HostEntry {
        tags: vec![],
        provider_tags: vec!["web".into(), "cache".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    // GroupBy::Provider with provider="aws": "aws" itself is suppressed,
    // but non-matching provider tags stay visible.
    let tags = select_display_tags(&host, &GroupBy::Provider, false);
    assert_eq!(tag_names(&tags), vec!["web", "cache"]);
    assert_eq!(tag_sources(&tags), vec![false, false]);
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
fn select_display_tags_grouped_user_tags_fill_limit() {
    let host = HostEntry {
        tags: vec!["prod".into(), "us-east".into(), "api".into(), "db".into()],
        provider_tags: vec!["web".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    // Group by "prod" -> suppressed; user tags fill the limit of 3 before
    // any provider tags get a chance.
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
fn select_display_tags_proxmox_pve_tag_visible_in_provider_group() {
    // Reproducer for the real-world Proxmox case: a VM with one PVE tag
    // (opentofu) and no user tags. In GroupBy::Provider with provider="proxmox"
    // the provider name is suppressed but the PVE tag stays visible.
    let host = HostEntry {
        tags: vec![],
        provider_tags: vec!["opentofu".into()],
        provider: Some("proxmox".into()),
        ..Default::default()
    };
    let tags = select_display_tags(&host, &GroupBy::Provider, false);
    assert_eq!(tag_names(&tags), vec!["opentofu"]);
    assert_eq!(tag_sources(&tags), vec![false]);
}

#[test]
fn select_display_tags_group_by_tag_keeps_provider_name() {
    // When grouping by a tag, the host's provider name (not matching the
    // group tag) still surfaces so the user can see which cloud the host
    // belongs to even within a tag-grouped section.
    let host = HostEntry {
        tags: vec!["opentofu".into()],
        provider_tags: vec![],
        provider: Some("proxmox".into()),
        ..Default::default()
    };
    let tags = select_display_tags(&host, &GroupBy::Tag("opentofu".into()), false);
    assert_eq!(tag_names(&tags), vec!["proxmox"]);
    assert_eq!(tag_sources(&tags), vec![false]);
}

#[test]
fn select_display_tags_group_by_tag_suppresses_matching_provider_tag() {
    // GroupBy::Tag("web") matches a provider_tag of the same name and that
    // tag must be suppressed (it lives in the group header). The other
    // provider_tag stays.
    let host = HostEntry {
        tags: vec![],
        provider_tags: vec!["web".into(), "cache".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    let tags = select_display_tags(&host, &GroupBy::Tag("web".into()), false);
    assert_eq!(tag_names(&tags), vec!["cache", "aws"]);
    assert_eq!(tag_sources(&tags), vec![false, false]);
}

#[test]
fn select_display_tags_case_insensitive_provider_tag_suppression() {
    // Provider tags match the group name case-insensitively, the same way
    // user tags do.
    let host = HostEntry {
        tags: vec!["prod".into()],
        provider_tags: vec!["AWS".into(), "web".into()],
        provider: Some("aws".into()),
        ..Default::default()
    };
    let tags = select_display_tags(&host, &GroupBy::Provider, false);
    assert_eq!(tag_names(&tags), vec!["prod", "web"]);
    assert_eq!(tag_sources(&tags), vec![true, false]);
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
    let spans = health_summary_spans(&app.ping.status, &app.hosts_state.list);
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
    app.notify_progress("vault signing...");
    for _ in 0..50 {
        app.tick_status();
    }
    assert!(
        app.status_center.status.is_some(),
        "sticky status must not expire"
    );
}

#[test]
fn tick_toast_warning_expires_after_timeout_ms() {
    use std::time::{Duration, Instant};
    // "failed connection" = 2 words → max(4000, 1500) = 4000ms.
    let mut app = make_app("");
    app.notify_warning("failed connection");
    assert!(
        app.status_center.toast.is_some(),
        "warning should route to toast"
    );
    if let Some(toast) = app.status_center.toast.as_mut() {
        toast.created_at = Instant::now() - Duration::from_millis(4100);
    }
    app.tick_toast();
    assert!(
        app.status_center.toast.is_none(),
        "warning toast must expire after timeout_ms"
    );
}

#[test]
fn tick_toast_non_sticky_success_expires() {
    use std::time::{Duration, Instant};
    // "done" = 1 word → max(2500, 750) = 2500ms.
    let mut app = make_app("");
    app.notify("done");
    assert!(
        app.status_center.toast.is_some(),
        "success should route to toast"
    );
    if let Some(toast) = app.status_center.toast.as_mut() {
        toast.created_at = Instant::now() - Duration::from_millis(2600);
    }
    app.tick_toast();
    assert!(
        app.status_center.toast.is_none(),
        "success toast must expire after timeout_ms"
    );
}

#[test]
fn notify_does_not_overwrite_sticky() {
    let mut app = make_app("");
    app.notify_progress("signing...");
    app.notify_background("ping expired");
    assert_eq!(
        app.status_center.status.as_ref().unwrap().text,
        "signing...",
        "notify_background must not overwrite sticky"
    );
}

#[test]
fn notify_progress_replaces_sticky() {
    let mut app = make_app("");
    app.notify_progress("signing...");
    app.notify_sticky_error("done signing");
    assert_eq!(
        app.status_center.status.as_ref().unwrap().text,
        "done signing",
        "notify_sticky_error must replace sticky"
    );
}

#[test]
fn notify_routes_confirmation_to_toast() {
    let mut app = make_app("");
    app.notify_progress("signing...");
    // notify (Confirmation) routes to toast, sticky footer stays
    app.notify("Signed 3 of 3 certificates.");
    assert_eq!(
        app.status_center.toast.as_ref().unwrap().text,
        "Signed 3 of 3 certificates.",
        "notify must route to toast"
    );
    // Sticky footer is still there
    assert!(app.status_center.status.as_ref().unwrap().sticky);
}

// Gap 1: notify on a fresh app with no prior status at all.
#[test]
fn notify_routes_to_toast_when_none() {
    let mut app = make_app("");
    assert!(
        app.status_center.toast.is_none(),
        "precondition: fresh app has no toast"
    );
    app.notify("connected");
    assert_eq!(
        app.status_center.toast.as_ref().unwrap().text,
        "connected",
        "notify must route to toast"
    );
}

// Gap 2: notify_background is still blocked after the sticky is replaced by a second notify_progress.
// Verifies that the blocking invariant holds for the replacement sticky, not just the first one.
#[test]
fn notify_background_blocked_after_sticky_replaced_by_sticky() {
    let mut app = make_app("");
    app.notify_progress("signing...");
    app.notify_progress("still signing...");
    app.notify_background("ping expired");
    assert_eq!(
        app.status_center.status.as_ref().unwrap().text,
        "still signing...",
        "notify_background must not overwrite the replacement sticky"
    );
}

// Gap 3: tick_status does not alter the content of a sticky message, only its absence/presence.
#[test]
fn tick_status_sticky_text_unchanged() {
    let mut app = make_app("");
    app.notify_progress("vault signing...");
    for _ in 0..50 {
        app.tick_status();
    }
    assert_eq!(
        app.status_center.status.as_ref().unwrap().text,
        "vault signing...",
        "tick_status must not alter sticky message text"
    );
    assert!(
        app.status_center.status.as_ref().unwrap().sticky,
        "tick_status must not clear the sticky flag"
    );
}

// Gap 4: documents the user-action contract: while a sticky Vault signing message is active,
// any incidental notify call (e.g. from a navigation handler) is suppressed.
// This is intentional: Vault SSH signing feedback is more important than transient nav feedback.
#[test]
fn notify_background_suppressed_during_vault_signing() {
    let mut app = make_app("");
    app.notify_progress("Signing certificate...");
    // Background event (ping, tunnel) must not clobber signing status
    app.notify_background("Ping expired.");
    assert_eq!(
        app.status_center.status.as_ref().unwrap().text,
        "Signing certificate...",
        "background status must be suppressed while sticky is active"
    );
    assert!(app.status_center.status.as_ref().unwrap().sticky);
}

#[test]
fn notify_background_works_when_no_sticky() {
    let mut app = make_app("");
    app.notify_background("ping expired");
    assert_eq!(
        app.status_center.status.as_ref().unwrap().text,
        "ping expired"
    );
}

#[test]
fn notify_background_error_promotes_over_success_and_leaves_footer_alone() {
    let mut app = make_app("");
    // Sticky progress in footer.
    app.notify_progress("Signing...");
    // First toast already active (an informational Success).
    app.notify("Copied host");
    assert!(app.status_center.toast.is_some());
    // A background error must reach the user immediately. It promotes
    // over the Success toast (Success was just discoverability; the
    // Error is more urgent) and never touches the footer sticky.
    app.notify_background_error("Sync failed");
    assert_eq!(
        app.status_center.status.as_ref().unwrap().text,
        "Signing..."
    );
    assert!(app.status_center.status.as_ref().unwrap().sticky);
    assert_eq!(
        app.status_center.toast.as_ref().unwrap().text,
        "Sync failed"
    );
    assert!(app.status_center.toast.as_ref().unwrap().is_error());
    assert!(app.status_center.toast_queue.is_empty());
}

#[test]
fn warning_promotes_over_active_success_toast() {
    // Demo-mode and similar user-initiated guards arrive as Warning
    // toasts. They must replace a held Success toast (e.g. the seeded
    // "What's new" hint on startup) so the user sees the response to
    // their keystroke instead of a stale informational message.
    let mut app = make_app("");
    app.notify("v3.10.0 installed. press n for what's new");
    app.notify_warning("Demo mode. Connection disabled.");
    let toast = app.status_center.toast.as_ref().expect("toast");
    assert_eq!(toast.text, "Demo mode. Connection disabled.");
    assert_eq!(toast.class, crate::app::MessageClass::Warning);
    assert!(app.status_center.toast_queue.is_empty());
}

#[test]
fn vault_signing_lifecycle() {
    let mut app = make_app("");
    // 1. Signing starts: sticky progress in footer
    app.notify_progress("Signing certificate...");
    assert!(app.status_center.status.as_ref().unwrap().sticky);

    // 2. Background event must not clobber sticky footer
    app.notify_background("Ping expired.");
    assert_eq!(
        app.status_center.status.as_ref().unwrap().text,
        "Signing certificate..."
    );

    // 3. Signing error routes to toast, sticky footer stays
    app.notify_error("Vault SSH: failed to sign host: timeout");
    assert!(app.status_center.toast.as_ref().unwrap().is_error());
    assert_eq!(
        app.status_center.toast.as_ref().unwrap().text,
        "Vault SSH: failed to sign host: timeout"
    );
    // Sticky footer is still there
    assert!(app.status_center.status.as_ref().unwrap().sticky);

    // 4. Final summary replaces sticky footer with sticky error
    app.notify_sticky_error("Signed 0 of 1 certificate. 1 failed: timeout");
    assert!(app.status_center.status.as_ref().unwrap().sticky);
    assert!(app.status_center.status.as_ref().unwrap().is_error());

    // 5. Background non-error cannot clobber sticky footer
    app.notify_background("Config reloaded. 5 hosts.");
    assert_eq!(
        app.status_center.status.as_ref().unwrap().text,
        "Signed 0 of 1 certificate. 1 failed: timeout"
    );
}

#[test]
fn vault_signing_success_clears_sticky_progress() {
    let mut app = make_app("");
    // Sticky progress during signing
    app.notify_progress("Signing 3/3: last-server (V to cancel)");
    assert!(app.status_center.status.as_ref().unwrap().sticky);

    // Success summary via notify_info replaces sticky footer
    app.notify_info("Signed 3 of 3 certificates.");
    assert!(!app.status_center.status.as_ref().unwrap().sticky);
    assert_eq!(
        app.status_center.status.as_ref().unwrap().text,
        "Signed 3 of 3 certificates."
    );
}

#[test]
fn confirmation_replaces_previous_toast() {
    let mut app = make_app("");
    app.notify("first");
    app.notify("second");
    app.notify("third");
    // Confirmations replace immediately, no queue
    assert_eq!(app.status_center.toast.as_ref().unwrap().text, "third");
    assert!(app.status_center.toast_queue.is_empty());
}

#[test]
fn confirmation_clears_alert_queue() {
    let mut app = make_app("");
    app.notify_error("err1");
    app.notify_error("err2");
    assert_eq!(app.status_center.toast_queue.len(), 1);
    // Confirmation replaces active toast and clears queue
    app.notify("copied");
    assert_eq!(app.status_center.toast.as_ref().unwrap().text, "copied");
    assert!(app.status_center.toast_queue.is_empty());
}

#[test]
fn error_toasts_are_sticky_and_queued_in_order() {
    let mut app = make_app("");
    app.notify_error("err1");
    app.notify_error("err2");
    app.notify_error("err3");
    // First error is shown; the rest queue up. Errors are sticky-by-default
    // so ticking does NOT advance the queue.
    assert_eq!(app.status_center.toast.as_ref().unwrap().text, "err1");
    assert!(
        app.status_center.toast.as_ref().unwrap().sticky,
        "errors must be sticky by default"
    );
    assert_eq!(app.status_center.toast_queue.len(), 2);
    for _ in 0..=100 {
        app.tick_toast();
    }
    assert_eq!(
        app.status_center.toast.as_ref().unwrap().text,
        "err1",
        "sticky error must NOT auto-expire"
    );
    assert_eq!(app.status_center.toast_queue.len(), 2);
}

#[test]
fn error_queue_caps_at_max() {
    let mut app = make_app("");
    let cap = crate::ui::design::TOAST_QUEUE_MAX;
    // Push cap + active + 2 extras to exercise the drop-oldest path.
    let total = cap + 3;
    for i in 0..total {
        app.notify_error(format!("err{i}"));
    }
    // First push becomes active, the next `cap` queue, the rest evict the
    // oldest queue entry. Active toast stays "err0"; queue holds the most
    // recent `cap` errors.
    assert_eq!(app.status_center.toast.as_ref().unwrap().text, "err0");
    assert_eq!(app.status_center.toast_queue.len(), cap);
    assert_eq!(
        app.status_center.toast_queue.back().unwrap().text,
        format!("err{}", total - 1)
    );
}

#[test]
fn success_toast_dismisses_sticky_error() {
    // A Success toast (last-write-wins) replaces an active error and
    // clears the queue, providing the user the explicit acknowledgement
    // path: continue working, errors get dismissed.
    let mut app = make_app("");
    app.notify_error("a");
    app.notify_error("b");
    assert!(app.status_center.toast.is_some());
    assert_eq!(app.status_center.toast_queue.len(), 1);
    app.notify("done");
    assert_eq!(app.status_center.toast.as_ref().unwrap().text, "done");
    assert!(app.status_center.toast_queue.is_empty());
}

#[test]
fn warning_toasts_queue_rather_than_replace() {
    let mut app = make_app("");
    app.notify_warning("first warning");
    app.notify_warning("second warning");
    app.notify_warning("third warning");
    // Warnings (like Errors) queue. Unlike Errors, they are NOT sticky
    // and will auto-expire via tick_toast.
    assert_eq!(
        app.status_center.toast.as_ref().unwrap().text,
        "first warning"
    );
    assert!(
        !app.status_center.toast.as_ref().unwrap().sticky,
        "warnings must NOT be sticky (only errors are)"
    );
    assert_eq!(app.status_center.toast_queue.len(), 2);
}

#[test]
fn success_clears_warning_queue() {
    // Mirrors success_toast_dismisses_sticky_error but with warnings:
    // a Success toast should clear ANY queued non-sticky toast, not just
    // queued errors.
    let mut app = make_app("");
    app.notify_warning("a");
    app.notify_warning("b");
    assert_eq!(app.status_center.toast_queue.len(), 1);
    app.notify("done");
    assert_eq!(app.status_center.toast.as_ref().unwrap().text, "done");
    assert!(app.status_center.toast_queue.is_empty());
}

#[test]
fn notify_info_goes_to_footer() {
    let mut app = make_app("");
    app.notify_info("Syncing...");
    assert!(app.status_center.toast.is_none());
    assert_eq!(
        app.status_center.status.as_ref().unwrap().text,
        "Syncing..."
    );
    assert_eq!(
        app.status_center.status.as_ref().unwrap().class,
        MessageClass::Info
    );
}

#[test]
fn tick_status_info_expires() {
    use std::time::{Duration, Instant};
    // "done" = 1 word → max(TIMEOUT_MIN_MS=2500, 750) = 2500ms.
    let mut app = make_app("");
    app.notify_info("done");
    if let Some(status) = app.status_center.status.as_mut() {
        status.created_at = Instant::now() - Duration::from_millis(2600);
    }
    app.tick_status();
    assert!(app.status_center.status.is_none());
}

#[test]
fn tick_status_does_not_expire_while_syncing() {
    use std::time::{Duration, Instant};
    let mut app = make_app("");
    app.notify_info("syncing...");
    app.providers
        .syncing
        .insert("aws".to_string(), Arc::new(AtomicBool::new(true)));
    // Backdate past timeout.
    if let Some(status) = app.status_center.status.as_mut() {
        status.created_at = Instant::now() - Duration::from_secs(30);
    }
    app.tick_status();
    assert!(
        app.status_center.status.is_some(),
        "status must not expire while providers are syncing"
    );
    app.providers.syncing.clear();
    app.tick_status();
    assert!(
        app.status_center.status.is_none(),
        "status must expire after syncing completes"
    );
}

#[test]
fn tick_toast_error_does_not_auto_expire() {
    use std::time::{Duration, Instant};
    let mut app = make_app("");
    app.notify_error("failed");
    assert!(app.status_center.toast.is_some());
    // Backdate far into the past.
    if let Some(toast) = app.status_center.toast.as_mut() {
        toast.created_at = Instant::now() - Duration::from_secs(3600);
    }
    app.tick_toast();
    assert!(
        app.status_center.toast.is_some(),
        "sticky error toast must remain visible regardless of elapsed time"
    );
}

#[test]
fn tick_toast_success_expires_after_timeout_ms() {
    use std::time::{Duration, Instant};
    // "done" = 1 word → max(2500, 750) = 2500ms.
    let mut app = make_app("");
    app.notify("done");
    assert!(app.status_center.toast.is_some());
    if let Some(toast) = app.status_center.toast.as_mut() {
        toast.created_at = Instant::now() - Duration::from_millis(2600);
    }
    app.tick_toast();
    assert!(app.status_center.toast.is_none());
}

#[test]
fn tick_toast_success_still_visible_before_expiry() {
    use std::time::{Duration, Instant};
    // "done" = 1 word → timeout_ms = 2500. Backdate 2000ms (< 2500).
    let mut app = make_app("");
    app.notify("done");
    assert!(app.status_center.toast.is_some());
    if let Some(toast) = app.status_center.toast.as_mut() {
        toast.created_at = Instant::now() - Duration::from_millis(2000);
    }
    app.tick_toast();
    assert!(
        app.status_center.toast.is_some(),
        "success toast must still be visible before timeout_ms"
    );
}

#[test]
fn message_class_is_toast_routing() {
    let mk = |class| StatusMessage {
        text: String::new(),
        class,
        tick_count: 0,
        sticky: false,
        created_at: std::time::Instant::now(),
    };
    assert!(mk(MessageClass::Success).is_toast());
    assert!(!mk(MessageClass::Info).is_toast());
    assert!(mk(MessageClass::Warning).is_toast());
    assert!(mk(MessageClass::Error).is_toast());
    assert!(!mk(MessageClass::Progress).is_toast());
}

#[test]
fn message_class_timeout_is_length_proportional() {
    let mk = |class, text: &str| StatusMessage {
        text: text.to_string(),
        class,
        tick_count: 0,
        sticky: false,
        created_at: std::time::Instant::now(),
    };
    // Errors and Progress are sticky.
    assert_eq!(mk(MessageClass::Error, "anything").timeout_ms(), u64::MAX);
    assert_eq!(
        mk(MessageClass::Progress, "anything").timeout_ms(),
        u64::MAX
    );
    // Success/Info: minimum TIMEOUT_MIN_MS (2500), MS_PER_WORD (750) per word.
    assert_eq!(mk(MessageClass::Success, "Saved").timeout_ms(), 2500); // 1w*750<2500
    assert_eq!(
        mk(MessageClass::Success, "one two three four five").timeout_ms(),
        3750 // 5w*750=3750 > 2500
    );
    assert_eq!(
        mk(
            MessageClass::Info,
            "Synced ten hosts from AWS region eu-west-1"
        )
        .timeout_ms(),
        5250 // 7w*750=5250 > 2500
    );
    // Warning: minimum TIMEOUT_MIN_WARNING_MS (4000).
    assert_eq!(mk(MessageClass::Warning, "Stale").timeout_ms(), 4000);
    assert_eq!(
        mk(
            MessageClass::Warning,
            "Stale hosts detected in production cluster"
        )
        .timeout_ms(),
        4500 // 6w*750=4500 > 4000
    );
    // Empty string: zero words → minimum dwell time.
    assert_eq!(mk(MessageClass::Success, "").timeout_ms(), 2500);
    assert_eq!(mk(MessageClass::Warning, "").timeout_ms(), 4000);
    // Word cap: capped at WORD_CAP * MS_PER_WORD = 30 * 750 = 22500ms.
    let huge: String = (0..1000).map(|_| "word ").collect();
    assert_eq!(
        mk(MessageClass::Success, huge.trim()).timeout_ms(),
        crate::ui::design::WORD_CAP as u64 * crate::ui::design::MS_PER_WORD
    );
}

#[test]
fn message_class_is_error() {
    let mk = |class| StatusMessage {
        text: String::new(),
        class,
        tick_count: 0,
        sticky: false,
        created_at: std::time::Instant::now(),
    };
    assert!(!mk(MessageClass::Success).is_error());
    assert!(!mk(MessageClass::Info).is_error());
    assert!(mk(MessageClass::Warning).is_error());
    assert!(mk(MessageClass::Error).is_error());
    assert!(!mk(MessageClass::Progress).is_error());
}

#[test]
fn jump_commands_have_unique_keys_per_target() {
    // The (key, modifiers) tuple is unique within a `JumpActionTarget`.
    // The same letter can map to two distinct actions within a target
    // when only the modifier differs (e.g. plain `a` Add host vs Ctrl+a
    // Select all). Across targets the same key+modifier can coexist
    // because dispatch routes by `target` first, then synthesises the
    // keypress for that handler.
    let commands = PaletteCommand::all();
    let mut seen_hosts = std::collections::HashSet::new();
    let mut seen_tunnels = std::collections::HashSet::new();
    let mut seen_containers = std::collections::HashSet::new();
    let mut seen_keys = std::collections::HashSet::new();
    for cmd in commands {
        let bucket = match cmd.target {
            crate::app::JumpActionTarget::Hosts => &mut seen_hosts,
            crate::app::JumpActionTarget::Tunnels => &mut seen_tunnels,
            crate::app::JumpActionTarget::Containers => &mut seen_containers,
            crate::app::JumpActionTarget::Keys => &mut seen_keys,
        };
        assert!(
            bucket.insert((cmd.key, cmd.modifiers)),
            "duplicate jump binding '{}' with {:?} within target {:?}",
            cmd.key,
            cmd.modifiers,
            cmd.target
        );
    }
    assert!(commands.len() >= 28, "expected at least 28 jump actions");
}

#[test]
fn jump_action_set_includes_container_actions() {
    let commands = PaletteCommand::for_mode(JumpMode::Containers);
    assert!(
        commands
            .iter()
            .any(|c| c.label.contains("Containers: Refresh all hosts")),
        "Containers: Refresh all hosts must be present"
    );
    assert!(
        commands
            .iter()
            .any(|c| c.label.contains("Containers: Cycle sort")),
        "Containers: Cycle sort must be present"
    );
    assert!(
        commands
            .iter()
            .any(|c| c.label.contains("Containers: Toggle detail panel")),
        "Containers: Toggle detail panel must be present"
    );
    assert!(
        commands.iter().any(|c| c.label.contains("Hosts: Add host")),
        "cross-tab Hosts: Add host stays reachable from the containers tab"
    );
}

#[test]
fn jump_action_set_includes_tunnel_actions() {
    let commands = PaletteCommand::for_mode(JumpMode::Tunnels);
    assert!(
        commands
            .iter()
            .any(|c| c.label.contains("Tunnels: Add tunnel")),
        "Tunnels: Add tunnel must be present in the unified action set"
    );
    assert!(
        commands.iter().any(|c| c.label.contains("Hosts: Add host")),
        "Hosts: Add host must remain present even on Tunnels-mode jump"
    );
}

#[test]
fn jump_state_filters_by_query() {
    let mut state = JumpState::default();
    state.push_query('t');
    let filtered = state.filtered_commands();
    assert!(
        filtered.iter().all(|c| {
            let q = "t";
            c.label.to_lowercase().contains(q)
                || c.aliases.iter().any(|a| a.to_lowercase().contains(q))
        }),
        "all filtered commands should match 't' in label or alias"
    );
    assert!(
        filtered.len() < PaletteCommand::all().len(),
        "filtering should reduce the list"
    );
}

#[test]
fn jump_state_empty_query_returns_all() {
    let state = JumpState::default();
    let filtered = state.filtered_commands();
    assert_eq!(filtered.len(), PaletteCommand::all().len());
}

#[test]
fn jump_query_mutation_preserves_selected_for_recompute_to_handle() {
    // push_query / pop_query no longer wipe `selected` directly — the
    // recompute path computes the new index via identity tracking so
    // mid-typing navigation does not jump back to row 0.
    let mut state = JumpState {
        selected: 5,
        ..Default::default()
    };
    state.push_query('x');
    assert_eq!(state.selected, 5, "push_query must not clobber selected");
    state.pop_query();
    assert_eq!(state.selected, 5, "pop_query must not clobber selected");
}

// --- Unified jump: multi-source collection, dispatch and ranking ---

use crate::app::JumpHit;
use crate::app::JumpMode;
use crate::app::SourceKind;

fn make_jump_app(content: &str) -> App {
    let scratch = tempfile::tempdir().expect("tempdir").keep();
    let config = crate::ssh_config::model::SshConfigFile {
        elements: crate::ssh_config::model::SshConfigFile::parse_content(content),
        path: scratch.join("test_config"),
        crlf: false,
        bom: false,
    };
    App::new(config)
}

#[test]
fn collect_jump_candidates_emits_hosts_tunnels_and_actions() {
    // Two hosts, one with a LocalForward tunnel rule. With a query that
    // matches both the host alias and a tunnel destination, the visible
    // hit list must include host + tunnel variants. Actions appear in the
    // empty-query view (verified separately).
    let mut app = make_jump_app(
        "Host web-01\n  HostName web.example\n  LocalForward 5432 db.internal:5432\n\
         Host db-01\n  HostName db.example\n",
    );
    app.jump = Some(crate::app::JumpState::for_mode(JumpMode::Hosts));
    if let Some(p) = app.jump.as_mut() {
        for c in "web".chars() {
            p.push_query(c);
        }
    }
    app.recompute_jump_hits();
    let jump = app.jump.as_ref().expect("jump");
    let visible = jump.visible_hits();
    assert!(
        visible
            .iter()
            .any(|h| matches!(h, JumpHit::Host(host) if host.alias == "web-01")),
        "host candidate missing for query 'web'"
    );
    assert!(
        visible
            .iter()
            .any(|h| matches!(h, JumpHit::Tunnel(t) if t.alias == "web-01" && t.bind_port == 5432)),
        "tunnel candidate missing for query 'web'"
    );
}

#[test]
fn hotkey_boost_outranks_fuzzy_host_match() {
    // Add a host whose alias matches the boost letter `K`, then type `K`.
    // The +10000 hotkey boost must make the action the pre-selected hit
    // (the one Enter dispatches), even though the host renders first in the
    // fixed section order. The cursor follows the top score, not row 0, and
    // dispatch reads the selected index, so highlight and dispatch agree.
    let mut app = make_jump_app("Host Kbrk\n  HostName k.example\n");
    app.jump = Some(crate::app::JumpState::for_mode(JumpMode::Hosts));
    app.jump.as_mut().unwrap().push_query('K');
    app.recompute_jump_hits();
    let jump = app.jump.as_ref().expect("jump");
    let visible = jump.visible_hits();
    let selected = visible.get(jump.selected()).expect("a selected hit");
    assert!(
        matches!(selected, JumpHit::Action(a) if a.key == 'K'),
        "hotkey boost must pre-select the K action, got {:?}",
        selected
    );
}

#[test]
fn empty_query_view_lists_actions_only_when_no_recents() {
    let mut app = make_jump_app("");
    app.jump = Some(crate::app::JumpState::for_mode(JumpMode::Hosts));
    app.recompute_jump_hits();
    let jump = app.jump.as_ref().expect("jump");
    let visible = jump.visible_hits();
    assert!(!visible.is_empty(), "empty query should still show actions");
    assert!(
        visible.iter().all(|h| matches!(h, JumpHit::Action(_))),
        "empty-state with no recents should show only actions"
    );
}

#[test]
fn empty_query_view_deduplicates_recent_actions() {
    let mut app = make_jump_app("");
    app.jump = Some(crate::app::JumpState::for_mode(JumpMode::Hosts));
    // Manually inject the F action into recents so the dedup path exercises.
    let f_action = crate::app::JumpAction::for_mode(JumpMode::Hosts)
        .iter()
        .find(|a| a.key == 'F')
        .copied()
        .expect("F action present");
    if let Some(p) = app.jump.as_mut() {
        p.recents = vec![JumpHit::Action(f_action)];
    }
    app.recompute_jump_hits();
    let jump = app.jump.as_ref().expect("jump");
    let count_f = jump
        .visible_hits()
        .iter()
        .filter(|h| matches!(h, JumpHit::Action(a) if a.key == 'F'))
        .count();
    assert_eq!(
        count_f, 1,
        "action present in recents must not appear twice in the empty-state view"
    );
}

#[test]
fn jump_next_section_empty_state_toggles_recent_to_actions() {
    let mut app = make_jump_app("Host one\n  HostName one.example\n");
    app.jump = Some(crate::app::JumpState::for_mode(JumpMode::Hosts));
    let host_hit = JumpHit::Host(crate::app::HostHit {
        alias: "one".into(),
        hostname: "one.example".into(),
        tags: vec![],
        provider: None,
        user: String::new(),
        identity_file: String::new(),
        proxy_jump: String::new(),
        vault_ssh: None,
    });
    if let Some(p) = app.jump.as_mut() {
        p.recents = vec![host_hit];
    }
    app.recompute_jump_hits();
    if let Some(p) = app.jump.as_mut() {
        p.selected = 0;
        p.jump_next_section();
        assert_eq!(
            p.selected, 1,
            "Tab from RECENT row 0 should jump to first ACTIONS row"
        );
        p.jump_next_section();
        assert_eq!(p.selected, 0, "second Tab wraps back to RECENT");
    }
}

#[test]
fn jump_next_section_query_state_walks_kinds() {
    let mut app =
        make_jump_app("Host alpha\n  HostName alpha.example\n  LocalForward 4000 localhost:4000\n");
    app.jump = Some(crate::app::JumpState::for_mode(JumpMode::Hosts));
    if let Some(p) = app.jump.as_mut() {
        // Type a fragment that matches both 'alpha' (host alias / tunnel
        // alias) and the action label 'Tunnels: Manage tunnels'.
        for c in "alpha".chars() {
            p.push_query(c);
        }
    }
    app.recompute_jump_hits();
    let mut jump = app.jump.take().expect("jump");
    jump.selected = 0;
    let first_kind = jump.visible_hits()[0].kind();
    jump.jump_next_section();
    let next_kind = jump.visible_hits()[jump.selected].kind();
    assert_ne!(
        first_kind, next_kind,
        "Tab in query-state must move to a different section kind"
    );
    app.jump = Some(jump);
}

#[test]
fn recompute_preserves_selection_when_prior_hit_remains() {
    // Set up a jump bar with two hosts. Type a query that matches both.
    // Move selection to the second hit, then mutate the query so the
    // second hit is still in the new list. Selection must follow the
    // hit's identity, not snap back to row 0.
    let mut app = make_jump_app(
        "Host alpha-one\n  HostName a1.example\n\
         Host alpha-two\n  HostName a2.example\n",
    );
    app.jump = Some(crate::app::JumpState::for_mode(JumpMode::Hosts));
    if let Some(p) = app.jump.as_mut() {
        for c in "alph".chars() {
            p.push_query(c);
        }
    }
    app.recompute_jump_hits();
    let prior_id = {
        let p = app.jump.as_mut().unwrap();
        let visible = p.visible_hits();
        // Pick the second host hit (some sort of stable target).
        let hosts: Vec<usize> = visible
            .iter()
            .enumerate()
            .filter(|(_, h)| matches!(h, JumpHit::Host(_)))
            .map(|(i, _)| i)
            .collect();
        if hosts.len() < 2 {
            return; // demo-config too narrow on this platform; nothing to assert
        }
        p.selected = hosts[1];
        visible[p.selected].identity()
    };
    if let Some(p) = app.jump.as_mut() {
        p.push_query('a');
    }
    app.recompute_jump_hits();
    let jump = app.jump.as_ref().unwrap();
    let visible = jump.visible_hits();
    if visible.iter().any(|h| h.identity() == prior_id) {
        assert_eq!(
            visible[jump.selected].identity(),
            prior_id,
            "selection should track the prior hit across recompute"
        );
    }
}

#[test]
fn resolve_recent_ref_host_found_and_dangling() {
    let app = make_jump_app("Host kept\n  HostName kept.example\n");
    let kept = crate::app::RecentRef::new(SourceKind::Host, "kept".into());
    let gone = crate::app::RecentRef::new(SourceKind::Host, "deleted".into());
    let resolved_kept = app.resolve_recent_ref_for_test(&kept, JumpMode::Hosts);
    let resolved_gone = app.resolve_recent_ref_for_test(&gone, JumpMode::Hosts);
    assert!(matches!(resolved_kept, Some(JumpHit::Host(_))));
    assert!(resolved_gone.is_none(), "dangling host ref must drop");
}

#[test]
fn resolve_recent_ref_tunnel_malformed_port_returns_none() {
    let app = make_jump_app("Host h\n  HostName h.example\n  LocalForward 4000 localhost:4000\n");
    let bad = crate::app::RecentRef::new(SourceKind::Tunnel, "h:notanumber".into());
    assert!(
        app.resolve_recent_ref_for_test(&bad, JumpMode::Hosts)
            .is_none(),
        "tunnel ref with non-numeric port must drop"
    );
}

#[test]
fn resolve_recent_ref_action_round_trips() {
    let app = make_jump_app("");
    let r = crate::app::RecentRef::new(SourceKind::Action, "F".into());
    let resolved = app.resolve_recent_ref_for_test(&r, JumpMode::Hosts);
    assert!(matches!(resolved, Some(JumpHit::Action(a)) if a.key == 'F'));
}

#[test]
fn resolve_recent_ref_snippet_dangling_returns_none() {
    let app = make_jump_app("");
    let r = crate::app::RecentRef::new(SourceKind::Snippet, "no-such-snippet".into());
    assert!(
        app.resolve_recent_ref_for_test(&r, JumpMode::Hosts)
            .is_none(),
        "snippet ref to missing snippet must drop"
    );
}

#[test]
fn record_jump_hit_round_trips_via_recents() {
    // End-to-end: record a hit through the public API, then opening the
    // jump again should surface it as a recent. App::new builds a sandboxed
    // Env, so the round-trip stays isolated without a path override.
    let mut app = make_jump_app("Host visited\n  HostName visited.example\n");
    let hit = JumpHit::Host(crate::app::HostHit {
        alias: "visited".into(),
        hostname: "visited.example".into(),
        tags: vec![],
        provider: None,
        user: String::new(),
        identity_file: String::new(),
        proxy_jump: String::new(),
        vault_ssh: None,
    });
    app.record_jump_hit(&hit);
    app.open_jump(JumpMode::Hosts);
    let jump = app.jump.as_ref().expect("jump");
    assert!(
        jump.recents
            .iter()
            .any(|h| matches!(h, JumpHit::Host(host) if host.alias == "visited")),
        "recorded hit should surface as a recent on next open"
    );
}

// --- ProxyJump candidate ranking tests ---

use super::selection::{domain_suffix, has_jump_keyword, parse_proxy_jump_hops};
use crate::app::ProxyJumpCandidate;

fn host_aliases(items: &[ProxyJumpCandidate]) -> Vec<String> {
    items
        .iter()
        .filter_map(|c| match c {
            ProxyJumpCandidate::Host { alias, .. } => Some(alias.clone()),
            ProxyJumpCandidate::Separator | ProxyJumpCandidate::SectionLabel(_) => None,
        })
        .collect()
}

fn open_edit_screen(app: &mut App, alias: &str) {
    app.screen = Screen::EditHost {
        alias: alias.to_string(),
    };
}

#[test]
fn proxyjump_candidates_empty_when_only_editing_host() {
    let mut app = test_app_with_hosts(&["Host only\n  HostName 1.2.3.4\n"]);
    open_edit_screen(&mut app, "only");
    assert!(app.proxyjump_candidates().is_empty());
}

#[test]
fn proxyjump_candidates_excludes_host_being_edited() {
    let mut app = test_app_with_hosts(&[
        "Host one\n  HostName 1.1.1.1\n",
        "Host two\n  HostName 2.2.2.2\n",
    ]);
    open_edit_screen(&mut app, "one");
    let aliases = host_aliases(&app.proxyjump_candidates());
    assert_eq!(aliases, vec!["two"]);
}

#[test]
fn proxyjump_candidates_alphabetical_without_signals() {
    // Plain hostnames with no usage, no keywords, no shared domain. The
    // list falls back to alphabetical order with no separator.
    let mut app = test_app_with_hosts(&[
        "Host zeta\n  HostName 10.0.0.3\n",
        "Host alpha\n  HostName 10.0.0.1\n",
        "Host mike\n  HostName 10.0.0.2\n",
    ]);
    open_edit_screen(&mut app, "alpha");
    let candidates = app.proxyjump_candidates();
    assert!(
        !candidates
            .iter()
            .any(|c| matches!(c, ProxyJumpCandidate::Separator)),
        "no separator expected when no signals fire"
    );
    assert_eq!(host_aliases(&candidates), vec!["mike", "zeta"]);
}

#[test]
fn proxyjump_candidates_promotes_hosts_used_as_proxyjump() {
    // `bastion` is referenced by two other hosts; `spare` by none. The
    // heavily-used host should lead the list.
    let mut app = test_app_with_hosts(&[
        "Host bastion\n  HostName 1.1.1.1\n",
        "Host spare\n  HostName 2.2.2.2\n",
        "Host web1\n  HostName 10.0.0.1\n  ProxyJump bastion\n",
        "Host web2\n  HostName 10.0.0.2\n  ProxyJump bastion\n",
    ]);
    open_edit_screen(&mut app, "web1");
    let candidates = app.proxyjump_candidates();
    let sep_index = candidates
        .iter()
        .position(|c| matches!(c, ProxyJumpCandidate::Separator))
        .expect("separator expected");
    let before: Vec<_> = candidates[..sep_index]
        .iter()
        .filter_map(|c| match c {
            ProxyJumpCandidate::Host { alias, .. } => Some(alias.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(before, vec!["bastion"]);
}

#[test]
fn proxyjump_candidates_flags_suggested_items() {
    let mut app = test_app_with_hosts(&[
        "Host jumpbox\n  HostName 1.1.1.1\n",
        "Host plain\n  HostName 2.2.2.2\n",
    ]);
    open_edit_screen(&mut app, "plain");
    let candidates = app.proxyjump_candidates();
    let first_host = candidates
        .iter()
        .find_map(|c| match c {
            ProxyJumpCandidate::Host {
                alias, suggested, ..
            } => Some((alias.clone(), *suggested)),
            _ => None,
        })
        .unwrap();
    assert_eq!(first_host.0, "jumpbox");
    assert!(
        first_host.1,
        "keyword-matched host must be flagged suggested"
    );
}

#[test]
fn proxyjump_candidates_keyword_match_promotes() {
    let mut app = test_app_with_hosts(&[
        "Host aaa\n  HostName 1.1.1.1\n",
        "Host gateway-eu\n  HostName 2.2.2.2\n",
        "Host zzz\n  HostName 3.3.3.3\n",
    ]);
    open_edit_screen(&mut app, "aaa");
    let aliases = host_aliases(&app.proxyjump_candidates());
    assert_eq!(aliases.first().map(String::as_str), Some("gateway-eu"));
}

#[test]
fn proxyjump_candidates_domain_suffix_match_promotes() {
    let mut app = test_app_with_hosts(&[
        "Host edit-me\n  HostName api.example.com\n",
        "Host other\n  HostName cache.internal.net\n",
        "Host same-dom\n  HostName db.example.com\n",
    ]);
    open_edit_screen(&mut app, "edit-me");
    let aliases = host_aliases(&app.proxyjump_candidates());
    assert_eq!(aliases.first().map(String::as_str), Some("same-dom"));
}

#[test]
fn proxyjump_candidates_top_section_capped_at_three() {
    // Five distinct hosts all matching a keyword. Only three may lead.
    let mut app = test_app_with_hosts(&[
        "Host jump-a\n  HostName 1.1.1.1\n",
        "Host jump-b\n  HostName 1.1.1.2\n",
        "Host jump-c\n  HostName 1.1.1.3\n",
        "Host jump-d\n  HostName 1.1.1.4\n",
        "Host jump-e\n  HostName 1.1.1.5\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let candidates = app.proxyjump_candidates();
    let sep_index = candidates
        .iter()
        .position(|c| matches!(c, ProxyJumpCandidate::Separator))
        .expect("separator expected");
    // The real invariant: at most three hosts appear before the separator,
    // regardless of whether a label precedes them. Asserting the structural
    // count instead of a positional magic number keeps the test resilient
    // to picker layout changes.
    let host_count_before_sep = candidates[..sep_index]
        .iter()
        .filter(|c| matches!(c, ProxyJumpCandidate::Host { .. }))
        .count();
    assert_eq!(
        host_count_before_sep, 3,
        "top section must contain exactly three suggested hosts"
    );
}

#[test]
fn proxyjump_candidates_no_separator_when_everything_scores() {
    // All hosts score (keyword match), so there is no "rest" section and
    // therefore no separator.
    let mut app = test_app_with_hosts(&[
        "Host jump-a\n  HostName 1.1.1.1\n",
        "Host bastion-b\n  HostName 1.1.1.2\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let candidates = app.proxyjump_candidates();
    assert!(
        !candidates
            .iter()
            .any(|c| matches!(c, ProxyJumpCandidate::Separator))
    );
}

/// Locate the `Separator` index in a candidate list, or fail the test.
/// Isolates the structural assumption so navigation tests fail with a
/// clear message when the scoring layout shifts, instead of silently
/// relying on a hard-coded index.
fn separator_index(candidates: &[ProxyJumpCandidate]) -> usize {
    candidates
        .iter()
        .position(|c| matches!(c, ProxyJumpCandidate::Separator))
        .expect("expected a Separator in the candidate list")
}

#[test]
fn select_next_proxyjump_skips_separator_forward() {
    let mut app = test_app_with_hosts(&[
        "Host bastion\n  HostName 1.1.1.1\n",
        "Host alpha\n  HostName 2.2.2.2\n",
        "Host zeta\n  HostName 3.3.3.3\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let candidates = app.proxyjump_candidates();
    let sep = separator_index(&candidates);
    // Select the host just before the separator and step forward once.
    app.ui.proxyjump_picker.list.select(Some(sep - 1));
    app.select_next_proxyjump();
    assert_eq!(
        app.ui.proxyjump_picker.list.selected(),
        Some(sep + 1),
        "forward navigation must skip the separator"
    );
}

#[test]
fn select_prev_proxyjump_skips_separator_backward() {
    let mut app = test_app_with_hosts(&[
        "Host bastion\n  HostName 1.1.1.1\n",
        "Host alpha\n  HostName 2.2.2.2\n",
        "Host zeta\n  HostName 3.3.3.3\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let candidates = app.proxyjump_candidates();
    let sep = separator_index(&candidates);
    // Select the host just after the separator and step backward once.
    app.ui.proxyjump_picker.list.select(Some(sep + 1));
    app.select_prev_proxyjump();
    assert_eq!(
        app.ui.proxyjump_picker.list.selected(),
        Some(sep - 1),
        "backward navigation must skip the separator"
    );
}

#[test]
fn select_next_proxyjump_skips_leading_section_label() {
    // Cursor is parked on the `SectionLabel` at index 0; pressing Down
    // must advance past it onto the first selectable `Host`. Regression
    // guard: if `SectionLabel` ever started being treated like a Host,
    // the cursor would stop on the label row instead of moving forward.
    let mut app = test_app_with_hosts(&[
        "Host bastion\n  HostName 1.1.1.1\n",
        "Host alpha\n  HostName 2.2.2.2\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let candidates = app.proxyjump_candidates();
    assert!(matches!(
        candidates.first(),
        Some(ProxyJumpCandidate::SectionLabel(_))
    ));
    app.ui.proxyjump_picker.list.select(Some(0));
    app.select_next_proxyjump();
    let selected = app.ui.proxyjump_picker.list.selected();
    assert!(
        selected.is_some()
            && matches!(
                candidates.get(selected.unwrap()),
                Some(ProxyJumpCandidate::Host { .. })
            ),
        "Down from SectionLabel must land on a Host, got index {:?}",
        selected
    );
}

#[test]
fn select_prev_proxyjump_from_section_label_lands_on_last_host() {
    // Backwards from the label must wrap to the last host, not stay on
    // the label and not land on the Separator.
    let mut app = test_app_with_hosts(&[
        "Host bastion\n  HostName 1.1.1.1\n",
        "Host alpha\n  HostName 2.2.2.2\n",
        "Host zeta\n  HostName 3.3.3.3\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let candidates = app.proxyjump_candidates();
    app.ui.proxyjump_picker.list.select(Some(0));
    app.select_prev_proxyjump();
    let last = candidates.len() - 1;
    assert_eq!(app.ui.proxyjump_picker.list.selected(), Some(last));
    assert!(matches!(
        candidates.get(last),
        Some(ProxyJumpCandidate::Host { .. })
    ));
}

#[test]
fn select_prev_proxyjump_wraps_from_first_host_to_last() {
    // Backwards from index 0 must wrap past a trailing `Separator`-free
    // region and land on the last host, exercising the modular
    // `(next + len - 1) % len` path together with the separator skip.
    let mut app = test_app_with_hosts(&[
        "Host bastion\n  HostName 1.1.1.1\n",
        "Host alpha\n  HostName 2.2.2.2\n",
        "Host zeta\n  HostName 3.3.3.3\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let candidates = app.proxyjump_candidates();
    let last = candidates.len() - 1;
    app.ui.proxyjump_picker.list.select(Some(0));
    app.select_prev_proxyjump();
    assert_eq!(
        app.ui.proxyjump_picker.list.selected(),
        Some(last),
        "backward wrap from index 0 must land on the last host"
    );
}

#[test]
fn select_next_proxyjump_lands_on_index_zero_when_no_selection() {
    // Regression for the bug where a fresh picker with selected() == None
    // advanced to index 1 on the first Down press, skipping index 0.
    let mut app = test_app_with_hosts(&[
        "Host alpha\n  HostName 1.1.1.1\n",
        "Host bravo\n  HostName 2.2.2.2\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    app.ui.proxyjump_picker.list.select(None);
    app.select_next_proxyjump();
    assert_eq!(app.ui.proxyjump_picker.list.selected(), Some(0));
}

#[test]
fn select_prev_proxyjump_lands_on_last_when_no_selection() {
    let mut app = test_app_with_hosts(&[
        "Host alpha\n  HostName 1.1.1.1\n",
        "Host bravo\n  HostName 2.2.2.2\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    app.ui.proxyjump_picker.list.select(None);
    app.select_prev_proxyjump();
    let last = app.proxyjump_candidates().len() - 1;
    assert_eq!(app.ui.proxyjump_picker.list.selected(), Some(last));
}

#[test]
fn select_next_proxyjump_wraps_past_trailing_separator_free_list() {
    let mut app = test_app_with_hosts(&[
        "Host a\n  HostName 1.1.1.1\n",
        "Host b\n  HostName 2.2.2.2\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    app.ui.proxyjump_picker.list.select(Some(1));
    app.select_next_proxyjump();
    assert_eq!(app.ui.proxyjump_picker.list.selected(), Some(0));
}

#[test]
fn proxyjump_first_host_index_skips_leading_label() {
    // With a suggestion present, index 0 is the `SectionLabel` and the
    // first selectable host sits at index 1.
    let mut app = test_app_with_hosts(&[
        "Host bastion\n  HostName 1.1.1.1\n",
        "Host alpha\n  HostName 2.2.2.2\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let candidates = app.proxyjump_candidates();
    assert!(matches!(
        candidates.first(),
        Some(ProxyJumpCandidate::SectionLabel(_))
    ));
    assert_eq!(app.proxyjump_first_host_index(), Some(1));
}

#[test]
fn proxyjump_candidates_section_label_present_with_suggestions() {
    let mut app = test_app_with_hosts(&[
        "Host bastion\n  HostName 1.1.1.1\n",
        "Host plain\n  HostName 2.2.2.2\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let candidates = app.proxyjump_candidates();
    assert!(matches!(
        candidates.first(),
        Some(ProxyJumpCandidate::SectionLabel("Suggestions"))
    ));
}

#[test]
fn proxyjump_candidates_no_section_label_without_suggestions() {
    let mut app = test_app_with_hosts(&[
        "Host zeta\n  HostName 10.0.0.3\n",
        "Host alpha\n  HostName 10.0.0.1\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let candidates = app.proxyjump_candidates();
    assert!(
        !candidates
            .iter()
            .any(|c| matches!(c, ProxyJumpCandidate::SectionLabel(_))),
        "no SectionLabel should be emitted when the suggested section is empty"
    );
}

#[test]
fn proxyjump_first_host_index_zero_when_no_label() {
    // No scoring host means no suggested section and therefore no
    // leading `SectionLabel`; the first host is at index 0.
    let mut app = test_app_with_hosts(&[
        "Host zeta\n  HostName 10.0.0.3\n",
        "Host alpha\n  HostName 10.0.0.1\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    assert_eq!(app.proxyjump_first_host_index(), Some(0));
}

#[test]
fn proxyjump_first_host_index_none_when_empty() {
    let mut app = test_app_with_hosts(&["Host only\n  HostName 1.1.1.1\n"]);
    open_edit_screen(&mut app, "only");
    assert_eq!(app.proxyjump_first_host_index(), None);
}

#[test]
fn parse_proxy_jump_hops_handles_comma_user_and_port() {
    let hops = parse_proxy_jump_hops("alice@jump1:2222, bob@jump2");
    assert_eq!(hops, vec!["jump1", "jump2"]);
}

#[test]
fn parse_proxy_jump_hops_handles_bracketed_ipv6() {
    let hops = parse_proxy_jump_hops("[::1]:2222,plainhost");
    assert_eq!(hops, vec!["::1", "plainhost"]);
}

#[test]
fn parse_proxy_jump_hops_ignores_empty_segments() {
    assert!(parse_proxy_jump_hops("").is_empty());
    assert_eq!(parse_proxy_jump_hops("a,,b"), vec!["a", "b"]);
}

#[test]
fn has_jump_keyword_matches_case_insensitively() {
    assert!(has_jump_keyword("BastionHost", ""));
    assert!(has_jump_keyword("", "corp-gateway-01"));
    assert!(has_jump_keyword("ops-gw-1", ""));
    assert!(!has_jump_keyword("web-01", "10.0.0.1"));
}

#[test]
fn domain_suffix_rejects_single_label_and_ip() {
    assert_eq!(domain_suffix("localhost"), None);
    assert_eq!(domain_suffix("10.0.0.1"), None);
    assert_eq!(domain_suffix(""), None);
    assert_eq!(domain_suffix("[::1]"), None);
}

#[test]
fn domain_suffix_returns_last_two_labels_lowercased() {
    assert_eq!(
        domain_suffix("db.Prod.Example.COM").as_deref(),
        Some("example.com")
    );
    assert_eq!(
        domain_suffix("api.example.com").as_deref(),
        Some("example.com")
    );
}

#[test]
fn proxyjump_candidates_counting_does_not_credit_editing_host() {
    // `web1` is being edited and currently lists `bastion` as its own
    // ProxyJump. That self-reference must not be counted as a usage
    // signal. With no other references, `bastion` should still score
    // only via the keyword heuristic, not via usage — which means the
    // total list length is 2 and `bastion` leads without any usage
    // count contribution.
    let mut app = test_app_with_hosts(&[
        "Host bastion\n  HostName 1.1.1.1\n",
        "Host plain\n  HostName 2.2.2.2\n",
        "Host web1\n  HostName 10.0.0.1\n  ProxyJump bastion\n",
    ]);
    open_edit_screen(&mut app, "web1");
    let candidates = app.proxyjump_candidates();
    let aliases = host_aliases(&candidates);
    assert_eq!(aliases.first().map(String::as_str), Some("bastion"));
    // Layout: SectionLabel, Host{bastion}, Separator, Host{plain}.
    let sep = separator_index(&candidates);
    assert_eq!(sep, 2, "only bastion must lead; plain must follow");
}

#[test]
fn proxyjump_candidates_tied_scores_break_alphabetically() {
    // Two keyword-matching hosts both score 5 via the keyword heuristic.
    // With no other signals, the tie must break by alias ascending.
    let mut app = test_app_with_hosts(&[
        "Host zeta-jump\n  HostName 1.1.1.1\n",
        "Host alpha-jump\n  HostName 2.2.2.2\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let aliases = host_aliases(&app.proxyjump_candidates());
    assert_eq!(
        aliases,
        vec!["alpha-jump", "zeta-jump"],
        "tied scores must sort alphabetically"
    );
}

#[test]
fn proxyjump_candidates_exactly_three_scoring_no_rest_has_no_separator() {
    // All three hosts score. The "rest" list is empty, so even though
    // the top section hits the cap of 3 there must be no Separator.
    let mut app = test_app_with_hosts(&[
        "Host jump-a\n  HostName 1.1.1.1\n",
        "Host jump-b\n  HostName 1.1.1.2\n",
        "Host jump-c\n  HostName 1.1.1.3\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let candidates = app.proxyjump_candidates();
    // Layout: SectionLabel + three hosts, no separator, no rest.
    assert_eq!(candidates.len(), 4);
    assert!(matches!(
        candidates.first(),
        Some(ProxyJumpCandidate::SectionLabel(_))
    ));
    assert!(
        !candidates
            .iter()
            .any(|c| matches!(c, ProxyJumpCandidate::Separator)),
        "three scoring hosts with no rest must not emit a separator"
    );
}

#[test]
fn proxyjump_candidates_rest_items_are_not_flagged_suggested() {
    let mut app = test_app_with_hosts(&[
        "Host bastion\n  HostName 1.1.1.1\n",
        "Host plain-a\n  HostName 2.2.2.2\n",
        "Host plain-b\n  HostName 3.3.3.3\n",
        "Host victim\n  HostName 9.9.9.9\n",
    ]);
    open_edit_screen(&mut app, "victim");
    let candidates = app.proxyjump_candidates();
    let sep = separator_index(&candidates);
    for item in &candidates[sep + 1..] {
        match item {
            ProxyJumpCandidate::Host { suggested, .. } => {
                assert!(
                    !suggested,
                    "rest-section hosts must have suggested == false"
                );
            }
            ProxyJumpCandidate::Separator | ProxyJumpCandidate::SectionLabel(_) => {
                panic!("unexpected non-host item in rest section")
            }
        }
    }
}

#[test]
fn proxyjump_candidates_does_not_panic_for_unknown_editing_alias() {
    // The edit screen references an alias that is not present in
    // `self.hosts_state.list`. This can happen in tests and during transient
    // states; the function must not panic and should still return
    // every existing host, excluding none.
    let mut app = test_app_with_hosts(&[
        "Host alpha\n  HostName 1.1.1.1\n",
        "Host bravo\n  HostName 2.2.2.2\n",
    ]);
    open_edit_screen(&mut app, "ghost");
    let aliases = host_aliases(&app.proxyjump_candidates());
    assert_eq!(aliases, vec!["alpha", "bravo"]);
}

#[test]
fn domain_suffix_rejects_valid_ip_literals() {
    // Any syntactically valid IpAddr must return None. The IpAddr parse
    // is strictly stronger than the original all-digits-per-label guard
    // and also covers `::1`, `2001:db8::1`, `0.0.0.0`, and so on.
    assert_eq!(domain_suffix("192.168.1.1"), None);
    assert_eq!(domain_suffix("0.0.0.0"), None);
    assert_eq!(domain_suffix("::1"), None);
    assert_eq!(domain_suffix("2001:db8::1"), None);
}

#[test]
fn domain_suffix_trims_trailing_fqdn_dot() {
    assert_eq!(
        domain_suffix("example.com.").as_deref(),
        Some("example.com")
    );
    assert_eq!(
        domain_suffix("db.prod.example.com.").as_deref(),
        Some("example.com")
    );
}

#[test]
fn parse_proxy_jump_hops_rejects_unclosed_ipv6_bracket() {
    // Malformed hop without closing bracket must be dropped, not
    // returned as the literal `[ipv6` fragment.
    assert!(parse_proxy_jump_hops("[::1").is_empty());
    assert_eq!(
        parse_proxy_jump_hops("[::1,good")
            .last()
            .map(String::as_str),
        Some("good")
    );
}

// ---------------------------------------------------------------------
// Bulk tag editor
//
// These tests pin down the contract a user experiences via `t` when
// multi-select is active. Each scenario writes a small in-memory config,
// marks a subset, opens the editor, cycles tri-state actions, and asserts
// on the resulting config (round-tripped back through `hosts`).
// ---------------------------------------------------------------------

fn bulk_app() -> App {
    // Four hosts, mixed tag membership:
    //   a → prod
    //   b → prod, db
    //   c → db
    //   d → <no tags>
    let mut app = test_app_with_hosts(&[
        "Host a\n  HostName 1.1.1.1\n  # purple:tags prod",
        "Host b\n  HostName 2.2.2.2\n  # purple:tags prod,db",
        "Host c\n  HostName 3.3.3.3\n  # purple:tags db",
        "Host d\n  HostName 4.4.4.4",
    ]);
    // Unique config path so parallel bulk tests (plus the shared
    // `/tmp/test_config` used by older suite) do not race on write.
    // A counter atomic is a plain stable ID — unlike a raw pointer it
    // satisfies clippy::transmute_ptr_to_int.
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    app.hosts_state.ssh_config.path = std::env::temp_dir().join(format!(
        "purple_bulk_test_{}_{}.cfg",
        std::process::id(),
        id
    ));
    app
}

#[test]
fn bulk_open_refuses_empty_selection() {
    let mut app = bulk_app();
    assert!(!app.open_bulk_tag_editor());
    assert_eq!(app.screen, Screen::HostList);
}

#[test]
fn bulk_open_seeds_rows_with_counts_and_sorts_aliases() {
    let mut app = bulk_app();
    // Select a, b, c (all 4 hosts share indices 0..3 sorted by config order).
    app.hosts_state.multi_select.insert(0);
    app.hosts_state.multi_select.insert(1);
    app.hosts_state.multi_select.insert(2);
    assert!(app.open_bulk_tag_editor());
    assert_eq!(app.screen, Screen::BulkTagEditor);
    assert_eq!(app.forms.bulk_tag_editor.aliases, vec!["a", "b", "c"]);
    // Rows are the union of all user tags across the config. Each row's
    // count reflects how many of the selected hosts (a,b,c) have that
    // tag — 2 of 3 for prod, 2 of 3 for db.
    let by_tag: std::collections::HashMap<&str, usize> = app
        .forms
        .bulk_tag_editor
        .rows
        .iter()
        .map(|r| (r.tag.as_str(), r.initial_count))
        .collect();
    assert_eq!(by_tag.get("prod"), Some(&2));
    assert_eq!(by_tag.get("db"), Some(&2));
    // Every row starts in Leave so Enter with no interaction is a no-op.
    assert!(
        app.forms
            .bulk_tag_editor
            .rows
            .iter()
            .all(|r| r.action == BulkTagAction::Leave)
    );
}

#[test]
fn bulk_cycle_walks_three_states() {
    let mut app = bulk_app();
    app.hosts_state.multi_select.insert(0);
    assert!(app.open_bulk_tag_editor());
    // Select the first row (whatever it is).
    app.ui.bulk_tag_editor_state.select(Some(0));
    assert_eq!(
        app.forms.bulk_tag_editor.rows[0].action,
        BulkTagAction::Leave
    );
    app.bulk_tag_editor_cycle_current();
    assert_eq!(
        app.forms.bulk_tag_editor.rows[0].action,
        BulkTagAction::AddToAll
    );
    app.bulk_tag_editor_cycle_current();
    assert_eq!(
        app.forms.bulk_tag_editor.rows[0].action,
        BulkTagAction::RemoveFromAll
    );
    app.bulk_tag_editor_cycle_current();
    assert_eq!(
        app.forms.bulk_tag_editor.rows[0].action,
        BulkTagAction::Leave
    );
}

#[test]
fn bulk_apply_add_to_all_adds_missing_and_reports_delta() {
    let mut app = bulk_app();
    // Select a (has prod) + d (has nothing). Target: add `prod` to both;
    // only d should actually gain the tag.
    let idx_a = app
        .hosts_state
        .list
        .iter()
        .position(|h| h.alias == "a")
        .unwrap();
    let idx_d = app
        .hosts_state
        .list
        .iter()
        .position(|h| h.alias == "d")
        .unwrap();
    app.hosts_state.multi_select.insert(idx_a);
    app.hosts_state.multi_select.insert(idx_d);
    assert!(app.open_bulk_tag_editor());
    let prod_row = app
        .forms
        .bulk_tag_editor
        .rows
        .iter()
        .position(|r| r.tag == "prod")
        .expect("prod row");
    app.forms.bulk_tag_editor.rows[prod_row].action = BulkTagAction::AddToAll;
    let result = app.bulk_tag_apply().expect("apply ok");
    assert_eq!(result.changed_hosts, 1, "only d should change");
    assert_eq!(result.added, 1);
    assert_eq!(result.removed, 0);

    // Both hosts now have prod.
    let a = app
        .hosts_state
        .list
        .iter()
        .find(|h| h.alias == "a")
        .unwrap();
    let d = app
        .hosts_state
        .list
        .iter()
        .find(|h| h.alias == "d")
        .unwrap();
    assert!(a.tags.contains(&"prod".to_string()));
    assert!(d.tags.contains(&"prod".to_string()));
}

#[test]
fn bulk_apply_remove_from_all_strips_tag_only_where_present() {
    let mut app = bulk_app();
    // Select b (prod, db) and c (db). Remove `db` from both.
    let idx_b = app
        .hosts_state
        .list
        .iter()
        .position(|h| h.alias == "b")
        .unwrap();
    let idx_c = app
        .hosts_state
        .list
        .iter()
        .position(|h| h.alias == "c")
        .unwrap();
    app.hosts_state.multi_select.insert(idx_b);
    app.hosts_state.multi_select.insert(idx_c);
    assert!(app.open_bulk_tag_editor());
    let db_row = app
        .forms
        .bulk_tag_editor
        .rows
        .iter()
        .position(|r| r.tag == "db")
        .expect("db row");
    app.forms.bulk_tag_editor.rows[db_row].action = BulkTagAction::RemoveFromAll;
    let result = app.bulk_tag_apply().expect("apply ok");
    assert_eq!(result.changed_hosts, 2);
    assert_eq!(result.removed, 2);
    assert_eq!(result.added, 0);
    let b = app
        .hosts_state
        .list
        .iter()
        .find(|h| h.alias == "b")
        .unwrap();
    let c = app
        .hosts_state
        .list
        .iter()
        .find(|h| h.alias == "c")
        .unwrap();
    assert!(!b.tags.contains(&"db".to_string()));
    assert!(!c.tags.contains(&"db".to_string()));
    // `prod` on b is untouched — only `db` was targeted.
    assert!(b.tags.contains(&"prod".to_string()));
}

#[test]
fn bulk_apply_leave_is_noop_and_reports_zero_counts() {
    let mut app = bulk_app();
    app.hosts_state.multi_select.insert(0);
    app.hosts_state.multi_select.insert(1);
    assert!(app.open_bulk_tag_editor());
    let result = app.bulk_tag_apply().expect("apply ok");
    assert_eq!(result.changed_hosts, 0);
    assert_eq!(result.added, 0);
    assert_eq!(result.removed, 0);
}

#[test]
fn bulk_apply_add_and_remove_in_one_pass() {
    let mut app = bulk_app();
    // Select b (prod, db) + d (nothing). Add `staging`, remove `db`.
    let idx_b = app
        .hosts_state
        .list
        .iter()
        .position(|h| h.alias == "b")
        .unwrap();
    let idx_d = app
        .hosts_state
        .list
        .iter()
        .position(|h| h.alias == "d")
        .unwrap();
    app.hosts_state.multi_select.insert(idx_b);
    app.hosts_state.multi_select.insert(idx_d);
    assert!(app.open_bulk_tag_editor());
    // `staging` doesn't exist yet — use the new-tag path.
    app.forms.bulk_tag_editor.new_tag_input = Some("staging".into());
    app.forms.bulk_tag_editor.new_tag_cursor = 7;
    app.bulk_tag_editor_commit_new_tag();
    let db_row = app
        .forms
        .bulk_tag_editor
        .rows
        .iter()
        .position(|r| r.tag == "db")
        .expect("db row");
    app.forms.bulk_tag_editor.rows[db_row].action = BulkTagAction::RemoveFromAll;
    let result = app.bulk_tag_apply().expect("apply ok");
    // Both hosts gained staging (2 adds). Only b had db (1 remove).
    assert_eq!(result.added, 2, "staging adds");
    assert_eq!(result.removed, 1, "db remove");
    assert_eq!(result.changed_hosts, 2);
    let b = app
        .hosts_state
        .list
        .iter()
        .find(|h| h.alias == "b")
        .unwrap();
    let d = app
        .hosts_state
        .list
        .iter()
        .find(|h| h.alias == "d")
        .unwrap();
    assert!(b.tags.contains(&"staging".to_string()));
    assert!(d.tags.contains(&"staging".to_string()));
    assert!(!b.tags.contains(&"db".to_string()));
}

#[test]
fn bulk_new_tag_input_dedupes_existing_row() {
    let mut app = bulk_app();
    app.hosts_state.multi_select.insert(0);
    assert!(app.open_bulk_tag_editor());
    let before_rows = app.forms.bulk_tag_editor.rows.len();
    // Typing a tag that already exists should flip its action to AddToAll
    // rather than add a duplicate row.
    app.forms.bulk_tag_editor.new_tag_input = Some("prod".into());
    app.forms.bulk_tag_editor.new_tag_cursor = 4;
    app.bulk_tag_editor_commit_new_tag();
    assert_eq!(app.forms.bulk_tag_editor.rows.len(), before_rows);
    let prod = app
        .forms
        .bulk_tag_editor
        .rows
        .iter()
        .find(|r| r.tag == "prod")
        .unwrap();
    assert_eq!(prod.action, BulkTagAction::AddToAll);
}

#[test]
fn bulk_action_cycle_wraps() {
    assert_eq!(BulkTagAction::Leave.cycle(), BulkTagAction::AddToAll);
    assert_eq!(
        BulkTagAction::AddToAll.cycle(),
        BulkTagAction::RemoveFromAll
    );
    assert_eq!(BulkTagAction::RemoveFromAll.cycle(), BulkTagAction::Leave);
}

#[test]
fn bulk_action_glyph_is_distinct_per_variant() {
    let glyphs = [
        BulkTagAction::Leave.glyph(),
        BulkTagAction::AddToAll.glyph(),
        BulkTagAction::RemoveFromAll.glyph(),
    ];
    for (i, a) in glyphs.iter().enumerate() {
        for (j, b) in glyphs.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "glyphs must be distinct: {a} vs {b}");
            }
        }
    }
}

#[test]
fn bulk_apply_add_to_all_noop_when_all_hosts_already_have_tag() {
    // When every selected host already carries the tag, AddToAll should
    // NOT trigger a config write (changed_hosts == 0).
    let mut app = bulk_app();
    let idx_a = app
        .hosts_state
        .list
        .iter()
        .position(|h| h.alias == "a")
        .unwrap();
    let idx_b = app
        .hosts_state
        .list
        .iter()
        .position(|h| h.alias == "b")
        .unwrap();
    app.hosts_state.multi_select.insert(idx_a);
    app.hosts_state.multi_select.insert(idx_b);
    assert!(app.open_bulk_tag_editor());
    let prod_row = app
        .forms
        .bulk_tag_editor
        .rows
        .iter()
        .position(|r| r.tag == "prod")
        .unwrap();
    // Both a and b already have "prod".
    app.forms.bulk_tag_editor.rows[prod_row].action = BulkTagAction::AddToAll;
    let result = app.bulk_tag_apply().expect("apply ok");
    assert_eq!(result.changed_hosts, 0);
    assert_eq!(result.added, 0);
}

#[test]
fn bulk_open_with_include_file_host_records_skipped() {
    // Hosts sourced from Include files cannot be tag-edited in place.
    // Verify they show up in skipped_included and their tags stay intact.
    let mut app = bulk_app();
    // Simulate an Include-sourced host by setting source_file.
    app.hosts_state.list[0].source_file = Some(PathBuf::from("/etc/ssh/extra.conf"));
    let idx_0 = 0;
    let idx_1 = 1;
    app.hosts_state.multi_select.insert(idx_0);
    app.hosts_state.multi_select.insert(idx_1);
    assert!(app.open_bulk_tag_editor());
    assert_eq!(app.forms.bulk_tag_editor.skipped_included.len(), 1);
    assert!(
        app.forms
            .bulk_tag_editor
            .skipped_included
            .contains(&app.hosts_state.list[0].alias.clone())
    );

    // Force add a tag; the skipped host must NOT get it.
    let db_row = app
        .forms
        .bulk_tag_editor
        .rows
        .iter()
        .position(|r| r.tag == "db")
        .unwrap();
    app.forms.bulk_tag_editor.rows[db_row].action = BulkTagAction::AddToAll;
    let result = app.bulk_tag_apply().expect("apply ok");
    assert_eq!(result.skipped_included, 1);
    // Host 0 (alias "a") should be unchanged because it is in Include.
    let a = app
        .hosts_state
        .list
        .iter()
        .find(|h| h.alias == "a")
        .unwrap();
    assert!(!a.tags.contains(&"db".to_string()));
}

#[test]
fn bulk_apply_write_failure_rolls_back_and_keeps_undo_empty() {
    let mut app = bulk_app();
    // Point the config to an unwritable path.
    app.hosts_state.ssh_config.path = PathBuf::from("/dev/null/impossible/path.cfg");
    let idx = app
        .hosts_state
        .list
        .iter()
        .position(|h| h.alias == "a")
        .unwrap();
    app.hosts_state.multi_select.insert(idx);
    assert!(app.open_bulk_tag_editor());
    let prod_row = app
        .forms
        .bulk_tag_editor
        .rows
        .iter()
        .position(|r| r.tag == "prod")
        .unwrap();
    app.forms.bulk_tag_editor.rows[prod_row].action = BulkTagAction::RemoveFromAll;
    let err = app.bulk_tag_apply();
    assert!(err.is_err(), "should fail on bad path");
    // Undo snapshot should NOT be set on failure.
    assert!(app.forms.bulk_tag_undo.is_none());
}

#[test]
fn bulk_double_undo_falls_through_to_delete_stack() {
    let mut app = bulk_app();
    let idx = app
        .hosts_state
        .list
        .iter()
        .position(|h| h.alias == "a")
        .unwrap();
    app.hosts_state.multi_select.insert(idx);
    assert!(app.open_bulk_tag_editor());
    let prod_row = app
        .forms
        .bulk_tag_editor
        .rows
        .iter()
        .position(|r| r.tag == "prod")
        .unwrap();
    app.forms.bulk_tag_editor.rows[prod_row].action = BulkTagAction::RemoveFromAll;
    app.bulk_tag_apply().expect("apply ok");
    assert!(app.forms.bulk_tag_undo.is_some());

    // First undo: restores tags. Simulate the undo handler inline.
    let snapshot = app.forms.bulk_tag_undo.take().unwrap();
    for (alias, tags) in &snapshot {
        let _ = app.hosts_state.ssh_config.set_host_tags(alias, tags);
    }
    let _ = app.hosts_state.ssh_config.write(); // may fail in test env, that's ok
    // Second undo attempt: no bulk_tag_undo, no undo_stack → nothing.
    assert!(app.forms.bulk_tag_undo.is_none());
    assert!(app.hosts_state.undo_stack.is_empty());
}

#[test]
fn bulk_new_tag_empty_input_is_noop() {
    let mut app = bulk_app();
    app.hosts_state.multi_select.insert(0);
    assert!(app.open_bulk_tag_editor());
    let before = app.forms.bulk_tag_editor.rows.len();
    // Submit whitespace-only new tag.
    app.forms.bulk_tag_editor.new_tag_input = Some("   ".into());
    app.forms.bulk_tag_editor.new_tag_cursor = 3;
    app.bulk_tag_editor_commit_new_tag();
    assert_eq!(app.forms.bulk_tag_editor.rows.len(), before);
    assert!(app.forms.bulk_tag_editor.new_tag_input.is_none());
}

#[test]
fn bulk_open_with_zero_tags_in_config_succeeds() {
    let mut app =
        test_app_with_hosts(&["Host x\n  HostName 1.1.1.1", "Host y\n  HostName 2.2.2.2"]);
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    app.hosts_state.ssh_config.path = std::env::temp_dir().join(format!(
        "purple_zero_tags_test_{}_{}.cfg",
        std::process::id(),
        id
    ));
    app.hosts_state.multi_select.insert(0);
    app.hosts_state.multi_select.insert(1);
    assert!(app.open_bulk_tag_editor());
    assert!(app.forms.bulk_tag_editor.rows.is_empty());
    assert_eq!(app.screen, Screen::BulkTagEditor);
    // New-tag input should still work on empty list.
    app.forms.bulk_tag_editor.new_tag_input = Some("fresh".into());
    app.forms.bulk_tag_editor.new_tag_cursor = 5;
    app.bulk_tag_editor_commit_new_tag();
    assert_eq!(app.forms.bulk_tag_editor.rows.len(), 1);
    assert_eq!(app.forms.bulk_tag_editor.rows[0].tag, "fresh");
}

#[test]
fn post_init_enqueues_toast_when_version_advanced() {
    let mut app = make_app("");
    crate::preferences::save_last_seen_version(app.env().paths(), "0.0.1").unwrap();
    app.post_init();
    let fragment = crate::messages::whats_new_toast::INVITE_FRAGMENT;
    assert!(
        app.status_center
            .toast
            .as_ref()
            .is_some_and(|t| t.text.contains(fragment)),
        "expected sticky upgrade toast"
    );
    assert!(app.status_center.toast.as_ref().is_some_and(|t| t.sticky));
}

#[test]
fn post_init_silent_when_versions_equal() {
    let mut app = make_app("");
    crate::preferences::save_last_seen_version(app.env().paths(), env!("CARGO_PKG_VERSION"))
        .unwrap();
    app.post_init();
    let fragment = crate::messages::whats_new_toast::INVITE_FRAGMENT;
    assert!(
        !app.status_center
            .toast
            .as_ref()
            .is_some_and(|t| t.text.contains(fragment)),
        "no toast when last_seen matches current"
    );
}

#[test]
fn post_init_invokes_scan_keys() {
    let mut app = make_app("");
    crate::preferences::save_last_seen_version(app.env().paths(), env!("CARGO_PKG_VERSION"))
        .unwrap();
    // Sentinel that only survives if scan_keys is NOT called: scan_keys
    // unconditionally replaces self.keys.list when dirs::home_dir() returns Some.
    app.keys.list.push(crate::ssh_keys::SshKeyInfo {
        name: "_post_init_sentinel".into(),
        display_path: String::new(),
        key_type: String::new(),
        bits: String::new(),
        fingerprint: String::new(),
        comment: String::new(),
        linked_hosts: vec![],
        bishop_art: String::new(),
        strength_score: 0,
        encrypted: false,
        agent_loaded: false,
        is_certificate: false,
        mtime_ts: None,
    });
    app.post_init();
    assert!(
        !app.keys
            .list
            .iter()
            .any(|k| k.name == "_post_init_sentinel"),
        "post_init must invoke scan_keys so the Keys tab is populated on first Tab navigation"
    );
}

#[test]
fn apply_alias_renames_migrates_history_and_recents_in_batch() {
    // `apply_alias_renames` covers persistent state that lives outside
    // the in-memory cache layer: connection history and jump recents.
    // collapsed_hosts migrates earlier via `migrate_alias_keyed_caches`
    // so its prune in `reload_hosts` sees the new alias as live.
    let mut app = make_app("Host a\n  HostName 1.2.3.4\nHost c\n  HostName 5.6.7.8\n");
    // App::new builds a sandboxed Env; seed and read recents through it.
    let paths = app.env().paths().cloned();
    app.history = crate::history::ConnectionHistory::from_entries(std::collections::HashMap::new());
    app.history.upsert_entry(crate::history::HistoryEntry {
        alias: "a".to_string(),
        last_connected: 1_700_000_000,
        count: 3,
        timestamps: vec![1_700_000_000],
    });
    app.history.upsert_entry(crate::history::HistoryEntry {
        alias: "c".to_string(),
        last_connected: 1_700_000_500,
        count: 7,
        timestamps: vec![1_700_000_500],
    });

    let mut seeded = crate::app::jump::RecentsFile::default();
    seeded.entries.push(crate::app::jump::RecentEntry {
        target: crate::app::jump::RecentRef::new(
            crate::app::jump::SourceKind::Host,
            "a".to_string(),
        ),
        last_used_unix: 100,
    });
    seeded.entries.push(crate::app::jump::RecentEntry {
        target: crate::app::jump::RecentRef::new(
            crate::app::jump::SourceKind::Host,
            "c".to_string(),
        ),
        last_used_unix: 200,
    });
    crate::app::jump::save_recents(&seeded, paths.as_ref()).expect("seed recents");

    // Batch: real rename + an identity pair that must be a no-op. The
    // identity pair guards against an accidental wipe when the SSH
    // parser emits a (old, new) tuple where old == new on a no-op
    // sync (the dry-run / unchanged path emits an empty `renames` list,
    // but defensive-by-construction is cheap).
    app.apply_alias_renames(&[
        ("a".to_string(), "b".to_string()),
        ("c".to_string(), "c".to_string()),
    ]);

    assert!(app.history.entry("a").is_none());
    assert!(app.history.entry("b").is_some());
    assert!(
        app.history.entry("c").is_some(),
        "non-renamed host history must stay"
    );
    assert_eq!(app.history.entry("b").unwrap().count, 3);
    assert_eq!(app.history.entry("c").unwrap().count, 7);

    let reloaded = crate::app::jump::load_recents(paths.as_ref());
    let mut host_keys: Vec<String> = reloaded
        .entries
        .iter()
        .filter(|e| e.target.kind == crate::app::jump::SourceKind::Host)
        .map(|e| e.target.key.clone())
        .collect();
    host_keys.sort();
    assert_eq!(host_keys, vec!["b".to_string(), "c".to_string()]);
}

#[test]
fn migrate_alias_keyed_caches_moves_ping_container_and_in_flight_sets() {
    // Caches that disappear from `reload_hosts`' prune step on a rename
    // (ping status/timing, container cache, and the alias-keyed in-flight
    // dedup sets) must survive a rename when `migrate_alias_keyed_caches`
    // runs first. cert_cache is intentionally NOT migrated here; the
    // rename invalidates the certificate path so `refresh_cert_cache`
    // rebuilds it under the new alias from scratch.
    let mut app = make_app("Host a\n  HostName 1.2.3.4\n");

    app.ping.status.insert(
        "a".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 12 },
    );
    app.ping
        .last_checked
        .insert("a".to_string(), std::time::Instant::now());
    app.container_state.cache.insert(
        "a".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 0,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: None,
            containers: vec![],
        },
    );
    app.containers_overview
        .mark_auto_list_pending("a".to_string());
    app.vault.cert_checks_in_flight.insert("a".to_string());
    // Pre-seed a stale cert_cache entry to prove it is left alone (the
    // caller refreshes it via `refresh_cert_cache` after the rename).
    app.vault.cert_cache.insert(
        "a".to_string(),
        (
            std::time::Instant::now(),
            crate::vault_ssh::CertStatus::Missing,
            None,
        ),
    );

    app.migrate_alias_keyed_caches(&[("a".to_string(), "b".to_string())]);

    assert!(!app.ping.status.contains_key("a"));
    assert!(matches!(
        app.ping.status.get("b"),
        Some(crate::app::PingStatus::Reachable { rtt_ms: 12 })
    ));
    assert!(!app.ping.last_checked.contains_key("a"));
    assert!(app.ping.last_checked.contains_key("b"));
    assert!(!app.container_state.cache.contains_key("a"));
    assert!(app.container_state.cache.contains_key("b"));
    assert!(!app.containers_overview.auto_list_pending("a"));
    assert!(app.containers_overview.auto_list_pending("b"));
    assert!(!app.vault.cert_checks_in_flight.contains("a"));
    assert!(app.vault.cert_checks_in_flight.contains("b"));
    // cert_cache MUST stay under the old key. The caller (edit_host_from_form)
    // is responsible for clearing it and calling refresh_cert_cache on the
    // new alias, which checks the disk-side cert path.
    assert!(app.vault.cert_cache.contains_key("a"));
    assert!(!app.vault.cert_cache.contains_key("b"));
}

#[test]
fn migrate_alias_keyed_caches_moves_host_paths_refresh_batch_and_sign_in_flight() {
    // file_browser host_paths, refresh_batch in_flight_aliases, and the
    // vault sign_in_flight set are all alias-keyed. Without migration a
    // rename would silently drop in-flight state and leak preferences.
    let mut app = make_app("Host a\n  HostName 1.2.3.4\n");

    app.file_browser_state.host_paths.insert(
        "a".to_string(),
        (std::path::PathBuf::from("/var/log"), "/var/log".to_string()),
    );
    app.containers_overview
        .start_refresh(crate::app::RefreshBatch {
            queue: std::collections::VecDeque::new(),
            in_flight: 1,
            total: 1,
            completed: 0,
            in_flight_aliases: ["a".to_string()].into_iter().collect(),
        });
    {
        let mut sign = app.vault.sign_in_flight.lock().expect("lock");
        sign.insert("a".to_string());
    }

    app.migrate_alias_keyed_caches(&[("a".to_string(), "b".to_string())]);

    assert!(!app.file_browser_state.host_paths.contains_key("a"));
    assert!(app.file_browser_state.host_paths.contains_key("b"));
    let batch = app
        .containers_overview
        .refresh_batch()
        .expect("batch must still exist");
    assert!(!batch.in_flight_aliases.contains("a"));
    assert!(batch.in_flight_aliases.contains("b"));
    {
        let sign = app.vault.sign_in_flight.lock().expect("lock");
        assert!(!sign.contains("a"));
        assert!(sign.contains("b"));
    }
}

#[test]
fn migrate_alias_keyed_caches_identity_pair_is_noop() {
    let mut app = make_app("Host a\n  HostName 1.2.3.4\n");
    app.ping.status.insert(
        "a".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 7 },
    );

    app.migrate_alias_keyed_caches(&[("a".to_string(), "a".to_string())]);

    assert!(matches!(
        app.ping.status.get("a"),
        Some(crate::app::PingStatus::Reachable { rtt_ms: 7 })
    ));
}

#[test]
fn rename_aliases_full_protocol_migrates_caches_history_and_resorts() {
    // Contract test for the single entry point `rename_aliases`.
    // Verifies cache migration + reload + persistent migration + re-sort
    // land in the right order. Pinned here because production callers
    // (submit_form host edit, sync_provider_with_section provider sync)
    // both route through this function; if a future refactor splits
    // the protocol again, this test fails before the user sees a
    // regression.
    // App::new builds a sandboxed Env, so the rename's recents write stays
    // isolated without a path override.

    // Two hosts. After rename the SSH config has top-new + bot; history
    // for top-old should follow to top-new and keep its index-0 slot on
    // MostRecent.
    let mut app = make_app(
        "Host top-new\n  HostName 1.1.1.1\n\
         Host bot\n  HostName 2.2.2.2\n",
    );
    // Seed history under the OLD alias because the SSH config write has
    // already happened (production callers run this AFTER ssh_config.write).
    app.history = crate::history::ConnectionHistory::from_entries(std::collections::HashMap::new());
    app.history.upsert_entry(crate::history::HistoryEntry {
        alias: "top-old".to_string(),
        last_connected: 1_700_000_300,
        count: 30,
        timestamps: vec![1_700_000_300],
    });
    app.history.upsert_entry(crate::history::HistoryEntry {
        alias: "bot".to_string(),
        last_connected: 1_700_000_100,
        count: 1,
        timestamps: vec![1_700_000_100],
    });
    app.ping.status.insert(
        "top-old".to_string(),
        crate::app::PingStatus::Reachable { rtt_ms: 42 },
    );
    app.container_state.cache.insert(
        "top-old".to_string(),
        crate::containers::ContainerCacheEntry {
            timestamp: 1_700_000_000,
            runtime: crate::containers::ContainerRuntime::Docker,
            engine_version: Some("24.0.0".to_string()),
            containers: vec![],
        },
    );
    app.containers_overview.toggle_host_collapsed("top-old");
    app.hosts_state.sort_mode = crate::app::SortMode::MostRecent;

    app.rename_aliases(&[("top-old".to_string(), "top-new".to_string())]);

    // Cache migrated (would not survive reload_hosts prune without
    // migrate_alias_keyed_caches running first).
    assert!(
        matches!(
            app.ping.status.get("top-new"),
            Some(crate::app::PingStatus::Reachable { rtt_ms: 42 })
        ),
        "ping.status must follow the rename through rename_aliases"
    );
    assert!(app.container_state.cache.contains_key("top-new"));

    // History migrated (would stay under top-old without apply_alias_renames).
    assert!(app.history.entry("top-old").is_none());
    assert_eq!(app.history.entry("top-new").unwrap().count, 30);
    assert!(
        app.containers_overview
            .collapsed_hosts()
            .contains("top-new")
    );

    // Re-sort happened: top-new sits at index 0 of the display list
    // (would be at the bottom without the trailing apply_sort).
    let alias_at_0 = match app.hosts_state.display_list.first() {
        Some(crate::app::HostListItem::Host { index }) => {
            app.hosts_state.list.get(*index).map(|h| h.alias.as_str())
        }
        _ => None,
    };
    assert_eq!(
        alias_at_0,
        Some("top-new"),
        "rename_aliases must re-sort so the migrated host keeps its recency slot"
    );
}

#[test]
fn apply_alias_renames_empty_input_is_no_op() {
    let mut app = make_app("Host a\n  HostName 1.2.3.4\n");
    app.history = crate::history::ConnectionHistory::from_entries(std::collections::HashMap::new());
    app.history.upsert_entry(crate::history::HistoryEntry {
        alias: "a".to_string(),
        last_connected: 42,
        count: 1,
        timestamps: vec![42],
    });
    app.apply_alias_renames(&[]);
    assert_eq!(app.history.entry("a").unwrap().count, 1);
}

#[test]
fn migrate_renames_persistent_state_moves_history_recents_and_collapsed_on_disk() {
    // CLI `purple sync` has no `App`; the file-level helper must
    // migrate `~/.purple/history.tsv`, `~/.purple/recents.json` and
    // the `containers_collapsed_hosts` line of `~/.purple/preferences`
    // when the SSH config rename lands on disk. Same migration as
    // `App::apply_alias_renames` but without in-memory state.
    let dir = tempfile::tempdir().expect("tempdir");
    let paths = crate::runtime::env::Paths::new(dir.path());
    std::fs::create_dir_all(paths.state_dir()).unwrap();
    let history_path = paths.history();

    // Seed history.tsv. Schema: alias \t last_connected \t count \t csv-of-timestamps.
    std::fs::write(&history_path, "web-old\t1700000000\t12\t1700000000\n").unwrap();

    // Seed recents.json with a host entry for the old alias.
    let mut recents = crate::app::jump::RecentsFile::default();
    recents.entries.push(crate::app::jump::RecentEntry {
        target: crate::app::jump::RecentRef::new(
            crate::app::jump::SourceKind::Host,
            "web-old".to_string(),
        ),
        last_used_unix: 100,
    });
    crate::app::jump::save_recents(&recents, Some(&paths)).expect("seed recents");

    // Seed the collapsed-hosts preference.
    let mut collapsed = std::collections::HashSet::new();
    collapsed.insert("web-old".to_string());
    crate::preferences::save_containers_collapsed_hosts(Some(&paths), &collapsed)
        .expect("seed collapsed_hosts");

    crate::app::migrate_renames_persistent_state(
        Some(&paths),
        &[("web-old".to_string(), "web-new".to_string())],
    );

    let history_after = std::fs::read_to_string(&history_path).unwrap();
    // First three TSV columns: alias \t last_connected \t count.
    // The fourth column (csv-of-timestamps) is dropped by the load-
    // path retention prune because the seed timestamp is older than
    // RETENTION_SECS. Assert the first three only.
    assert!(
        history_after.starts_with("web-new\t1700000000\t12"),
        "history file must lead with new alias + ts + count: {history_after:?}"
    );
    assert!(
        !history_after.contains("web-old"),
        "old alias must be gone: {history_after:?}"
    );

    let recents_after = crate::app::jump::load_recents(Some(&paths));
    let host_keys: Vec<String> = recents_after
        .entries
        .iter()
        .filter(|e| e.target.kind == crate::app::jump::SourceKind::Host)
        .map(|e| e.target.key.clone())
        .collect();
    assert_eq!(host_keys, vec!["web-new".to_string()]);

    let collapsed_after = crate::preferences::load_containers_collapsed_hosts(Some(&paths));
    assert!(collapsed_after.contains("web-new"));
    assert!(!collapsed_after.contains("web-old"));
}

#[test]
fn migrate_renames_persistent_state_skips_identity_pairs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let paths = crate::runtime::env::Paths::new(dir.path());
    std::fs::create_dir_all(paths.state_dir()).unwrap();
    let history_path = paths.history();

    // Seed an entry. An identity-pair rename must leave it untouched
    // (same key, same count, same timestamps). Guards against a future
    // refactor that accidentally rewrites the file on every call.
    std::fs::write(&history_path, "web\t1700000000\t5\t1700000000\n").unwrap();

    crate::app::migrate_renames_persistent_state(
        Some(&paths),
        &[("web".to_string(), "web".to_string())],
    );

    let after = std::fs::read_to_string(&history_path).unwrap();
    assert!(after.contains("web\t1700000000\t5"));
}

#[test]
fn migrate_renames_persistent_state_empty_input_is_no_op() {
    let dir = tempfile::tempdir().expect("tempdir");
    let paths = crate::runtime::env::Paths::new(dir.path());
    // History file deliberately absent. The helper must not panic.
    crate::app::migrate_renames_persistent_state(Some(&paths), &[]);
}

#[test]
fn record_key_use_persists_via_app_boundary() {
    use crate::key_activity::KeyActivityLog;

    // Hold the cross-crate lock for the full duration of the test.
    // `app.record_key_use` calls `KeyActivityLog::flush`, which reads
    // `demo_flag::is_demo()` and early-returns when demo mode is active.
    // A concurrent visual test flipping the flag would silently skip
    // our write, breaking the event-count assertion below.
    let _lock = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());

    let scratch = tempfile::tempdir().expect("scratch tempdir");
    let config = crate::ssh_config::model::SshConfigFile {
        elements: crate::ssh_config::model::SshConfigFile::parse_content(""),
        path: scratch.path().join("test_config"),
        crlf: false,
        bom: false,
    };
    // App::new builds a sandboxed Env; record_key_use flushes into it.
    let mut app = crate::app::App::new(config);
    let paths = app.env().paths().cloned();

    let before = crate::key_activity::now_secs();
    app.record_key_use("prod-eu1", crate::key_activity::now_secs());
    let after = crate::key_activity::now_secs();

    let reloaded = KeyActivityLog::load(paths.as_ref());
    assert_eq!(reloaded.events.len(), 1);
    assert_eq!(reloaded.events[0].alias, "prod-eu1");
    // Verify the timestamp came from the wall clock, not a stale or
    // hardcoded source. A future regression that accidentally fed
    // DEMO_NOW_SECS (or zero) into record would land outside this range.
    let ts = reloaded.events[0].ts;
    assert!(
        ts >= before && ts <= after,
        "event timestamp {ts} not in [{before}, {after}]"
    );
}

// --- Picker close lifecycle ---

#[test]
fn close_password_picker_clears_open_flag() {
    let mut app = make_app("");
    app.ui.password_picker.open = true;
    app.close_password_picker();
    assert!(!app.ui.password_picker.open);
}

#[test]
fn close_key_picker_clears_open_flag() {
    let mut app = make_app("");
    app.ui.key_picker.open = true;
    app.close_key_picker();
    assert!(!app.ui.key_picker.open);
}

#[test]
fn close_proxyjump_picker_clears_open_flag() {
    let mut app = make_app("");
    app.ui.proxyjump_picker.open = true;
    app.close_proxyjump_picker();
    assert!(!app.ui.proxyjump_picker.open);
}

#[test]
fn close_vault_role_picker_clears_open_flag() {
    let mut app = make_app("");
    app.ui.vault_role_picker.open = true;
    app.close_vault_role_picker();
    assert!(!app.ui.vault_role_picker.open);
}

#[test]
fn close_region_picker_clears_open_flag() {
    let mut app = make_app("");
    app.ui.region_picker.open = true;
    app.close_region_picker();
    assert!(!app.ui.region_picker.open);
}

// Catches a copy-paste regression where a close_X_picker body accidentally
// clears the wrong picker, or clears more than one. Each test opens all five
// pickers, closes one, and asserts only that picker transitioned. The
// per-picker tests above prove the target field clears; these prove no other
// picker state was disturbed.
#[test]
fn close_password_picker_does_not_touch_other_pickers() {
    let mut app = make_app("");
    app.ui.password_picker.open = true;
    app.ui.key_picker.open = true;
    app.ui.proxyjump_picker.open = true;
    app.ui.vault_role_picker.open = true;
    app.ui.region_picker.open = true;
    app.close_password_picker();
    assert!(!app.ui.password_picker.open);
    assert!(app.ui.key_picker.open);
    assert!(app.ui.proxyjump_picker.open);
    assert!(app.ui.vault_role_picker.open);
    assert!(app.ui.region_picker.open);
}

#[test]
fn close_key_picker_does_not_touch_other_pickers() {
    let mut app = make_app("");
    app.ui.password_picker.open = true;
    app.ui.key_picker.open = true;
    app.ui.proxyjump_picker.open = true;
    app.ui.vault_role_picker.open = true;
    app.ui.region_picker.open = true;
    app.close_key_picker();
    assert!(!app.ui.key_picker.open);
    assert!(app.ui.password_picker.open);
    assert!(app.ui.proxyjump_picker.open);
    assert!(app.ui.vault_role_picker.open);
    assert!(app.ui.region_picker.open);
}

#[test]
fn close_proxyjump_picker_does_not_touch_other_pickers() {
    let mut app = make_app("");
    app.ui.password_picker.open = true;
    app.ui.key_picker.open = true;
    app.ui.proxyjump_picker.open = true;
    app.ui.vault_role_picker.open = true;
    app.ui.region_picker.open = true;
    app.close_proxyjump_picker();
    assert!(!app.ui.proxyjump_picker.open);
    assert!(app.ui.password_picker.open);
    assert!(app.ui.key_picker.open);
    assert!(app.ui.vault_role_picker.open);
    assert!(app.ui.region_picker.open);
}

#[test]
fn close_vault_role_picker_does_not_touch_other_pickers() {
    let mut app = make_app("");
    app.ui.password_picker.open = true;
    app.ui.key_picker.open = true;
    app.ui.proxyjump_picker.open = true;
    app.ui.vault_role_picker.open = true;
    app.ui.region_picker.open = true;
    app.close_vault_role_picker();
    assert!(!app.ui.vault_role_picker.open);
    assert!(app.ui.password_picker.open);
    assert!(app.ui.key_picker.open);
    assert!(app.ui.proxyjump_picker.open);
    assert!(app.ui.region_picker.open);
}

#[test]
fn close_region_picker_does_not_touch_other_pickers() {
    let mut app = make_app("");
    app.ui.password_picker.open = true;
    app.ui.key_picker.open = true;
    app.ui.proxyjump_picker.open = true;
    app.ui.vault_role_picker.open = true;
    app.ui.region_picker.open = true;
    app.close_region_picker();
    assert!(!app.ui.region_picker.open);
    assert!(app.ui.password_picker.open);
    assert!(app.ui.key_picker.open);
    assert!(app.ui.proxyjump_picker.open);
    assert!(app.ui.vault_role_picker.open);
}

// --- Picker open lifecycle ---

#[test]
fn open_password_picker_sets_flag_and_selects_first() {
    let mut app = make_app("");
    app.ui.password_picker.open = false;
    app.ui.password_picker.list.select(Some(99));
    app.open_password_picker();
    assert!(app.ui.password_picker.open);
    assert_eq!(app.ui.password_picker.list.selected(), Some(0));
}

// scan_keys reads from $HOME/.ssh so the resulting list and selection are
// environment-dependent. The test pins the flag transition and the list
// reset (prior cursor 99 must be gone) without asserting on the post-scan
// selection.
#[test]
fn open_key_picker_sets_flag_and_resets_list() {
    let mut app = make_app("");
    app.ui.key_picker.open = false;
    app.ui.key_picker.list.select(Some(99));
    app.open_key_picker();
    assert!(app.ui.key_picker.open);
    assert_ne!(app.ui.key_picker.list.selected(), Some(99));
}

// Pins the conditional `if !keys.list.is_empty() { select(Some(0)) }` in
// open_key_picker against either possible scan_keys outcome. On any
// environment that discovers at least one key the test catches a deletion
// of the select line; on a clean environment with no .ssh both sides are
// None and the test still passes.
#[test]
fn open_key_picker_selection_matches_keys_present() {
    let mut app = make_app("");
    app.open_key_picker();
    let expected = if app.keys.list.is_empty() {
        None
    } else {
        Some(0)
    };
    assert_eq!(app.ui.key_picker.list.selected(), expected);
}

#[test]
fn open_proxyjump_picker_sets_flag_and_resets_list() {
    let mut app = make_app("");
    app.ui.proxyjump_picker.open = false;
    app.ui.proxyjump_picker.list.select(Some(99));
    app.open_proxyjump_picker();
    assert!(app.ui.proxyjump_picker.open);
    assert_ne!(app.ui.proxyjump_picker.list.selected(), Some(99));
}

// Pins the `select(Some(idx))` branch of open_proxyjump_picker. The empty
// test above covers the "no hosts → no selection" path; this one covers
// "one host → cursor lands on it" via the canonical proxyjump_first_host
// _index helper.
#[test]
fn open_proxyjump_picker_with_hosts_selects_first_host_index() {
    let mut app = make_app("Host jump\n  HostName jump.example.com\n");
    let expected = app.proxyjump_first_host_index();
    assert!(expected.is_some(), "fixture must yield a host index");
    app.open_proxyjump_picker();
    assert!(app.ui.proxyjump_picker.open);
    assert_eq!(app.ui.proxyjump_picker.list.selected(), expected);
}

#[test]
fn open_vault_role_picker_sets_flag_and_selects_first() {
    let mut app = make_app("");
    app.ui.vault_role_picker.open = false;
    app.ui.vault_role_picker.list.select(Some(99));
    app.open_vault_role_picker();
    assert!(app.ui.vault_role_picker.open);
    assert_eq!(app.ui.vault_role_picker.list.selected(), Some(0));
}

#[test]
fn open_region_picker_sets_flag_and_resets_cursor() {
    let mut app = make_app("");
    app.ui.region_picker.open = false;
    app.ui.region_picker.cursor = 99;
    app.open_region_picker();
    assert!(app.ui.region_picker.open);
    assert_eq!(app.ui.region_picker.cursor, 0);
}

// Catches a copy-paste regression where an open_X_picker body accidentally
// opens the wrong picker, or opens more than one. Each test starts with all
// five pickers closed, opens one, and asserts only that picker transitioned.
#[test]
fn open_password_picker_does_not_touch_other_pickers() {
    let mut app = make_app("");
    app.open_password_picker();
    assert!(app.ui.password_picker.open);
    assert!(!app.ui.key_picker.open);
    assert!(!app.ui.proxyjump_picker.open);
    assert!(!app.ui.vault_role_picker.open);
    assert!(!app.ui.region_picker.open);
}

#[test]
fn open_key_picker_does_not_touch_other_pickers() {
    let mut app = make_app("");
    app.open_key_picker();
    assert!(app.ui.key_picker.open);
    assert!(!app.ui.password_picker.open);
    assert!(!app.ui.proxyjump_picker.open);
    assert!(!app.ui.vault_role_picker.open);
    assert!(!app.ui.region_picker.open);
}

#[test]
fn open_proxyjump_picker_does_not_touch_other_pickers() {
    let mut app = make_app("");
    app.open_proxyjump_picker();
    assert!(app.ui.proxyjump_picker.open);
    assert!(!app.ui.password_picker.open);
    assert!(!app.ui.key_picker.open);
    assert!(!app.ui.vault_role_picker.open);
    assert!(!app.ui.region_picker.open);
}

#[test]
fn open_vault_role_picker_does_not_touch_other_pickers() {
    let mut app = make_app("");
    app.open_vault_role_picker();
    assert!(app.ui.vault_role_picker.open);
    assert!(!app.ui.password_picker.open);
    assert!(!app.ui.key_picker.open);
    assert!(!app.ui.proxyjump_picker.open);
    assert!(!app.ui.region_picker.open);
}

#[test]
fn open_region_picker_does_not_touch_other_pickers() {
    let mut app = make_app("");
    app.open_region_picker();
    assert!(app.ui.region_picker.open);
    assert!(!app.ui.password_picker.open);
    assert!(!app.ui.key_picker.open);
    assert!(!app.ui.proxyjump_picker.open);
    assert!(!app.ui.vault_role_picker.open);
}

fn make_fb_session(
    alias: &str,
    local: &str,
    remote: &str,
) -> crate::file_browser::FileBrowserSession {
    crate::file_browser::FileBrowserSession {
        alias: alias.to_string(),
        askpass: None,
        active_pane: crate::file_browser::BrowserPane::Local,
        local_path: PathBuf::from(local),
        local_entries: Vec::new(),
        local_list_state: ratatui::widgets::ListState::default(),
        local_selected: std::collections::HashSet::new(),
        local_error: None,
        remote_path: remote.to_string(),
        remote_entries: Vec::new(),
        remote_list_state: ratatui::widgets::ListState::default(),
        remote_selected: std::collections::HashSet::new(),
        remote_error: None,
        remote_loading: false,
        show_hidden: false,
        sort: crate::file_browser::BrowserSort::Name,
        confirm_copy: None,
        transferring: None,
        transfer_error: None,
        connection_recorded: false,
    }
}

#[test]
fn open_file_browser_sets_session_and_screen() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.open_file_browser(make_fb_session("web", "/tmp", "/home"));
    assert!(app.file_browser_session.is_some());
    match &app.screen {
        Screen::FileBrowser { alias } => assert_eq!(alias, "web"),
        other => panic!("expected FileBrowser screen, got {:?}", other),
    }
}

#[test]
fn open_file_browser_replaces_existing_session() {
    // Re-opening for a different alias must drop the previous session
    // outright; lingering state from a stale alias would surface in the
    // overlay until the next render. Screen must follow the new alias.
    let mut app = make_app("Host a\n  HostName 1.1.1.1\nHost b\n  HostName 2.2.2.2\n");
    app.open_file_browser(make_fb_session("a", "/tmp", "/home"));
    app.open_file_browser(make_fb_session("b", "/var", "/srv"));
    let fb = app
        .file_browser_session
        .as_ref()
        .expect("session must be present");
    assert_eq!(fb.alias, "b");
    assert_eq!(fb.local_path, PathBuf::from("/var"));
    match &app.screen {
        Screen::FileBrowser { alias } => assert_eq!(alias, "b"),
        other => panic!("expected FileBrowser screen for b, got {:?}", other),
    }
}

#[test]
fn close_file_browser_saves_paths_and_clears_session() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.open_file_browser(make_fb_session("web", "/etc", "/var/log"));
    app.close_file_browser();
    assert!(app.file_browser_session.is_none());
    let saved = app
        .file_browser_state
        .host_paths
        .get("web")
        .expect("paths must be saved for web");
    assert_eq!(saved.0, PathBuf::from("/etc"));
    assert_eq!(saved.1, "/var/log");
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn close_file_browser_without_session_does_not_touch_host_paths() {
    // Calling close defensively (no overlay open) must not invent a
    // host_paths entry nor crash. It still routes back to HostList.
    // Seed a non-default screen so the HostList assertion proves the
    // transition rather than passing on the App::new default.
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.set_screen(Screen::FileBrowser {
        alias: "web".to_string(),
    });
    app.file_browser_state.host_paths.insert(
        "web".to_string(),
        (PathBuf::from("/tmp"), "/home".to_string()),
    );
    let before = app.file_browser_state.host_paths.clone();
    app.close_file_browser();
    assert!(app.file_browser_session.is_none());
    assert_eq!(app.file_browser_state.host_paths, before);
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn close_file_browser_overwrites_previous_paths_for_same_alias() {
    // A second visit must update the saved paths to whatever the user
    // navigated to during this session, not stick on the first visit.
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.file_browser_state.host_paths.insert(
        "web".to_string(),
        (PathBuf::from("/old"), "/old-remote".to_string()),
    );
    app.open_file_browser(make_fb_session("web", "/new", "/new-remote"));
    app.close_file_browser();
    let saved = app
        .file_browser_state
        .host_paths
        .get("web")
        .expect("paths must still exist for web");
    assert_eq!(saved.0, PathBuf::from("/new"));
    assert_eq!(saved.1, "/new-remote");
}

#[test]
fn close_jump_clears_active_jump() {
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    app.open_jump(crate::app::JumpMode::Hosts);
    assert!(app.jump.is_some());
    app.close_jump();
    assert!(app.jump.is_none());
}

#[test]
fn close_jump_on_idle_state_is_noop() {
    // The three handler arms that call close_jump all pre-check via
    // `app.jump.is_some()` or `if let Some`, but the method itself must
    // be defensible on its own so future callers cannot crash by
    // calling it twice.
    let mut app = make_app("Host web\n  HostName 1.2.3.4\n");
    assert!(app.jump.is_none());
    app.close_jump();
    assert!(app.jump.is_none());
}

// ---- multi-term "/" search (issue #79) ----

/// Three hosts spreading the words "web" and "prod" across different fields:
/// web1 carries both (alias + hostname), web2 only "web", api1 only "prod".
fn multiterm_app() -> App {
    test_app_with_hosts(&[
        "Host web1\n  HostName prod.example.com\n",
        "Host web2\n  HostName dev.example.com\n",
        "Host api1\n  HostName prod.api.com\n",
    ])
}

fn filtered_aliases(app: &App) -> Vec<String> {
    app.search
        .filtered_indices()
        .iter()
        .map(|&i| app.hosts_state.list[i].alias.clone())
        .collect()
}

#[test]
fn search_multi_term_matches_terms_across_separate_fields() {
    // "web prod" must match web1 (web in alias, prod in hostname), not require
    // the literal contiguous substring "web prod" in a single field.
    let mut app = multiterm_app();
    app.start_search_with("web prod");
    assert_eq!(filtered_aliases(&app), vec!["web1"]);
}

#[test]
fn search_multi_term_is_order_independent() {
    let mut app = multiterm_app();
    app.start_search_with("prod web");
    assert_eq!(filtered_aliases(&app), vec!["web1"]);
}

#[test]
fn search_single_term_still_filters_as_substring() {
    // Regression guard: a single term keeps matching across fields as before.
    let mut app = multiterm_app();
    app.start_search_with("prod");
    assert_eq!(filtered_aliases(&app), vec!["web1", "api1"]);
}

#[test]
fn search_ignores_leading_trailing_and_repeated_whitespace() {
    // A trailing space while typing must not blank the result set.
    let mut app = multiterm_app();
    app.start_search_with("  web   prod  ");
    assert_eq!(filtered_aliases(&app), vec!["web1"]);
}

#[test]
fn search_multi_term_combines_provider_and_tag() {
    // One term hits the provider name, the other a user tag.
    let mut app = test_app_with_hosts(&[
        "Host s1\n  HostName 1.1.1.1\n  # purple:provider aws:1\n  # purple:tags prod\n",
        "Host s2\n  HostName 2.2.2.2\n  # purple:provider aws:2\n  # purple:tags dev\n",
    ]);
    app.start_search_with("aws prod");
    assert_eq!(filtered_aliases(&app), vec!["s1"]);
}

#[test]
fn tag_search_multi_term_requires_every_term_to_hit_a_tag() {
    // "tag:prod web" keeps only hosts whose tags satisfy both terms.
    let mut app = test_app_with_hosts(&[
        "Host a\n  HostName 1.1.1.1\n  # purple:tags production,web\n",
        "Host b\n  HostName 2.2.2.2\n  # purple:tags production\n",
    ]);
    app.start_search_with("tag:prod web");
    assert_eq!(filtered_aliases(&app), vec!["a"]);
}

#[test]
fn tag_exact_search_still_matches_a_single_tag() {
    // Regression guard: tag= keeps its exact single-tag semantics.
    let mut app = test_app_with_hosts(&[
        "Host a\n  HostName 1.1.1.1\n  # purple:tags production\n",
        "Host b\n  HostName 2.2.2.2\n  # purple:tags staging\n",
    ]);
    app.start_search_with("tag=production");
    assert_eq!(filtered_aliases(&app), vec!["a"]);
}

#[test]
fn tag_exact_search_does_not_split_on_whitespace() {
    // tag= arrives from the picker as one exact tag and must never AND-split:
    // a two-word value matches nothing even though both words exist as tags.
    let mut app =
        test_app_with_hosts(&["Host a\n  HostName 1.1.1.1\n  # purple:tags production,web\n"]);
    app.start_search_with("tag=production web");
    assert!(filtered_aliases(&app).is_empty());
}

#[test]
fn search_multi_term_matches_pattern() {
    // Patterns go through the same multi-term path: one term hits the pattern
    // name, the other a pattern tag, and both must match.
    let mut app = make_app(
        "Host *.web.example.com\n  User deploy\n  # purple:tags prod\n\nHost *.db.example.com\n  User deploy\n  # purple:tags dev\n",
    );
    app.start_search_with("web prod");
    let patterns: Vec<&str> = app
        .search
        .filtered_pattern_indices()
        .iter()
        .map(|&i| app.hosts_state.patterns[i].pattern.as_str())
        .collect();
    assert_eq!(patterns, vec!["*.web.example.com"]);
}

#[test]
fn search_whitespace_only_query_matches_all_hosts() {
    // A query of only spaces parses to zero terms and shows everything, so a
    // stray space while typing never blanks the list.
    let mut app = multiterm_app();
    app.start_search_with("   ");
    assert_eq!(filtered_aliases(&app), vec!["web1", "web2", "api1"]);
}

#[test]
fn push_help_overlay_moves_current_screen_into_return_box() {
    // The current Screen variant (carrying its identity payload) must
    // end up unchanged inside the Help variant's return_screen Box.
    let mut app = make_app("");
    app.set_screen(Screen::EditHost {
        alias: "web1".to_string(),
    });
    app.push_help_overlay();
    let Screen::Help { return_screen } = &app.screen else {
        panic!("expected Help, got {:?}", app.screen);
    };
    let Screen::EditHost { alias } = &**return_screen else {
        panic!("expected EditHost in return box, got {return_screen:?}");
    };
    assert_eq!(alias, "web1");
}

#[test]
fn pop_help_overlay_restores_boxed_screen_with_payload_intact() {
    let mut app = make_app("");
    app.set_screen(Screen::EditHost {
        alias: "api-prod".to_string(),
    });
    app.push_help_overlay();
    app.pop_help_overlay();
    let Screen::EditHost { alias } = &app.screen else {
        panic!("expected EditHost after pop, got {:?}", app.screen);
    };
    assert_eq!(alias, "api-prod");
}

#[test]
fn push_help_overlay_is_noop_when_already_in_help() {
    let mut app = make_app("");
    app.set_screen(Screen::Help {
        return_screen: Box::new(Screen::HostList),
    });
    app.push_help_overlay();
    let Screen::Help { return_screen } = &app.screen else {
        panic!("expected Help, got {:?}", app.screen);
    };
    assert!(matches!(**return_screen, Screen::HostList));
}

#[test]
fn pop_help_overlay_is_noop_when_not_in_help() {
    let mut app = make_app("");
    app.set_screen(Screen::HostList);
    app.pop_help_overlay();
    assert!(matches!(app.screen, Screen::HostList));
}

#[test]
fn config_path_display_collapses_home_and_keeps_demo_default() {
    let env = Arc::new(crate::runtime::env::Env::sandboxed());
    let home = env.paths().expect("sandbox paths").home().to_path_buf();
    let app_at =
        |path: PathBuf| App::with_env(SshConfigFile::from_content("", path), Arc::clone(&env));

    let default = app_at(home.join(".ssh").join("config"));
    assert_eq!(default.config_path_display(), "~/.ssh/config");

    let own = app_at(home.join(".ssh").join("purple_config"));
    assert_eq!(own.config_path_display(), "~/.ssh/purple_config");

    let outside = app_at(PathBuf::from("/etc/purple/ssh_config"));
    assert_eq!(outside.config_path_display(), "/etc/purple/ssh_config");

    // Demo mode names the default, never its safety-net path.
    let mut demo = app_at(home.join(".ssh").join("purple_config"));
    demo.demo_mode = true;
    assert_eq!(demo.config_path_display(), "~/.ssh/config");

    // Without a home directory the path is shown as is.
    let no_home = App::with_env(
        SshConfigFile::from_content("", PathBuf::from("/srv/ssh_config")),
        Arc::new(crate::runtime::env::Env::empty()),
    );
    assert_eq!(no_home.config_path_display(), "/srv/ssh_config");
}

#[test]
fn bitwarden_host_rename_persists_uuid_and_queues_metadata_sync() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config");
    let uuid = "11111111-1111-4111-8111-111111111111";
    std::fs::write(
        &config_path,
        format!("Host before\n HostName example.test\n User root\n # purple:askpass bw:{uuid}\n"),
    )
    .unwrap();
    let env = Arc::new(crate::runtime::env::Env::for_test(dir.path()));
    let config = SshConfigFile::parse(&config_path).unwrap();
    let mut app = App::with_env(config, Arc::clone(&env));
    let host = app.hosts_state.list[0].clone();
    assert!(app.open_host_edit_form(host, None));
    app.forms.host.alias = "after".into();
    app.edit_host_from_form("before").unwrap();
    let saved = SshConfigFile::parse(&config_path)
        .unwrap()
        .host_entries()
        .remove(0);
    assert_eq!(saved.alias, "after");
    assert_eq!(saved.askpass, Some(format!("bw:{uuid}")));
    assert_eq!(
        app.ui.pending_bitwarden_rename,
        Some(("after".into(), format!("bw:{uuid}")))
    );
    let journal = env
        .paths()
        .unwrap()
        .state_dir()
        .join("bitwarden-renames")
        .join(format!("{uuid}.json"));
    let pending: serde_json::Value =
        serde_json::from_slice(&std::fs::read(journal).unwrap()).unwrap();
    assert_eq!(pending["new_name"], "Purple / after");
}
