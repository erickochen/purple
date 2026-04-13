//! Safety audit tests for the HashiCorp Vault SSH signing feature.
//!
//! Purple's core USP is that it NEVER corrupts ~/.ssh/config. These tests
//! exist because the Vault SSH feature is the newest write path and the
//! blast radius of a bug here (lost ssh config, hosts connected to the
//! wrong block, Match blocks mutated) is catastrophic.
//!
//! Each test locks down one specific safety invariant:
//!   1. Isolation: mutating host A leaves host B byte-identical.
//!   2. Match blocks remain inert (treated as GlobalLines, never written).
//!   3. Include files are not followed for writes.
//!   4. Silent no-op on missing alias (rename-in-flight scenario).
//!   5. Write failure leaves the on-disk config byte-identical (rollback).
//!   6. The public `set_host_certificate_file` API returns `false` so
//!      asynchronous callers (the bulk-sign worker) can surface the
//!      silent no-op to the user.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use purple_ssh::ssh_config::model::SshConfigFile;

/// Helper: parse a string into an SshConfigFile without touching disk.
fn parse_str(content: &str) -> SshConfigFile {
    SshConfigFile {
        elements: SshConfigFile::parse_content(content),
        path: PathBuf::from("/tmp/purple_vault_safety_test"),
        crlf: content.contains("\r\n"),
        bom: false,
    }
}

/// Helper: unique temp directory per test run. Uses PID + nanos + counter so
/// parallel test execution never collides. Caller is responsible for cleanup
/// via the returned `TempDir` guard.
static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "purple_vault_safety_{}_{}_{}_{}",
            label,
            std::process::id(),
            nanos,
            counter
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        Self { path: dir }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

// ============================================================================
// TEST 1 — ISOLATION
// Mutating one host block must leave every other block byte-identical,
// including comments, unknown directives, inline comments, blank separators
// and indentation style.
// ============================================================================

#[test]
fn vault_cert_write_leaves_sibling_hosts_byte_identical() {
    let input = "\
# Top-of-file comment. Must survive.

Host alpha
  HostName alpha.example.com
  User deploy
  Port 2222
  # indented comment inside alpha
  ForwardAgent yes

Host beta
\tHostName beta.example.com
\tUser root
\t# beta uses tabs on purpose
\tUnknownDirective42 keep-me
\tSetEnv FOO=bar # inline comment
\tPort = 22022

Host gamma
  HostName gamma.example.com
  User svc
  IdentityFile ~/.ssh/gamma_ed25519
";
    let mut config = parse_str(input);
    assert!(config.set_host_certificate_file("alpha", "/home/me/.purple/certs/alpha-cert.pub"));
    let out = config.serialize();

    // alpha got the directive
    assert!(
        out.contains("CertificateFile /home/me/.purple/certs/alpha-cert.pub"),
        "alpha should have the CertificateFile directive. Got:\n{}",
        out
    );

    // beta is untouched — every distinctive feature must survive verbatim
    assert!(out.contains("Host beta\n\tHostName beta.example.com"));
    assert!(out.contains("\tUser root\n\t# beta uses tabs on purpose"));
    assert!(out.contains("\tUnknownDirective42 keep-me"));
    assert!(out.contains("\tSetEnv FOO=bar # inline comment"));
    assert!(out.contains("\tPort = 22022"));

    // gamma is untouched
    assert!(out.contains(
        "Host gamma\n  HostName gamma.example.com\n  User svc\n  IdentityFile ~/.ssh/gamma_ed25519"
    ));

    // Top-of-file comment survives
    assert!(out.starts_with("# Top-of-file comment. Must survive."));

    // Beta must NOT have received a CertificateFile directive
    let beta_section = out
        .split("Host beta")
        .nth(1)
        .expect("beta section")
        .split("Host gamma")
        .next()
        .unwrap();
    assert!(
        !beta_section.contains("CertificateFile"),
        "beta unexpectedly received a CertificateFile directive:\n{}",
        beta_section
    );
}

// ============================================================================
// TEST 2 — MATCH BLOCKS REMAIN INERT
// A Match block must never be mutated by vault signing, even when the Match
// pattern would logically match the host alias being signed.
// ============================================================================

