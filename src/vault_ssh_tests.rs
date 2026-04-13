use super::*;

/// Module-wide lock shared by every test that mutates `PATH` to install
/// a mock `ssh-keygen` or `vault` binary. Without this, parallel tests
/// race on the process-wide environment and one test's PATH restore
/// overwrites another's mock.
#[cfg(unix)]
static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn cert_path_for_simple_alias() {
    let path = cert_path_for("webserver").unwrap();
    assert!(path.ends_with("certs/webserver-cert.pub"));
    assert!(path.to_string_lossy().contains(".purple/certs/"));
}

#[test]
fn cert_path_for_alias_with_prefix() {
    let path = cert_path_for("aws-prod-web01").unwrap();
    assert!(path.ends_with("certs/aws-prod-web01-cert.pub"));
}

/// Regression: a public key path that contains `=` would split the
/// `public_key=@<path>` argument mid-pair when handed to the Vault CLI and
/// produce a cryptic parse error. `sign_certificate` rejects such paths up
/// front so the user gets a clear actionable message instead.
#[test]
fn sign_certificate_rejects_pubkey_path_with_equals() {
    let dir = std::env::temp_dir().join(format!(
        "purple_test_pubkey_eq_{:?}",
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("key=foo.pub");
    std::fs::write(&bad, "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI test@test\n").unwrap();

    let result = sign_certificate("ssh/sign/test", &bad, "alias", None);
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains('=') && err.contains("Vault CLI"),
        "expected explicit `=` rejection, got: {}",
        err
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sign_certificate_missing_pubkey() {
    let result = sign_certificate(
        "ssh/sign/test",
        Path::new("/tmp/purple_nonexistent_key.pub"),
        "test",
        None,
    );
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Public key not found"), "got: {}", err);
}

#[test]
fn sign_certificate_vault_not_configured() {
    let tmpdir = std::env::temp_dir();
    let fake_key = tmpdir.join("purple_test_vault_sign_key.pub");
    std::fs::write(
        &fake_key,
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI test@test\n",
    )
    .unwrap();

    let result = sign_certificate("nonexistent/sign/role", &fake_key, "test-host", None);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("vault") || err.contains("Vault") || err.contains("Failed"),
        "Error should mention vault: {}",
        err
    );

    let _ = std::fs::remove_file(&fake_key);
}

#[test]
fn parse_valid_line_standard() {
    let line = "        Valid: from 2026-04-08T10:00:00 to 2026-04-09T10:00:00";
    let (from, to) = parse_valid_line(line).unwrap();
    assert!(from > 0);
    assert!(to > from);
    assert_eq!(to - from, 86400);
}

#[test]
fn parse_valid_line_no_match() {
    assert!(parse_valid_line("        Type: ssh-ed25519-cert-v01@openssh.com").is_none());
}

#[test]
fn parse_valid_line_forever() {
    let line = "        Valid: from 2026-04-08T10:00:00 to forever";
    assert!(parse_valid_line(line).is_none());
}

#[test]
fn parse_ssh_datetime_valid() {
    let epoch = parse_ssh_datetime("2026-04-08T12:00:00").unwrap();
    assert!(epoch > 1_700_000_000);
    assert!(epoch < 2_000_000_000);
}

#[test]
fn parse_ssh_datetime_invalid() {
    assert!(parse_ssh_datetime("not-a-date").is_none());
    assert!(parse_ssh_datetime("2026-13-08T12:00:00").is_none());
}

#[test]
fn check_cert_validity_missing() {
    let path = Path::new("/tmp/purple_test_nonexistent_cert.pub");
    assert_eq!(check_cert_validity(path), CertStatus::Missing);
}

#[test]
fn needs_renewal_missing() {
    assert!(needs_renewal(&CertStatus::Missing));
}

#[test]
fn needs_renewal_expired() {
    assert!(needs_renewal(&CertStatus::Expired));
}

#[test]
fn needs_renewal_invalid() {
    assert!(needs_renewal(&CertStatus::Invalid("bad".to_string())));
}

#[test]
fn needs_renewal_valid_plenty_of_time() {
    assert!(!needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: 3600,
        total_secs: 3600,
    }));
}

#[test]
fn needs_renewal_valid_under_threshold() {
    assert!(needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: 60,
        total_secs: 3600,
    }));
}

#[test]
fn needs_renewal_at_threshold_boundary() {
    // A freshly signed cert with remaining == threshold must NOT trigger
    // renewal. Otherwise a cert whose TTL equals the threshold (or close
    // to it) would be re-signed on every check, causing an infinite loop.
    assert!(!needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: RENEWAL_THRESHOLD_SECS,
        total_secs: 3600,
    }));
    // Just under the threshold is the renewal tipping point.
    assert!(needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: RENEWAL_THRESHOLD_SECS - 1,
        total_secs: 3600,
    }));
    // Above threshold: still valid.
    assert!(!needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: RENEWAL_THRESHOLD_SECS + 1,
        total_secs: 3600,
    }));
}

