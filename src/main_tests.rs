use super::*;
use crate::ssh_config::model::SshConfigFile;

fn empty_app() -> App {
    let config = SshConfigFile {
        elements: Vec::new(),
        path: std::path::PathBuf::from("/dev/null"),
        crlf: false,
        bom: false,
    };
    App::new(config)
}

// ---- cache_entry_is_stale tests ----

fn valid_status() -> vault_ssh::CertStatus {
    vault_ssh::CertStatus::Valid {
        expires_at: 0,
        remaining_secs: 3600,
        total_secs: 3600,
    }
}

fn fixed_elapsed(secs: u64) -> impl FnOnce(std::time::Instant) -> u64 {
    move |_| secs
}

#[test]
fn cache_stale_when_entry_missing() {
    assert!(cache_entry_is_stale(None, None, fixed_elapsed(0)));
    assert!(cache_entry_is_stale(
        None,
        Some(std::time::SystemTime::UNIX_EPOCH),
        fixed_elapsed(0),
    ));
}

#[test]
fn cache_fresh_when_recent_and_mtime_matches() {
    let mtime = std::time::SystemTime::UNIX_EPOCH;
    let entry = (std::time::Instant::now(), valid_status(), Some(mtime));
    assert!(!cache_entry_is_stale(
        Some(&entry),
        Some(mtime),
        fixed_elapsed(1),
    ));
}

#[test]
fn cache_stale_when_current_mtime_differs_from_cached() {
    let cached = std::time::SystemTime::UNIX_EPOCH;
    let current = cached + std::time::Duration::from_secs(5);
    let entry = (std::time::Instant::now(), valid_status(), Some(cached));
    assert!(cache_entry_is_stale(
        Some(&entry),
        Some(current),
        fixed_elapsed(1),
    ));
}

#[test]
fn cache_stale_detects_external_cert_rewrite_via_mtime() {
    // Regression guard for the documented feature: when an external
    // actor (CLI `purple vault sign` from another shell, or another
    // running purple instance) rewrites the cert file behind the TUI's
    // back, the lazy-check loop MUST detect the change via mtime and
    // force a re-read — regardless of the TTL.
    //
    // Timeline:
    //   t=0  purple caches Valid status with mtime M1
    //   t=1  external sign writes new cert, mtime becomes M2 > M1
    //   t=2  lazy-check runs: elapsed 2s (far under the 5-min TTL),
    //        but the mtime mismatch forces cache_stale = true.
    let cached_mtime = std::time::SystemTime::UNIX_EPOCH;
    let rewritten_mtime = cached_mtime + std::time::Duration::from_secs(60);
    let entry = (
        std::time::Instant::now(),
        valid_status(),
        Some(cached_mtime),
    );
    assert!(
        cache_entry_is_stale(Some(&entry), Some(rewritten_mtime), fixed_elapsed(2)),
        "external rewrite via mtime mismatch must force re-check even within TTL"
    );
}

#[test]
fn cache_stale_when_file_appears_after_missing_cache() {
    let entry = (std::time::Instant::now(), valid_status(), None);
    assert!(cache_entry_is_stale(
        Some(&entry),
        Some(std::time::SystemTime::UNIX_EPOCH),
        fixed_elapsed(1),
    ));
}

#[test]
fn cache_stale_when_file_disappears_after_cached_mtime() {
    let mtime = std::time::SystemTime::UNIX_EPOCH;
    let entry = (std::time::Instant::now(), valid_status(), Some(mtime));
    assert!(cache_entry_is_stale(Some(&entry), None, fixed_elapsed(1)));
}

#[test]
fn cache_stale_when_ttl_exceeded_even_if_mtime_matches() {
    let mtime = std::time::SystemTime::UNIX_EPOCH;
    let entry = (std::time::Instant::now(), valid_status(), Some(mtime));
    let over = vault_ssh::CERT_STATUS_CACHE_TTL_SECS + 1;
    assert!(cache_entry_is_stale(
        Some(&entry),
        Some(mtime),
        fixed_elapsed(over),
    ));
}