#[test]
fn vault_cert_write_does_not_touch_match_blocks() {
    let input = "\
Host alpha
  HostName 10.0.0.1

Match host alpha
  CertificateFile /user/configured/match-cert.pub
  User override
  ForwardAgent no

Host beta
  HostName 10.0.0.2
";
    let before_match_section = input
        .split("Match host alpha\n")
        .nth(1)
        .unwrap()
        .split("Host beta")
        .next()
        .unwrap()
        .to_string();

    let mut config = parse_str(input);
    assert!(config.set_host_certificate_file("alpha", "/home/me/.purple/certs/alpha-cert.pub"));

    let out = config.serialize();

    // The top-level alpha block got purple's cert path
    let alpha_section = out
        .split("Host alpha\n")
        .nth(1)
        .unwrap()
        .split("Match host alpha")
        .next()
        .unwrap();
    assert!(
        alpha_section.contains("CertificateFile /home/me/.purple/certs/alpha-cert.pub"),
        "alpha Host block should have purple's cert path. Got:\n{}",
        alpha_section
    );

    // The Match block survives completely unchanged
    let after_match_section = out
        .split("Match host alpha\n")
        .nth(1)
        .unwrap()
        .split("Host beta")
        .next()
        .unwrap()
        .to_string();
    assert_eq!(
        before_match_section, after_match_section,
        "Match block was mutated by vault signing!\nBEFORE:\n{}\nAFTER:\n{}",
        before_match_section, after_match_section
    );

    // The user-set Match CertificateFile is still present
    assert!(out.contains("CertificateFile /user/configured/match-cert.pub"));
}

// ============================================================================
// TEST 3 — INCLUDE FILES ARE NEVER WRITTEN
// Writing the main config must not mutate any included file on disk. The
// top-level block wins when the same alias appears in both.
// ============================================================================

#[test]
fn vault_cert_write_never_touches_include_file() {
    let tmp = TempDir::new("include");
    let include_path = tmp.path().join("included.conf");
    let main_path = tmp.path().join("config");

    let included_content = "\
Host alpha
  HostName from-include.example.com
  User include-user
  # Include file must survive unchanged
";
    let main_content = format!(
        "\
Include {}

Host alpha
  HostName from-main.example.com
  User main-user
",
        include_path.display()
    );

    fs::write(&include_path, included_content).unwrap();
    fs::write(&main_path, &main_content).unwrap();

    // Parse the main file (parser will resolve the Include for reads).
    let mut config = SshConfigFile::parse(&main_path).expect("parse main config");

    // Mutate alpha. The top-level block must absorb the change; the include
    // must not be touched.
    assert!(config.set_host_certificate_file("alpha", "/home/me/.purple/certs/alpha-cert.pub"));

    config.write().expect("write main config");

    // Include file is byte-identical.
    let include_after = fs::read_to_string(&include_path).unwrap();
    assert_eq!(
        included_content, include_after,
        "Include file was mutated!\nBEFORE:\n{}\nAFTER:\n{}",
        included_content, include_after
    );

    // Main file gained the directive on its own alpha block.
    let main_after = fs::read_to_string(&main_path).unwrap();
    assert!(
        main_after.contains("CertificateFile /home/me/.purple/certs/alpha-cert.pub"),
        "Main file should contain the new CertificateFile. Got:\n{}",
        main_after
    );
    // And the main file still references the include unchanged.
    assert!(main_after.contains(&format!("Include {}", include_path.display())));
}

// ============================================================================
// TEST 4 — RENAME-IN-FLIGHT: SILENT NO-OP DETECTED
// When the alias no longer exists at the moment set_host_certificate_file
// runs (because another code path renamed or deleted it), the call must
// return false AND leave the config byte-identical.
// ============================================================================

#[test]
fn vault_cert_write_is_noop_when_alias_missing() {
    let input = "\
Host alpha
  HostName 10.0.0.1
  User deploy

Host beta
  HostName 10.0.0.2
  User deploy
";
    let mut config = parse_str(input);

    // Simulate a rename-in-flight: the worker was going to sign "alpha_old"
    // but a concurrent edit renamed it to "alpha". The call must not touch
    // anything.
    let updated = config.set_host_certificate_file("alpha_old", "/tmp/should-never-land.pub");
    assert!(
        !updated,
        "set_host_certificate_file should return false for a missing alias"
    );

    // Config bytes are byte-identical to the input.
    assert_eq!(
        input,
        config.serialize(),
        "Config was mutated despite missing alias"
    );

    // And the real alpha block does NOT contain the ghost path.
    assert!(!config.serialize().contains("should-never-land.pub"));
}