#[test]
fn needs_renewal_short_ttl_freshly_signed_not_renewed() {
    // Regression: a cert with a total TTL shorter than RENEWAL_THRESHOLD_SECS
    // must not be flagged for renewal the instant it is signed. Prior to the
    // fix this caused an infinite re-sign loop for sub-5-minute roles.
    let total = 120i64; // 2-minute role
    // Freshly signed: remaining ~= total.
    assert!(!needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: total,
        total_secs: total,
    }));
    // Half-life: still valid under the proportional threshold (total/2 = 60).
    assert!(!needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: 61,
        total_secs: total,
    }));
    // Under proportional threshold: renew.
    assert!(needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: 30,
        total_secs: total,
    }));
}

#[test]
fn needs_renewal_total_zero_uses_fixed_threshold() {
    // total_secs == 0 is unusual (forever certs use i64::MAX) but must
    // not divide by zero or trigger the proportional path. Fall back to
    // the fixed 5-minute threshold.
    assert!(!needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: RENEWAL_THRESHOLD_SECS + 1,
        total_secs: 0,
    }));
    assert!(needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: RENEWAL_THRESHOLD_SECS - 1,
        total_secs: 0,
    }));
}

#[test]
fn needs_renewal_total_one_uses_proportional_threshold() {
    // total_secs == 1: proportional threshold is 1/2 == 0. With `<`
    // comparison, remaining == 0 does NOT renew, which matches the
    // "don't re-sign a cert that just expired on the client clock"
    // intent. (CertStatus::Expired is the normal path for that.)
    assert!(!needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: 1,
        total_secs: 1,
    }));
}

#[test]
fn needs_renewal_forever_cert_never_renews() {
    // "Valid: forever" certs use i64::MAX for both remaining and total.
    // These must never be flagged for renewal regardless of threshold.
    assert!(!needs_renewal(&CertStatus::Valid {
        expires_at: i64::MAX,
        remaining_secs: i64::MAX,
        total_secs: i64::MAX,
    }));
}

#[test]
fn cert_error_backoff_is_shorter_than_normal_ttl() {
    // The lazy cert-check loop picks a shorter TTL for Invalid entries so
    // transient check failures recover quickly without hammering the
    // background thread on every poll tick. This invariant is structural
    // — if a future change swaps the constants the lazy-check branch in
    // main.rs becomes useless. Enforced at compile time via const block.
    const _: () = assert!(CERT_ERROR_BACKOFF_SECS < CERT_STATUS_CACHE_TTL_SECS);
    const _: () = assert!(CERT_ERROR_BACKOFF_SECS >= 5);
}

#[test]
fn needs_renewal_negative_remaining_is_expired() {
    // Defensive: a negative remaining (clock skew) falls under the
    // normal threshold so the caller re-signs. check_cert_validity
    // actually returns Expired in this case, but needs_renewal must
    // also be correct standalone.
    assert!(needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: -100,
        total_secs: 3600,
    }));
}

#[test]
fn needs_renewal_short_ttl_at_exact_threshold() {
    // Boundary case: remaining == total/2 should NOT renew (uses `<`).
    let total = 200i64;
    assert!(!needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: 100,
        total_secs: total,
    }));
    assert!(needs_renewal(&CertStatus::Valid {
        expires_at: 0,
        remaining_secs: 99,
        total_secs: total,
    }));
}

#[test]
fn resolve_pubkey_from_identity_file() {
    let path = resolve_pubkey_path("~/.ssh/id_rsa").unwrap();
    let s = path.to_string_lossy();
    assert!(s.ends_with("id_rsa.pub"), "got: {}", s);
    assert!(!s.contains('~'), "tilde should be expanded: {}", s);
}

#[test]
fn resolve_pubkey_already_pub_no_double_suffix() {
    let path = resolve_pubkey_path("~/.ssh/id_ed25519.pub").unwrap();
    let s = path.to_string_lossy();
    assert!(s.ends_with("id_ed25519.pub"), "got: {}", s);
    assert!(!s.ends_with(".pub.pub"), "double .pub suffix: {}", s);
}

#[test]
fn resolve_pubkey_empty_falls_back() {
    let path = resolve_pubkey_path("").unwrap();
    let s = path.to_string_lossy();
    assert!(s.ends_with("id_ed25519.pub"), "got: {}", s);
    assert!(s.contains(".ssh/"), "should be in .ssh dir: {}", s);
}

#[test]
fn resolve_pubkey_absolute_path_inside_home() {
    // An absolute path inside the user's home should be honored.
    let home = dirs::home_dir().expect("home dir");
    let abs = home.join(".ssh/deploy_key");
    let path = resolve_pubkey_path(abs.to_str().unwrap()).unwrap();
    let expected = home.join(".ssh/deploy_key.pub");
    assert_eq!(path, expected);
}

#[test]
fn resolve_vault_role_host_override() {
    let config = crate::providers::config::ProviderConfig::default();
    let role = resolve_vault_role(Some("ssh/sign/admin"), Some("aws"), &config);
    assert_eq!(role.as_deref(), Some("ssh/sign/admin"));
}

// ---- is_valid_vault_addr tests ----

#[test]
fn is_valid_vault_addr_accepts_typical_urls() {
    assert!(is_valid_vault_addr("http://127.0.0.1:8200"));
    assert!(is_valid_vault_addr("https://vault.example.com:8200"));
    assert!(is_valid_vault_addr("https://vault.internal/v1"));
}

