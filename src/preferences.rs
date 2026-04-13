use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use log::debug;

use crate::app::{SortMode, ViewMode};
use crate::fs_util;

static PATH_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Override the preferences file path (used in tests to avoid writing to ~/.purple).
#[cfg(test)]
pub fn set_path_override(path: PathBuf) {
    *PATH_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
}

/// Clear the path override so `path()` falls back to the real ~/.purple/preferences.
/// This avoids a race where other test threads (e.g. App::new() calling load_auto_ping())
/// read a stale override left behind by a preferences IO test.
#[cfg(test)]
fn clear_path_override() {
    *PATH_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn path() -> Option<PathBuf> {
    if let Some(p) = PATH_OVERRIDE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    {
        return Some(p);
    }
    dirs::home_dir().map(|h| h.join(".purple/preferences"))
}

/// Load a value for a given key from ~/.purple/preferences.
fn load_value(key: &str) -> Option<String> {
    let path = path()?;
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
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Save a key=value pair to ~/.purple/preferences. Preserves unknown keys and comments.
fn save_value(key: &str, value: &str) -> io::Result<()> {
    if crate::demo_flag::is_demo() {
        return Ok(());
    }
    let path = match path() {
        Some(p) => p,
        None => return Ok(()),
    };

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
pub fn load_sort_mode() -> SortMode {
    load_value("sort_mode")
        .map(|v| SortMode::from_key(&v))
        .unwrap_or(SortMode::MostRecent)
}

/// Save sort mode to ~/.purple/preferences.
pub fn save_sort_mode(mode: SortMode) -> io::Result<()> {
    save_value("sort_mode", mode.to_key())
}

/// Load group_by from ~/.purple/preferences. New `group_by` key takes precedence
/// over the legacy `group_by_provider` key for backward compatibility.
/// Returns `GroupBy::Provider` if missing (preserving old default behavior).
pub fn load_group_by() -> crate::app::GroupBy {
    use crate::app::GroupBy;
    if let Some(v) = load_value("group_by") {
        return GroupBy::from_key(&v);
    }
    if let Some(v) = load_value("group_by_provider") {
        return if v == "true" {
            GroupBy::Provider
        } else {
            GroupBy::None
        };
    }
    GroupBy::Provider
}

/// Remove a key from ~/.purple/preferences. No-op if the key or file does not exist.
fn remove_value(key: &str) -> io::Result<()> {
    if crate::demo_flag::is_demo() {
        return Ok(());
    }
    let path = match path() {
        Some(p) => p,
        None => return Ok(()),
    };
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
pub fn save_group_by(mode: &crate::app::GroupBy) -> io::Result<()> {
    save_value("group_by", &mode.to_key())?;
    // Best-effort cleanup: group_by key takes precedence on load, so
    // a leftover group_by_provider key is harmless if removal fails.
    let _ = remove_value("group_by_provider");
    Ok(())
}

/// Load view mode from ~/.purple/preferences. Returns Detailed if missing or invalid.
pub fn load_view_mode() -> ViewMode {
    load_value("view_mode")
        .map(|v| match v.as_str() {
            "compact" => ViewMode::Compact,
            _ => ViewMode::Detailed,
        })
        .unwrap_or(ViewMode::Detailed)
}

/// Save view mode to ~/.purple/preferences.
pub fn save_view_mode(mode: ViewMode) -> io::Result<()> {
    save_value(
        "view_mode",
        match mode {
            ViewMode::Compact => "compact",
            ViewMode::Detailed => "detailed",
        },
    )
}

/// Load global askpass default from ~/.purple/preferences.
pub fn load_askpass_default() -> Option<String> {
    load_value("askpass").filter(|v| !v.is_empty())
}

/// Save global askpass default to ~/.purple/preferences.
pub fn save_askpass_default(source: &str) -> io::Result<()> {
    save_value("askpass", source)
}

/// Load slow threshold from ~/.purple/preferences. Returns 200 if missing or invalid.
pub fn load_slow_threshold() -> u16 {
    load_value("slow_threshold_ms")
        .and_then(|v| v.parse().ok())
        .unwrap_or(200)
}

/// Save slow threshold to ~/.purple/preferences.
#[allow(dead_code)]
pub fn save_slow_threshold(ms: u16) -> io::Result<()> {
    save_value("slow_threshold_ms", &ms.to_string())
}

/// Load theme name from ~/.purple/preferences. Returns None if missing.
pub fn load_theme() -> Option<String> {
    load_value("theme").filter(|v| !v.is_empty())
}

/// Save theme name to ~/.purple/preferences.
pub fn save_theme(name: &str) -> io::Result<()> {
    save_value("theme", name)
}

/// Load auto_ping preference. Returns true if missing (default: enabled).
pub fn load_auto_ping() -> bool {
    load_value("auto_ping")
        .map(|v| v != "false")
        .unwrap_or(true)
}

/// Save auto_ping preference.
#[allow(dead_code)]
pub fn save_auto_ping(enabled: bool) -> io::Result<()> {
    save_value("auto_ping", if enabled { "true" } else { "false" })
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
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim().to_string());
                }
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

    // --- Real file I/O tests using set_path_override ---
    //
    // PATH_OVERRIDE is a global Mutex<Option<PathBuf>>, so these tests must
    // not run concurrently. We use a module-level Mutex (IO_TEST_LOCK) as a
    // guard: holding it serialises access to PATH_OVERRIDE for the duration
    // of each test body.

    static IO_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static TEST_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn with_temp_prefs<F: FnOnce(&std::path::Path)>(label: &str, f: F) {
        let _guard = IO_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let id = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "purple_prefs_{}_{}_{id}",
            label,
            std::process::id(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("preferences");
        set_path_override(path.clone());
        f(&path);
        std::fs::remove_dir_all(&dir).ok();
        // Clear override so other test threads (e.g. App::new() → load_auto_ping())
        // fall back to the real path instead of reading a stale temp file.
        clear_path_override();
    }

    #[test]
    fn save_and_load_group_by_roundtrip_tag() {
        with_temp_prefs("roundtrip_tag", |_path| {
            let mode = crate::app::GroupBy::Tag("production".to_string());
            save_group_by(&mode).unwrap();
            let loaded = load_group_by();
            assert_eq!(loaded, crate::app::GroupBy::Tag("production".to_string()));
        });
    }

    #[test]
    fn save_and_load_group_by_roundtrip_provider() {
        with_temp_prefs("roundtrip_provider", |_path| {
            save_group_by(&crate::app::GroupBy::Provider).unwrap();
            let loaded = load_group_by();
            assert_eq!(loaded, crate::app::GroupBy::Provider);
        });
    }

    #[test]
    fn save_and_load_group_by_roundtrip_none() {
        with_temp_prefs("roundtrip_none", |_path| {
            save_group_by(&crate::app::GroupBy::None).unwrap();
            let loaded = load_group_by();
            assert_eq!(loaded, crate::app::GroupBy::None);
        });
    }

    #[test]
    fn save_group_by_removes_legacy_key() {
        with_temp_prefs("legacy_key", |path| {
            std::fs::write(path, "group_by_provider=true\nsort_mode=alpha\n").unwrap();
            save_group_by(&crate::app::GroupBy::Provider).unwrap();
            let content = std::fs::read_to_string(path).unwrap();
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
        with_temp_prefs("compat_true", |path| {
            std::fs::write(path, "group_by_provider=true\n").unwrap();
            let loaded = load_group_by();
            assert_eq!(loaded, crate::app::GroupBy::Provider);
        });
    }

    #[test]
    fn load_group_by_empty_file_defaults_to_provider() {
        with_temp_prefs("empty_file", |path| {
            std::fs::write(path, "").unwrap();
            let loaded = load_group_by();
            assert_eq!(loaded, crate::app::GroupBy::Provider);
        });
    }

    #[test]
    fn load_group_by_missing_file_defaults_to_provider() {
        let _guard = IO_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path =
            std::env::temp_dir().join(format!("purple_prefs_missing_{}", std::process::id()));
        // Ensure it does not exist
        let _ = std::fs::remove_file(&path);
        set_path_override(path);
        let loaded = load_group_by();
        assert_eq!(loaded, crate::app::GroupBy::Provider);
        clear_path_override();
    }

    #[test]
    fn save_group_by_tag_with_special_chars_roundtrip() {
        with_temp_prefs("tag_special", |_path| {
            let mode = crate::app::GroupBy::Tag("us-east-1".to_string());
            save_group_by(&mode).unwrap();
            let loaded = load_group_by();
            assert_eq!(loaded, crate::app::GroupBy::Tag("us-east-1".to_string()));
        });
    }

    #[test]
    fn save_group_by_preserves_other_prefs() {
        with_temp_prefs("preserves_other", |path| {
            std::fs::write(path, "sort_mode=alpha\nview_mode=detailed\n").unwrap();
            save_group_by(&crate::app::GroupBy::Tag("staging".to_string())).unwrap();
            let content = std::fs::read_to_string(path).unwrap();
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
        with_temp_prefs("remove_real_io", |path| {
            std::fs::write(
                path,
                "sort_mode=alpha\ngroup_by_provider=true\nview_mode=compact\n",
            )
            .unwrap();
            // save_group_by calls remove_value("group_by_provider") internally
            save_group_by(&crate::app::GroupBy::Provider).unwrap();
            let content = std::fs::read_to_string(path).unwrap();
            assert!(!content.contains("group_by_provider"));
            assert!(content.contains("sort_mode=alpha"));
            assert!(content.contains("view_mode=compact"));
        });
    }

    #[test]
    fn remove_value_noop_real_file_io() {
        with_temp_prefs("remove_noop_io", |path| {
            std::fs::write(path, "sort_mode=alpha\n").unwrap();
            let before = std::fs::read_to_string(path).unwrap();
            // save_group_by calls remove_value("group_by_provider"), which should be a no-op
            // since the key doesn't exist. We save Provider to trigger the remove path.
            save_group_by(&crate::app::GroupBy::Provider).unwrap();
            let after = std::fs::read_to_string(path).unwrap();
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
        with_temp_prefs("view_mode_default", |_path| {
            // No preferences file content written, but file exists (empty)
            // load_view_mode reads "view_mode" key; missing -> Detailed
            let mode = load_view_mode();
            assert_eq!(mode, ViewMode::Detailed);
        });
    }

    #[test]
    fn load_view_mode_explicit_compact() {
        with_temp_prefs("view_mode_compact", |path| {
            std::fs::write(path, "view_mode=compact\n").unwrap();
            let mode = load_view_mode();
            assert_eq!(mode, ViewMode::Compact);
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
        with_temp_prefs("slow_threshold", |_path| {
            save_slow_threshold(500).unwrap();
            let loaded = load_slow_threshold();
            assert_eq!(loaded, 500);
        });
    }

    #[test]
    fn auto_ping_roundtrip_true() {
        // Verify save_auto_ping writes a value that load_auto_ping parses back
        // correctly. Uses the parse_value helper to avoid global PATH_OVERRIDE
        // races when other tests call App::new() → load_auto_ping() in parallel.
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
        with_temp_prefs("theme_roundtrip", |_path| {
            save_theme("catppuccin-mocha").unwrap();
            let loaded = load_theme();
            assert_eq!(loaded, Some("catppuccin-mocha".to_string()));
        });
    }

    #[test]
    fn load_theme_missing_returns_none() {
        with_temp_prefs("theme_missing", |path| {
            std::fs::write(path, "sort_mode=alpha\n").unwrap();
            let loaded = load_theme();
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

    // Verifies the poison-recovery pattern used by `path()` and `set_path_override()`.
    // Uses a local Mutex to avoid poisoning the global PATH_OVERRIDE permanently
    // (a poisoned Mutex cannot be un-poisoned, which would contaminate other tests).
    // The production code uses the same `.lock().unwrap_or_else(|e| e.into_inner())`
    // pattern, so this test proves the pattern survives a poisoned lock.
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

        // The exact pattern used by path() and set_path_override().
        let recovered = lock.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            recovered.is_none(),
            "recovered lock must expose inner value"
        );
    }
}