// ============================================================================
// TEST 5 — WRITE FAILURE ROLLBACK
// When config.write() fails (target directory not writable, target file
// vanished, etc.), the original file on disk must be byte-identical.
// atomic_write uses temp-file + rename, so even a failed write leaves the
// original untouched — this test locks that invariant down.
// ============================================================================

#[cfg(unix)]
#[test]
fn vault_cert_write_failure_leaves_disk_byte_identical() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new("writefail");
    let config_path = tmp.path().join("config");
    let original = "\
Host alpha
  HostName 10.0.0.1
  User deploy
";
    fs::write(&config_path, original).unwrap();

    // Parse, mutate in memory.
    let mut config = SshConfigFile::parse(&config_path).expect("parse");
    assert!(config.set_host_certificate_file("alpha", "/home/me/.purple/certs/alpha-cert.pub"));

    // Make the PARENT directory read-only so atomic_write cannot create its
    // temp file there. On Unix this reliably fails the write call.
    let parent_perms = fs::metadata(tmp.path()).unwrap().permissions();
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o555)).unwrap();

    let write_result = config.write();

    // Restore permissions so TempDir::drop can clean up regardless.
    fs::set_permissions(tmp.path(), parent_perms).unwrap();

    assert!(
        write_result.is_err(),
        "Expected write to fail with read-only parent dir"
    );

    // On-disk content MUST be byte-identical to the original.
    let after = fs::read_to_string(&config_path).unwrap();
    assert_eq!(
        original, after,
        "Write failed but on-disk config was mutated!\nBEFORE:\n{}\nAFTER:\n{}",
        original, after
    );
}

// ============================================================================
// TEST 6 — API SURFACE CONTRACT
// Confirms the public API returns `bool` so the bulk-sign worker (and any
// future async caller) can distinguish a successful wire-up from a
// silently-dropped mutation.
// ============================================================================

#[test]
fn set_host_certificate_file_returns_true_on_success_false_on_missing() {
    let mut config = parse_str("Host alpha\n  HostName 10.0.0.1\n");
    assert!(
        config.set_host_certificate_file("alpha", "/tmp/alpha-cert.pub"),
        "present alias must return true"
    );
    assert!(
        !config.set_host_certificate_file("does_not_exist", "/tmp/ghost.pub"),
        "missing alias must return false"
    );
    // Second call for a present alias (update of existing directive) is also true.
    assert!(
        config.set_host_certificate_file("alpha", "/tmp/alpha-cert-rotated.pub"),
        "existing alias update must return true"
    );

    let out = config.serialize();
    assert!(out.contains("CertificateFile /tmp/alpha-cert-rotated.pub"));
    assert!(!out.contains("ghost.pub"));
}

// ============================================================================
// TEST 7 — WILDCARD ALIAS DEFENSE
// set_host_certificate_file must refuse aliases containing SSH glob
// characters even if a block with that literal pattern exists. Writing
// CertificateFile onto a `Host *` block would affect every concrete host at
// connection time — that is never the intent of the Vault SSH feature.
// ============================================================================

#[test]
fn vault_cert_write_refuses_wildcard_alias() {
    let input = "\
Host *
  User default-user

Host alpha
  HostName 10.0.0.1
";
    let mut config = parse_str(input);

    // Even though a literal `Host *` block exists, we refuse to target it.
    assert!(!config.set_host_certificate_file("*", "/tmp/should-never-land.pub"));
    assert!(!config.set_host_certificate_file("?", "/tmp/nope.pub"));
    assert!(!config.set_host_certificate_file("alpha*", "/tmp/nope.pub"));
    assert!(!config.set_host_certificate_file("al[ph]a", "/tmp/nope.pub"));
    assert!(!config.set_host_certificate_file("!alpha", "/tmp/nope.pub"));
    assert!(!config.set_host_certificate_file("", "/tmp/nope.pub"));
    // Multi-host / whitespace-separated aliases must also be refused.
    assert!(!config.set_host_certificate_file("web-* db-*", "/tmp/nope.pub"));
    assert!(!config.set_host_certificate_file("a b", "/tmp/nope.pub"));

    // Input is byte-identical after all those rejected calls.
    assert_eq!(input, config.serialize());

    // The concrete alias still works.
    assert!(config.set_host_certificate_file("alpha", "/tmp/alpha-cert.pub"));
    assert!(
        config
            .serialize()
            .contains("CertificateFile /tmp/alpha-cert.pub")
    );
}