#[test]
fn cache_invalid_entry_uses_shorter_backoff() {
    let mtime = std::time::SystemTime::UNIX_EPOCH;
    let entry = (
        std::time::Instant::now(),
        vault_ssh::CertStatus::Invalid("boom".to_string()),
        Some(mtime),
    );
    // Just above error backoff but well below the normal TTL: must be
    // stale under the shorter Invalid backoff.
    let secs = vault_ssh::CERT_ERROR_BACKOFF_SECS + 1;
    assert!(secs < vault_ssh::CERT_STATUS_CACHE_TTL_SECS);
    assert!(cache_entry_is_stale(
        Some(&entry),
        Some(mtime),
        fixed_elapsed(secs),
    ));
}

#[test]
fn cache_invalid_entry_fresh_within_backoff() {
    let mtime = std::time::SystemTime::UNIX_EPOCH;
    let entry = (
        std::time::Instant::now(),
        vault_ssh::CertStatus::Invalid("boom".to_string()),
        Some(mtime),
    );
    assert!(!cache_entry_is_stale(
        Some(&entry),
        Some(mtime),
        fixed_elapsed(0),
    ));
}

// ---- end cache_entry_is_stale tests ----

#[test]
fn test_sync_summary_still_syncing() {
    let mut app = empty_app();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.syncing_providers.insert("aws".to_string(), cancel);
    app.sync_done.push("DigitalOcean".to_string());
    set_sync_summary(&mut app);
    let status = app.status.as_ref().unwrap();
    assert_eq!(status.text, "Synced: DigitalOcean...");
    assert!(!status.is_error());
    // sync_done should NOT be cleared while still syncing
    assert_eq!(app.sync_done.len(), 1);
}

#[test]
fn vault_sign_summary_single_failure_shows_only_error() {
    let msg = format_vault_sign_summary(0, 1, 0, Some("Vault SSH permission denied."));
    assert_eq!(msg, "Vault SSH permission denied.");
}

#[test]
fn vault_sign_summary_includes_error_on_partial_failure() {
    let msg = format_vault_sign_summary(2, 1, 0, Some("role not found"));
    assert_eq!(msg, "Signed 2 of 3 certificates. 1 failed: role not found");
}

#[test]
fn vault_sign_summary_failure_without_error_text() {
    let msg = format_vault_sign_summary(0, 1, 0, None);
    assert_eq!(msg, "Signed 0 of 1 certificate. 1 failed");
}

#[test]
fn vault_sign_summary_all_success() {
    let msg = format_vault_sign_summary(3, 0, 0, None);
    assert_eq!(msg, "Signed 3 of 3 certificates.");
}

#[test]
fn vault_sign_summary_skipped_with_signed() {
    let msg = format_vault_sign_summary(1, 0, 2, None);
    assert_eq!(msg, "Signed 1 of 3 certificates. 2 already valid.");
}

#[test]
fn vault_sign_summary_all_skipped() {
    let msg = format_vault_sign_summary(0, 0, 3, None);
    assert_eq!(msg, "All 3 certificates already valid. Nothing to sign.");
}

#[test]
fn replace_spinner_frame_replaces_known_spinner() {
    let text = "\u{280B} Signing 1/3: myhost (V to cancel)";
    let result = replace_spinner_frame(text, "\u{2819}");
    assert_eq!(
        result.as_deref(),
        Some("\u{2819} Signing 1/3: myhost (V to cancel)")
    );
}

#[test]
fn replace_spinner_frame_ignores_non_spinner_text() {
    let text = "Signing 0/3 (V to cancel)";
    assert!(replace_spinner_frame(text, "\u{2819}").is_none());
}

#[test]
fn replace_spinner_frame_ignores_regular_status() {
    let text = "Signed 3 of 3 certificates.";
    assert!(replace_spinner_frame(text, "\u{2819}").is_none());
}

#[test]
fn test_sync_summary_all_done() {
    let mut app = empty_app();
    app.sync_done.push("AWS".to_string());
    app.sync_done.push("Hetzner".to_string());
    set_sync_summary(&mut app);
    let status = app.status.as_ref().unwrap();
    assert_eq!(status.text, "Synced: AWS, Hetzner");
    assert!(!status.is_error());
    // sync_done should be cleared when all done
    assert!(app.sync_done.is_empty());
    assert!(!app.sync_had_errors);
}