#[test]
fn is_valid_vault_addr_rejects_empty_and_blank() {
    assert!(!is_valid_vault_addr(""));
    assert!(!is_valid_vault_addr("   "));
    assert!(!is_valid_vault_addr("\t"));
}

#[test]
fn is_valid_vault_addr_rejects_whitespace_inside() {
    assert!(!is_valid_vault_addr("http://host :8200"));
    assert!(!is_valid_vault_addr("http://host\t:8200"));
}

#[test]
fn is_valid_vault_addr_rejects_control_chars() {
    assert!(!is_valid_vault_addr("http://host\n8200"));
    assert!(!is_valid_vault_addr("http://host\r8200"));
    assert!(!is_valid_vault_addr("http://host\x00:8200"));
}

#[test]
fn is_valid_vault_addr_rejects_overlong() {
    let long = "http://".to_string() + &"a".repeat(600);
    assert!(!is_valid_vault_addr(&long));
}

// ---- resolve_vault_addr tests ----

#[test]
fn resolve_vault_addr_none_when_nothing_set() {
    let config = crate::providers::config::ProviderConfig::default();
    assert!(resolve_vault_addr(None, None, &config).is_none());
}

#[test]
fn resolve_vault_addr_uses_host_override() {
    let config = crate::providers::config::ProviderConfig::default();
    let addr = resolve_vault_addr(Some("http://127.0.0.1:8200"), Some("aws"), &config);
    assert_eq!(addr.as_deref(), Some("http://127.0.0.1:8200"));
}

#[test]
fn resolve_vault_addr_falls_back_to_provider() {
    let config = crate::providers::config::ProviderConfig::parse(
        "[aws]\ntoken=abc\nvault_addr=https://vault.example:8200\n",
    );
    let addr = resolve_vault_addr(None, Some("aws"), &config);
    assert_eq!(addr.as_deref(), Some("https://vault.example:8200"));
}

#[test]
fn resolve_vault_addr_host_beats_provider() {
    let config = crate::providers::config::ProviderConfig::parse(
        "[aws]\ntoken=abc\nvault_addr=https://provider:8200\n",
    );
    let addr = resolve_vault_addr(Some("http://host:8200"), Some("aws"), &config);
    assert_eq!(addr.as_deref(), Some("http://host:8200"));
}

#[test]
fn resolve_vault_addr_empty_host_falls_through_to_provider() {
    let config = crate::providers::config::ProviderConfig::parse(
        "[aws]\ntoken=abc\nvault_addr=https://provider:8200\n",
    );
    let addr = resolve_vault_addr(Some(""), Some("aws"), &config);
    assert_eq!(addr.as_deref(), Some("https://provider:8200"));
}

#[test]
fn resolve_vault_addr_whitespace_host_falls_through_to_provider() {
    let config = crate::providers::config::ProviderConfig::parse(
        "[aws]\ntoken=abc\nvault_addr=https://provider:8200\n",
    );
    let addr = resolve_vault_addr(Some("   "), Some("aws"), &config);
    assert_eq!(addr.as_deref(), Some("https://provider:8200"));
}

#[test]
fn resolve_vault_addr_normalizes_bare_host_input() {
    let config = crate::providers::config::ProviderConfig::default();
    let addr = resolve_vault_addr(Some("192.168.1.100"), None, &config);
    assert_eq!(addr.as_deref(), Some("https://192.168.1.100:8200"));
}

#[test]
fn resolve_vault_addr_normalizes_provider_bare_addr() {
    let config = crate::providers::config::ProviderConfig::parse(
        "[aws]\ntoken=abc\nvault_addr=vault.example\n",
    );
    let addr = resolve_vault_addr(None, Some("aws"), &config);
    assert_eq!(addr.as_deref(), Some("https://vault.example:8200"));
}

// ---- normalize_vault_addr tests ----

#[test]
fn normalize_vault_addr_bare_ip() {
    assert_eq!(
        normalize_vault_addr("192.168.1.100"),
        "https://192.168.1.100:8200"
    );
}

#[test]
fn normalize_vault_addr_bare_hostname() {
    assert_eq!(
        normalize_vault_addr("vault.local"),
        "https://vault.local:8200"
    );
}

#[test]
fn normalize_vault_addr_ip_with_port() {
    assert_eq!(
        normalize_vault_addr("192.168.1.100:8200"),
        "https://192.168.1.100:8200"
    );
}

#[test]
fn normalize_vault_addr_ip_with_custom_port() {
    assert_eq!(normalize_vault_addr("10.0.0.1:443"), "https://10.0.0.1:443");
}

#[test]
fn normalize_vault_addr_full_http_url() {
    assert_eq!(
        normalize_vault_addr("http://127.0.0.1:8200"),
        "http://127.0.0.1:8200"
    );
}

#[test]
fn normalize_vault_addr_full_https_url() {
    assert_eq!(
        normalize_vault_addr("https://vault.example.com:8200"),
        "https://vault.example.com:8200"
    );
}

#[test]
fn normalize_vault_addr_https_without_port() {
    assert_eq!(
        normalize_vault_addr("https://vault.example.com"),
        "https://vault.example.com:443"
    );
}

#[test]
fn normalize_vault_addr_trims_whitespace() {
    assert_eq!(
        normalize_vault_addr("  10.0.0.1  "),
        "https://10.0.0.1:8200"
    );
}