// ============================================================================
// TEST 8 — CRLF PRESERVATION
// A CRLF-encoded ssh config must remain CRLF after a vault cert write.
// ============================================================================

#[test]
fn vault_cert_write_preserves_crlf_line_endings() {
    let input = "Host alpha\r\n  HostName 10.0.0.1\r\n\r\nHost beta\r\n  HostName 10.0.0.2\r\n";
    let mut config = parse_str(input);
    assert!(config.set_host_certificate_file("alpha", "/tmp/alpha-cert.pub"));
    let out = config.serialize();
    // Output must be CRLF
    assert!(out.contains("\r\n"), "Output lost CRLF line endings");
    assert!(
        !out.contains("\n\n") || out.contains("\r\n\r\n"),
        "Output has bare LFs where CRLFs should be"
    );
    // Beta still intact with CRLF
    assert!(out.contains("Host beta\r\n  HostName 10.0.0.2\r\n"));
}

// ============================================================================
// TEST 9 — PROPERTY-BASED ISOLATION
// For any randomly generated multi-host config, writing a CertificateFile
// to one target host must leave every OTHER host block byte-identical to
// its serialize()-before state. This is the strongest defense against
// "edit landed in the wrong place" corruption, because proptest will try
// edge cases that hand-written tests never cover.
// ============================================================================

use proptest::prelude::*;

// A restricted alias alphabet that stays inside OpenSSH's "safe" range and
// avoids the glob characters we reject in set_host_certificate_file.
fn arb_alias() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{0,15}".prop_filter("non-empty", |s| !s.is_empty())
}

fn arb_hostname() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9.-]{0,30}".prop_filter("non-empty", |s| !s.is_empty())
}

fn arb_directive_line() -> impl Strategy<Value = String> {
    prop_oneof![
        arb_hostname().prop_map(|h| format!("  HostName {}", h)),
        "[a-z]{2,10}".prop_map(|u| format!("  User {}", u)),
        (1u16..=65535).prop_map(|p| format!("  Port {}", p)),
        Just("  ForwardAgent yes".to_string()),
        Just("  # a comment inside the block".to_string()),
        // An unknown directive that the parser must preserve verbatim.
        "[A-Z][a-zA-Z0-9]{3,12}".prop_map(|k| format!("  {} keep-me-verbatim", k)),
    ]
}

#[derive(Debug, Clone)]
struct ArbHost {
    alias: String,
    directives: Vec<String>,
}

fn arb_host() -> impl Strategy<Value = ArbHost> {
    (
        arb_alias(),
        prop::collection::vec(arb_directive_line(), 1..6),
    )
        .prop_map(|(alias, directives)| ArbHost { alias, directives })
}

fn render_config(hosts: &[ArbHost]) -> String {
    let mut s = String::new();
    for (i, h) in hosts.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        s.push_str(&format!("Host {}\n", h.alias));
        for d in &h.directives {
            s.push_str(d);
            s.push('\n');
        }
    }
    s
}

