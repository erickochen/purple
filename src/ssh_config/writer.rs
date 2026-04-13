use std::fs;
use std::time::SystemTime;

use anyhow::{Context, Result};
use log::{debug, error};

use super::model::{ConfigElement, SshConfigFile};
use crate::fs_util;

impl SshConfigFile {
    /// Write the config back to disk.
    /// Creates a backup before writing and uses atomic write (temp file + rename).
    /// Resolves symlinks so the rename targets the real file, not the link.
    /// Acquires an advisory lock to prevent concurrent writes from multiple
    /// purple processes or background sync threads.
    pub fn write(&self) -> Result<()> {
        if crate::demo_flag::is_demo() {
            return Ok(());
        }
        // Resolve symlinks so we write through to the real file
        let target_path = fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone());

        // Acquire advisory lock (blocks until available)
        let _lock =
            fs_util::FileLock::acquire(&target_path).context("Failed to acquire config lock")?;

        // Create backup if the file exists, keep only last 5
        if self.path.exists() {
            self.create_backup()
                .context("Failed to create backup of SSH config")?;
            self.prune_backups(5).ok();
        }

        let content = self.serialize();

        fs_util::atomic_write(&target_path, content.as_bytes())
            .map_err(|err| {
                error!(
                    "[purple] SSH config write failed: {}: {err}",
                    target_path.display()
                );
                err
            })
            .with_context(|| format!("Failed to write SSH config to {}", target_path.display()))?;

