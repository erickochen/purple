use std::io;
use std::path::PathBuf;

use log::debug;

use crate::app::{ContainersSortMode, SortMode, ViewMode};
use crate::fs_util;
use crate::runtime::env::Paths;

/// Cross-suite test lock for the `demo_flag` and theme globals, which remain
/// process-global for now. Aliased here so existing binary-side callers keep
/// resolving it under its historical name. The preferences file path itself is
/// no longer global: it is derived from the injected `Paths`.
#[cfg(test)]
pub(crate) use crate::demo_flag::GLOBAL_TEST_LOCK as GLOBAL_TEST_IO_LOCK;

/// The preferences file under the given paths, or `None` when the home
/// directory is unknown. A `None` here makes every load return the default and
/// every save a silent no-op, matching the historical behaviour when
/// `dirs::home_dir()` returned `None`.
fn prefs_file(paths: Option<&Paths>) -> Option<PathBuf> {
    paths.map(Paths::preferences)
}

/// Load a value for a given key from the preferences file.
fn load_value(paths: Option<&Paths>, key: &str) -> Option<String> {
    let path = prefs_file(paths)?;
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                debug!("[config] Failed to read preferences file: {e}");
            }
            return None;
        }
    };
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=')
            && k.trim() == key
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// Save a key=value pair to the preferences file. Preserves unknown keys and comments.
fn save_value(paths: Option<&Paths>, key: &str, value: &str) -> io::Result<()> {
    let path = match prefs_file(paths) {
        Some(p) => p,
        None => return Ok(()),
    };
    // In production demo mode disk writes are suppressed so the user's
    // real preferences file stays untouched. Inside tests the path comes
    // from the injected sandbox `Paths`, so we let writes through regardless
    // of the global demo flag (handler fixtures flip that flag and would
    // otherwise mute every prefs assertion that runs in parallel with them).
    #[cfg(not(test))]
    if crate::demo_flag::is_demo() {
        return Ok(());
    }

    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    let mut found = false;

    for line in existing.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('#')
            && !trimmed.is_empty()
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key)
        {
            lines.push(format!("{}={}", key, value));
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !found {
        lines.push(format!("{}={}", key, value));
    }

    let content = lines.join("\n") + "\n";

    fs_util::atomic_write(&path, content.as_bytes())
}

/// Load sort mode from ~/.purple/preferences. Returns MostRecent if missing or invalid.
pub fn load_sort_mode(paths: Option<&Paths>) -> SortMode {
    load_value(paths, "sort_mode")
        .map(|v| SortMode::from_key(&v))
        .unwrap_or(SortMode::MostRecent)
}

/// Save sort mode to ~/.purple/preferences.
pub fn save_sort_mode(paths: Option<&Paths>, mode: SortMode) -> io::Result<()> {
    log::debug!("[purple] saving sort_mode={}", mode.to_key());
    save_value(paths, "sort_mode", mode.to_key()).inspect_err(|e| {
        log::warn!("[config] failed to save sort_mode={}: {}", mode.to_key(), e);
    })
}

/// Load group_by from ~/.purple/preferences. New `group_by` key takes precedence
/// over the legacy `group_by_provider` key for backward compatibility.
/// Returns `GroupBy::Provider` if missing (preserving old default behavior).
pub fn load_group_by(paths: Option<&Paths>) -> crate::app::GroupBy {
    use crate::app::GroupBy;
    if let Some(v) = load_value(paths, "group_by") {
        return GroupBy::from_key(&v);
    }
    if let Some(v) = load_value(paths, "group_by_provider") {
        return if v == "true" {
            GroupBy::Provider
        } else {
            GroupBy::None
        };
    }
    GroupBy::Provider
}

/// Remove a key from the preferences file. No-op if the key or file does not exist.
fn remove_value(paths: Option<&Paths>, key: &str) -> io::Result<()> {
    let path = match prefs_file(paths) {
        Some(p) => p,
        None => return Ok(()),
    };
    // Same demo-vs-test gate as `save_value`: production demo mode
    // suppresses disk writes; in tests the path is an injected sandbox
    // so writes go through irrespective of the global flag.
    #[cfg(not(test))]
    if crate::demo_flag::is_demo() {
        return Ok(());
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    // Early return if key not present — avoids unnecessary rewrite
    let has_key = existing.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.starts_with('#')
            && !trimmed.is_empty()
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == key)
    });
    if !has_key {
        return Ok(());
    }

    let lines: Vec<String> = existing
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                return true;
            }
            trimmed.split_once('=').is_none_or(|(k, _)| k.trim() != key)
        })
        .map(|l| l.to_string())
        .collect();
    let content = lines.join("\n") + "\n";
    fs_util::atomic_write(&path, content.as_bytes())
}

