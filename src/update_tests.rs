use super::*;

#[test]
fn test_parse_version() {
    assert_eq!(parse_version("1.5.0"), Some((1, 5, 0)));
    assert_eq!(parse_version("0.1.2"), Some((0, 1, 2)));
    assert_eq!(parse_version("10.20.30"), Some((10, 20, 30)));
}

#[test]
fn test_parse_version_invalid() {
    assert_eq!(parse_version(""), None);
    assert_eq!(parse_version("1.2"), None);
    assert_eq!(parse_version("abc"), None);
    assert_eq!(parse_version("1.2.x"), None);
    assert_eq!(parse_version("1.5.0-rc1"), None);
}

#[test]
fn test_is_newer_patch() {
    assert!(is_newer("1.5.0", "1.5.1"));
    assert!(!is_newer("1.5.1", "1.5.0"));
}

#[test]
fn test_is_newer_minor() {
    assert!(is_newer("1.5.0", "1.6.0"));
    assert!(!is_newer("1.6.0", "1.5.0"));
}

#[test]
fn test_is_newer_major() {
    assert!(is_newer("1.5.0", "2.0.0"));
    assert!(!is_newer("2.0.0", "1.5.0"));
}

#[test]
fn test_is_newer_equal() {
    assert!(!is_newer("1.5.0", "1.5.0"));
}

#[test]
fn test_is_newer_invalid() {
    assert!(!is_newer("1.5.0", "bad"));
    assert!(!is_newer("bad", "1.5.0"));
}

#[test]
fn test_extract_version_with_v_prefix() {
    let json = serde_json::json!({"tag_name": "v1.6.0"});
    let info = extract_release_info(&json).unwrap();
    assert_eq!(info.version, "1.6.0");
}

#[test]
fn test_extract_version_without_prefix() {
    let json = serde_json::json!({"tag_name": "1.6.0"});
    let info = extract_release_info(&json).unwrap();
    assert_eq!(info.version, "1.6.0");
}

#[test]
fn test_extract_version_missing_tag() {
    let json = serde_json::json!({"name": "Release"});
    assert!(extract_release_info(&json).is_err());
}

#[test]
fn test_extract_version_invalid_format() {
    let json = serde_json::json!({"tag_name": "v1.2.3-rc1"});
    assert!(extract_release_info(&json).is_err());
}

#[test]
fn test_extract_release_notes() {
    let json = serde_json::json!({"tag_name": "v1.6.0", "body": "Bug fixes and improvements"});
    let info = extract_release_info(&json).unwrap();
    assert_eq!(info.version, "1.6.0");
    assert_eq!(info.notes, "Bug fixes and improvements");
}

#[test]
fn test_extract_release_notes_missing_body() {
    let json = serde_json::json!({"tag_name": "v1.6.0"});
    let info = extract_release_info(&json).unwrap();
    assert_eq!(info.notes, "");
}

#[test]
fn test_extract_headline_bullet() {
    assert_eq!(
        extract_headline("- Added new feature\n- Fixed bug"),
        Some("Added new feature".to_string())
    );
}

#[test]
fn test_extract_headline_no_bullet() {
    assert_eq!(
        extract_headline("Some plain text"),
        Some("Some plain text".to_string())
    );
}

#[test]
fn test_extract_headline_skips_heading() {
    assert_eq!(
        extract_headline("## What's new\n- The actual headline"),
        Some("The actual headline".to_string())
    );
}

#[test]
fn test_extract_headline_skips_blank_lines() {
    assert_eq!(
        extract_headline("\n\n- First item"),
        Some("First item".to_string())
    );
}

#[test]
fn test_extract_headline_strips_changelog_prefix() {
    for (notes, want) in [
        (
            "- feat: Connect to new hosts that use a saved password.\n- change: Other",
            "Connect to new hosts that use a saved password.",
        ),
        (
            "- change: Hosts connect as before.",
            "Hosts connect as before.",
        ),
        ("- fix: Sync keeps your tags.", "Sync keeps your tags."),
        ("- FEAT: Shouted prefix", "Shouted prefix"),
        ("- Thanks to @someone for #1", "Thanks to @someone for #1"),
    ] {
        assert_eq!(extract_headline(notes), Some(want.to_string()), "{notes}");
    }
}

#[test]
fn test_extract_headline_prefix_only_is_none() {
    assert_eq!(extract_headline("- feat:"), None);
}

#[test]
fn test_extract_headline_empty() {
    assert_eq!(extract_headline(""), None);
}