        // Lock released on drop
        Ok(())
    }

    /// Serialize the config to a string.
    /// Collapses consecutive blank lines to prevent accumulation after deletions.
    pub fn serialize(&self) -> String {
        let mut lines = Vec::new();

        for element in &self.elements {
            match element {
                ConfigElement::GlobalLine(line) => {
                    lines.push(line.clone());
                }
                ConfigElement::HostBlock(block) => {
                    lines.push(block.raw_host_line.clone());
                    for directive in &block.directives {
                        lines.push(directive.raw_line.clone());
                    }
                }
                ConfigElement::Include(include) => {
                    lines.push(include.raw_line.clone());
                }
            }
        }

        // Collapse consecutive blank lines (keep at most one)
        let mut collapsed = Vec::with_capacity(lines.len());
        let mut prev_blank = false;
        for line in lines {
            let is_blank = line.trim().is_empty();
            if is_blank && prev_blank {
                continue;
            }
            prev_blank = is_blank;
            collapsed.push(line);
        }

        let line_ending = if self.crlf { "\r\n" } else { "\n" };
        let mut result = String::new();
        // Restore UTF-8 BOM if the original file had one
        if self.bom {
            result.push('\u{FEFF}');
        }
        for line in &collapsed {
            result.push_str(line);
            result.push_str(line_ending);
        }
        // Ensure files always end with exactly one newline
        // (check collapsed instead of result, since BOM makes result non-empty)
        if collapsed.is_empty() {
            result.push_str(line_ending);
        }
        result
    }

    /// Create a timestamped backup of the current config file.
    /// Backup files are created with chmod 600 to match the source file's sensitivity.
    fn create_backup(&self) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let backup_name = format!(
            "{}.bak.{}",
            self.path.file_name().unwrap_or_default().to_string_lossy(),
            timestamp
        );
        let backup_path = self.path.with_file_name(backup_name);
        fs::copy(&self.path, &backup_path).with_context(|| {
            format!(
                "Failed to copy {} to {}",
                self.path.display(),
                backup_path.display()
            )
        })?;

        // Set backup permissions to 600 (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = fs::set_permissions(&backup_path, fs::Permissions::from_mode(0o600)) {
                debug!(
                    "[config] Failed to set backup permissions on {}: {e}",
                    backup_path.display()
                );
            }
        }

        Ok(())
    }

    /// Remove old backups, keeping only the most recent `keep` files.
    fn prune_backups(&self, keep: usize) -> Result<()> {
        let parent = self.path.parent().context("No parent directory")?;
        let prefix = format!(
            "{}.bak.",
            self.path.file_name().unwrap_or_default().to_string_lossy()
        );
        let mut backups: Vec<_> = fs::read_dir(parent)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .collect();
        backups.sort_by_key(|e| e.file_name());
        if backups.len() > keep {
            for old in &backups[..backups.len() - keep] {
                if let Err(e) = fs::remove_file(old.path()) {
                    debug!(
                        "[config] Failed to prune old backup {}: {e}",
                        old.path().display()
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_config::model::HostEntry;
    use std::path::PathBuf;

    fn parse_str(content: &str) -> SshConfigFile {
        SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: PathBuf::from("/tmp/test_config"),
            crlf: content.contains("\r\n"),
            bom: false,
        }
    }

    #[test]
    fn test_round_trip_basic() {
        let content = "\
Host myserver
  HostName 192.168.1.10
  User admin
  Port 2222
";
        let config = parse_str(content);
        assert_eq!(config.serialize(), content);
    }

    #[test]
    fn test_round_trip_with_comments() {
        let content = "\
# My SSH config
# Generated by hand

Host alpha
  HostName alpha.example.com
  # Deploy user
  User deploy

Host beta
  HostName beta.example.com
  User root
";
        let config = parse_str(content);
        assert_eq!(config.serialize(), content);
    }

    #[test]
    fn test_round_trip_with_globals_and_wildcards() {
        let content = "\
# Global settings
Host *
  ServerAliveInterval 60
  ServerAliveCountMax 3

Host production
  HostName prod.example.com
  User deployer
  IdentityFile ~/.ssh/prod_key
";
        let config = parse_str(content);
        assert_eq!(config.serialize(), content);
    }

    #[test]
    fn test_add_host_serializes() {
        let mut config = parse_str("Host existing\n  HostName 10.0.0.1\n");
        config.add_host(&HostEntry {
            alias: "newhost".to_string(),
            hostname: "10.0.0.2".to_string(),
            user: "admin".to_string(),
            port: 22,
            ..Default::default()
        });
        let output = config.serialize();
        assert!(output.contains("Host newhost"));
        assert!(output.contains("HostName 10.0.0.2"));
        assert!(output.contains("User admin"));
        // Port 22 is default, should not be written
        assert!(!output.contains("Port 22"));
    }

    #[test]
    fn test_delete_host_serializes() {
        let content = "\
Host alpha
  HostName alpha.example.com

Host beta
  HostName beta.example.com
";
        let mut config = parse_str(content);
        config.delete_host("alpha");
        let output = config.serialize();
        assert!(!output.contains("Host alpha"));
        assert!(output.contains("Host beta"));
    }

    #[test]
    fn test_update_host_serializes() {
        let content = "\
Host myserver
  HostName 10.0.0.1
  User old_user
";
        let mut config = parse_str(content);
        config.update_host(
            "myserver",
            &HostEntry {
                alias: "myserver".to_string(),
                hostname: "10.0.0.2".to_string(),
                user: "new_user".to_string(),
                port: 22,
                ..Default::default()
            },
        );
        let output = config.serialize();
        assert!(output.contains("HostName 10.0.0.2"));
        assert!(output.contains("User new_user"));
        assert!(!output.contains("old_user"));
    }

    #[test]
    fn test_update_host_preserves_unknown_directives() {
        let content = "\
Host myserver
  HostName 10.0.0.1
  User admin
  ForwardAgent yes
  LocalForward 8080 localhost:80
  Compression yes
";
        let mut config = parse_str(content);
        config.update_host(
            "myserver",
            &HostEntry {
                alias: "myserver".to_string(),
                hostname: "10.0.0.2".to_string(),
                user: "admin".to_string(),
                port: 22,
                ..Default::default()
            },
        );
        let output = config.serialize();
        assert!(output.contains("HostName 10.0.0.2"));
        assert!(output.contains("ForwardAgent yes"));
        assert!(output.contains("LocalForward 8080 localhost:80"));
        assert!(output.contains("Compression yes"));
    }
}
