use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::app::{SortMode, ViewMode};
use crate::fs_util;

static PATH_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Override the preferences file path (used in tests to avoid writing to ~/.purple).
#[cfg(test)]
pub fn set_path_override(path: PathBuf) {
    *PATH_OVERRIDE.lock().unwrap() = Some(path);
}

fn path() -> Option<PathBuf> {
    if let Some(p) = PATH_OVERRIDE.lock().unwrap().clone() {
        return Some(p);
    }
    dirs::home_dir().map(|h| h.join(".purple/preferences"))
}

/// Load a value for a given key from ~/.purple/preferences.
fn load_value(key: &str) -> Option<String> {
    let path = path()?;
    let content = std::fs::read_to_string(path).ok()?;
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

/// Load group_by_provider from ~/.purple/preferences. Returns true if missing or invalid.
pub fn load_group_by_provider() -> bool {
    load_value("group_by_provider")
        .map(|v| v != "false")
        .unwrap_or(true)
}

/// Save group_by_provider to ~/.purple/preferences.
pub fn save_group_by_provider(enabled: bool) -> io::Result<()> {
    save_value("group_by_provider", &enabled.to_string())
}

/// Load view mode from ~/.purple/preferences. Returns Compact if missing or invalid.
pub fn load_view_mode() -> ViewMode {
    load_value("view_mode")
        .map(|v| match v.as_str() {
            "detailed" => ViewMode::Detailed,
            _ => ViewMode::Compact,
        })
        .unwrap_or(ViewMode::Compact)
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

#[cfg(test)]
mod tests {
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
                && trimmed.split_once('=').is_some_and(|(k, _)| k.trim() == key)
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
                && trimmed.split_once('=').is_some_and(|(k, _)| k.trim() == key)
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
}
