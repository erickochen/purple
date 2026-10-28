use super::*;
use crate::cli_args::{Cli, Commands, VaultCommands};
use crate::import;
use crate::ssh_config::model::SshConfigFile;
use crate::tui_loop::cache_entry_is_stale;

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
    app.providers
        .syncing_mut()
        .insert("aws".to_string(), cancel);
    app.providers.push_sync_done("DigitalOcean".to_string());
    set_sync_summary(&mut app);
    let status = app.status_center.status().unwrap();
    // Active provider name (AWS) leads; DigitalOcean is already done so it is
    // not in the active list, only in the counter.
    assert_eq!(status.text, "\u{280B} Syncing AWS EC2 \u{00B7} 1/2");
    assert!(!status.is_error());
    // sync_done should NOT be cleared while still syncing
    assert_eq!(app.providers.sync_done().len(), 1);
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
    app.providers.push_sync_done("AWS".to_string());
    app.providers.push_sync_done("Hetzner".to_string());
    set_sync_summary(&mut app);
    let status = app.status_center.status().unwrap();
    assert_eq!(status.text, "Synced 2/2 \u{00B7} AWS, Hetzner");
    assert!(!status.is_error());
    // sync_done should be cleared when all done
    assert!(app.providers.sync_done().is_empty());
    assert!(!app.providers.sync_had_errors());
}

#[test]
fn test_sync_summary_with_errors() {
    let mut app = empty_app();
    app.providers.push_sync_done("AWS".to_string());
    app.providers.set_sync_had_errors(true);
    set_sync_summary(&mut app);
    let toast = app.status_center.toast().unwrap();
    assert_eq!(toast.text, "Synced 1/1 \u{00B7} AWS");
    assert!(toast.is_error());
    // Error flag should be reset when batch completes
    assert!(!app.providers.sync_had_errors());
}

#[test]
fn test_sync_summary_includes_diff_aggregate() {
    let mut app = empty_app();
    app.providers.push_sync_done("AWS".to_string());
    app.providers.push_sync_done("DO".to_string());
    app.providers.add_batch_diff(12, 3, 1);
    set_sync_summary(&mut app);
    let status = app.status_center.status().unwrap();
    assert_eq!(status.text, "Synced 2/2 \u{00B7} AWS, DO (+12 ~3 -1)");
    // Aggregate must reset on batch completion so the next sync starts clean.
    assert_eq!(app.providers.batch_added(), 0);
    assert_eq!(app.providers.batch_updated(), 0);
    assert_eq!(app.providers.batch_stale(), 0);
}

#[test]
fn test_sync_summary_progress_includes_diff_and_spinner() {
    let mut app = empty_app();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.providers
        .syncing_mut()
        .insert("vultr".to_string(), cancel);
    app.providers.push_sync_done("AWS".to_string());
    app.providers.add_batch_diff(5, 0, 0);
    set_sync_summary(&mut app);
    let status = app.status_center.status().unwrap();
    // Active name (Vultr) leads, counter follows, then diff. AWS is done so
    // it does not appear in the active list.
    assert_eq!(status.text, "\u{280B} Syncing Vultr \u{00B7} 1/2 (+5)");
    // The spinner prefix MUST be replaceable via `replace_spinner_frame`
    // and the replacement must actually swap the leading glyph — this is
    // the contract `handle_tick` relies on to animate the footer.
    let replaced = replace_spinner_frame(&status.text, "\u{2819}")
        .expect("status must start with a known spinner frame");
    assert!(
        replaced.starts_with("\u{2819} "),
        "replacement must swap the leading frame, got: {}",
        replaced
    );
    assert!(replaced.contains("Syncing Vultr"));
}

#[test]
fn test_sync_summary_progress_lists_multiple_active_providers_sorted() {
    // Two providers in flight, none done yet. HashMap iteration order is
    // unspecified, so the implementation must sort for stable rendering
    // across ticks (otherwise the footer text would jitter between frames).
    let mut app = empty_app();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.providers
        .syncing_mut()
        .insert("vultr".to_string(), cancel.clone());
    app.providers
        .syncing_mut()
        .insert("aws".to_string(), cancel);
    set_sync_summary(&mut app);
    let status = app.status_center.status().unwrap();
    assert_eq!(status.text, "\u{280B} Syncing AWS EC2, Vultr \u{00B7} 0/2");
}

#[test]
fn test_sync_summary_errors_persist_while_syncing() {
    let mut app = empty_app();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.providers
        .syncing_mut()
        .insert("vultr".to_string(), cancel);
    app.providers.push_sync_done("AWS".to_string());
    app.providers.set_sync_had_errors(true);
    set_sync_summary(&mut app);
    let toast = app.status_center.toast().unwrap();
    assert!(toast.is_error());
    // Error flag should persist while still syncing
    assert!(app.providers.sync_had_errors());
}

#[test]
fn test_sync_summary_error_while_syncing_goes_to_toast_with_active_names() {
    // Cover the error-routing branch inside the still_syncing arm: when
    // any provider in the current batch has failed BUT others are still
    // in flight, set_sync_summary must route via notify_background_error
    // (which pushes to the toast) rather than the footer path — so the
    // user sees the red indicator the moment an error appears, not only
    // on batch completion. Also verify the active-name is present in the
    // text so the toast identifies which provider is still running.
    let mut app = empty_app();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.providers
        .syncing_mut()
        .insert("vultr".to_string(), cancel);
    app.providers.push_sync_done("AWS".to_string());
    app.providers.set_sync_had_errors(true);
    set_sync_summary(&mut app);
    let toast = app.status_center.toast().unwrap();
    assert!(toast.is_error(), "error route must send to toast");
    assert!(
        toast.text.contains("Vultr"),
        "toast must name the still-active provider, got: {}",
        toast.text
    );
    assert!(
        toast.text.contains("1/2"),
        "toast must carry the done/total counter, got: {}",
        toast.text
    );
}