#[test]
fn normalize_vault_addr_ipv6_bare() {
    assert_eq!(normalize_vault_addr("[::1]"), "https://[::1]:8200");
}

#[test]
fn normalize_vault_addr_ipv6_with_port() {
    assert_eq!(normalize_vault_addr("[::1]:8200"), "https://[::1]:8200");
}

#[test]
fn normalize_vault_addr_url_with_path_no_port() {
    assert_eq!(
        normalize_vault_addr("http://vault.host/v1"),
        "http://vault.host:80/v1"
    );
}

#[test]
fn normalize_vault_addr_trailing_slash() {
    assert_eq!(
        normalize_vault_addr("http://vault.host/"),
        "http://vault.host:80/"
    );
}

#[test]
fn normalize_vault_addr_uppercase_scheme() {
    assert_eq!(
        normalize_vault_addr("HTTP://vault.host"),
        "HTTP://vault.host:80"
    );
}

#[test]
fn normalize_vault_addr_unknown_scheme_passthrough() {
    assert_eq!(normalize_vault_addr("ftp://vault.host"), "ftp://vault.host");
}

#[test]
fn normalize_vault_addr_ipv6_https_without_port() {
    assert_eq!(normalize_vault_addr("https://[::1]"), "https://[::1]:443");
}

#[test]
fn normalize_vault_addr_https_custom_port() {
    assert_eq!(
        normalize_vault_addr("https://vault.host:9200"),
        "https://vault.host:9200"
    );
}

// ---- end vault_addr tests ----

#[test]
fn resolve_vault_role_provider_fallback() {
    let config = crate::providers::config::ProviderConfig::parse(
        "[aws]\ntoken=abc\nvault_role=ssh/sign/engineer\n",
    );
    let role = resolve_vault_role(None, Some("aws"), &config);
    assert_eq!(role.as_deref(), Some("ssh/sign/engineer"));
}

#[test]
fn resolve_vault_role_none_when_no_config() {
    let config = crate::providers::config::ProviderConfig::default();
    assert!(resolve_vault_role(None, None, &config).is_none());
}

#[test]
fn resolve_vault_role_none_when_provider_has_no_role() {
    let config = crate::providers::config::ProviderConfig::parse("[aws]\ntoken=abc\n");
    assert!(resolve_vault_role(None, Some("aws"), &config).is_none());
}

#[test]
fn resolve_vault_role_host_overrides_provider() {
    let config = crate::providers::config::ProviderConfig::parse(
        "[aws]\ntoken=abc\nvault_role=ssh/sign/default\n",
    );
    let role = resolve_vault_role(Some("ssh/sign/admin"), Some("aws"), &config);
    assert_eq!(role.as_deref(), Some("ssh/sign/admin"));
}

#[test]
fn format_remaining_hours() {
    assert_eq!(format_remaining(7200 + 900), "2h 15m");
}

#[test]
fn format_remaining_minutes_only() {
    assert_eq!(format_remaining(300), "5m");
}

#[test]
fn format_remaining_expired() {
    assert_eq!(format_remaining(0), "expired");
    assert_eq!(format_remaining(-100), "expired");
}

#[test]
fn resolve_cert_path_uses_certificate_file_when_set() {
    let path = resolve_cert_path("myhost", "~/.ssh/my-cert.pub").unwrap();
    let s = path.to_string_lossy();
    assert!(s.ends_with("my-cert.pub"), "got: {}", s);
    assert!(!s.contains('~'), "tilde should be expanded: {}", s);
}

#[test]
fn resolve_cert_path_falls_back_to_default() {
    let path = resolve_cert_path("myhost", "").unwrap();
    assert!(
        path.to_string_lossy()
            .contains(".purple/certs/myhost-cert.pub"),
        "got: {}",
        path.display()
    );
}

#[test]
fn resolve_cert_path_absolute() {
    let path = resolve_cert_path("myhost", "/etc/ssh/certs/myhost.pub").unwrap();
    assert_eq!(path, PathBuf::from("/etc/ssh/certs/myhost.pub"));
}

#[test]
fn cert_path_for_rejects_path_traversal() {
    assert!(cert_path_for("../../tmp/evil").is_err());
    assert!(cert_path_for("foo/bar").is_err());
    assert!(cert_path_for("foo\\bar").is_err());
    assert!(cert_path_for("host:22").is_err());
}

#[test]
fn cert_path_for_rejects_empty_alias() {
    assert!(cert_path_for("").is_err());
}

#[test]
fn sign_certificate_rejects_role_starting_with_dash() {
    let tmpdir = std::env::temp_dir();
    let fake_key = tmpdir.join("purple_test_dash_role.pub");
    std::fs::write(
        &fake_key,
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI test@test\n",
    )
    .unwrap();
    let result = sign_certificate("-format=json", &fake_key, "test", None);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid Vault SSH role")
    );
    let _ = std::fs::remove_file(&fake_key);
}

#[test]
fn resolve_vault_role_empty_host_falls_through_to_provider() {
    let config = crate::providers::config::ProviderConfig::parse(
        "[aws]\ntoken=abc\nvault_role=ssh/sign/default\n",
    );
    let role = resolve_vault_role(Some(""), Some("aws"), &config);
    assert_eq!(role.as_deref(), Some("ssh/sign/default"));
}