/// Save group_by to ~/.purple/preferences.
pub fn save_group_by(paths: Option<&Paths>, mode: &crate::app::GroupBy) -> io::Result<()> {
    log::debug!("[purple] saving group_by={}", mode.to_key());
    save_value(paths, "group_by", &mode.to_key()).inspect_err(|e| {
        log::warn!("[config] failed to save group_by={}: {}", mode.to_key(), e);
    })?;
    // Best-effort cleanup: group_by key takes precedence on load, so
    // a leftover group_by_provider key is harmless if removal fails.
    let _ = remove_value(paths, "group_by_provider");
    Ok(())
}

/// Load view mode from ~/.purple/preferences. Returns Detailed if missing or invalid.
pub fn load_view_mode(paths: Option<&Paths>) -> ViewMode {
    load_value(paths, "view_mode")
        .map(|v| match v.as_str() {
            "compact" => ViewMode::Compact,
            _ => ViewMode::Detailed,
        })
        .unwrap_or(ViewMode::Detailed)
}

/// Save view mode to ~/.purple/preferences.
pub fn save_view_mode(paths: Option<&Paths>, mode: ViewMode) -> io::Result<()> {
    let value = match mode {
        ViewMode::Compact => "compact",
        ViewMode::Detailed => "detailed",
    };
    log::debug!("[purple] saving view_mode={}", value);
    save_value(paths, "view_mode", value).inspect_err(|e| {
        log::warn!("[config] failed to save view_mode={}: {}", value, e);
    })
}

/// Containers-tab sort order. Separate key from the host-list `sort_mode`
/// so the two screens persist independently. Default `AlphaHost` matches
/// `ContainersSortMode::default()`.
pub fn load_containers_sort_mode(paths: Option<&Paths>) -> ContainersSortMode {
    load_value(paths, "containers_sort_mode")
        .map(|v| ContainersSortMode::from_key(&v))
        .unwrap_or_default()
}

pub fn save_containers_sort_mode(
    paths: Option<&Paths>,
    mode: ContainersSortMode,
) -> io::Result<()> {
    log::debug!("[purple] saving containers_sort_mode={}", mode.to_key());
    save_value(paths, "containers_sort_mode", mode.to_key()).inspect_err(|e| {
        log::warn!(
            "[config] failed to save containers_sort_mode={}: {}",
            mode.to_key(),
            e
        );
    })
}

/// Containers-tab detail panel toggle. Separate key so the host-list
/// preference does not bleed into the containers screen and vice versa.
/// Default Detailed: when nothing is saved yet the detail panel renders
/// alongside the list whenever the terminal is wide enough.
pub fn load_containers_view_mode(paths: Option<&Paths>) -> ViewMode {
    load_value(paths, "containers_view_mode")
        .map(|v| match v.as_str() {
            "compact" => ViewMode::Compact,
            _ => ViewMode::Detailed,
        })
        .unwrap_or(ViewMode::Detailed)
}

pub fn save_containers_view_mode(paths: Option<&Paths>, mode: ViewMode) -> io::Result<()> {
    let value = match mode {
        ViewMode::Compact => "compact",
        ViewMode::Detailed => "detailed",
    };
    log::debug!("[purple] saving containers_view_mode={}", value);
    save_value(paths, "containers_view_mode", value).inspect_err(|e| {
        log::warn!(
            "[config] failed to save containers_view_mode={}: {}",
            value,
            e
        );
    })
}