/// Split a serialized config into a map of `alias -> block text (including
/// Host header and all directives until the next Host/blank separator)`.
/// Used to compare per-host byte slices across a mutation.
fn host_block_slices(serialized: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut current: Option<(String, String)> = None;
    for line in serialized.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("Host ") {
            if let Some((alias, body)) = current.take() {
                out.insert(alias, body);
            }
            let alias = rest.split_whitespace().next().unwrap_or("").to_string();
            current = Some((alias, format!("{}\n", line)));
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some((alias, body)) = current {
        out.insert(alias, body);
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        max_shrink_iters: 256,
        ..ProptestConfig::default()
    })]

    /// For any generated multi-host config, writing CertificateFile to one
    /// target host must leave every OTHER host's block bytes unchanged.
    #[test]
    fn proptest_cert_write_leaves_siblings_byte_identical(
        hosts in prop::collection::vec(arb_host(), 2..8),
        target_idx in 0usize..8,
    ) {
        // Ensure unique aliases so the "exact match" invariant is
        // unambiguous.
        let mut seen = std::collections::HashSet::new();
        let unique: Vec<_> = hosts
            .into_iter()
            .filter(|h| seen.insert(h.alias.clone()))
            .collect();
        prop_assume!(unique.len() >= 2);

        let target_idx = target_idx % unique.len();
        let target_alias = unique[target_idx].alias.clone();

        let input = render_config(&unique);
        let before_config = parse_str(&input);
        let before_slices = host_block_slices(&before_config.serialize());

        let mut after_config = parse_str(&input);
        let cert_path = format!("/home/me/.purple/certs/{}-cert.pub", target_alias);
        prop_assert!(after_config.set_host_certificate_file(&target_alias, &cert_path));
        let after_slices = host_block_slices(&after_config.serialize());

        prop_assert_eq!(
            before_slices.keys().collect::<Vec<_>>(),
            after_slices.keys().collect::<Vec<_>>(),
            "host block set changed during vault write"
        );

        for (alias, before_body) in &before_slices {
            let after_body = after_slices.get(alias).unwrap();
            if alias == &target_alias {
                // Target must contain the new CertificateFile directive,
                // and must be a superset of the original (existing
                // directives preserved).
                prop_assert!(
                    after_body.contains(&format!("CertificateFile {}", cert_path)),
                    "target block missing the new CertificateFile directive"
                );
                // Every original directive line must still be present.
                for line in before_body.lines() {
                    prop_assert!(
                        after_body.contains(line),
                        "target block lost an original line: {:?}",
                        line
                    );
                }
            } else {
                // Sibling blocks must be byte-identical.
                prop_assert_eq!(
                    before_body,
                    after_body,
                    "sibling block '{}' was mutated by vault write",
                    alias
                );
            }
        }
    }

    /// Writing to a missing alias must leave the ENTIRE serialized config
    /// byte-identical.
    #[test]
    fn proptest_cert_write_missing_alias_is_total_noop(
        hosts in prop::collection::vec(arb_host(), 1..6),
        ghost_alias in arb_alias(),
    ) {
        let mut seen = std::collections::HashSet::new();
        let unique: Vec<_> = hosts
            .into_iter()
            .filter(|h| seen.insert(h.alias.clone()))
            .collect();
        prop_assume!(!unique.iter().any(|h| h.alias == ghost_alias));

        let input = render_config(&unique);
        let before = parse_str(&input).serialize();

        let mut config = parse_str(&input);
        prop_assert!(!config.set_host_certificate_file(&ghost_alias, "/tmp/ghost.pub"));
        let after = config.serialize();

        prop_assert_eq!(before, after);
    }

    // ========================================================================
    // vault_addr write path — mirror the CertificateFile invariants.
    // The VAULT_ADDR comment has an identical blast radius (it lives inside
    // a host block) and therefore must respect the same guarantees:
    //   - sibling host blocks stay byte-identical after a mutation
    //   - missing aliases produce a total no-op
    // ========================================================================

    /// For any generated multi-host config, writing `vault-addr` to one
    /// target host must leave every OTHER host's block bytes unchanged.
    #[test]
    fn proptest_vault_addr_write_leaves_siblings_byte_identical(
        hosts in prop::collection::vec(arb_host(), 2..8),
        target_idx in 0usize..8,
    ) {
        let mut seen = std::collections::HashSet::new();
        let unique: Vec<_> = hosts
            .into_iter()
            .filter(|h| seen.insert(h.alias.clone()))
            .collect();
        prop_assume!(unique.len() >= 2);

        let target_idx = target_idx % unique.len();
        let target_alias = unique[target_idx].alias.clone();

        let input = render_config(&unique);
        let before_config = parse_str(&input);
        let before_slices = host_block_slices(&before_config.serialize());

        let mut after_config = parse_str(&input);
        let addr = "http://127.0.0.1:8200";
        prop_assert!(after_config.set_host_vault_addr(&target_alias, addr));
        let after_slices = host_block_slices(&after_config.serialize());

        prop_assert_eq!(
            before_slices.keys().collect::<Vec<_>>(),
            after_slices.keys().collect::<Vec<_>>(),
            "host block set changed during vault-addr write"
        );

        for (alias, before_body) in &before_slices {
            let after_body = after_slices.get(alias).unwrap();
            if alias == &target_alias {
                prop_assert!(
                    after_body.contains(&format!("# purple:vault-addr {}", addr)),
                    "target block missing the new vault-addr comment"
                );
                for line in before_body.lines() {
                    prop_assert!(
                        after_body.contains(line),
                        "target block lost an original line: {:?}",
                        line
                    );
                }
            } else {
                prop_assert_eq!(
                    before_body,
                    after_body,
                    "sibling block '{}' was mutated by vault-addr write",
                    alias
                );
            }
        }
    }

    /// Writing to a missing alias must leave the ENTIRE serialized config
    /// byte-identical.
    #[test]
    fn proptest_vault_addr_write_missing_alias_is_total_noop(
        hosts in prop::collection::vec(arb_host(), 1..6),
        ghost_alias in arb_alias(),
    ) {
        let mut seen = std::collections::HashSet::new();
        let unique: Vec<_> = hosts
            .into_iter()
            .filter(|h| seen.insert(h.alias.clone()))
            .collect();
        prop_assume!(!unique.iter().any(|h| h.alias == ghost_alias));

        let input = render_config(&unique);
        let before = parse_str(&input).serialize();

        let mut config = parse_str(&input);
        prop_assert!(!config.set_host_vault_addr(&ghost_alias, "http://vault.example:8200"));
        let after = config.serialize();

        prop_assert_eq!(before, after);
    }
}