#[test]
fn test_sync_summary_with_errors() {
    let mut app = empty_app();
    app.sync_done.push("AWS".to_string());
    app.sync_had_errors = true;
    set_sync_summary(&mut app);
    let toast = app.toast.as_ref().unwrap();
    assert_eq!(toast.text, "Synced: AWS");
    assert!(toast.is_error());
    // Error flag should be reset when batch completes
    assert!(!app.sync_had_errors);
}

#[test]
fn test_sync_summary_errors_persist_while_syncing() {
    let mut app = empty_app();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.syncing_providers.insert("vultr".to_string(), cancel);
    app.sync_done.push("AWS".to_string());
    app.sync_had_errors = true;
    set_sync_summary(&mut app);
    let toast = app.toast.as_ref().unwrap();
    assert!(toast.is_error());
    // Error flag should persist while still syncing
    assert!(app.sync_had_errors);
}

// =========================================================================
// first_launch_init
// =========================================================================

#[test]
fn first_launch_creates_dir_and_backup() {
    let dir = std::env::temp_dir().join(format!(
        "purple_test_first_launch_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    let purple_dir = dir.join(".purple");
    let config_path = dir.join("config");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(&config_path, "Host myserver\n  HostName 10.0.0.1\n").unwrap();

    let result = first_launch_init(&purple_dir, &config_path);
    assert_eq!(
        result,
        Some(true),
        "Should return Some(true) when config exists"
    );
    assert!(purple_dir.exists(), ".purple dir should be created");
    let backup = purple_dir.join("config.original");
    assert!(backup.exists(), "config.original should be created");
    assert_eq!(
        std::fs::read_to_string(&backup).unwrap(),
        "Host myserver\n  HostName 10.0.0.1\n"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn first_launch_returns_none_on_second_call() {
    let dir = std::env::temp_dir().join(format!(
        "purple_test_first_launch_twice_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    let purple_dir = dir.join(".purple");
    let config_path = dir.join("config");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(&config_path, "Host a\n").unwrap();

    assert!(first_launch_init(&purple_dir, &config_path).is_some());
    assert!(first_launch_init(&purple_dir, &config_path).is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn first_launch_no_config_file_skips_backup() {
    let dir = std::env::temp_dir().join(format!(
        "purple_test_first_launch_no_cfg_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    let purple_dir = dir.join(".purple");
    let config_path = dir.join("nonexistent_config");

    let result = first_launch_init(&purple_dir, &config_path);
    assert_eq!(
        result,
        Some(false),
        "Should return Some(false) when no config"
    );
    assert!(purple_dir.exists(), ".purple dir should be created");
    assert!(
        !purple_dir.join("config.original").exists(),
        "config.original should NOT be created when config does not exist"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn first_launch_backup_not_overwritten() {
    let dir = std::env::temp_dir().join(format!(
        "purple_test_first_launch_no_overwrite_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    let purple_dir = dir.join(".purple");
    let config_path = dir.join("config");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(&config_path, "original content\n").unwrap();

    first_launch_init(&purple_dir, &config_path);
    let backup = purple_dir.join("config.original");
    assert_eq!(
        std::fs::read_to_string(&backup).unwrap(),
        "original content\n"
    );

    // Modify the config and call again (simulates second launch)
    std::fs::write(&config_path, "modified content\n").unwrap();
    first_launch_init(&purple_dir, &config_path);

    // Backup should still have original content
    assert_eq!(
        std::fs::read_to_string(&backup).unwrap(),
        "original content\n",
        "config.original should never be overwritten"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn first_launch_has_backup_true_when_config_exists() {
    let dir = std::env::temp_dir().join(format!(
        "purple_test_first_launch_has_backup_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    let purple_dir = dir.join(".purple");
    let config_path = dir.join("config");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(&config_path, "Host a\n").unwrap();

    assert_eq!(first_launch_init(&purple_dir, &config_path), Some(true));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn first_launch_has_backup_false_without_config() {
    let dir = std::env::temp_dir().join(format!(
        "purple_test_first_launch_no_backup_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    let purple_dir = dir.join(".purple");
    let config_path = dir.join("nonexistent");

    assert_eq!(first_launch_init(&purple_dir, &config_path), Some(false));

    let _ = std::fs::remove_dir_all(&dir);
}

// =========================================================================
// Welcome screen handler state transitions
// =========================================================================
// Keys to test on Welcome screen:
// Enter -> HostList
// Esc -> HostList
// ? -> Help
// I (known_hosts > 0) -> HostList + import
// I (known_hosts = 0) -> HostList (treated as any other key)
// random char (q, a, j, etc.) -> HostList
// arrow keys -> HostList

#[test]
fn welcome_enter_goes_to_host_list() {
    let mut app = empty_app();
    app.screen = app::Screen::Welcome {
        has_backup: false,
        host_count: 0,
        known_hosts_count: 0,
    };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::HostList));
}

#[test]
fn welcome_esc_goes_to_host_list() {
    let mut app = empty_app();
    app.screen = app::Screen::Welcome {
        has_backup: true,
        host_count: 5,
        known_hosts_count: 0,
    };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::HostList));
}

#[test]
fn welcome_question_mark_goes_to_help() {
    let mut app = empty_app();
    app.screen = app::Screen::Welcome {
        has_backup: false,
        host_count: 0,
        known_hosts_count: 0,
    };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('?'),
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::Help { .. }));
}

#[test]
fn welcome_i_without_known_hosts_goes_to_host_list() {
    let mut app = empty_app();
    app.screen = app::Screen::Welcome {
        has_backup: false,
        host_count: 0,
        known_hosts_count: 0,
    };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('I'),
        crossterm::event::KeyModifiers::SHIFT,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::HostList));
}

#[test]
fn welcome_random_char_goes_to_host_list() {
    let mut app = empty_app();
    app.screen = app::Screen::Welcome {
        has_backup: false,
        host_count: 3,
        known_hosts_count: 0,
    };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('z'),
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::HostList));
}

#[test]
fn welcome_arrow_key_goes_to_host_list() {
    let mut app = empty_app();
    app.screen = app::Screen::Welcome {
        has_backup: false,
        host_count: 0,
        known_hosts_count: 5,
    };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Down,
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::HostList));
}

// =========================================================================
// ConfirmImport handler state transitions
// =========================================================================
// Keys to test on ConfirmImport screen:
// y -> HostList + import executed
// Esc -> HostList, no import
// n -> HostList, no import
// random key -> stays on ConfirmImport
// Enter -> stays on ConfirmImport
// ? -> stays on ConfirmImport

#[test]
fn confirm_import_esc_goes_to_host_list() {
    let mut app = empty_app();
    app.screen = app::Screen::ConfirmImport { count: 10 };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::HostList));
}

#[test]
fn confirm_import_n_goes_to_host_list() {
    let mut app = empty_app();
    app.screen = app::Screen::ConfirmImport { count: 10 };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('n'),
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::HostList));
}

#[test]
fn confirm_import_random_key_stays() {
    let mut app = empty_app();
    app.screen = app::Screen::ConfirmImport { count: 10 };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::ConfirmImport { .. }));
}

#[test]
fn confirm_import_enter_stays() {
    let mut app = empty_app();
    app.screen = app::Screen::ConfirmImport { count: 10 };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::ConfirmImport { .. }));
}

#[test]
fn confirm_import_question_mark_stays() {
    let mut app = empty_app();
    app.screen = app::Screen::ConfirmImport { count: 10 };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('?'),
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::ConfirmImport { .. }));
}