#[test]
fn test_extract_headline_only_blanks() {
    assert_eq!(extract_headline("\n\n\n"), None);
}

#[test]
fn test_extract_headline_truncates_long_input() {
    let long = "- ".to_string() + &"a".repeat(500);
    let result = extract_headline(&long).expect("headline must be returned");
    assert!(
        result.len() <= 200,
        "expected truncation, got {} bytes",
        result.len()
    );
    assert!(result.starts_with('a'), "bullet marker must be stripped");
}

#[test]
fn test_extract_headline_truncates_on_char_boundary() {
    // Build a 300-byte string where byte 200 lands mid-codepoint.
    // 'ü' is 2 bytes, so 150 of them = 300 bytes. Cutting at 200 lands
    // between the two bytes of a 'ü' and must back off to 199.
    let long = "ü".repeat(150);
    let result = extract_headline(&long).expect("headline must be returned");
    assert!(result.len() <= 200);
    // The result must be valid UTF-8 that we can still inspect.
    assert!(result.chars().all(|c| c == 'ü'));
}

#[test]
fn test_current_version_is_valid() {
    assert!(parse_version(current_version()).is_some());
}

// --- is_homebrew_path tests ---

#[test]
fn test_homebrew_cellar_apple_silicon() {
    let path = Path::new("/opt/homebrew/Cellar/purple/1.5.0/bin/purple");
    assert!(is_homebrew_path(path, Path::new("/opt/homebrew/Cellar")));
}

#[test]
fn test_homebrew_cellar_intel() {
    let path = Path::new("/usr/local/Cellar/purple/1.5.0/bin/purple");
    assert!(is_homebrew_path(path, Path::new("/usr/local/Cellar")));
}

#[test]
fn test_homebrew_cellar_linuxbrew() {
    let path = Path::new("/home/linuxbrew/.linuxbrew/Cellar/purple/2.3.0/bin/purple");
    assert!(is_homebrew_path(
        path,
        Path::new("/home/linuxbrew/.linuxbrew/Cellar")
    ));
}

#[test]
fn test_homebrew_cellar_rejects_non_cellar_suffix() {
    // Env var points to a dir that doesn't end in "Cellar"
    let path = Path::new("/opt/homebrew/lib/purple");
    assert!(!is_homebrew_path(path, Path::new("/opt/homebrew/lib")));
}

#[test]
fn test_homebrew_cellar_rejects_bare_cellar() {
    // Binary directly inside Cellar with no formula subdirectory
    let path = Path::new("/opt/homebrew/Cellar");
    assert!(!is_homebrew_path(path, Path::new("/opt/homebrew/Cellar")));
}

#[test]
fn test_homebrew_cellar_rejects_prefix_overlap() {
    // /usr/local/Cellar-custom is not /usr/local/Cellar
    // Path::starts_with is component-aware so this must not match
    let path = Path::new("/usr/local/Cellar-custom/purple/bin/purple");
    assert!(!is_homebrew_path(path, Path::new("/usr/local/Cellar")));
}

// --- is_cargo_path tests ---

#[test]
fn test_cargo_default_path() {
    let path = Path::new("/Users/user/.cargo/bin/purple");
    assert!(is_cargo_path(path, Path::new("/Users/user/.cargo")));
}

#[test]
fn test_cargo_custom_home() {
    let path = Path::new("/data/rust/cargo/bin/purple");
    assert!(is_cargo_path(path, Path::new("/data/rust/cargo")));
}

#[test]
fn test_cargo_rejects_nested_bin() {
    // Binary in a subdir of bin — not a direct cargo install
    let path = Path::new("/Users/user/.cargo/bin/subdir/purple");
    assert!(!is_cargo_path(path, Path::new("/Users/user/.cargo")));
}

#[test]
fn test_cargo_rejects_prefix_overlap() {
    // /.cargo-custom/bin is not /.cargo/bin
    let path = Path::new("/Users/user/.cargo-custom/bin/purple");
    assert!(!is_cargo_path(path, Path::new("/Users/user/.cargo")));
}

// --- detect_install_method tests (path-only, no env vars) ---

#[test]
fn test_detect_homebrew_cellar() {
    let path = Path::new("/opt/homebrew/Cellar/purple/1.5.0/bin/purple");
    assert!(matches!(
        detect_install_method(path, &crate::runtime::env::Env::empty()),
        InstallMethod::Homebrew
    ));
}

#[test]
fn test_detect_homebrew_default_intel() {
    let path = Path::new("/usr/local/Cellar/purple/1.5.0/bin/purple");
    assert!(matches!(
        detect_install_method(path, &crate::runtime::env::Env::empty()),
        InstallMethod::Homebrew
    ));
}

