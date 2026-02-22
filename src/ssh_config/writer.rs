use std::fs;
use std::time::SystemTime;

use anyhow::{Context, Result};

use super::model::{ConfigElement, SshConfigFile};

impl SshConfigFile {
    /// Write the config back to disk.
    /// Creates a backup before writing and uses atomic write (temp file + rename).
    pub fn write(&self) -> Result<()> {
        // Create backup if the file exists, keep only last 5
        if self.path.exists() {
            self.create_backup()
                .context("Failed to create backup of SSH config")?;
            self.prune_backups(5).ok();
        }

        let content = self.serialize();

        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        // Atomic write: write to temp file (created with 0o600), then rename
        let tmp_path = self.path.with_extension(format!("purple_tmp.{}", std::process::id()));

        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp_path)
                .with_context(|| format!("Failed to create temp file {}", tmp_path.display()))?;
            file.write_all(content.as_bytes())
                .with_context(|| format!("Failed to write temp file {}", tmp_path.display()))?;
        }

        #[cfg(not(unix))]
        fs::write(&tmp_path, &content)
            .with_context(|| format!("Failed to write temp file {}", tmp_path.display()))?;

        let result = fs::rename(&tmp_path, &self.path);
        if result.is_err() {
            let _ = fs::remove_file(&tmp_path);
        }
        result.with_context(|| {
            format!(
                "Failed to rename {} to {}",
                tmp_path.display(),
                self.path.display()
            )
        })?;

        Ok(())
    }

    /// Serialize the config to a string.
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

        let mut result = lines.join("\n");
        // Ensure file ends with a newline
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result
    }

    /// Create a timestamped backup of the current config file.
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
                let _ = fs::remove_file(old.path());
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