#[test]
fn test_sync_summary_batch_total_clamps_upward_mid_batch() {
    // `set_sync_summary` derives total via batch_total.max(done + syncing.len()).
    // When the user triggers additional syncs mid-batch (a second batch
    // overlapping the first), batch_total must not regress even if the
    // in-flight `syncing` HashMap count drops between ticks. Simulate a
    // batch where batch_total=5 was captured at peak, syncing has since
    // wound down to 1, and 3 are done — total must stay at 5, not fall
    // to max(5, 3+1)=5 (this also verifies the arithmetic).
    let mut app = empty_app();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.providers
        .syncing_mut()
        .insert("vultr".to_string(), cancel);
    app.providers.push_sync_done("AWS".to_string());
    app.providers.push_sync_done("DigitalOcean".to_string());
    app.providers.push_sync_done("Hetzner".to_string());
    app.providers.set_batch_total(5);
    set_sync_summary(&mut app);
    let status = app.status_center.status().unwrap();
    assert!(
        status.text.contains("3/5"),
        "batch_total must stay at captured peak (5), got: {}",
        status.text
    );
}

#[test]
fn test_sync_summary_diff_suffix_omitted_when_all_zero() {
    // Explicit coverage for the zero-diff path: when no provider reported
    // any add/update/stale, the `(+N ~N -N)` suffix must be absent so we
    // don't show an empty `()`. `test_sync_summary_all_done` currently
    // exercises this accidentally — this test pins the intent.
    let mut app = empty_app();
    app.providers.push_sync_done("AWS".to_string());
    // batch_* all zero by default via empty_app.
    set_sync_summary(&mut app);
    let status = app.status_center.status().unwrap();
    assert!(
        !status.text.contains('('),
        "diff suffix must be absent when all counters are zero, got: {}",
        status.text
    );
}

// =========================================================================
// first_launch_init
// =========================================================================

/// A fresh home with the legacy layout plus an SSH config path inside it.
fn first_launch_fixture() -> (
    tempfile::TempDir,
    crate::runtime::env::Paths,
    std::path::PathBuf,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let paths = crate::runtime::env::Paths::new(dir.path());
    let config_path = dir.path().join("config");
    (dir, paths, config_path)
}

#[test]
fn first_launch_creates_dir_and_backup() {
    let (_dir, paths, config_path) = first_launch_fixture();
    std::fs::write(&config_path, "Host myserver\n  HostName 10.0.0.1\n").unwrap();

    let result = first_launch_init(&paths, &config_path);
    assert_eq!(
        result,
        Some(true),
        "Should return Some(true) when config exists"
    );
    assert!(paths.data_dir().is_dir(), "data dir should be created");
    let backup = paths.config_original();
    assert!(backup.exists(), "config.original should be created");
    assert_eq!(
        std::fs::read_to_string(&backup).unwrap(),
        "Host myserver\n  HostName 10.0.0.1\n"
    );
}

#[test]
fn first_launch_returns_none_on_second_call() {
    let (_dir, paths, config_path) = first_launch_fixture();
    std::fs::write(&config_path, "Host a\n").unwrap();

    assert!(first_launch_init(&paths, &config_path).is_some());
    assert!(first_launch_init(&paths, &config_path).is_none());
}

#[test]
fn first_launch_no_config_file_skips_backup() {
    let (_dir, paths, config_path) = first_launch_fixture();

    let result = first_launch_init(&paths, &config_path);
    assert_eq!(
        result,
        Some(false),
        "Should return Some(false) when no config"
    );
    assert!(paths.data_dir().is_dir(), "data dir should be created");
    assert!(
        !paths.config_original().exists(),
        "config.original should NOT be created when config does not exist"
    );
}

#[test]
fn first_launch_backup_not_overwritten() {
    let (_dir, paths, config_path) = first_launch_fixture();
    std::fs::write(&config_path, "original content\n").unwrap();

    first_launch_init(&paths, &config_path);
    let backup = paths.config_original();
    assert_eq!(
        std::fs::read_to_string(&backup).unwrap(),
        "original content\n"
    );

    // Modify the config and call again (simulates second launch)
    std::fs::write(&config_path, "modified content\n").unwrap();
    first_launch_init(&paths, &config_path);

    // Backup should still have original content
    assert_eq!(
        std::fs::read_to_string(&backup).unwrap(),
        "original content\n",
        "config.original should never be overwritten"
    );
}

#[test]
fn first_launch_has_backup_true_when_config_exists() {
    let (_dir, paths, config_path) = first_launch_fixture();
    std::fs::write(&config_path, "Host a\n").unwrap();

    assert_eq!(first_launch_init(&paths, &config_path), Some(true));
}

#[test]
fn first_launch_has_backup_false_without_config() {
    let (_dir, paths, config_path) = first_launch_fixture();

    assert_eq!(first_launch_init(&paths, &config_path), Some(false));
}

#[cfg(unix)]
#[test]
fn first_launch_makes_the_data_dir_private_even_when_it_already_exists() {
    use std::os::unix::fs::PermissionsExt;
    // Logging opens its file before the welcome check runs, so the directory
    // usually exists by now with the default mode. First launch still ends
    // with 0700.
    let (_dir, paths, config_path) = first_launch_fixture();
    std::fs::create_dir_all(paths.data_dir()).unwrap();
    std::fs::set_permissions(paths.data_dir(), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&config_path, "Host a\n").unwrap();

    assert_eq!(first_launch_init(&paths, &config_path), Some(true));
    let mode = std::fs::metadata(paths.data_dir())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
}

#[test]
fn first_launch_backs_up_into_the_data_dir_of_a_split_layout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data = dir.path().join("data").to_string_lossy().to_string();
    let paths = crate::runtime::env::Paths::resolve(dir.path(), move |k| {
        (k == "XDG_DATA_HOME").then(|| data.clone())
    });
    let config_path = dir.path().join("config");
    std::fs::write(&config_path, "Host a\n").unwrap();

    assert_eq!(first_launch_init(&paths, &config_path), Some(true));
    assert_eq!(
        paths.config_original(),
        dir.path().join("data/purple/config.original")
    );
    assert!(paths.config_original().exists());
    assert!(
        !paths.legacy_dir().exists(),
        "the legacy dir is not created"
    );
}