#[test]
fn test_detect_homebrew_default_linuxbrew() {
    let path = Path::new("/home/linuxbrew/.linuxbrew/Cellar/purple/2.3.0/bin/purple");
    assert!(matches!(
        detect_install_method(path, &crate::runtime::env::Env::empty()),
        InstallMethod::Homebrew
    ));
}

#[test]
fn test_detect_cargo_default() {
    let path = Path::new("/Users/user/.cargo/bin/purple");
    assert!(matches!(
        detect_install_method(path, &crate::runtime::env::Env::empty()),
        InstallMethod::Cargo
    ));
}

#[test]
fn test_detect_curl_usr_local_bin() {
    let path = Path::new("/usr/local/bin/purple");
    assert!(matches!(
        detect_install_method(path, &crate::runtime::env::Env::empty()),
        InstallMethod::CurlOrManual
    ));
}

#[test]
fn test_detect_curl_local_bin() {
    let path = Path::new("/Users/user/.local/bin/purple");
    assert!(matches!(
        detect_install_method(path, &crate::runtime::env::Env::empty()),
        InstallMethod::CurlOrManual
    ));
}

#[test]
fn test_detect_no_false_positive_homebrew_in_name() {
    let path = Path::new("/Users/user/homebrew-tools/bin/purple");
    assert!(matches!(
        detect_install_method(path, &crate::runtime::env::Env::empty()),
        InstallMethod::CurlOrManual
    ));
}

// --- fail-open: ambiguous paths default to CurlOrManual ---

#[test]
fn test_detect_unknown_path() {
    let path = Path::new("/some/random/path/purple");
    assert!(matches!(
        detect_install_method(path, &crate::runtime::env::Env::empty()),
        InstallMethod::CurlOrManual
    ));
}

#[test]
fn test_detect_root_path() {
    let path = Path::new("/purple");
    assert!(matches!(
        detect_install_method(path, &crate::runtime::env::Env::empty()),
        InstallMethod::CurlOrManual
    ));
}

// --- parse_version_cache tests ---

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn test_cache_fresh_newer_version() {
    let now = now_secs();
    let content = format!("{}\n99.0.0\n", now);
    // 99.0.0 is newer than any current version
    let result = parse_version_cache(&content, now, "1.5.0");
    let cached = result.unwrap().unwrap();
    assert_eq!(cached.version, "99.0.0");
    assert_eq!(cached.headline, None);
}

#[test]
fn test_cache_fresh_newer_with_headline() {
    let now = now_secs();
    let content = format!("{}\n99.0.0\nNew feature added\n", now);
    let cached = parse_version_cache(&content, now, "1.5.0")
        .unwrap()
        .unwrap();
    assert_eq!(cached.version, "99.0.0");
    assert_eq!(cached.headline, Some("New feature added".to_string()));
}

#[test]
fn test_cache_fresh_up_to_date() {
    let now = now_secs();
    let content = format!("{}\n1.5.0\n", now);
    // Same version: up-to-date
    assert_eq!(parse_version_cache(&content, now, "1.5.0"), Some(None));
}

#[test]
fn test_cache_fresh_older_version() {
    let now = now_secs();
    let content = format!("{}\n1.0.0\n", now);
    // Cached version is older than current: up-to-date
    assert_eq!(parse_version_cache(&content, now, "1.5.0"), Some(None));
}

#[test]
fn test_cache_expired() {
    let now = now_secs();
    let old = now - VERSION_CHECK_TTL.as_secs() - 1;
    let content = format!("{}\n99.0.0\n", old);
    assert_eq!(parse_version_cache(&content, now, "1.5.0"), None);
}

#[test]
fn test_cache_exactly_at_ttl() {
    let now = now_secs();
    let at_ttl = now - VERSION_CHECK_TTL.as_secs();
    let content = format!("{}\n99.0.0\n", at_ttl);
    // At exactly TTL boundary: still valid (saturating_sub > TTL, not >=)
    let cached = parse_version_cache(&content, now, "1.5.0")
        .unwrap()
        .unwrap();
    assert_eq!(cached.version, "99.0.0");
}

#[test]
fn test_cache_empty_content() {
    assert_eq!(parse_version_cache("", now_secs(), "1.5.0"), None);
}

#[test]
fn test_cache_missing_version_line() {
    let content = format!("{}\n", now_secs());
    assert_eq!(parse_version_cache(&content, now_secs(), "1.5.0"), None);
}

