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
fn test_extract_headline_empty() {
    assert_eq!(extract_headline(""), None);
}

#[test]
fn test_extract_headline_only_blanks() {
    assert_eq!(extract_headline("\n\n\n"), None);
}

// --- strip_markdown tests ---

#[test]
fn test_strip_markdown_plain_text() {
    assert_eq!(strip_markdown("hello world"), "hello world");
}

#[test]
fn test_strip_markdown_bullet() {
    assert_eq!(strip_markdown("- Added feature X"), "- Added feature X");
}

#[test]
fn test_strip_markdown_bold() {
    assert_eq!(strip_markdown("This is **bold** text"), "This is bold text");
}

#[test]
fn test_strip_markdown_underscore_bold() {
    assert_eq!(strip_markdown("This is __bold__ text"), "This is bold text");
}

#[test]
fn test_strip_markdown_heading() {
    assert_eq!(strip_markdown("## What's new"), "What's new");
}

#[test]
fn test_strip_markdown_heading_h3() {
    assert_eq!(strip_markdown("### Details"), "Details");
}

#[test]
fn test_strip_markdown_link() {
    assert_eq!(
        strip_markdown("See [the docs](https://example.com) for details"),
        "See the docs for details"
    );
}

#[test]
fn test_strip_markdown_multiple_links() {
    assert_eq!(
        strip_markdown("[a](http://a.com) and [b](http://b.com)"),
        "a and b"
    );
}

#[test]
fn test_strip_markdown_bare_brackets() {
    assert_eq!(strip_markdown("array[0] = 1"), "array[0] = 1");
}

#[test]
fn test_strip_markdown_nested_brackets_in_link() {
    // Link text contains a bracket — must not infinite loop
    assert_eq!(
        strip_markdown("See [[x]](http://example.com) here"),
        "See [x] here"
    );
}

#[test]
fn test_strip_markdown_escape_sequences() {
    assert_eq!(
        strip_markdown("hello \x1b[31mred\x1b[0m world"),
        "hello [31mred[0m world"
    );
}

#[test]
fn test_strip_markdown_control_chars() {
    assert_eq!(strip_markdown("a\x07b\x08c"), "abc");
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
        detect_install_method(path),
        InstallMethod::Homebrew
    ));
}

#[test]
fn test_detect_homebrew_default_intel() {
    let path = Path::new("/usr/local/Cellar/purple/1.5.0/bin/purple");
    assert!(matches!(
        detect_install_method(path),
        InstallMethod::Homebrew
    ));
}

#[test]
fn test_detect_homebrew_default_linuxbrew() {
    let path = Path::new("/home/linuxbrew/.linuxbrew/Cellar/purple/2.3.0/bin/purple");
    assert!(matches!(
        detect_install_method(path),
        InstallMethod::Homebrew
    ));
}

#[test]
fn test_detect_cargo_default() {
    let path = Path::new("/Users/user/.cargo/bin/purple");
    assert!(matches!(detect_install_method(path), InstallMethod::Cargo));
}

#[test]
fn test_detect_curl_usr_local_bin() {
    let path = Path::new("/usr/local/bin/purple");
    assert!(matches!(
        detect_install_method(path),
        InstallMethod::CurlOrManual
    ));
}

#[test]
fn test_detect_curl_local_bin() {
    let path = Path::new("/Users/user/.local/bin/purple");
    assert!(matches!(
        detect_install_method(path),
        InstallMethod::CurlOrManual
    ));
}

#[test]
fn test_detect_no_false_positive_homebrew_in_name() {
    let path = Path::new("/Users/user/homebrew-tools/bin/purple");
    assert!(matches!(
        detect_install_method(path),
        InstallMethod::CurlOrManual
    ));
}

// --- fail-open: ambiguous paths default to CurlOrManual ---

#[test]
fn test_detect_unknown_path() {
    let path = Path::new("/some/random/path/purple");
    assert!(matches!(
        detect_install_method(path),
        InstallMethod::CurlOrManual
    ));
}

#[test]
fn test_detect_root_path() {
    let path = Path::new("/purple");
    assert!(matches!(
        detect_install_method(path),
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