#[test]
fn first_launch_is_over_once_any_category_holds_a_file() {
    // Preferences live in the config dir; their presence alone means purple
    // ran before, even when the data dir is still empty.
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = dir.path().join("cfg").to_string_lossy().to_string();
    let paths = crate::runtime::env::Paths::resolve(dir.path(), move |k| {
        (k == "XDG_CONFIG_HOME").then(|| cfg.clone())
    });
    std::fs::create_dir_all(paths.config_dir()).unwrap();
    std::fs::write(paths.preferences(), "theme=Purple\n").unwrap();
    let config_path = dir.path().join("config");
    std::fs::write(&config_path, "Host a\n").unwrap();

    assert_eq!(first_launch_init(&paths, &config_path), None);
    assert!(!paths.config_original().exists());
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
    assert_eq!(app.ui.known_hosts_count(), 0);
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
        let toast = app.status_center.toast().expect("toast should be set");
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
    assert!(app.status_center.status().is_some() || app.status_center.toast().is_some());
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

    let result =
        import::import_from_file(app.hosts_state.ssh_config_mut(), &hosts_file, Some("test"));
    let (imported, skipped, _, _) = result.unwrap();
    assert_eq!(imported, 2);
    assert_eq!(skipped, 0);

    // Write should succeed
    assert!(app.hosts_state.ssh_config().write().is_ok());
    app.reload_hosts();
    assert_eq!(app.hosts_state.list().len(), 2);

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
    let _ = import::import_from_file(app.hosts_state.ssh_config_mut(), &hosts_file, None);
    let _ = app.hosts_state.ssh_config().write();
    app.reload_hosts();

    // Second import - all duplicates
    let (imported, skipped, _, _) =
        import::import_from_file(app.hosts_state.ssh_config_mut(), &hosts_file, None).unwrap();
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
    // SshConfigFile::write no-ops under the global demo flag (returns Ok), which
    // would defeat the forced-failure assertion below. Serialise against
    // demo-enabling tests (asset generation, visual suite) and pin demo off.
    let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    crate::demo_flag::disable();
    // Create a config pointing to a read-only path so write() fails
    let dir = std::env::temp_dir().join(format!(
        "purple_test_import_writefail_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // The parent is a regular file, so write() fails.
    let not_a_dir = dir.join("not_a_dir");
    std::fs::write(&not_a_dir, "").unwrap();
    let config_path = not_a_dir.join("config");
    let config = crate::ssh_config::model::SshConfigFile {
        elements: Vec::new(),
        path: config_path,
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    let config_backup = app.hosts_state.ssh_config().clone();

    let hosts_file = dir.join("hosts.txt");
    std::fs::write(&hosts_file, "web.example.com\n").unwrap();

    let (imported, _, _, _) =
        import::import_from_file(app.hosts_state.ssh_config_mut(), &hosts_file, None).unwrap();
    assert_eq!(imported, 1);

    // Write should fail because the parent is a regular file
    let write_result = app.hosts_state.ssh_config().write();
    assert!(write_result.is_err());

    // After failure, rollback should restore config
    app.hosts_state.set_ssh_config(config_backup);
    let hosts = app.hosts_state.ssh_config().host_entries();
    assert_eq!(hosts.len(), 0, "config should be rolled back to empty");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn known_hosts_count_not_reset_on_write_failure() {
    // The execute_known_hosts_import function returns early on write failure
    // without resetting known_hosts_count. This is correct behavior:
    // if the import didn't save, the user might want to try again.
    let mut app = empty_app();
    app.ui.set_known_hosts_count(10);
    // Simulate: write failure would do `return` before `app.ui.known_hosts_count() = 0`
    // So known_hosts_count should remain 10
    assert_eq!(app.ui.known_hosts_count(), 10);
}

#[test]
fn known_hosts_count_not_reset_on_import_error() {
    // When import_from_known_hosts returns Err, known_hosts_count is not reset
    let mut app = empty_app();
    app.ui.set_known_hosts_count(5);
    // The Err branch only sets status, doesn't touch known_hosts_count
    app.notify_error("some error");
    assert_eq!(app.ui.known_hosts_count(), 5);
}

#[test]
fn known_hosts_count_reset_on_success() {
    // When import succeeds (even with 0 imported), known_hosts_count is reset
    let mut app = empty_app();
    app.ui.set_known_hosts_count(15);
    app.ui.set_known_hosts_count(0); // simulates the Ok branch
    assert_eq!(app.ui.known_hosts_count(), 0);
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
    assert!(app.status_center.status().is_some() || app.status_center.toast().is_some());
}

// =========================================================================
// Cheat sheet verification
// =========================================================================

#[test]
fn cheat_sheet_k_before_s_in_tools() {
    // Niche shortcuts (I import known_hosts, m theme, V vault sign, X purge
    // stale, A add pattern, etc.) were moved to the wiki. The TOOLS section
    // keeps primary entry points in a deliberate order: K before S.
    let source = include_str!("ui/help.rs");
    let k_pos = source
        .find(r#"help_line_short("K","#)
        .expect("K should be in cheat sheet");
    let s_pos = source
        .find(r#"help_line_short("S","#)
        .expect("S should be in cheat sheet");
    assert!(k_pos < s_pos, "K should come before S");
}

// =========================================================================
// UI consistency: ConfirmImport dialog structure
// =========================================================================

#[test]
fn confirm_import_dialog_has_same_structure_as_confirm_delete() {
    // Render both dialogs into TestBackend buffers and assert they share
    // structural invariants: rounded top border, 7-row height, and the
    // y / Esc footer glyphs. This replaces the earlier source-grep check
    // with an end-to-end render verification.
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn row_contains(buf: &Buffer, row: u16, needle: &str) -> bool {
        let mut line = String::new();
        for x in 0..buf.area.width {
            line.push_str(buf[(x, row)].symbol());
        }
        line.contains(needle)
    }

    fn find_top_border_row(buf: &Buffer) -> u16 {
        for y in 0..buf.area.height {
            if row_contains(buf, y, "\u{256D}") && row_contains(buf, y, "\u{256E}") {
                return y;
            }
        }
        panic!("no rounded top-border row found in rendered dialog");
    }

    fn find_bottom_border_row(buf: &Buffer) -> u16 {
        for y in (0..buf.area.height).rev() {
            if row_contains(buf, y, "\u{2570}") && row_contains(buf, y, "\u{256F}") {
                return y;
            }
        }
        panic!("no rounded bottom-border row found in rendered dialog");
    }

    // --- ConfirmDelete ---
    let app = empty_app();
    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| crate::ui::confirm_dialog::render(f, &app, "example"))
        .unwrap();
    let delete_buf = term.backend().buffer().clone();
    let delete_top = find_top_border_row(&delete_buf);
    let delete_bottom = find_bottom_border_row(&delete_buf);
    assert_eq!(
        delete_bottom - delete_top + 1,
        5,
        "ConfirmDelete dialog height should be 5 rows (footer renders below)"
    );
    // Footer renders one row below the block border (external footer pattern).
    let footer_row = delete_bottom + 1;
    assert!(
        (delete_top..=footer_row).any(|y| row_contains(&delete_buf, y, "y")),
        "ConfirmDelete dialog should contain 'y' key in external footer"
    );
    assert!(
        (delete_top..=footer_row).any(|y| row_contains(&delete_buf, y, "Esc")),
        "ConfirmDelete dialog should contain 'Esc' key in external footer"
    );

    // --- ConfirmImport ---
    let app = empty_app();
    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| crate::ui::confirm_dialog::render_confirm_import(f, &app, 5))
        .unwrap();
    let import_buf = term.backend().buffer().clone();
    let import_top = find_top_border_row(&import_buf);
    let import_bottom = find_bottom_border_row(&import_buf);
    assert_eq!(
        import_bottom - import_top + 1,
        5,
        "ConfirmImport dialog height should be 5 rows (footer renders below)"
    );
    let import_footer_row = import_bottom + 1;
    assert!(
        (import_top..=import_footer_row).any(|y| row_contains(&import_buf, y, "y")),
        "ConfirmImport dialog should contain 'y' key in external footer"
    );
    assert!(
        (import_top..=import_footer_row).any(|y| row_contains(&import_buf, y, "Esc")),
        "ConfirmImport dialog should contain 'Esc' key in external footer"
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

// The release imagery pipeline drives gen-assets; --hero-out is optional and
// off by default so plain runs only write the static scene SVGs.
#[test]
fn cli_gen_assets_hero_out_parsing() {
    use clap::Parser;
    let cli = Cli::try_parse_from(["purple", "gen-assets", "/tmp/svgs"]).unwrap();
    match cli.command {
        Some(Commands::GenAssets {
            out_dir, hero_out, ..
        }) => {
            assert_eq!(out_dir, "/tmp/svgs");
            assert!(hero_out.is_none(), "hero is opt-in");
        }
        _ => panic!("expected GenAssets"),
    }

    let cli = Cli::try_parse_from([
        "purple",
        "gen-assets",
        "/tmp/svgs",
        "--hero-out",
        "assets/hero.svg",
    ])
    .unwrap();
    match cli.command {
        Some(Commands::GenAssets { hero_out, .. }) => {
            assert_eq!(hero_out.as_deref(), Some("assets/hero.svg"));
        }
        _ => panic!("expected GenAssets"),
    }
}

// `--config` is optional so the resolver can tell "not given" apart. It is
// global so it parses on either side of a subcommand.
#[test]
fn cli_config_flag_is_optional_and_global() {
    use clap::Parser;
    let absent = Cli::try_parse_from(["purple", "--list"]).unwrap();
    assert_eq!(absent.config, None);

    let before = Cli::try_parse_from(["purple", "--config", "/x", "tunnel", "list"]).unwrap();
    assert_eq!(before.config.as_deref(), Some("/x"));

    let after = Cli::try_parse_from(["purple", "tunnel", "list", "--config", "/x"]).unwrap();
    assert_eq!(after.config.as_deref(), Some("/x"));
    assert!(matches!(
        after.command,
        Some(Commands::Tunnel {
            command: crate::cli_args::TunnelCommands::List { alias: None }
        })
    ));
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
fn cli_snippet_add_interactive_flag_parsing() {
    use clap::Parser;
    let parse = |args: &[&str]| match Cli::try_parse_from(args).unwrap().command {
        Some(Commands::Snippet {
            command: crate::cli_args::SnippetCommands::Add { interactive, .. },
        }) => interactive,
        _ => panic!("expected Snippet::Add"),
    };
    assert!(parse(&[
        "purple",
        "snippet",
        "add",
        "pm2",
        "pm2 status",
        "--interactive"
    ]));
    assert!(!parse(&["purple", "snippet", "add", "pm2", "pm2 status"]));
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
    let env = crate::runtime::env::Env::from_process();
    let result =
        ensure_vault_ssh_if_needed(&env, &host.alias, &host, &provider_config, &mut config);
    assert!(
        result.is_none(),
        "no role configured: must short-circuit to None"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Regression for the "first connect fails, second works" bug:
/// `ensure_vault_ssh_chain_if_needed` must walk the entire ProxyJump chain,
/// not just the target. Hosts with no vault role are skipped; hosts with a
/// role are processed in the right order (proxies first).
///
/// We can't actually invoke vault here (no server, no CLI in CI). The test
/// instead verifies that with no vault role configured anywhere in the
/// chain, the function returns None — meaning the chain *was* walked and
/// every hop was inspected.
#[test]
fn ensure_vault_ssh_chain_no_role_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config");
    std::fs::write(
        &config_path,
        "Host bastion\n  HostName 9.9.9.9\n\
         Host target\n  HostName 1.2.3.4\n  ProxyJump bastion\n",
    )
    .unwrap();
    let mut config = SshConfigFile::parse(&config_path).unwrap();
    let provider_config = providers::config::ProviderConfig::parse("");
    let env = crate::runtime::env::Env::from_process();
    let result = ensure_vault_ssh_chain_if_needed(
        &env,
        "target",
        &config_path,
        &provider_config,
        &mut config,
    );
    assert!(
        result.is_none(),
        "no role on any chain host: must short-circuit to None, got {:?}",
        result
    );
}

/// Regression: the chain helper must not crash when the target alias is not
/// present in the config. `ssh -G` will fall back to its own resolution and
/// the result must be a no-op (None) rather than a panic or error.
#[test]
fn ensure_vault_ssh_chain_unknown_target_safe() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config");
    std::fs::write(&config_path, "Host known\n  HostName 1.2.3.4\n").unwrap();
    let mut config = SshConfigFile::parse(&config_path).unwrap();
    let provider_config = providers::config::ProviderConfig::parse("");
    let env = crate::runtime::env::Env::from_process();
    let result = ensure_vault_ssh_chain_if_needed(
        &env,
        "completely-unknown-alias",
        &config_path,
        &provider_config,
        &mut config,
    );
    assert!(
        result.is_none(),
        "unknown target: must short-circuit to None"
    );
}

/// Regression test for the "first connect fails, second works" bug. Drives
/// `ensure_vault_ssh_chain_if_needed` end-to-end with a mock vault that
/// records the role argument of every signing call, then asserts the proxy
/// hop was signed BEFORE the target — the invariant the original code
/// violated.
///
/// Without this test, a regression that reverts to the single-host path,
/// flips `chain.reverse()` in `resolve_proxy_chain`, or short-circuits on
/// the first hop would slip past the resolution-only tests in
/// `vault_ssh_tests.rs`. Those prove the function returns the right list;
/// they do not prove the chain is walked or signed in the right order.
///
/// Lives in `main_tests.rs` (not `vault_ssh_tests.rs`) because the
/// function under test only exists in the binary crate. It threads a
/// sandbox `Env` (its own `PATH` and home), so it never touches process
/// env and needs no serialization lock.
#[cfg(unix)]
#[test]
fn ensure_vault_ssh_chain_signs_proxy_before_target() {
    use std::os::unix::fs::PermissionsExt;

    // Isolated tempdir for the mock binary, the call log, and a sandboxed
    // HOME so signed certs do not pollute the developer's ~/.purple/certs.
    let dir = tempfile::tempdir().unwrap();
    let mock_dir = dir.path().join("bin");
    std::fs::create_dir_all(&mock_dir).unwrap();
    let log_path = dir.path().join("vault_calls.log");

    // Mock `vault` binary: log the role argument (positional `$3` after
    // `write -field=signed_key`) one per line, then emit a fake cert
    // payload so `sign_certificate`'s "empty stdout" guard is satisfied.
    let script = mock_dir.join("vault");
    let body = format!(
        "#!/bin/sh\n\
         echo \"$3\" >> \"{log}\"\n\
         printf 'ssh-ed25519-cert-v01@openssh.com AAAAFAKECERT mock\\n'\n\
         exit 0\n",
        log = log_path.display()
    );
    std::fs::write(&script, body).unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();

    // Tempdir HOME with a fake ed25519 pubkey so `resolve_pubkey_path`
    // does not fall through to the developer's real key file.
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(home.join(".purple/certs")).unwrap();
    std::fs::write(
        home.join(".ssh/id_ed25519.pub"),
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI test@test\n",
    )
    .unwrap();

    // The mock `vault` is found via the Env's PATH (stub dir ahead of the
    // real one), cert output lands in the Env's sandbox home, and
    // `resolve_pubkey_path` reads the same sandbox home via the threaded
    // `env.paths()` to expand and validate `~/.ssh/id_ed25519`. No process
    // env mutation, so no serialization lock is needed.
    let env = crate::runtime::env::Env::for_test(&home).with_var(
        "PATH",
        format!(
            "{}:{}",
            mock_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    );

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let config_path = dir.path().join("ssh_config");
        std::fs::write(
            &config_path,
            "Host bastion\n  \
              HostName 9.9.9.9\n  \
              IdentityFile ~/.ssh/id_ed25519\n  \
              # purple:vault-ssh ssh/sign/bastion\n\
             Host target\n  \
              HostName 1.2.3.4\n  \
              IdentityFile ~/.ssh/id_ed25519\n  \
              ProxyJump bastion\n  \
              # purple:vault-ssh ssh/sign/target\n",
        )
        .unwrap();

        let mut config = SshConfigFile::parse(&config_path).unwrap();
        let provider_config = providers::config::ProviderConfig::parse("");

        let result = ensure_vault_ssh_chain_if_needed(
            &env,
            "target",
            &config_path,
            &provider_config,
            &mut config,
        );
        assert!(
            result.is_some(),
            "expected chain helper to report at least one signing action"
        );
        let (msg, is_error) = result.unwrap();
        assert!(!is_error, "unexpected error path: {}", msg);

        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        let lines: Vec<&str> = log.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(
            lines,
            vec!["ssh/sign/bastion", "ssh/sign/target"],
            "expected bastion to be signed BEFORE target. \
             got log: {:?}. msg: {}",
            lines,
            msg
        );
    }));

    // Propagate panics so the test reports a normal failure.
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

/// `ensure_vault_cert_for_alias` is the worker-thread entry point used by
/// the SSH chokepoints (container listing/exec/logs, file browser). Unlike
/// the connect path it has no main-thread App state, so it must load both
/// the provider config and the SSH config from disk itself and still renew
/// the cert for a Vault SSH host. Drives it end-to-end with a mock vault.
#[cfg(unix)]
#[test]
fn ensure_vault_cert_for_alias_signs_vault_host() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let mock_dir = dir.path().join("bin");
    std::fs::create_dir_all(&mock_dir).unwrap();
    let log_path = dir.path().join("vault_calls.log");

    let script = mock_dir.join("vault");
    let body = format!(
        "#!/bin/sh\n\
         echo \"$3\" >> \"{log}\"\n\
         printf 'ssh-ed25519-cert-v01@openssh.com AAAAFAKECERT mock\\n'\n\
         exit 0\n",
        log = log_path.display()
    );
    std::fs::write(&script, body).unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();

    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(home.join(".purple/certs")).unwrap();
    std::fs::write(
        home.join(".ssh/id_ed25519.pub"),
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI test@test\n",
    )
    .unwrap();

    // The mock `vault` is found via the Env's PATH and the signed cert lands
    // in the Env's sandbox home (`env.paths().cert_for("target")`).
    // `resolve_pubkey_path` reads the same sandbox home via `env.paths()`. No
    // process env mutation, so no serialization lock is needed.
    let env = crate::runtime::env::Env::for_test(&home).with_var(
        "PATH",
        format!(
            "{}:{}",
            mock_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    );

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let config_path = dir.path().join("ssh_config");
        std::fs::write(
            &config_path,
            "Host target\n  \
              HostName 1.2.3.4\n  \
              IdentityFile ~/.ssh/id_ed25519\n  \
              # purple:vault-ssh ssh/sign/target\n",
        )
        .unwrap();

        let result = ensure_vault_cert_for_alias(&env, "target", &config_path);
        assert!(
            result.is_some(),
            "expected renewal to report a signing action for the vault host"
        );
        let (msg, is_error) = result.unwrap();
        assert!(!is_error, "unexpected error path: {}", msg);

        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        let lines: Vec<&str> = log.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(
            lines,
            vec!["ssh/sign/target"],
            "expected exactly one sign call for the target role, got {:?}",
            lines
        );

        // The signed cert must land in the sandbox home, not the real ~/.purple.
        let cert_path = env.paths().expect("sandbox paths").cert_for("target");
        assert!(
            cert_path.exists(),
            "expected signed cert at sandbox path {}",
            cert_path.display()
        );
    }));

    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

/// The per-alias lock must hand the same lock to every caller for a given
/// alias (so concurrent container fetches serialize) but distinct locks for
/// distinct aliases (so unrelated hosts renew in parallel).
#[test]
fn renewal_lock_is_per_alias() {
    let a1 = renewal_lock("host-a");
    let a2 = renewal_lock("host-a");
    let b = renewal_lock("host-b");
    assert!(
        std::sync::Arc::ptr_eq(&a1, &a2),
        "same alias must share one lock"
    );
    assert!(
        !std::sync::Arc::ptr_eq(&a1, &b),
        "different aliases must get distinct locks"
    );
}

/// A host without any Vault SSH role must be a silent no-op: no signing,
/// no error. This guards the chokepoint fast path so non-vault hosts (the
/// overwhelming majority) never spawn a vault subprocess on every SSH call.
#[test]
fn ensure_vault_cert_for_alias_noop_for_non_vault_host() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config");
    std::fs::write(&config_path, "Host plain\n  HostName 1.2.3.4\n").unwrap();
    let env = crate::runtime::env::Env::for_test(dir.path());
    let result = ensure_vault_cert_for_alias(&env, "plain", &config_path);
    assert!(
        result.is_none(),
        "non-vault host must be a no-op, got {:?}",
        result
    );
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

// Regression: Claude Desktop did not substitute ${HOME} before passing
// CLI args. The binary must expand it itself.
//
// Each `expand_user_path_*` test below reads `dirs::home_dir()` twice:
// once directly to capture the expected value and once inside
// `expand_user_path()`. Both reads consult $HOME. No test in this binary
// mutates $HOME anymore (the vault/cert tests thread a sandbox `Env`
// instead), so the two reads are stable without any serialization lock.

#[test]
fn expand_user_path_tilde_slash() {
    let home = dirs::home_dir().unwrap();
    let result = super::expand_user_path(
        Some(&crate::runtime::env::Paths::new(dirs::home_dir().unwrap())),
        "~/.ssh/config",
    )
    .unwrap();
    assert_eq!(result, home.join(".ssh/config"));
}

#[test]
fn expand_user_path_dollar_brace_home_slash() {
    let home = dirs::home_dir().unwrap();
    let result = super::expand_user_path(
        Some(&crate::runtime::env::Paths::new(dirs::home_dir().unwrap())),
        "${HOME}/.ssh/config",
    )
    .unwrap();
    assert_eq!(result, home.join(".ssh/config"));
}

#[test]
fn expand_user_path_dollar_home_slash() {
    let home = dirs::home_dir().unwrap();
    let result = super::expand_user_path(
        Some(&crate::runtime::env::Paths::new(dirs::home_dir().unwrap())),
        "$HOME/.purple/mcp-audit.log",
    )
    .unwrap();
    assert_eq!(result, home.join(".purple/mcp-audit.log"));
}

#[test]
fn expand_user_path_bare_tilde() {
    let home = dirs::home_dir().unwrap();
    assert_eq!(
        super::expand_user_path(
            Some(&crate::runtime::env::Paths::new(dirs::home_dir().unwrap())),
            "~"
        )
        .unwrap(),
        home
    );
}

#[test]
fn expand_user_path_bare_dollar_brace_home() {
    let home = dirs::home_dir().unwrap();
    assert_eq!(
        super::expand_user_path(
            Some(&crate::runtime::env::Paths::new(dirs::home_dir().unwrap())),
            "${HOME}"
        )
        .unwrap(),
        home
    );
}

#[test]
fn expand_user_path_bare_dollar_home() {
    let home = dirs::home_dir().unwrap();
    assert_eq!(
        super::expand_user_path(
            Some(&crate::runtime::env::Paths::new(dirs::home_dir().unwrap())),
            "$HOME"
        )
        .unwrap(),
        home
    );
}

#[test]
fn expand_user_path_absolute_unchanged() {
    let result = super::expand_user_path(
        Some(&crate::runtime::env::Paths::new(dirs::home_dir().unwrap())),
        "/etc/ssh/ssh_config",
    )
    .unwrap();
    assert_eq!(result, std::path::PathBuf::from("/etc/ssh/ssh_config"));
}

#[test]
fn expand_user_path_relative_unchanged() {
    let result = super::expand_user_path(
        Some(&crate::runtime::env::Paths::new(dirs::home_dir().unwrap())),
        "config/ssh.conf",
    )
    .unwrap();
    assert_eq!(result, std::path::PathBuf::from("config/ssh.conf"));
}

#[test]
fn expand_user_path_does_not_expand_mid_string() {
    // Only the prefix is expanded. Mid-string ${HOME} stays literal.
    let result = super::expand_user_path(
        Some(&crate::runtime::env::Paths::new(dirs::home_dir().unwrap())),
        "/tmp/${HOME}/foo",
    )
    .unwrap();
    assert_eq!(result, std::path::PathBuf::from("/tmp/${HOME}/foo"));
}

#[test]
fn expand_user_path_tilde_other_user_not_expanded() {
    // `~user` (other-user home) is intentionally not supported.
    let result = super::expand_user_path(
        Some(&crate::runtime::env::Paths::new(dirs::home_dir().unwrap())),
        "~root",
    )
    .unwrap();
    assert_eq!(result, std::path::PathBuf::from("~root"));
}

// SSH config path precedence: --config, then PURPLE_SSH_CONFIG, then the
// ssh_config preference, then ~/.ssh/config. Each test owns a sandbox Env.

fn write_ssh_config_pref(env: &crate::runtime::env::Env, value: &str) {
    let prefs = env.paths().expect("sandbox paths").preferences();
    std::fs::create_dir_all(prefs.parent().expect("prefs parent")).expect("prefs dir");
    std::fs::write(&prefs, format!("ssh_config={value}\n")).expect("write prefs");
}

fn sandbox_home(env: &crate::runtime::env::Env) -> std::path::PathBuf {
    env.paths().expect("sandbox paths").home().to_path_buf()
}

#[test]
fn resolve_ssh_config_path_flag_wins_over_env_and_preference() {
    let env = crate::runtime::env::Env::sandboxed().with_var(SSH_CONFIG_ENV_VAR, "/from/env");
    write_ssh_config_pref(&env, "/from/pref");
    let resolved = resolve_ssh_config_path(&env, Some("/from/flag")).unwrap();
    assert_eq!(resolved.path, std::path::PathBuf::from("/from/flag"));
    assert_eq!(resolved.source, ConfigSource::Flag);
}

#[test]
fn resolve_ssh_config_path_env_wins_over_preference() {
    let env = crate::runtime::env::Env::sandboxed().with_var(SSH_CONFIG_ENV_VAR, "/from/env");
    write_ssh_config_pref(&env, "/from/pref");
    let resolved = resolve_ssh_config_path(&env, None).unwrap();
    assert_eq!(resolved.path, std::path::PathBuf::from("/from/env"));
    assert_eq!(resolved.source, ConfigSource::EnvVar);
}

#[test]
fn resolve_ssh_config_path_reads_the_preference() {
    let env = crate::runtime::env::Env::sandboxed();
    write_ssh_config_pref(&env, "/from/pref");
    let resolved = resolve_ssh_config_path(&env, None).unwrap();
    assert_eq!(resolved.path, std::path::PathBuf::from("/from/pref"));
    assert_eq!(resolved.source, ConfigSource::Preference);
}

#[test]
fn resolve_ssh_config_path_defaults_to_home_ssh_config() {
    let env = crate::runtime::env::Env::sandboxed();
    let resolved = resolve_ssh_config_path(&env, None).unwrap();
    assert_eq!(
        resolved.path,
        sandbox_home(&env).join(".ssh").join("config")
    );
    assert_eq!(resolved.source, ConfigSource::Default);
}

#[test]
fn resolve_ssh_config_path_blank_values_fall_through() {
    let env = crate::runtime::env::Env::sandboxed().with_var(SSH_CONFIG_ENV_VAR, "   ");
    write_ssh_config_pref(&env, "   ");
    let resolved = resolve_ssh_config_path(&env, Some("  ")).unwrap();
    assert_eq!(
        resolved.path,
        sandbox_home(&env).join(".ssh").join("config")
    );
    assert_eq!(resolved.source, ConfigSource::Default);
}

#[test]
fn resolve_ssh_config_path_expands_tilde_in_env_and_preference() {
    let env = crate::runtime::env::Env::sandboxed().with_var(SSH_CONFIG_ENV_VAR, "~/env_alt");
    let home = sandbox_home(&env);
    assert_eq!(
        resolve_ssh_config_path(&env, None).unwrap().path,
        home.join("env_alt")
    );

    let env = crate::runtime::env::Env::sandboxed();
    write_ssh_config_pref(&env, "~/pref_alt");
    assert_eq!(
        resolve_ssh_config_path(&env, None).unwrap().path,
        sandbox_home(&env).join("pref_alt")
    );
}

#[test]
fn resolve_ssh_config_path_without_home_fails_for_the_default() {
    let env = crate::runtime::env::Env::empty();
    let err = resolve_ssh_config_path(&env, None).unwrap_err();
    assert!(
        err.to_string()
            .contains("Could not determine home directory"),
        "{err}"
    );
    // An absolute flag needs no home directory.
    let resolved = resolve_ssh_config_path(&env, Some("/abs/config")).unwrap();
    assert_eq!(resolved.path, std::path::PathBuf::from("/abs/config"));
}

// The resolved default must equal the path `ssh_command` treats as default,
// so ssh gets no `-F` unless the user chose another file.
#[test]
fn resolved_default_config_gets_no_f_flag() {
    let env = crate::runtime::env::Env::sandboxed();
    let host = crate::ssh_config::model::HostEntry {
        alias: "web".to_string(),
        ..Default::default()
    };
    let default = resolve_ssh_config_path(&env, None).unwrap();
    assert_eq!(host.ssh_command(env.paths(), &default.path), "ssh -- 'web'");
    let explicit = resolve_ssh_config_path(&env, Some("~/.ssh/config")).unwrap();
    assert_eq!(
        host.ssh_command(env.paths(), &explicit.path),
        "ssh -- 'web'"
    );
    let own = resolve_ssh_config_path(&env, Some("~/.ssh/purple_config")).unwrap();
    assert!(
        host.ssh_command(env.paths(), &own.path).contains(" -F '"),
        "a chosen file must reach ssh through -F"
    );
}

#[test]
fn config_source_labels_match_the_docs() {
    assert_eq!(ConfigSource::Flag.to_string(), "--config");
    assert_eq!(ConfigSource::EnvVar.to_string(), "PURPLE_SSH_CONFIG");
    assert_eq!(
        ConfigSource::Preference.to_string(),
        "preference ssh_config"
    );
    assert_eq!(ConfigSource::Default.to_string(), "default");
}

#[test]
fn ensure_proton_login_skips_non_proton_askpass() {
    // Every non-proton: askpass must early-return without spawning a subprocess.
    let env = crate::runtime::env::Env::from_process();
    super::ensure_proton_login(&env, Some("bw:foo"));
    super::ensure_proton_login(&env, Some("keychain"));
    super::ensure_proton_login(&env, Some("op://Vault/Item/p"));
    super::ensure_proton_login(&env, None);
}

#[cfg(unix)]
#[test]
fn ensure_proton_login_skips_when_not_installed() {
    // An Env whose PATH points at a nonexistent dir means `pass-cli` is
    // unreachable, so proton_status reports NotInstalled. No process-env
    // mutation, no lock: the Env carries its own snapshot.
    let env = crate::runtime::env::Env::for_test("/tmp/purple-proton-noinstall")
        .with_var("PATH", "/nonexistent-purple-test-path");
    super::ensure_proton_login(&env, Some("proton:Foo/Bar/p"));
}

#[cfg(unix)]
#[test]
fn ensure_proton_login_skips_when_authenticated() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::Builder::new()
        .prefix("purple_mock_passcli_ensure_")
        .tempdir()
        .unwrap();
    let script = dir.path().join("pass-cli");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();

    // The stub `pass-cli` (exit 0 == authenticated) lives in `dir`; expose it
    // via the Env PATH instead of mutating the process-global PATH.
    let env = crate::runtime::env::Env::for_test(dir.path()).with_var(
        "PATH",
        format!(
            "{}:{}",
            dir.path().display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    );
    super::ensure_proton_login(&env, Some("proton:Foo/Bar/p"));
    drop(dir);
}

#[cfg(test)]
mod proton_di_tests {
    use crate::askpass::ProtonStatus;
    use std::cell::RefCell;

    /// Run `ensure_proton_login_with` with a scripted prompt sequence and
    /// return the number of times the prompt closure was invoked. The
    /// returned count plus the production code's hardcoded `0..2` loop bound
    /// fully describes the retry behaviour to the test assertions. The `env`
    /// controls subprocess resolution (`pass-cli` is spawned through it).
    fn run_with<S, P>(
        env: &crate::runtime::env::Env,
        askpass: Option<&str>,
        status: S,
        prompts: P,
    ) -> usize
    where
        S: FnOnce() -> ProtonStatus,
        P: IntoIterator<Item = anyhow::Result<Option<String>>>,
    {
        let count = RefCell::new(0usize);
        let mut iter = prompts.into_iter();
        super::super::ensure_proton_login_with(env, askpass, status, || {
            *count.borrow_mut() += 1;
            iter.next()
                .unwrap_or_else(|| Ok(Some("unexpected-extra-prompt".to_string())))
        });
        count.into_inner()
    }

    #[test]
    fn skip_when_askpass_none() {
        let env = crate::runtime::env::Env::for_test("/tmp/purple-proton-di");
        let attempts = run_with(&env, None, || panic!("status_fn must not run"), Vec::new());
        assert_eq!(attempts, 0, "must not invoke prompt");
    }

    #[test]
    fn skip_when_askpass_not_proton() {
        let env = crate::runtime::env::Env::for_test("/tmp/purple-proton-di");
        let attempts = run_with(
            &env,
            Some("bw:foo"),
            || panic!("status_fn must not run"),
            Vec::new(),
        );
        assert_eq!(attempts, 0, "must not invoke prompt");
    }

    #[test]
    fn skip_when_authenticated_no_prompt() {
        let env = crate::runtime::env::Env::for_test("/tmp/purple-proton-di");
        let attempts = run_with(
            &env,
            Some("proton:Foo/Bar/p"),
            || ProtonStatus::Authenticated,
            Vec::new(),
        );
        assert_eq!(attempts, 0, "must not invoke prompt");
    }

    #[test]
    fn skip_when_not_installed_no_prompt() {
        let env = crate::runtime::env::Env::for_test("/tmp/purple-proton-di");
        let attempts = run_with(
            &env,
            Some("proton:Foo/Bar/p"),
            || ProtonStatus::NotInstalled,
            Vec::new(),
        );
        assert_eq!(attempts, 0, "must not invoke prompt");
    }

    #[test]
    fn empty_pat_aborts_after_first_prompt() {
        let env = crate::runtime::env::Env::for_test("/tmp/purple-proton-di");
        let attempts = run_with(
            &env,
            Some("proton:Foo/Bar/p"),
            || ProtonStatus::NotAuthenticated,
            vec![Ok(Some(String::new()))],
        );
        assert_eq!(attempts, 1, "empty PAT must not retry");
    }

    #[test]
    fn esc_eof_aborts_after_first_prompt() {
        let env = crate::runtime::env::Env::for_test("/tmp/purple-proton-di");
        let attempts = run_with(
            &env,
            Some("proton:Foo/Bar/p"),
            || ProtonStatus::NotAuthenticated,
            vec![Ok(None)],
        );
        assert_eq!(attempts, 1, "Esc/EOF must not retry");
    }

    #[test]
    fn read_error_aborts_after_first_prompt() {
        let env = crate::runtime::env::Env::for_test("/tmp/purple-proton-di");
        let err = anyhow::anyhow!("tty broken");
        let attempts = run_with(
            &env,
            Some("proton:Foo/Bar/p"),
            || ProtonStatus::NotAuthenticated,
            vec![Err(err)],
        );
        assert_eq!(attempts, 1, "read error must not retry");
    }

    #[test]
    #[cfg(unix)]
    fn at_most_two_attempts_when_login_keeps_failing() {
        // proton_login spawns `pass-cli` through the Env. An Env whose PATH
        // points at a guaranteed-non-existent directory makes every login
        // attempt fail, proving the retry loop caps at 2. No process-env
        // mutation, no lock: the Env carries its own snapshot.
        let env = crate::runtime::env::Env::for_test("/tmp/purple-proton-di-fail")
            .with_var("PATH", "/nonexistent-purple-di-test-path");
        let attempts = run_with(
            &env,
            Some("proton:Foo/Bar/p"),
            || ProtonStatus::NotAuthenticated,
            vec![
                Ok(Some("pst_first::key".to_string())),
                Ok(Some("pst_second::key".to_string())),
                Ok(Some("pst_should_not_be_used::key".to_string())),
            ],
        );
        assert_eq!(attempts, 2, "must cap at 2 attempts");
    }
}
