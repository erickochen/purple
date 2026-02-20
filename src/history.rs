use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single history entry for a host.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub alias: String,
    pub last_connected: u64,
    pub count: u32,
}

/// Connection history tracking.
#[derive(Debug, Clone, Default)]
pub struct ConnectionHistory {
    pub entries: HashMap<String, HistoryEntry>,
    path: PathBuf,
}

impl ConnectionHistory {
    /// Load connection history from disk.
    pub fn load() -> Self {
        let path = Self::history_path();
        if !path.exists() {
            return Self {
                entries: HashMap::new(),
                path,
            };
        }
        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut entries = HashMap::new();
        for line in content.lines() {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() == 3 {
                if let (Ok(ts), Ok(count)) = (parts[1].parse::<u64>(), parts[2].parse::<u32>()) {
                    entries.insert(
                        parts[0].to_string(),
                        HistoryEntry {
                            alias: parts[0].to_string(),
                            last_connected: ts,
                            count,
                        },
                    );
                }
            }
        }
        Self { entries, path }
    }

    /// Record a connection to a host.
    pub fn record(&mut self, alias: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let entry = self
            .entries
            .entry(alias.to_string())
            .or_insert(HistoryEntry {
                alias: alias.to_string(),
                last_connected: 0,
                count: 0,
            });
        entry.last_connected = now;
        entry.count += 1;
        let _ = self.save();
    }

    /// Last connected timestamp for a host (0 if never connected).
    pub fn last_connected(&self, alias: &str) -> u64 {
        self.entries.get(alias).map_or(0, |e| e.last_connected)
    }

    /// Frecency score: count weighted by recency.
    pub fn frecency_score(&self, alias: &str) -> f64 {
        let entry = match self.entries.get(alias) {
            Some(e) => e,
            None => return 0.0,
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let age_hours = (now.saturating_sub(entry.last_connected)) as f64 / 3600.0;
        let recency = 1.0 / (1.0 + age_hours / 24.0);
        entry.count as f64 * recency
    }

    /// Format a timestamp as a human-readable "time ago" string.
    pub fn format_time_ago(timestamp: u64) -> String {
        if timestamp == 0 {
            return String::new();
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let diff = now.saturating_sub(timestamp);
        if diff < 60 {
            "just now".to_string()
        } else if diff < 3600 {
            format!("{}m ago", diff / 60)
        } else if diff < 86400 {
            format!("{}h ago", diff / 3600)
        } else if diff < 604800 {
            format!("{}d ago", diff / 86400)
        } else {
            format!("{}w ago", diff / 604800)
        }
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content: String = self
            .entries
            .values()
            .map(|e| format!("{}\t{}\t{}", e.alias, e.last_connected, e.count))
            .collect::<Vec<_>>()
            .join("\n");
        // Atomic write: tmp file + rename
        let tmp_path = self.path.with_extension(format!("tmp.{}", std::process::id()));
        fs::write(&tmp_path, &content)?;
        fs::rename(&tmp_path, &self.path)
    }

    fn history_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".purple")
            .join("history.tsv")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frecency_score_unknown_alias() {
        let history = ConnectionHistory::default();
        assert_eq!(history.frecency_score("unknown"), 0.0);
    }

    #[test]
    fn test_format_time_ago_zero() {
        assert_eq!(ConnectionHistory::format_time_ago(0), "");
    }

    #[test]
    fn test_format_time_ago_recent() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(ConnectionHistory::format_time_ago(now), "just now");
        assert_eq!(
            ConnectionHistory::format_time_ago(now - 300),
            "5m ago"
        );
        assert_eq!(
            ConnectionHistory::format_time_ago(now - 7200),
            "2h ago"
        );
        assert_eq!(
            ConnectionHistory::format_time_ago(now - 172800),
            "2d ago"
        );
    }
}