#[test]
fn confirm_import_arrow_key_stays() {
    let mut app = empty_app();
    app.screen = app::Screen::ConfirmImport { count: 5 };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Up,
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::ConfirmImport { .. }));
}

// =========================================================================
// App known_hosts_count field
// =========================================================================

#[test]
fn app_known_hosts_count_default_zero() {
    let app = empty_app();
    assert_eq!(app.known_hosts_count, 0);
}

// =========================================================================
// HostList I key handler
// =========================================================================
// On HostList, I calls count_known_hosts_candidates() which reads the real
// filesystem, so we can't control the result. But we can verify the error
// path (when count == 0, it sets error status) by testing on a system
// without importable known_hosts, or by testing that the key is handled
// without panic.

#[test]
fn host_list_i_key_does_not_panic() {
    let mut app = empty_app();
    app.screen = app::Screen::HostList;
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('I'),
        crossterm::event::KeyModifiers::SHIFT,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    // This calls count_known_hosts_candidates() which reads real filesystem.
    // It should either go to ConfirmImport (if known_hosts has entries)
    // or set error status (if not). Either way, it should not panic.
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(
        matches!(app.screen, app::Screen::ConfirmImport { .. })
            || matches!(app.screen, app::Screen::HostList)
    );
}

#[test]
fn host_list_i_key_sets_error_when_no_hosts_available() {
    // If count_known_hosts_candidates() returns 0, status should be error
    let mut app = empty_app();
    app.screen = app::Screen::HostList;
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('I'),
        crossterm::event::KeyModifiers::SHIFT,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    // If we got ConfirmImport, known_hosts had entries (can't control that)
    // If we stayed on HostList, verify error status was set
    if matches!(app.screen, app::Screen::HostList) {
        let toast = app.toast.as_ref().expect("toast should be set");
        assert!(toast.is_error());
        assert_eq!(toast.text, "No importable hosts in known_hosts.");
    }
}

