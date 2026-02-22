use std::io;
use std::path::PathBuf;

use crate::app::SortMode;

fn path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".purple/preferences"))
}

/// Load sort mode from ~/.purple/preferences. Returns Original if missing or invalid.
pub fn load_sort_mode() -> SortMode {
    let path = match path() {
        Some(p) => p,
        None => return SortMode::Original,
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return SortMode::Original,
    };
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if key.trim() == "sort_mode" {
                return SortMode::from_key(value.trim());
            }
        }
    }
    SortMode::Original
}

/// Save sort mode to ~/.purple/preferences. Preserves unknown keys and comments.
/// Uses atomic write (tmp + rename) to prevent corruption.
pub fn save_sort_mode(mode: SortMode) -> io::Result<()> {
    let path = match path() {
        Some(p) => p,
        None => return Ok(()),
    };

    // Ensure parent dir exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Read existing content to preserve unknown keys
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    let mut found = false;

    for line in existing.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('#')
            && !trimmed.is_empty()
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == "sort_mode")
        {
            lines.push(format!("sort_mode={}", mode.to_key()));
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !found {
        lines.push(format!("sort_mode={}", mode.to_key()));
    }

    let content = lines.join("\n") + "\n";

    // Atomic write: tmp file (created with 0o600) + rename
    let tmp_path = path.with_extension(format!("tmp.{}", std::process::id()));

    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp_path)?;
        file.write_all(content.as_bytes())?;
    }

    #[cfg(not(unix))]
    std::fs::write(&tmp_path, &content)?;

    let result = std::fs::rename(&tmp_path, &path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result?;
    Ok(())
}