/// Aliases whose containers group is currently folded in the
/// containers-tab AlphaHost view. Persists as a comma-separated list so
/// a fresh start restores the user's last fold state. Empty list means
/// every group is expanded.
pub fn load_containers_collapsed_hosts(paths: Option<&Paths>) -> std::collections::HashSet<String> {
    load_value(paths, "containers_collapsed_hosts")
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn save_containers_collapsed_hosts(
    paths: Option<&Paths>,
    aliases: &std::collections::HashSet<String>,
) -> io::Result<()> {
    if aliases.is_empty() {
        log::debug!("[purple] clearing containers_collapsed_hosts");
        let _ = remove_value(paths, "containers_collapsed_hosts");
        return Ok(());
    }
    let mut sorted: Vec<&str> = aliases.iter().map(|s| s.as_str()).collect();
    sorted.sort_unstable();
    let joined = sorted.join(",");
    log::debug!(
        "[purple] saving containers_collapsed_hosts={} ({} aliases)",
        joined,
        sorted.len()
    );
    save_value(paths, "containers_collapsed_hosts", &joined).inspect_err(|e| {
        log::warn!("[config] failed to save containers_collapsed_hosts: {}", e);
    })
}

/// Load global askpass default from ~/.purple/preferences.
pub fn load_askpass_default(paths: Option<&Paths>) -> Option<String> {
    load_value(paths, "askpass").filter(|v| !v.is_empty())
}

/// Save global askpass default to ~/.purple/preferences.
pub fn save_askpass_default(paths: Option<&Paths>, source: &str) -> io::Result<()> {
    log::debug!("[purple] saving askpass default={}", source);
    save_value(paths, "askpass", source).inspect_err(|e| {
        log::warn!("[config] failed to save askpass={}: {}", source, e);
    })
}

/// Load slow threshold from ~/.purple/preferences. Returns 200 if missing or invalid.
pub fn load_slow_threshold(paths: Option<&Paths>) -> u16 {
    load_value(paths, "slow_threshold_ms")
        .and_then(|v| v.parse().ok())
        .unwrap_or(200)
}

/// Save slow threshold to ~/.purple/preferences.
#[allow(dead_code)]
pub fn save_slow_threshold(paths: Option<&Paths>, ms: u16) -> io::Result<()> {
    log::debug!("[purple] saving slow_threshold_ms={}", ms);
    save_value(paths, "slow_threshold_ms", &ms.to_string()).inspect_err(|e| {
        log::warn!("[config] failed to save slow_threshold_ms={}: {}", ms, e);
    })
}

/// Load theme name from ~/.purple/preferences. Returns None if missing.
pub fn load_theme(paths: Option<&Paths>) -> Option<String> {
    load_value(paths, "theme").filter(|v| !v.is_empty())
}

/// Save theme name to ~/.purple/preferences.
pub fn save_theme(paths: Option<&Paths>, name: &str) -> io::Result<()> {
    log::debug!("[purple] saving theme={}", name);
    save_value(paths, "theme", name).inspect_err(|e| {
        log::warn!("[config] failed to save theme={}: {}", name, e);
    })
}

const LAST_SEEN_VERSION_KEY: &str = "last_seen_version";

/// Save the last seen version string to ~/.purple/preferences.
pub fn save_last_seen_version(paths: Option<&Paths>, version: &str) -> io::Result<()> {
    log::debug!("[purple] saving last_seen_version={}", version);
    save_value(paths, LAST_SEEN_VERSION_KEY, version)
}

/// Load the last seen version string from ~/.purple/preferences. Returns None if missing.
pub fn load_last_seen_version(paths: Option<&Paths>) -> io::Result<Option<String>> {
    Ok(load_value(paths, LAST_SEEN_VERSION_KEY))
}

/// Public test helpers for other test modules that need isolated preferences I/O.
#[cfg(test)]
pub(crate) mod tests_helpers {
    pub fn with_temp_prefs<F: FnOnce(&crate::runtime::env::Paths)>(f: F) {
        let dir = tempfile::tempdir().expect("create temp prefs dir");
        let paths = crate::runtime::env::Paths::new(dir.path());
        // Tests that seed the file with std::fs::write need the parent to exist;
        // atomic_write creates it on its own but a bare std::fs::write does not.
        if let Some(parent) = paths.preferences().parent() {
            std::fs::create_dir_all(parent).expect("create prefs parent dir");
        }
        f(&paths);
    }
}

/// Load auto_ping preference. Returns true if missing (default: enabled).
pub fn load_auto_ping(paths: Option<&Paths>) -> bool {
    load_value(paths, "auto_ping")
        .map(|v| v != "false")
        .unwrap_or(true)
}

/// Save auto_ping preference.
#[allow(dead_code)]
pub fn save_auto_ping(paths: Option<&Paths>, enabled: bool) -> io::Result<()> {
    let value = if enabled { "true" } else { "false" };
    log::debug!("[purple] saving auto_ping={}", value);
    save_value(paths, "auto_ping", value).inspect_err(|e| {
        log::warn!("[config] failed to save auto_ping={}: {}", value, e);
    })
}

/// Load the interactive ssh command (`ssh_command`, e.g. `kitten ssh`).
/// Returns None when missing or empty; the caller falls back to plain `ssh`.
pub fn load_ssh_command(paths: Option<&Paths>) -> Option<String> {
    load_value(paths, crate::ssh_launcher::PREF_KEY).filter(|v| !v.trim().is_empty())
}

/// Load the SSH config path (`ssh_config`). Returns None when missing or
/// empty; the caller falls back to `~/.ssh/config`.
pub fn load_ssh_config_path(paths: Option<&Paths>) -> Option<String> {
    load_value(paths, crate::runtime::helpers::SSH_CONFIG_PREF_KEY).filter(|v| !v.trim().is_empty())
}

/// Save the interactive ssh command preference.
#[allow(dead_code)]
pub fn save_ssh_command(paths: Option<&Paths>, command: &str) -> io::Result<()> {
    log::debug!("[purple] saving ssh_command={}", command);
    save_value(paths, crate::ssh_launcher::PREF_KEY, command).inspect_err(|e| {
        log::warn!("[config] failed to save ssh_command={}: {}", command, e);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // We test load_value/save_value logic by replicating the parsing inline,
    // since the real functions read from ~/.purple/preferences.

    fn parse_value(content: &str, key: &str) -> Option<String> {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some((k, v)) = line.split_once('=')
                && k.trim() == key
            {
                return Some(v.trim().to_string());
            }
        }
        None
    }

    #[test]
    fn load_askpass_returns_value() {
        let content = "askpass=keychain\n";
        let val = parse_value(content, "askpass").filter(|v| !v.is_empty());
        assert_eq!(val, Some("keychain".to_string()));
    }

    #[test]
    fn load_askpass_returns_none_for_empty() {
        let content = "askpass=\n";
        let val = parse_value(content, "askpass").filter(|v| !v.is_empty());
        assert_eq!(val, None);
    }

    #[test]
    fn load_askpass_returns_none_when_missing() {
        let content = "sort_mode=alpha\n";
        let val = parse_value(content, "askpass").filter(|v| !v.is_empty());
        assert_eq!(val, None);
    }

    #[test]
    fn load_askpass_preserves_vault_uri() {
        let content = "askpass=vault:secret/ssh#password\n";
        let val = parse_value(content, "askpass").filter(|v| !v.is_empty());
        assert_eq!(val, Some("vault:secret/ssh#password".to_string()));
    }

    #[test]
    fn load_askpass_preserves_op_uri() {
        let content = "askpass=op://Vault/SSH/password\n";
        let val = parse_value(content, "askpass").filter(|v| !v.is_empty());
        assert_eq!(val, Some("op://Vault/SSH/password".to_string()));
    }

    #[test]
    fn load_askpass_among_other_prefs() {
        let content = "sort_mode=alpha\ngroup_by_provider=true\naskpass=bw:my-item\n";
        let val = parse_value(content, "askpass").filter(|v| !v.is_empty());
        assert_eq!(val, Some("bw:my-item".to_string()));
    }

    #[test]
    fn save_value_builds_correct_line() {
        // Verify the format that save_value produces
        let key = "askpass";
        let value = "keychain";
        let line = format!("{}={}", key, value);
        assert_eq!(line, "askpass=keychain");
    }

    #[test]
    fn save_value_replaces_existing() {
        // Simulate save_value logic
        let existing = "sort_mode=alpha\naskpass=old\n";
        let key = "askpass";
        let new_value = "vault:secret/ssh";

        let mut lines: Vec<String> = Vec::new();
        let mut found = false;
        for line in existing.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with('#')
                && !trimmed.is_empty()
                && trimmed
                    .split_once('=')
                    .is_some_and(|(k, _)| k.trim() == key)
            {
                lines.push(format!("{}={}", key, new_value));
                found = true;
            } else {
                lines.push(line.to_string());
            }
        }
        if !found {
            lines.push(format!("{}={}", key, new_value));
        }
        let content = lines.join("\n") + "\n";
        assert!(content.contains("askpass=vault:secret/ssh"));
        assert!(!content.contains("askpass=old"));
        assert!(content.contains("sort_mode=alpha"));
        assert!(found);
    }

    #[test]
    fn load_group_by_new_key_none() {
        let content = "group_by=none\n";
        let val = parse_value(content, "group_by").unwrap_or_default();
        assert_eq!(
            crate::app::GroupBy::from_key(&val),
            crate::app::GroupBy::None
        );
    }

    #[test]
    fn load_group_by_new_key_provider() {
        let content = "group_by=provider\n";
        let val = parse_value(content, "group_by").unwrap_or_default();
        assert_eq!(
            crate::app::GroupBy::from_key(&val),
            crate::app::GroupBy::Provider
        );
    }

    #[test]
    fn load_group_by_new_key_tag() {
        let content = "group_by=tag:production\n";
        let val = parse_value(content, "group_by").unwrap_or_default();
        assert_eq!(
            crate::app::GroupBy::from_key(&val),
            crate::app::GroupBy::Tag("production".to_string())
        );
    }

    #[test]
    fn load_group_by_backward_compat_true() {
        let content = "group_by_provider=true\n";
        let new_val = parse_value(content, "group_by");
        let old_val = parse_value(content, "group_by_provider");
        let result = if let Some(v) = new_val {
            crate::app::GroupBy::from_key(&v)
        } else if let Some(v) = old_val {
            if v == "true" {
                crate::app::GroupBy::Provider
            } else {
                crate::app::GroupBy::None
            }
        } else {
            crate::app::GroupBy::None
        };
        assert_eq!(result, crate::app::GroupBy::Provider);
    }

    #[test]
    fn load_group_by_backward_compat_false() {
        let content = "group_by_provider=false\n";
        let new_val = parse_value(content, "group_by");
        let old_val = parse_value(content, "group_by_provider");
        let result = if let Some(v) = new_val {
            crate::app::GroupBy::from_key(&v)
        } else if let Some(v) = old_val {
            if v == "true" {
                crate::app::GroupBy::Provider
            } else {
                crate::app::GroupBy::None
            }
        } else {
            crate::app::GroupBy::None
        };
        assert_eq!(result, crate::app::GroupBy::None);
    }

    #[test]
    fn load_group_by_new_key_overrides_old() {
        let content = "group_by_provider=true\ngroup_by=tag:staging\n";
        let new_val = parse_value(content, "group_by");
        let old_val = parse_value(content, "group_by_provider");
        let result = if let Some(v) = new_val {
            crate::app::GroupBy::from_key(&v)
        } else if let Some(v) = old_val {
            if v == "true" {
                crate::app::GroupBy::Provider
            } else {
                crate::app::GroupBy::None
            }
        } else {
            crate::app::GroupBy::None
        };
        assert_eq!(result, crate::app::GroupBy::Tag("staging".to_string()));
    }

    #[test]
    fn load_group_by_missing_defaults_to_provider() {
        let content = "sort_mode=alpha\n";
        let new_val = parse_value(content, "group_by");
        let old_val = parse_value(content, "group_by_provider");
        let result = if let Some(v) = new_val {
            crate::app::GroupBy::from_key(&v)
        } else if let Some(v) = old_val {
            if v == "true" {
                crate::app::GroupBy::Provider
            } else {
                crate::app::GroupBy::None
            }
        } else {
            crate::app::GroupBy::Provider
        };
        assert_eq!(result, crate::app::GroupBy::Provider);
    }

    #[test]
    fn save_group_by_format() {
        let key = "group_by";
        let value = crate::app::GroupBy::Tag("production".to_string()).to_key();
        let line = format!("{}={}", key, value);
        assert_eq!(line, "group_by=tag:production");
    }

    #[test]
    fn save_value_appends_new_key() {
        let existing = "sort_mode=alpha\n";
        let key = "askpass";
        let new_value = "keychain";

        let mut lines: Vec<String> = Vec::new();
        let mut found = false;
        for line in existing.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with('#')
                && !trimmed.is_empty()
                && trimmed
                    .split_once('=')
                    .is_some_and(|(k, _)| k.trim() == key)
            {
                lines.push(format!("{}={}", key, new_value));
                found = true;
            } else {
                lines.push(line.to_string());
            }
        }
        if !found {
            lines.push(format!("{}={}", key, new_value));
        }
        let content = lines.join("\n") + "\n";
        assert!(content.contains("askpass=keychain"));
        assert!(content.contains("sort_mode=alpha"));
        assert!(!found); // Was appended, not replaced
    }

    // --- Real file I/O tests against a per-test temp Paths ---
    //
    // Each test gets its own tempfile::tempdir() via with_temp_prefs, so the
    // tests are isolated and need no shared lock.

    fn with_temp_prefs<F: FnOnce(&crate::runtime::env::Paths)>(f: F) {
        super::tests_helpers::with_temp_prefs(f);
    }

    #[test]
    fn save_and_load_group_by_roundtrip_tag() {
        with_temp_prefs(|paths| {
            let mode = crate::app::GroupBy::Tag("production".to_string());
            save_group_by(Some(paths), &mode).unwrap();
            let loaded = load_group_by(Some(paths));
            assert_eq!(loaded, crate::app::GroupBy::Tag("production".to_string()));
        });
    }

    #[test]
    fn ssh_command_roundtrip_and_empty_means_unset() {
        with_temp_prefs(|paths| {
            assert_eq!(load_ssh_command(Some(paths)), None);
            save_ssh_command(Some(paths), "kitten ssh").unwrap();
            assert_eq!(
                load_ssh_command(Some(paths)),
                Some("kitten ssh".to_string())
            );
            // A blank value reads as unset so the default `ssh` applies.
            save_ssh_command(Some(paths), "").unwrap();
            assert_eq!(load_ssh_command(Some(paths)), None);
            std::fs::write(paths.preferences(), "ssh_command=   \ntheme=purple\n").unwrap();
            assert_eq!(load_ssh_command(Some(paths)), None);
        });
    }

    #[test]
    fn ssh_config_path_roundtrip_and_blank_means_unset() {
        with_temp_prefs(|paths| {
            assert_eq!(load_ssh_config_path(Some(paths)), None);
            std::fs::write(paths.preferences(), "ssh_config=/tmp/x\n").unwrap();
            assert_eq!(
                load_ssh_config_path(Some(paths)),
                Some("/tmp/x".to_string())
            );
            // A blank value reads as unset so the next source applies.
            std::fs::write(paths.preferences(), "ssh_config=   \ntheme=purple\n").unwrap();
            assert_eq!(load_ssh_config_path(Some(paths)), None);
        });
    }

    #[test]
    fn save_and_load_group_by_roundtrip_provider() {
        with_temp_prefs(|paths| {
            save_group_by(Some(paths), &crate::app::GroupBy::Provider).unwrap();
            let loaded = load_group_by(Some(paths));
            assert_eq!(loaded, crate::app::GroupBy::Provider);
        });
    }

    #[test]
    fn save_and_load_group_by_roundtrip_none() {
        with_temp_prefs(|paths| {
            save_group_by(Some(paths), &crate::app::GroupBy::None).unwrap();
            let loaded = load_group_by(Some(paths));
            assert_eq!(loaded, crate::app::GroupBy::None);
        });
    }

    #[test]
    fn save_group_by_removes_legacy_key() {
        with_temp_prefs(|paths| {
            let path = paths.preferences();
            std::fs::write(&path, "group_by_provider=true\nsort_mode=alpha\n").unwrap();
            save_group_by(Some(paths), &crate::app::GroupBy::Provider).unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(
                content.contains("group_by=provider"),
                "new key should exist"
            );
            assert!(
                !content.contains("group_by_provider"),
                "legacy key should be removed"
            );
            assert!(content.contains("sort_mode=alpha"), "other keys preserved");
        });
    }

    #[test]
    fn load_group_by_backward_compat_real_file() {
        with_temp_prefs(|paths| {
            std::fs::write(paths.preferences(), "group_by_provider=true\n").unwrap();
            let loaded = load_group_by(Some(paths));
            assert_eq!(loaded, crate::app::GroupBy::Provider);
        });
    }

    #[test]
    fn load_group_by_empty_file_defaults_to_provider() {
        with_temp_prefs(|paths| {
            std::fs::write(paths.preferences(), "").unwrap();
            let loaded = load_group_by(Some(paths));
            assert_eq!(loaded, crate::app::GroupBy::Provider);
        });
    }

    #[test]
    fn load_group_by_missing_file_defaults_to_provider() {
        with_temp_prefs(|paths| {
            // The preferences file is never created in the temp dir, so this
            // exercises the missing-file path.
            assert!(!paths.preferences().exists());
            let loaded = load_group_by(Some(paths));
            assert_eq!(loaded, crate::app::GroupBy::Provider);
        });
    }

    #[test]
    fn save_group_by_tag_with_special_chars_roundtrip() {
        with_temp_prefs(|paths| {
            let mode = crate::app::GroupBy::Tag("us-east-1".to_string());
            save_group_by(Some(paths), &mode).unwrap();
            let loaded = load_group_by(Some(paths));
            assert_eq!(loaded, crate::app::GroupBy::Tag("us-east-1".to_string()));
        });
    }

    #[test]
    fn save_group_by_preserves_other_prefs() {
        with_temp_prefs(|paths| {
            let path = paths.preferences();
            std::fs::write(&path, "sort_mode=alpha\nview_mode=detailed\n").unwrap();
            save_group_by(
                Some(paths),
                &crate::app::GroupBy::Tag("staging".to_string()),
            )
            .unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.contains("sort_mode=alpha"), "sort_mode preserved");
            assert!(
                content.contains("view_mode=detailed"),
                "view_mode preserved"
            );
            assert!(content.contains("group_by=tag:staging"), "group_by written");
        });
    }

    #[test]
    fn remove_value_noop_when_key_not_present() {
        let content = "sort_mode=alpha\nview_mode=compact\n";
        let lines: Vec<&str> = content.lines().collect();
        let has_key = lines.iter().any(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with('#')
                && !trimmed.is_empty()
                && trimmed
                    .split_once('=')
                    .is_some_and(|(k, _)| k.trim() == "nonexistent")
        });
        assert!(!has_key);
    }

    #[test]
    fn remove_value_preserves_comments_and_empty_lines() {
        let content = "# comment\n\nsort_mode=alpha\ngroup_by_provider=true\nview_mode=compact\n";
        let key = "group_by_provider";
        let lines: Vec<String> = content
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.is_empty() {
                    return true;
                }
                trimmed.split_once('=').is_none_or(|(k, _)| k.trim() != key)
            })
            .map(|l| l.to_string())
            .collect();
        let result = lines.join("\n") + "\n";
        assert!(result.contains("# comment"));
        assert!(result.contains("sort_mode=alpha"));
        assert!(result.contains("view_mode=compact"));
        assert!(!result.contains("group_by_provider"));
    }

    #[test]
    fn remove_value_handles_key_as_only_line() {
        let content = "group_by_provider=true\n";
        let key = "group_by_provider";
        let lines: Vec<String> = content
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.is_empty() {
                    return true;
                }
                trimmed.split_once('=').is_none_or(|(k, _)| k.trim() != key)
            })
            .map(|l| l.to_string())
            .collect();
        let result = lines.join("\n") + "\n";
        assert!(!result.contains("group_by_provider"));
    }

    #[test]
    fn remove_value_real_file_io() {
        with_temp_prefs(|paths| {
            let path = paths.preferences();
            std::fs::write(
                &path,
                "sort_mode=alpha\ngroup_by_provider=true\nview_mode=compact\n",
            )
            .unwrap();
            // save_group_by calls remove_value("group_by_provider") internally
            save_group_by(Some(paths), &crate::app::GroupBy::Provider).unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(!content.contains("group_by_provider"));
            assert!(content.contains("sort_mode=alpha"));
            assert!(content.contains("view_mode=compact"));
        });
    }

    #[test]
    fn remove_value_noop_real_file_io() {
        with_temp_prefs(|paths| {
            let path = paths.preferences();
            std::fs::write(&path, "sort_mode=alpha\n").unwrap();
            let before = std::fs::read_to_string(&path).unwrap();
            // save_group_by calls remove_value("group_by_provider"), which should be a no-op
            // since the key doesn't exist. We save Provider to trigger the remove path.
            save_group_by(Some(paths), &crate::app::GroupBy::Provider).unwrap();
            let after = std::fs::read_to_string(&path).unwrap();
            // The file will have group_by=provider added, but group_by_provider should
            // not have been written and removed (no-op path exercised)
            assert!(after.contains("sort_mode=alpha"));
            assert!(!before.contains("group_by_provider"));
            assert!(!after.contains("group_by_provider"));
        });
    }

    // --- View mode defaults ---

    #[test]
    fn load_view_mode_defaults_to_detailed() {
        with_temp_prefs(|paths| {
            // No preferences file content written.
            // load_view_mode reads "view_mode" key; missing -> Detailed
            let mode = load_view_mode(Some(paths));
            assert_eq!(mode, ViewMode::Detailed);
        });
    }

    #[test]
    fn load_view_mode_explicit_compact() {
        with_temp_prefs(|paths| {
            std::fs::write(paths.preferences(), "view_mode=compact\n").unwrap();
            let mode = load_view_mode(Some(paths));
            assert_eq!(mode, ViewMode::Compact);
        });
    }

    // --- Containers sort mode (separate key from host-list sort_mode) ---

    #[test]
    fn load_containers_sort_mode_defaults_to_alpha_host() {
        with_temp_prefs(|paths| {
            assert_eq!(
                load_containers_sort_mode(Some(paths)),
                ContainersSortMode::AlphaHost
            );
        });
    }

    #[test]
    fn save_load_containers_sort_mode_round_trip() {
        with_temp_prefs(|paths| {
            save_containers_sort_mode(Some(paths), ContainersSortMode::AlphaContainer).unwrap();
            assert_eq!(
                load_containers_sort_mode(Some(paths)),
                ContainersSortMode::AlphaContainer
            );
            save_containers_sort_mode(Some(paths), ContainersSortMode::AlphaHost).unwrap();
            assert_eq!(
                load_containers_sort_mode(Some(paths)),
                ContainersSortMode::AlphaHost
            );
        });
    }

    #[test]
    fn load_containers_sort_mode_unknown_value_falls_back_to_default() {
        with_temp_prefs(|paths| {
            std::fs::write(paths.preferences(), "containers_sort_mode=garbage\n").unwrap();
            assert_eq!(
                load_containers_sort_mode(Some(paths)),
                ContainersSortMode::AlphaHost
            );
        });
    }

    #[test]
    fn containers_sort_mode_does_not_clobber_host_sort_mode() {
        with_temp_prefs(|paths| {
            save_sort_mode(Some(paths), SortMode::AlphaAlias).unwrap();
            save_containers_sort_mode(Some(paths), ContainersSortMode::AlphaContainer).unwrap();
            let content = std::fs::read_to_string(paths.preferences()).unwrap();
            assert!(content.contains("sort_mode=alpha_alias"));
            assert!(content.contains("containers_sort_mode=alpha_container"));
            assert_eq!(load_sort_mode(Some(paths)), SortMode::AlphaAlias);
            assert_eq!(
                load_containers_sort_mode(Some(paths)),
                ContainersSortMode::AlphaContainer
            );
        });
    }

    // --- Containers view mode (separate key from host-list view_mode) ---

    #[test]
    fn load_containers_view_mode_defaults_to_detailed() {
        with_temp_prefs(|paths| {
            assert_eq!(load_containers_view_mode(Some(paths)), ViewMode::Detailed);
        });
    }

    #[test]
    fn save_load_containers_view_mode_round_trip() {
        with_temp_prefs(|paths| {
            save_containers_view_mode(Some(paths), ViewMode::Compact).unwrap();
            assert_eq!(load_containers_view_mode(Some(paths)), ViewMode::Compact);
            save_containers_view_mode(Some(paths), ViewMode::Detailed).unwrap();
            assert_eq!(load_containers_view_mode(Some(paths)), ViewMode::Detailed);
        });
    }

    #[test]
    fn save_containers_collapsed_hosts_writes_sorted_csv() {
        with_temp_prefs(|paths| {
            let mut set = std::collections::HashSet::new();
            set.insert("zeus".to_string());
            set.insert("apollo".to_string());
            set.insert("hera".to_string());
            save_containers_collapsed_hosts(Some(paths), &set).unwrap();
            let content = std::fs::read_to_string(paths.preferences()).unwrap();
            // Sorted output keeps the prefs file diff-friendly across runs.
            assert!(content.contains("containers_collapsed_hosts=apollo,hera,zeus"));
        });
    }

    #[test]
    fn save_containers_collapsed_hosts_empty_clears_key() {
        with_temp_prefs(|paths| {
            let path = paths.preferences();
            std::fs::write(&path, "containers_collapsed_hosts=alpha\n").unwrap();
            save_containers_collapsed_hosts(Some(paths), &std::collections::HashSet::new())
                .unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(
                !content.contains("containers_collapsed_hosts"),
                "empty set must remove the key entirely"
            );
        });
    }

    #[test]
    fn load_containers_collapsed_hosts_round_trip() {
        with_temp_prefs(|paths| {
            let mut set = std::collections::HashSet::new();
            set.insert("alpha".to_string());
            set.insert("bravo".to_string());
            save_containers_collapsed_hosts(Some(paths), &set).unwrap();
            let loaded = load_containers_collapsed_hosts(Some(paths));
            assert_eq!(loaded, set);
        });
    }

    // --- slow_threshold_ms ---

    #[test]
    fn load_slow_threshold_default() {
        let content = "sort_mode=alpha\n";
        let val = parse_value(content, "slow_threshold_ms");
        let threshold: u16 = val.and_then(|v| v.parse().ok()).unwrap_or(200);
        assert_eq!(threshold, 200);
    }

    #[test]
    fn load_slow_threshold_custom() {
        let content = "slow_threshold_ms=500\n";
        let val = parse_value(content, "slow_threshold_ms");
        let threshold: u16 = val.and_then(|v| v.parse().ok()).unwrap_or(200);
        assert_eq!(threshold, 500);
    }

    #[test]
    fn load_auto_ping_default_true() {
        let content = "sort_mode=alpha\n";
        let val = parse_value(content, "auto_ping");
        let auto_ping = val.map(|v| v != "false").unwrap_or(true);
        assert!(auto_ping);
    }

    #[test]
    fn load_auto_ping_explicit_true() {
        let content = "auto_ping=true\n";
        let val = parse_value(content, "auto_ping");
        let auto_ping = val.map(|v| v != "false").unwrap_or(true);
        assert!(auto_ping);
    }

    #[test]
    fn save_and_load_slow_threshold_roundtrip() {
        with_temp_prefs(|paths| {
            save_slow_threshold(Some(paths), 500).unwrap();
            let loaded = load_slow_threshold(Some(paths));
            assert_eq!(loaded, 500);
        });
    }

    #[test]
    fn auto_ping_roundtrip_true() {
        // Verify the auto_ping parse logic with the inline parse_value helper.
        // Pure parsing, no disk I/O needed for this assertion.
        let content = "auto_ping=true\n";
        let val = parse_value(content, "auto_ping");
        assert_eq!(val.as_deref(), Some("true"));
        // Confirm load_auto_ping's parsing logic: anything != "false" → true
        assert!(val.map(|v| v != "false").unwrap_or(true));
    }

    #[test]
    fn auto_ping_roundtrip_false() {
        let content = "auto_ping=false\n";
        let val = parse_value(content, "auto_ping");
        assert_eq!(val.as_deref(), Some("false"));
        // Confirm load_auto_ping's parsing logic: "false" → false
        assert!(!val.map(|v| v != "false").unwrap_or(true));
    }

    #[test]
    fn load_slow_threshold_invalid_defaults() {
        let content = "slow_threshold_ms=abc\n";
        let val = parse_value(content, "slow_threshold_ms");
        let threshold: u16 = val.and_then(|v| v.parse().ok()).unwrap_or(200);
        assert_eq!(threshold, 200);
    }

    #[test]
    fn save_and_load_theme_roundtrip() {
        with_temp_prefs(|paths| {
            save_theme(Some(paths), "catppuccin-mocha").unwrap();
            let loaded = load_theme(Some(paths));
            assert_eq!(loaded, Some("catppuccin-mocha".to_string()));
        });
    }

    #[test]
    fn load_theme_missing_returns_none() {
        with_temp_prefs(|paths| {
            std::fs::write(paths.preferences(), "sort_mode=alpha\n").unwrap();
            let loaded = load_theme(Some(paths));
            assert_eq!(loaded, None);
        });
    }

    #[test]
    fn load_auto_ping_explicit_false() {
        let content = "auto_ping=false\n";
        let val = parse_value(content, "auto_ping");
        let auto_ping = val.map(|v| v != "false").unwrap_or(true);
        assert!(!auto_ping);
    }

    #[test]
    fn last_seen_version_round_trip() {
        with_temp_prefs(|paths| {
            save_last_seen_version(Some(paths), "2.41.0").unwrap();
            let loaded = load_last_seen_version(Some(paths)).unwrap();
            assert_eq!(loaded.as_deref(), Some("2.41.0"));
        });
    }

    #[test]
    fn last_seen_version_returns_none_when_unset() {
        with_temp_prefs(|paths| {
            let loaded = load_last_seen_version(Some(paths)).unwrap();
            assert_eq!(loaded, None);
        });
    }

    #[test]
    fn recovered_lock_survives_poison() {
        let lock: std::sync::Arc<std::sync::Mutex<Option<PathBuf>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let poisoner = lock.clone();
        let joined = std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("intentional poison for test");
        })
        .join();
        assert!(joined.is_err(), "poisoning thread must have panicked");
        assert!(lock.is_poisoned(), "mutex must be poisoned after panic");

        // The poison-recovery idiom used wherever a shared Mutex guards cross-test state.
        let recovered = lock.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            recovered.is_none(),
            "recovered lock must expose inner value"
        );
    }
}