#[test]
fn ensure_cert_returns_error_without_vault() {
    let tmpdir = std::env::temp_dir();
    let fake_key = tmpdir.join("purple_test_ensure_cert_key.pub");
    std::fs::write(
        &fake_key,
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI test@test\n",
    )
    .unwrap();

    let result = ensure_cert("ssh/sign/test", &fake_key, "ensure-test-host", "", None);
    // Should fail because vault CLI is not available
    assert!(result.is_err());
    let _ = std::fs::remove_file(&fake_key);
}

#[test]
fn parse_ssh_datetime_rejects_zero_month_and_day() {
    assert!(parse_ssh_datetime("2026-00-08T12:00:00").is_none());
    assert!(parse_ssh_datetime("2026-04-00T12:00:00").is_none());
}

#[test]
fn format_remaining_exactly_one_hour() {
    assert_eq!(format_remaining(3600), "1h 0m");
}

#[test]
fn cert_path_rejects_nul_byte() {
    assert!(cert_path_for("host\0name").is_err());
}

#[test]
fn is_valid_role_rejects_shell_metachars() {
    for bad in [
        "ssh/sign/role$x",
        "ssh/sign/role;rm",
        "ssh/sign/role|cat",
        "ssh/sign/role`id`",
        "ssh/sign/role&bg",
        "ssh/sign/role x",
        "ssh/sign/role\nx",
    ] {
        assert!(!is_valid_role(bad), "should reject {:?}", bad);
    }
}

#[test]
fn scrub_vault_stderr_redacts_all_marker_types() {
    let raw = "error contacting server\n\
               x-vault-token: abcdef\n\
               Authorization: Bearer xyz\n\
               Cookie: session=1\n\
               SECRET=foo\n\
               token expired perhaps\n\
               harmless trailing line";
    let out = scrub_vault_stderr(raw).to_ascii_lowercase();
    assert!(!out.contains("token"));
    assert!(!out.contains("x-vault-"));
    assert!(!out.contains("authorization"));
    assert!(!out.contains("cookie"));
    assert!(!out.contains("secret"));
}

#[test]
fn scrub_vault_stderr_truncation_bound() {
    let raw = "a".repeat(500);
    let out = scrub_vault_stderr(&raw);
    assert!(
        out.chars().count() <= 203,
        "len was {}",
        out.chars().count()
    );
    assert!(out.ends_with("..."));
}

#[test]
fn scrub_vault_stderr_default_when_all_filtered() {
    let raw = "token abc\nsecret def\nauthorization ghi";
    let out = scrub_vault_stderr(raw);
    assert_eq!(
        out,
        "Vault SSH signing failed. Check vault status and policy"
    );
}

// TODO: resolve_pubkey_path_rejects_symlink_escape — requires mutating $HOME
// for the current process, which races with other tests that read dirs::home_dir().
// The canonicalize-based check is exercised manually; skipped here to keep the
// test suite hermetic and parallel-safe.

#[test]
fn is_valid_role_accepts_typical_paths() {
    assert!(is_valid_role("ssh/sign/engineer"));
    assert!(is_valid_role("ssh-ca/sign/admin_role"));
    assert!(is_valid_role("a"));
    assert!(is_valid_role(&"a".repeat(128)));
}

#[test]
fn is_valid_role_rejects_bad_input() {
    assert!(!is_valid_role(""));
    assert!(!is_valid_role("-format=json"));
    assert!(!is_valid_role("ssh/sign/role with space"));
    assert!(!is_valid_role("ssh/sign/role;rm"));
    assert!(!is_valid_role("ssh/sign/rôle"));
    assert!(!is_valid_role(&"a".repeat(129)));
}

#[test]
fn scrub_vault_stderr_drops_token_lines() {
    let raw = "error occurred\nX-Vault-Token: abc123\nrole missing\n";
    let out = scrub_vault_stderr(raw);
    assert!(!out.to_lowercase().contains("token"));
    assert!(out.contains("error occurred"));
    assert!(out.contains("role missing"));
}

#[test]
fn scrub_vault_stderr_drops_secret_and_authorization() {
    let raw = "line one\nsecret=foo\nAuthorization: Bearer x\nline four\n";
    let out = scrub_vault_stderr(raw);
    assert!(!out.to_lowercase().contains("secret"));
    assert!(!out.to_lowercase().contains("authorization"));
    assert!(out.contains("line one"));
    assert!(out.contains("line four"));
}

#[test]
fn scrub_vault_stderr_empty_falls_back() {
    let out = scrub_vault_stderr("");
    assert!(out.contains("Vault SSH signing failed"));
}

#[test]
fn scrub_vault_stderr_only_filtered_falls_back() {
    let out = scrub_vault_stderr("X-Vault-Token: abc\nSecret: xyz\n");
    assert!(out.contains("Vault SSH signing failed"));
}

#[test]
fn scrub_vault_stderr_truncates_long_output() {
    let raw = "x".repeat(500);
    let out = scrub_vault_stderr(&raw);
    assert!(out.ends_with("..."));
    // 200 chars + "..."
    assert_eq!(out.chars().count(), 203);
}