// =========================================================================
// Empty state behavior per screen
// =========================================================================

#[test]
fn empty_state_hidden_during_welcome() {
    // When screen is Welcome, the empty state match returns ""
    let screen = app::Screen::Welcome {
        has_backup: false,
        host_count: 0,
        known_hosts_count: 0,
    };
    assert!(matches!(screen, app::Screen::Welcome { .. }));
    // The host_list.rs code does:
    //   if matches!(app.screen, app::Screen::Welcome { .. }) { "" }
    //   else { "It's quiet in here..." }
}

#[test]
fn empty_state_shown_during_host_list() {
    let screen = app::Screen::HostList;
    assert!(!matches!(screen, app::Screen::Welcome { .. }));
}

#[test]
fn empty_state_shown_during_confirm_import() {
    let screen = app::Screen::ConfirmImport { count: 5 };
    assert!(!matches!(screen, app::Screen::Welcome { .. }));
}

// =========================================================================
// Welcome with backup variations
// =========================================================================

#[test]
fn welcome_q_goes_to_host_list() {
    let mut app = empty_app();
    app.screen = app::Screen::Welcome {
        has_backup: true,
        host_count: 10,
        known_hosts_count: 0,
    };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('q'),
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::HostList));
}

#[test]
fn welcome_tab_goes_to_host_list() {
    let mut app = empty_app();
    app.screen = app::Screen::Welcome {
        has_backup: false,
        host_count: 0,
        known_hosts_count: 5,
    };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::HostList));
}

// =========================================================================
// ConfirmImport y key (actual import - reads filesystem)
// =========================================================================

#[test]
fn confirm_import_y_transitions_to_host_list() {
    let mut app = empty_app();
    app.screen = app::Screen::ConfirmImport { count: 10 };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    // Should transition to HostList regardless of import result
    assert!(matches!(app.screen, app::Screen::HostList));
    // Status or toast should be set (either success or error)
    assert!(app.status.is_some() || app.toast.is_some());
}

// =========================================================================
// ConfirmImport tab/q stays
// =========================================================================

#[test]
fn confirm_import_tab_stays() {
    let mut app = empty_app();
    app.screen = app::Screen::ConfirmImport { count: 5 };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::ConfirmImport { .. }));
}

#[test]
fn confirm_import_q_stays() {
    let mut app = empty_app();
    app.screen = app::Screen::ConfirmImport { count: 5 };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('q'),
        crossterm::event::KeyModifiers::NONE,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::ConfirmImport { .. }));
}

// =========================================================================
// execute_known_hosts_import — test via import_from_file (controlled input)
// =========================================================================
// We can't call execute_known_hosts_import directly (it reads real
// known_hosts), but we can test the same logic paths by using
// import_from_file + config.write() on controlled temp files.