#[test]
fn test_cache_non_numeric_timestamp() {
    assert_eq!(
        parse_version_cache("abc\n99.0.0\n", now_secs(), "1.5.0"),
        None
    );
}

#[test]
fn test_cache_invalid_version_format() {
    let now = now_secs();
    let content = format!("{}\nnot-a-version\n", now);
    assert_eq!(parse_version_cache(&content, now, "1.5.0"), None);
}

#[test]
fn test_cache_empty_version() {
    let now = now_secs();
    // Second line is empty
    let content = format!("{}\n\n", now);
    assert_eq!(parse_version_cache(&content, now, "1.5.0"), None);
}

#[test]
fn test_cache_only_timestamp() {
    let content = format!("{}", now_secs());
    assert_eq!(parse_version_cache(&content, now_secs(), "1.5.0"), None);
}

#[test]
fn test_cache_garbage() {
    assert_eq!(parse_version_cache("garbage", now_secs(), "1.5.0"), None);
}

#[test]
fn test_cache_backwards_compat_no_headline() {
    // Old cache format without headline line should still work
    let now = now_secs();
    let content = format!("{}\n99.0.0", now);
    let cached = parse_version_cache(&content, now, "1.5.0")
        .unwrap()
        .unwrap();
    assert_eq!(cached.version, "99.0.0");
    assert_eq!(cached.headline, None);
}

// --- version_from_release_url tests ---

#[test]
fn test_version_from_release_url() {
    assert_eq!(
        version_from_release_url("https://github.com/erickochen/purple/releases/tag/v3.32.1"),
        Some("3.32.1".to_string())
    );
    assert_eq!(
        version_from_release_url("/erickochen/purple/releases/tag/3.32.1"),
        Some("3.32.1".to_string())
    );
}

#[test]
fn test_version_from_release_url_rejects_other_urls() {
    for url in [
        "https://github.com/erickochen/purple/releases",
        "https://github.com/login",
        "https://github.com/erickochen/purple/releases/tag/",
        "https://github.com/erickochen/purple/releases/tag/v3.32",
        "https://github.com/erickochen/purple/releases/tag/v3.32.1-rc1",
        "https://github.com/erickochen/purple/releases/tag/v3.32.1/assets",
    ] {
        assert_eq!(version_from_release_url(url), None, "{url}");
    }
}

#[test]
fn test_version_from_release_url_keeps_only_digits_and_dots() {
    // The version ends up in download URLs, so it is rebuilt from the numbers.
    assert_eq!(
        version_from_release_url("/releases/tag/v1.2.+3"),
        Some("1.2.3".to_string())
    );
}

// --- check_latest_release_at tests (local mock server, no real network) ---

const API_PATH: &str = "/repos/erickochen/purple/releases/latest";
const PAGE_PATH: &str = "/erickochen/purple/releases/latest";
const TAG_PATH: &str = "/erickochen/purple/releases/tag/v9.8.7";
/// Short timeout so a broken test fails instead of hanging.
const TEST_HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn test_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(TEST_HTTP_TIMEOUT))
        .build()
        .new_agent()
}

fn lookup(server: &mockito::Server) -> Result<ReleaseInfo> {
    check_latest_release_at(
        &test_agent(),
        &format!("{}{API_PATH}", server.url()),
        &format!("{}{PAGE_PATH}", server.url()),
    )
}

/// Release page mock that redirects to the v9.8.7 tag on the same server.
/// A tag mock that expects no hits proves the redirect is read, not followed.
fn mock_release_page(server: &mut mockito::Server) -> (mockito::Mock, mockito::Mock) {
    let page = server
        .mock("GET", PAGE_PATH)
        .with_status(302)
        .with_header("location", &format!("{}{TAG_PATH}", server.url()))
        .create();
    let tag = server.mock("GET", TAG_PATH).expect(0).create();
    (page, tag)
}