#[test]
fn resolve_pubkey_rejects_path_outside_home() {
    // Absolute path outside home should fall back to default in ~/.ssh
    let path = resolve_pubkey_path("/etc/passwd").unwrap();
    let s = path.to_string_lossy();
    assert!(s.ends_with("id_ed25519.pub"), "got: {}", s);
    assert!(s.contains(".ssh/"), "should be fallback: {}", s);
}

#[cfg(unix)]
fn unique_tmp_subdir(tag: &str) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "purple_mock_vault_{}_{}_{}",
        tag,
        std::process::id(),
        nanos
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(unix)]
fn with_mock_vault<F: FnOnce()>(tag: &str, stderr: &str, stdout: &str, exit_code: i32, f: F) {
    use std::os::unix::fs::PermissionsExt;
    // Use the module-wide PATH_LOCK so vault-mocking tests don't race
    // against ssh-keygen-mocking tests (both mutate the same PATH).
    let _guard = PATH_LOCK.lock().unwrap_or_else(|p| p.into_inner());

    let dir = unique_tmp_subdir(tag);
    let script = dir.join("vault");
    let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let body = format!(
        "#!/bin/sh\nprintf '%s' \"{}\" >&2\nprintf '%s' \"{}\"\nexit {}\n",
        escape(stderr),
        escape(stdout),
        exit_code
    );
    std::fs::write(&script, body).unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();

    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", dir.display(), old_path);
    // SAFETY: std::env::set_var is unsound in multi-threaded processes
    // (rust-lang/rust#27970). The invariant we uphold here is: all mutations
    // of PATH within this test binary happen through `with_mock_vault`, which
    // holds the process-wide `LOCK` for the full mutate/use/restore cycle.
    // No other test in this crate reads or writes PATH concurrently. If a
    // future test introduces another PATH writer, it MUST acquire this same
    // LOCK. PATH is restored before the guard is dropped.
    unsafe { std::env::set_var("PATH", &new_path) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    unsafe { std::env::set_var("PATH", &old_path) };
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[cfg(unix)]
fn write_fake_pubkey(tag: &str) -> PathBuf {
    let dir = unique_tmp_subdir(tag);
    let p = dir.join("fake.pub");
    std::fs::write(&p, "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI test@test\n").unwrap();
    p
}

#[cfg(unix)]
#[test]
fn sign_certificate_permission_denied_maps_to_friendly_error() {
    let key = write_fake_pubkey("perm_denied");
    let alias = "mock-perm-denied";
    with_mock_vault(
        "perm_denied",
        "Error making API request.\npermission denied",
        "",
        1,
        || {
            let result = sign_certificate("ssh/sign/role", &key, alias, None);
            let err = result.unwrap_err().to_string();
            assert!(err.contains("Vault SSH permission denied"), "got: {}", err);
        },
    );
    let _ = std::fs::remove_file(&key);
}

#[cfg(unix)]
#[test]
fn sign_certificate_token_expired_maps_to_friendly_error() {
    let key = write_fake_pubkey("tok_exp");
    let alias = "mock-tok-exp";
    with_mock_vault("tok_exp", "missing client token", "", 1, || {
        let result = sign_certificate("ssh/sign/role", &key, alias, None);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("token missing or expired"), "got: {}", err);
    });
    let _ = std::fs::remove_file(&key);
}

#[cfg(unix)]
#[test]
fn sign_certificate_scrubs_sensitive_stderr() {
    let key = write_fake_pubkey("scrub");
    let alias = "mock-scrub";
    with_mock_vault(
        "scrub",
        "role not configured\nX-Vault-Token: hvs.ABCDEFG",
        "",
        1,
        || {
            let result = sign_certificate("ssh/sign/role", &key, alias, None);
            let err = result.unwrap_err().to_string();
            assert!(!err.contains("hvs.ABCDEFG"), "leaked token: {}", err);
            assert!(!err.contains("X-Vault-Token"), "leaked header: {}", err);
        },
    );
    let _ = std::fs::remove_file(&key);
}

#[cfg(unix)]
#[test]
fn sign_certificate_empty_stdout_errors() {
    let key = write_fake_pubkey("empty");
    let alias = "mock-empty";
    with_mock_vault("empty", "", "", 0, || {
        let result = sign_certificate("ssh/sign/role", &key, alias, None);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("empty certificate"), "got: {}", err);
    });
    let _ = std::fs::remove_file(&key);
}

#[cfg(unix)]
#[test]
fn sign_certificate_generic_failure_no_stderr() {
    let key = write_fake_pubkey("generic");
    let alias = "mock-generic";
    with_mock_vault("generic", "", "", 1, || {
        let result = sign_certificate("ssh/sign/role", &key, alias, None);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Vault SSH failed"), "got: {}", err);
    });
    let _ = std::fs::remove_file(&key);
}

#[cfg(unix)]
#[test]
fn sign_certificate_success_writes_cert() {
    let key = write_fake_pubkey("success");
    let alias = "mock-success-host";
    let expected_cert = "ssh-ed25519-cert-v01@openssh.com AAAAFAKECERT test";
    with_mock_vault("success", "", expected_cert, 0, || {
        let result = sign_certificate("ssh/sign/role", &key, alias, None).unwrap();
        assert!(result.cert_path.exists());
        let content = std::fs::read_to_string(&result.cert_path).unwrap();
        assert_eq!(content, expected_cert);
        let _ = std::fs::remove_file(&result.cert_path);
    });
    let _ = std::fs::remove_file(&key);
}