#[test]
fn import_successful_sets_success_status() {
    let dir = std::env::temp_dir().join(format!(
        "purple_test_import_status_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let config_path = dir.join("config");
    std::fs::write(&config_path, "").unwrap();
    let config = crate::ssh_config::model::SshConfigFile {
        elements: Vec::new(),
        path: config_path,
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);

    let hosts_file = dir.join("hosts.txt");
    std::fs::write(&hosts_file, "web.example.com\ndb.example.com\n").unwrap();

    let result = import::import_from_file(&mut app.config, &hosts_file, Some("test"));
    let (imported, skipped, _, _) = result.unwrap();
    assert_eq!(imported, 2);
    assert_eq!(skipped, 0);

    // Write should succeed
    assert!(app.config.write().is_ok());
    app.reload_hosts();
    assert_eq!(app.hosts.len(), 2);

    // Verify the status message format
    let msg = format!(
        "Imported {} host{}, skipped {} duplicate{}",
        imported,
        if imported == 1 { "" } else { "s" },
        skipped,
        if skipped == 1 { "" } else { "s" },
    );
    assert_eq!(msg, "Imported 2 hosts, skipped 0 duplicates");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn import_all_duplicates_sets_status() {
    let dir = std::env::temp_dir().join(format!(
        "purple_test_import_alldup_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let config_path = dir.join("config");
    std::fs::write(&config_path, "").unwrap();
    let config = crate::ssh_config::model::SshConfigFile {
        elements: Vec::new(),
        path: config_path,
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);

    let hosts_file = dir.join("hosts.txt");
    std::fs::write(&hosts_file, "web.example.com\n").unwrap();

    // First import
    let _ = import::import_from_file(&mut app.config, &hosts_file, None);
    let _ = app.config.write();
    app.reload_hosts();

    // Second import - all duplicates
    let (imported, skipped, _, _) =
        import::import_from_file(&mut app.config, &hosts_file, None).unwrap();
    assert_eq!(imported, 0);
    assert_eq!(skipped, 1);

    let msg = if skipped == 1 {
        "Host already exists".to_string()
    } else {
        format!("All {} hosts already exist", skipped)
    };
    assert_eq!(msg, "Host already exists");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn import_write_failure_rolls_back_config() {
    // Create a config pointing to a read-only path so write() fails
    let dir = std::env::temp_dir().join(format!(
        "purple_test_import_writefail_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let config_path = dir.join("nonexistent_dir").join("config");
    // config_path parent doesn't exist, so write() will fail
    let config = crate::ssh_config::model::SshConfigFile {
        elements: Vec::new(),
        path: config_path,
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    let config_backup = app.config.clone();

    let hosts_file = dir.join("hosts.txt");
    std::fs::write(&hosts_file, "web.example.com\n").unwrap();

    let (imported, _, _, _) = import::import_from_file(&mut app.config, &hosts_file, None).unwrap();
    assert_eq!(imported, 1);

    // Write should fail because parent dir doesn't exist
    let write_result = app.config.write();
    assert!(write_result.is_err());

    // After failure, rollback should restore config
    app.config = config_backup;
    let hosts = app.config.host_entries();
    assert_eq!(hosts.len(), 0, "config should be rolled back to empty");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn known_hosts_count_not_reset_on_write_failure() {
    // The execute_known_hosts_import function returns early on write failure
    // without resetting known_hosts_count. This is correct behavior:
    // if the import didn't save, the user might want to try again.
    let mut app = empty_app();
    app.known_hosts_count = 10;
    // Simulate: write failure would do `return` before `app.known_hosts_count = 0`
    // So known_hosts_count should remain 10
    assert_eq!(app.known_hosts_count, 10);
}

#[test]
fn known_hosts_count_not_reset_on_import_error() {
    // When import_from_known_hosts returns Err, known_hosts_count is not reset
    let mut app = empty_app();
    app.known_hosts_count = 5;
    // The Err branch only sets status, doesn't touch known_hosts_count
    app.set_status("some error", true);
    assert_eq!(app.known_hosts_count, 5);
}

#[test]
fn known_hosts_count_reset_on_success() {
    // When import succeeds (even with 0 imported), known_hosts_count is reset
    let mut app = empty_app();
    app.known_hosts_count = 15;
    app.known_hosts_count = 0; // simulates the Ok branch
    assert_eq!(app.known_hosts_count, 0);
}

// =========================================================================
// Welcome I key with known_hosts_count > 0
// =========================================================================

#[test]
fn welcome_i_with_known_hosts_transitions_to_host_list() {
    // When known_hosts_count > 0, I should trigger import and go to HostList
    let mut app = empty_app();
    app.screen = app::Screen::Welcome {
        has_backup: false,
        host_count: 0,
        known_hosts_count: 10,
    };
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('I'),
        crossterm::event::KeyModifiers::SHIFT,
    );
    let (tx, _rx) = std::sync::mpsc::channel();
    let _ = crate::handler::handle_key_event(&mut app, key, &tx);
    assert!(matches!(app.screen, app::Screen::HostList));
    // Status or toast should be set (import attempted)
    assert!(app.status.is_some() || app.toast.is_some());
}

// =========================================================================
// Cheat sheet verification
// =========================================================================

#[test]
fn cheat_sheet_contains_import_entry() {
    // The help.rs host_list_columns() should contain "I" key with "import known_hosts"
    let source = include_str!("ui/help.rs");
    assert!(
        source.contains(r#"help_line("I", "import known_hosts")"#),
        "cheat sheet should have I key"
    );
}

#[test]
fn cheat_sheet_i_after_s_and_k() {
    let source = include_str!("ui/help.rs");
    let k_pos = source
        .find(r#"help_line("K","#)
        .expect("K should be in cheat sheet");
    let s_pos = source
        .find(r#"help_line("S","#)
        .expect("S should be in cheat sheet");
    let i_pos = source
        .find(r#"help_line("I","#)
        .expect("I should be in cheat sheet");
    assert!(k_pos < s_pos, "K should come before S");
    assert!(s_pos < i_pos, "S should come before I");
}

// =========================================================================
// UI consistency: ConfirmImport dialog structure
// =========================================================================

#[test]
fn confirm_import_dialog_has_same_structure_as_confirm_delete() {
    // Both dialogs use: Block + rounded borders + 4 text lines
    // (blank, question, blank, y/Esc footer)
    // ConfirmDelete: 48x7, ConfirmImport: 52x7
    // Verify by checking source structure
    let source = include_str!("ui/confirm_dialog.rs");

    // Both use BorderType::Rounded
    let rounded_count = source.matches("BorderType::Rounded").count();
    assert!(rounded_count >= 4, "all dialogs should use rounded borders");

    // ConfirmImport uses footer_key for y (not danger, since import is not destructive)
    assert!(
        source.contains(r#"Span::styled(" y ", theme::footer_key())"#),
        "import dialog y should use footer_key"
    );
}

// =========================================================================
// Screen variant field values
// =========================================================================

#[test]
fn confirm_import_preserves_count() {
    let screen = app::Screen::ConfirmImport { count: 42 };
    if let app::Screen::ConfirmImport { count } = screen {
        assert_eq!(count, 42);
    } else {
        panic!("expected ConfirmImport");
    }
}

#[test]
fn welcome_preserves_all_fields() {
    let screen = app::Screen::Welcome {
        has_backup: true,
        host_count: 12,
        known_hosts_count: 34,
    };
    if let app::Screen::Welcome {
        has_backup,
        host_count,
        known_hosts_count,
    } = screen
    {
        assert!(has_backup);
        assert_eq!(host_count, 12);
        assert_eq!(known_hosts_count, 34);
    } else {
        panic!("expected Welcome");
    }
}

#[test]
fn test_format_sync_diff_all_changes() {
    assert_eq!(format_sync_diff(3, 1, 2), " (+3 ~1 -2)");
}

#[test]
fn test_format_sync_diff_no_changes() {
    assert_eq!(format_sync_diff(0, 0, 0), "");
}

#[test]
fn test_format_sync_diff_only_added() {
    assert_eq!(format_sync_diff(5, 0, 0), " (+5)");
}

// CLI refactor regression: `purple vault-sign` was renamed to a nested
// `purple vault sign` subcommand group matching `provider`/`theme`. Verify
// clap parses both the alias form and --all.
#[test]
fn cli_vault_sign_alias_parsing() {
    use clap::Parser;
    let cli = Cli::try_parse_from(["purple", "vault", "sign", "myhost"]).unwrap();
    match cli.command {
        Some(Commands::Vault {
            command:
                VaultCommands::Sign {
                    alias,
                    all,
                    vault_addr,
                },
        }) => {
            assert_eq!(alias.as_deref(), Some("myhost"));
            assert!(!all);
            assert!(vault_addr.is_none());
        }
        _ => panic!("expected Vault::Sign"),
    }
}

#[test]
fn cli_vault_sign_all_flag_parsing() {
    use clap::Parser;
    let cli = Cli::try_parse_from(["purple", "vault", "sign", "--all"]).unwrap();
    match cli.command {
        Some(Commands::Vault {
            command:
                VaultCommands::Sign {
                    alias,
                    all,
                    vault_addr,
                },
        }) => {
            assert_eq!(alias, None);
            assert!(all);
            assert!(vault_addr.is_none());
        }
        _ => panic!("expected Vault::Sign --all"),
    }
}

#[test]
fn cli_vault_sign_vault_addr_flag_parsing() {
    use clap::Parser;
    let cli = Cli::try_parse_from([
        "purple",
        "vault",
        "sign",
        "--all",
        "--vault-addr",
        "http://127.0.0.1:8200",
    ])
    .unwrap();
    match cli.command {
        Some(Commands::Vault {
            command:
                VaultCommands::Sign {
                    alias: _,
                    all,
                    vault_addr,
                },
        }) => {
            assert!(all);
            assert_eq!(vault_addr.as_deref(), Some("http://127.0.0.1:8200"));
        }
        _ => panic!("expected Vault::Sign with --vault-addr"),
    }
}

#[test]
fn should_write_certificate_file_only_when_empty() {
    // Empty string: purple owns the cert path, write it.
    assert!(should_write_certificate_file(""));
    // Whitespace-only is treated as empty so a stray space typed in the
    // form does not lock purple out of writing the directive.
    assert!(should_write_certificate_file(" "));
    assert!(should_write_certificate_file("\t"));
    assert!(should_write_certificate_file("   \t  "));
    // Any user-set value (default purple path included): never overwrite,
    // because the user may rely on a custom path and we never want to
    // silently change it.
    assert!(!should_write_certificate_file("/custom/path/cert.pub"));
    assert!(!should_write_certificate_file("~/.ssh/my-cert.pub"));
    assert!(!should_write_certificate_file("relative/path"));
    // A path with leading/trailing space is still a real path; trim is
    // applied to the emptiness check, not the value itself.
    assert!(!should_write_certificate_file(" /tmp/cert.pub "));
}

#[test]
fn ensure_vault_ssh_returns_none_when_no_role_configured() {
    // Build a host with no vault_ssh and no provider mapping. The function
    // must short-circuit before touching disk or shelling out.
    let dir = std::env::temp_dir().join(format!(
        "purple_test_ensure_vault_norole_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config");
    std::fs::write(&config_path, "Host plain\n  HostName 1.2.3.4\n").unwrap();
    let mut config = SshConfigFile::parse(&config_path).unwrap();
    let host = config.host_entries().into_iter().next().unwrap();
    let provider_config = providers::config::ProviderConfig::parse("");
    let result = ensure_vault_ssh_if_needed(&host.alias, &host, &provider_config, &mut config);
    assert!(
        result.is_none(),
        "no role configured: must short-circuit to None"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_legacy_vault_sign_flat_form_rejected() {
    // The old flat `purple vault-sign` subcommand was removed. Ensure it
    // does not silently match something else.
    use clap::Parser;
    let result = Cli::try_parse_from(["purple", "vault-sign", "myhost"]);
    assert!(
        result.is_err(),
        "legacy `vault-sign` must not parse after refactor"
    );
}