// ============================================================================
// TEST — vault_addr single-case invariants (mirror certificate_file tests)
// ============================================================================

#[test]
fn set_host_vault_addr_returns_true_on_success_false_on_missing() {
    let mut config = parse_str("Host alpha\n  HostName 10.0.0.1\n");
    assert!(
        config.set_host_vault_addr("alpha", "http://127.0.0.1:8200"),
        "present alias must return true"
    );
    assert!(
        !config.set_host_vault_addr("does_not_exist", "http://vault.example:8200"),
        "missing alias must return false"
    );
    let out = config.serialize();
    assert!(out.contains("# purple:vault-addr http://127.0.0.1:8200"));
    assert!(!out.contains("vault.example"));
}

#[test]
fn vault_addr_write_does_not_touch_match_blocks() {
    let input = "\
Host alpha
  HostName 10.0.0.1

Match host alpha
  # purple:vault-addr http://match-original:8200
";
    let mut config = parse_str(input);
    assert!(config.set_host_vault_addr("alpha", "http://managed:8200"));
    let out = config.serialize();
    assert!(
        out.contains("Host alpha\n  HostName 10.0.0.1\n  # purple:vault-addr http://managed:8200")
    );
    assert!(out.contains("Match host alpha\n  # purple:vault-addr http://match-original:8200"));
}

#[test]
fn vault_addr_write_refuses_wildcard_alias() {
    let mut config = parse_str("Host *\n  HostName 10.0.0.1\n");
    assert!(!config.set_host_vault_addr("*", "http://127.0.0.1:8200"));
    assert!(!config.set_host_vault_addr("", "http://127.0.0.1:8200"));
    assert!(!config.set_host_vault_addr("a?b", "http://127.0.0.1:8200"));
    assert!(!config.set_host_vault_addr("a[bc]", "http://127.0.0.1:8200"));
    assert!(!config.set_host_vault_addr("!a", "http://127.0.0.1:8200"));
    let out = config.serialize();
    assert!(!out.contains("vault-addr"));
}

#[test]
fn vault_addr_write_preserves_crlf_line_endings() {
    let input = "Host alpha\r\n  HostName 10.0.0.1\r\n\r\nHost beta\r\n  HostName 10.0.0.2\r\n";
    let mut config = parse_str(input);
    assert!(config.set_host_vault_addr("alpha", "http://127.0.0.1:8200"));
    let out = config.serialize();
    assert!(out.contains("\r\n"), "Output lost CRLF line endings");
    assert!(out.contains("Host beta\r\n  HostName 10.0.0.2\r\n"));
}

#[test]
fn vault_addr_write_is_noop_when_alias_missing() {
    let input = "Host alpha\n  HostName 10.0.0.1\n\nHost beta\n  HostName 10.0.0.2\n";
    let before = parse_str(input).serialize();
    let mut config = parse_str(input);
    assert!(!config.set_host_vault_addr("ghost", "http://127.0.0.1:8200"));
    assert_eq!(config.serialize(), before);
}