/// Install a mock `vault` binary that captures `$VAULT_ADDR` into a file
/// and echoes a dummy cert on stdout. Returns the capture file path so
/// callers can assert on the recorded value.
#[cfg(unix)]
fn with_env_capturing_vault<F: FnOnce(&Path)>(tag: &str, f: F) {
    use std::os::unix::fs::PermissionsExt;
    let _guard = PATH_LOCK.lock().unwrap_or_else(|p| p.into_inner());

    let dir = unique_tmp_subdir(tag);
    let capture = dir.join("captured_addr.txt");
    let script = dir.join("vault");
    // The mock writes VAULT_ADDR to the capture file (empty if unset)
    // and prints a dummy cert to stdout so sign_certificate's
    // "signed_key empty" guard does not trip.
    let body = format!(
        "#!/bin/sh\nprintf '%s' \"${{VAULT_ADDR-}}\" > {}\nprintf '%s' 'ssh-ed25519-cert-v01@openssh.com AAAAMOCKCERT mock'\nexit 0\n",
        capture.display()
    );
    std::fs::write(&script, body).unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();

    let old_path = std::env::var("PATH").unwrap_or_default();
    let old_vault_addr = std::env::var("VAULT_ADDR").ok();
    let new_path = format!("{}:{}", dir.display(), old_path);
    // SAFETY: see with_mock_vault — PATH_LOCK serializes all env mutations
    // in this test module. We clear VAULT_ADDR up front so the
    // "None = inherit parent env" test starts from a clean slate.
    unsafe {
        std::env::set_var("PATH", &new_path);
        std::env::remove_var("VAULT_ADDR");
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&capture)));
    unsafe {
        std::env::set_var("PATH", &old_path);
        match old_vault_addr {
            Some(v) => std::env::set_var("VAULT_ADDR", v),
            None => std::env::remove_var("VAULT_ADDR"),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[cfg(unix)]
#[test]
fn sign_certificate_sets_vault_addr_env_on_subprocess() {
    let key = write_fake_pubkey("addr_set");
    let alias = "mock-addr-set";
    with_env_capturing_vault("addr_set", |capture| {
        let res = sign_certificate(
            "ssh/sign/role",
            &key,
            alias,
            Some("http://override.example:8200"),
        );
        assert!(res.is_ok(), "sign failed: {:?}", res);
        let captured = std::fs::read_to_string(capture).unwrap();
        assert_eq!(
            captured, "http://override.example:8200",
            "subprocess did not receive the overridden VAULT_ADDR"
        );
        if let Ok(r) = res {
            let _ = std::fs::remove_file(&r.cert_path);
        }
    });
    let _ = std::fs::remove_file(&key);
}

#[cfg(unix)]
#[test]
fn sign_certificate_does_not_set_vault_addr_when_none() {
    let key = write_fake_pubkey("addr_none");
    let alias = "mock-addr-none";
    with_env_capturing_vault("addr_none", |capture| {
        // with_env_capturing_vault clears VAULT_ADDR on entry, so when
        // sign_certificate passes None the subprocess inherits an empty
        // value. Assert exactly that — no override leaked through.
        let res = sign_certificate("ssh/sign/role", &key, alias, None);
        assert!(res.is_ok(), "sign failed: {:?}", res);
        let captured = std::fs::read_to_string(capture).unwrap();
        assert!(
            captured.is_empty(),
            "subprocess saw unexpected VAULT_ADDR: {:?}",
            captured
        );
        if let Ok(r) = res {
            let _ = std::fs::remove_file(&r.cert_path);
        }
    });
    let _ = std::fs::remove_file(&key);
}

#[cfg(unix)]
#[test]
fn sign_certificate_rejects_invalid_vault_addr() {
    // An invalid vault_addr (whitespace inside) must be rejected with a
    // clear error before spawning the vault CLI.
    let key = write_fake_pubkey("addr_bad");
    let alias = "mock-addr-bad";
    let res = sign_certificate("ssh/sign/role", &key, alias, Some("http://has space:8200"));
    assert!(res.is_err());
    let msg = res.unwrap_err().to_string();
    assert!(
        msg.contains("Invalid VAULT_ADDR"),
        "expected explicit rejection, got: {}",
        msg
    );
    let _ = std::fs::remove_file(&key);
}

#[cfg(unix)]
#[test]
fn check_cert_validity_handles_forever() {
    use std::os::unix::fs::PermissionsExt;
    let _guard = PATH_LOCK.lock().unwrap_or_else(|p| p.into_inner());

    let dir = unique_tmp_subdir("forever");
    let script = dir.join("ssh-keygen");
    let body = "#!/bin/sh\nprintf '%s\\n' '        Type: ssh-ed25519-cert-v01@openssh.com'\nprintf '%s\\n' '        Valid: forever'\nexit 0\n";
    std::fs::write(&script, body).unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    let cert = dir.join("cert.pub");
    std::fs::write(&cert, "stub").unwrap();

    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", dir.display(), old_path);
    // SAFETY: PATH mutation is serialized via LOCK above and restored before
    // the guard is released.
    unsafe { std::env::set_var("PATH", &new_path) };
    let status = check_cert_validity(&cert);
    unsafe { std::env::set_var("PATH", &old_path) };
    let _ = std::fs::remove_dir_all(&dir);

    match status {
        CertStatus::Valid {
            remaining_secs,
            total_secs,
            expires_at,
        } => {
            assert_eq!(remaining_secs, i64::MAX);
            assert_eq!(total_secs, i64::MAX);
            assert_eq!(expires_at, i64::MAX);
        }
        other => panic!("expected Valid(forever), got {:?}", other),
    }
}

#[cfg(unix)]
#[test]
fn check_cert_validity_rejects_non_positive_window() {
    // Regression: a malformed cert with `to < from` would produce a
    // negative total_secs that flowed into the needs_renewal threshold
    // calculation. The guard in check_cert_validity must reject it as
    // Invalid before it ever reaches the cache.
    use std::os::unix::fs::PermissionsExt;
    let _guard = PATH_LOCK.lock().unwrap_or_else(|p| p.into_inner());

    let dir = unique_tmp_subdir("non_positive");
    let script = dir.join("ssh-keygen");
    // Valid window with `to` == `from`, producing ttl == 0.
    let body = "#!/bin/sh\nprintf '%s\\n' '        Type: ssh-ed25519-cert-v01@openssh.com'\nprintf '%s\\n' '        Valid: from 2026-01-01T00:00:00 to 2026-01-01T00:00:00'\nexit 0\n";
    std::fs::write(&script, body).unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    let cert = dir.join("cert.pub");
    std::fs::write(&cert, "stub").unwrap();

    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", dir.display(), old_path);
    // SAFETY: see with_mock_vault for the full invariant. PATH is
    // serialized via LOCK and restored before the guard is released.
    unsafe { std::env::set_var("PATH", &new_path) };
    let status = check_cert_validity(&cert);
    unsafe { std::env::set_var("PATH", &old_path) };
    let _ = std::fs::remove_dir_all(&dir);

    match status {
        CertStatus::Invalid(msg) => {
            assert!(
                msg.contains("non-positive"),
                "expected non-positive window error, got: {}",
                msg
            );
        }
        other => panic!("expected Invalid, got {:?}", other),
    }
}

#[test]
fn is_valid_role_rejects_spaces_and_shell_metacharacters() {
    assert!(!is_valid_role(""));
    assert!(!is_valid_role("bad role"));
    assert!(!is_valid_role("role;rm"));
    assert!(!is_valid_role("role$(x)"));
    assert!(!is_valid_role("role|cat"));
    assert!(!is_valid_role("role`id`"));
    assert!(!is_valid_role("role&bg"));
    assert!(!is_valid_role("role\nx"));
    // "Missing /sign/" is not structurally enforced by is_valid_role (the
    // Vault CLI validates the mount), but character rules still pass:
    assert!(is_valid_role("ssh/engineer"));
}

#[test]
fn resolve_vault_role_host_overrides_provider_default() {
    let config = crate::providers::config::ProviderConfig::parse(
        "[aws]\ntoken=abc\nvault_role=ssh/sign/default\n",
    );
    let role = resolve_vault_role(Some("ssh/sign/override"), Some("aws"), &config);
    assert_eq!(role.as_deref(), Some("ssh/sign/override"));
}

#[test]
fn resolve_vault_role_falls_back_to_provider_when_host_empty() {
    let config = crate::providers::config::ProviderConfig::parse(
        "[aws]\ntoken=abc\nvault_role=ssh/sign/default\n",
    );
    let role = resolve_vault_role(None, Some("aws"), &config);
    assert_eq!(role.as_deref(), Some("ssh/sign/default"));
}

#[test]
fn resolve_vault_role_returns_none_when_neither_set() {
    let config = crate::providers::config::ProviderConfig::default();
    assert!(resolve_vault_role(None, Some("aws"), &config).is_none());
    assert!(resolve_vault_role(None, None, &config).is_none());
}

#[cfg(unix)]
#[test]
fn check_cert_validity_invalid_file() {
    // Use a mock ssh-keygen that exits with failure, because the real
    // ssh-keygen behavior on non-certificate files varies across
    // platforms (macOS returns Invalid, some Linux versions return Valid).
    use std::os::unix::fs::PermissionsExt;
    let _guard = PATH_LOCK.lock().unwrap_or_else(|p| p.into_inner());

    let dir = unique_tmp_subdir("invalid_file");
    let script = dir.join("ssh-keygen");
    let body = "#!/bin/sh\necho 'is not a certificate' >&2\nexit 1\n";
    std::fs::write(&script, body).unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    let cert = dir.join("cert.pub");
    std::fs::write(&cert, "this is not a certificate\n").unwrap();

    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", dir.display(), old_path);
    unsafe { std::env::set_var("PATH", &new_path) };
    let status = check_cert_validity(&cert);
    unsafe { std::env::set_var("PATH", &old_path) };
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        matches!(status, CertStatus::Invalid(_)),
        "Expected Invalid, got: {:?}",
        status
    );
}