#[test]
fn test_latest_release_uses_api_when_it_answers() {
    let mut server = mockito::Server::new();
    let api = server
        .mock("GET", API_PATH)
        .with_status(200)
        .with_body(r#"{"tag_name":"v9.8.7","body":"- Big news"}"#)
        .create();
    let page = server.mock("GET", PAGE_PATH).expect(0).create();

    let info = lookup(&server).unwrap();
    assert_eq!(info.version, "9.8.7");
    assert_eq!(info.notes, "- Big news");
    api.assert();
    page.assert();
}

#[test]
fn test_latest_release_falls_back_to_page_when_api_refuses() {
    for status in [403, 429, 500] {
        let mut server = mockito::Server::new();
        let api = server.mock("GET", API_PATH).with_status(status).create();
        let (page, tag) = mock_release_page(&mut server);

        let info = lookup(&server).unwrap_or_else(|e| panic!("status {status}: {e:#}"));
        assert_eq!(info.version, "9.8.7", "status {status}");
        assert_eq!(info.notes, "", "status {status}");
        api.assert();
        page.assert();
        tag.assert();
    }
}

#[test]
fn test_latest_release_falls_back_when_api_body_is_unusable() {
    let mut server = mockito::Server::new();
    server
        .mock("GET", API_PATH)
        .with_status(200)
        .with_body("<html>maintenance</html>")
        .create();
    let (page, tag) = mock_release_page(&mut server);

    assert_eq!(lookup(&server).unwrap().version, "9.8.7");
    page.assert();
    tag.assert();
}

#[test]
fn test_latest_release_fails_when_page_does_not_redirect() {
    let mut server = mockito::Server::new();
    server.mock("GET", API_PATH).with_status(403).create();
    server
        .mock("GET", PAGE_PATH)
        .with_status(200)
        .with_body("<html></html>")
        .create();

    let err = lookup(&server).unwrap_err();
    assert!(
        format!("{err:#}").contains("did not redirect to a release"),
        "{err:#}"
    );
}

#[test]
fn test_latest_release_fails_when_redirect_has_no_version() {
    let mut server = mockito::Server::new();
    server.mock("GET", API_PATH).with_status(403).create();
    server
        .mock("GET", PAGE_PATH)
        .with_status(302)
        .with_header("location", &format!("{}/login", server.url()))
        .create();
    let login = server.mock("GET", "/login").expect(0).create();

    let err = lookup(&server).unwrap_err();
    assert!(format!("{err:#}").contains("No release version"), "{err:#}");
    login.assert();
}

#[test]
fn test_latest_release_skips_page_when_network_is_down() {
    let mut server = mockito::Server::new();
    let page = server.mock("GET", PAGE_PATH).expect(0).create();
    // Bind and drop a listener to get a local port that refuses connections.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_api_url = format!("http://{}{API_PATH}", closed.local_addr().unwrap());
    drop(closed);

    let err = check_latest_release_at(
        &test_agent(),
        &closed_api_url,
        &format!("{}{PAGE_PATH}", server.url()),
    )
    .unwrap_err();
    assert!(is_network_failure(&err), "{err:#}");
    page.assert();
}

// --- is_network_failure tests (error shapes ureq produces) ---

#[test]
fn test_network_failure_covers_connect_errors() {
    for ureq_err in [
        ureq::Error::HostNotFound,
        ureq::Error::ConnectionFailed,
        ureq::Error::Timeout(ureq::Timeout::Global),
    ] {
        let err = anyhow::Error::new(ureq_err).context("Failed to fetch latest release");
        assert!(is_network_failure(&err), "{err:#}");
    }
}

#[test]
fn test_network_failure_covers_broken_body_reads() {
    // ureq reports a body timeout as an io::Error that wraps its own error,
    // and a dropped connection as UnexpectedEof.
    let wrapped_timeout = std::io::Error::other(ureq::Error::Timeout(ureq::Timeout::Global));
    let cut_off = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Peer disconnected");
    for io_err in [wrapped_timeout, cut_off] {
        let err = anyhow::Error::new(io_err).context("Failed to read release JSON");
        assert!(is_network_failure(&err), "{err:#}");
    }
}

#[test]
fn test_network_failure_excludes_answers_from_github() {
    let refused =
        anyhow::Error::new(ureq::Error::StatusCode(403)).context("Failed to fetch latest release");
    let not_json =
        anyhow::Error::new(serde_json::from_str::<serde_json::Value>("<html>").unwrap_err())
            .context("Failed to parse release JSON");
    let bad_data = anyhow::Error::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "invalid chunk",
    ))
    .context("Failed to read release JSON");
    for err in [refused, not_json, bad_data] {
        assert!(!is_network_failure(&err), "{err:#}");
    }
}

// =========================================================================
// ureq v3 agent construction tests
// =========================================================================

#[test]
fn test_version_check_agent_creates_without_panic() {
    // Smoke test: the agent used in spawn_version_check
    let _agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(5)))
        .build()
        .new_agent();
}

#[test]
fn test_update_agent_creates_without_panic() {
    // Smoke test: the agent used in run_update
    let _agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .new_agent();
}
