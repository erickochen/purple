use std::path::Path;
use std::process::Command;

use crate::ssh_config::model::HostEntry;

/// Information about an SSH key found on disk.
#[derive(Debug, Clone)]
pub struct SshKeyInfo {
    /// Display name (filename without path, e.g. "id_ed25519")
    pub name: String,
    /// Display path with tilde (e.g. "~/.ssh/id_ed25519")
    pub display_path: String,
    /// Key type (e.g. "ED25519", "RSA")
    pub key_type: String,
    /// Key bits (e.g. "256", "4096")
    pub bits: String,
    /// SHA256 fingerprint
    pub fingerprint: String,
    /// Comment from the public key
    pub comment: String,
    /// Host aliases that reference this key via IdentityFile
    pub linked_hosts: Vec<String>,
}

impl SshKeyInfo {
    /// Format type with bits (e.g. "ED25519" or "RSA 4096").
    pub fn type_display(&self) -> String {
        if self.bits.is_empty() {
            self.key_type.clone()
        } else {
            format!("{} {}", self.key_type, self.bits)
        }
    }
}

/// Discover SSH keys in the given directory and cross-reference with host entries.
pub fn discover_keys(ssh_dir: &Path, hosts: &[HostEntry]) -> Vec<SshKeyInfo> {
    let entries = match std::fs::read_dir(ssh_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let home = dirs::home_dir();

    let mut keys: Vec<SshKeyInfo> = entries
        .filter_map(|e| e.ok())
        .filter(is_public_key_file)
        .filter_map(|e| read_key_info(ssh_dir, &e.path(), home.as_deref(), hosts))
        .collect();

    keys.sort_by(|a, b| a.name.cmp(&b.name));
    keys
}

/// Check if a directory entry looks like a public key file.
fn is_public_key_file(entry: &std::fs::DirEntry) -> bool {
    let name = entry.file_name();
    let name = name.to_string_lossy();

    // Must end in .pub
    if !name.ends_with(".pub") {
        return false;
    }

    // Skip known non-key files
    let skip = ["authorized_keys.pub", "known_hosts.pub"];
    if skip.contains(&name.as_ref()) {
        return false;
    }

    // Must be a regular file
    entry.file_type().map(|t| t.is_file()).unwrap_or(false)
}

/// Read key metadata using ssh-keygen -lf.
fn read_key_info(
    ssh_dir: &Path,
    pub_path: &Path,
    home: Option<&Path>,
    hosts: &[HostEntry],
) -> Option<SshKeyInfo> {
    let output = Command::new("ssh-keygen")
        .args(["-lf", &pub_path.to_string_lossy(), "-E", "sha256"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let line = String::from_utf8_lossy(&output.stdout);
    let line = line.trim();

    // Format: "<bits> <fingerprint> <comment> (<type>)"
    let (bits, fingerprint, comment, key_type) = parse_keygen_output(line)?;

    // Derive the private key name (strip .pub)
    let pub_name = pub_path.file_name()?.to_string_lossy();
    let name = pub_name.strip_suffix(".pub").unwrap_or(&pub_name).to_string();

    // Private key path (without .pub extension)
    let private_path = ssh_dir.join(&name);

    // Display path: use ~ if ssh_dir is under home
    let display_path = match home {
        Some(home) if ssh_dir.starts_with(home) => {
            let relative = ssh_dir.strip_prefix(home).unwrap();
            format!("~/{}/{}", relative.display(), name)
        }
        _ => private_path.display().to_string(),
    };

    // Find hosts that reference this key
    let linked_hosts = find_linked_hosts(&private_path, &display_path, hosts);

    Some(SshKeyInfo {
        name,
        display_path,
        key_type,
        bits,
        fingerprint,
        comment,
        linked_hosts,
    })
}

/// Parse ssh-keygen -lf output line into (bits, fingerprint, comment, type).
fn parse_keygen_output(line: &str) -> Option<(String, String, String, String)> {
    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    if parts.len() < 3 {
        return None;
    }

    let bits = parts[0].to_string();
    let fingerprint = parts[1].to_string();

    // The rest is "<comment> (<type>)" — extract type from the end
    let rest = parts[2];
    let (comment, key_type) = if let Some(paren_start) = rest.rfind('(') {
        let comment = rest[..paren_start].trim().to_string();
        let key_type = rest[paren_start + 1..]
            .trim_end_matches(')')
            .to_string();
        (comment, key_type)
    } else {
        (rest.to_string(), String::new())
    };

    Some((bits, fingerprint, comment, key_type))
}

/// Find host aliases that reference a given key path via IdentityFile.
fn find_linked_hosts(full_path: &Path, display_path: &str, hosts: &[HostEntry]) -> Vec<String> {
    hosts
        .iter()
        .filter(|h| {
            if h.identity_file.is_empty() {
                return false;
            }
            // Match against both the display path (~/.ssh/...) and the full path
            h.identity_file == display_path
                || Path::new(&h.identity_file) == full_path
        })
        .map(|h| h.alias.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_keygen_output_ed25519() {
        let line = "256 SHA256:abcdef1234567890 user@host (ED25519)";
        let (bits, fp, comment, key_type) = parse_keygen_output(line).unwrap();
        assert_eq!(bits, "256");
        assert_eq!(fp, "SHA256:abcdef1234567890");
        assert_eq!(comment, "user@host");
        assert_eq!(key_type, "ED25519");
    }

    #[test]
    fn test_parse_keygen_output_rsa() {
        let line = "4096 SHA256:xyz9876543210 deploy@prod.example.com (RSA)";
        let (bits, fp, comment, key_type) = parse_keygen_output(line).unwrap();
        assert_eq!(bits, "4096");
        assert_eq!(fp, "SHA256:xyz9876543210");
        assert_eq!(comment, "deploy@prod.example.com");
        assert_eq!(key_type, "RSA");
    }

    #[test]
    fn test_parse_keygen_output_no_comment() {
        let line = "256 SHA256:fingerprint (ED25519)";
        let (bits, fp, comment, key_type) = parse_keygen_output(line).unwrap();
        assert_eq!(bits, "256");
        assert_eq!(fp, "SHA256:fingerprint");
        assert_eq!(comment, "");
        assert_eq!(key_type, "ED25519");
    }

    #[test]
    fn test_parse_keygen_output_comment_with_spaces() {
        let line = "256 SHA256:fingerprint eko@MacBook Pro (ED25519)";
        let (bits, fp, comment, key_type) = parse_keygen_output(line).unwrap();
        assert_eq!(bits, "256");
        assert_eq!(fp, "SHA256:fingerprint");
        assert_eq!(comment, "eko@MacBook Pro");
        assert_eq!(key_type, "ED25519");
    }

    #[test]
    fn test_parse_keygen_output_no_type_parens() {
        let line = "256 SHA256:fingerprint user@host";
        let (bits, fp, comment, key_type) = parse_keygen_output(line).unwrap();
        assert_eq!(bits, "256");
        assert_eq!(fp, "SHA256:fingerprint");
        assert_eq!(comment, "user@host");
        assert_eq!(key_type, "");
    }

    #[test]
    fn test_parse_keygen_output_too_short() {
        assert!(parse_keygen_output("256 SHA256:fp").is_none());
        assert!(parse_keygen_output("").is_none());
    }

    #[test]
    fn test_find_linked_hosts_display_path() {
        let hosts = vec![
            HostEntry {
                alias: "prod".to_string(),
                identity_file: "~/.ssh/id_ed25519".to_string(),
                ..Default::default()
            },
            HostEntry {
                alias: "staging".to_string(),
                identity_file: "~/.ssh/other_key".to_string(),
                ..Default::default()
            },
        ];
        let linked = find_linked_hosts(
            Path::new("/home/user/.ssh/id_ed25519"),
            "~/.ssh/id_ed25519",
            &hosts,
        );
        assert_eq!(linked, vec!["prod"]);
    }

    #[test]
    fn test_find_linked_hosts_full_path() {
        let hosts = vec![HostEntry {
            alias: "server".to_string(),
            identity_file: "/home/user/.ssh/deploy_key".to_string(),
            ..Default::default()
        }];
        let linked = find_linked_hosts(
            Path::new("/home/user/.ssh/deploy_key"),
            "~/.ssh/deploy_key",
            &hosts,
        );
        assert_eq!(linked, vec!["server"]);
    }

    #[test]
    fn test_find_linked_hosts_empty() {
        let hosts = vec![HostEntry {
            alias: "server".to_string(),
            identity_file: String::new(),
            ..Default::default()
        }];
        let linked = find_linked_hosts(
            Path::new("/home/user/.ssh/id_rsa"),
            "~/.ssh/id_rsa",
            &hosts,
        );
        assert!(linked.is_empty());
    }

    #[test]
    fn test_type_display() {
        let key = SshKeyInfo {
            name: "id_ed25519".to_string(),
            display_path: "~/.ssh/id_ed25519".to_string(),
            key_type: "ED25519".to_string(),
            bits: "256".to_string(),
            fingerprint: String::new(),
            comment: String::new(),
            linked_hosts: Vec::new(),
        };
        assert_eq!(key.type_display(), "ED25519 256");

        let key2 = SshKeyInfo {
            bits: String::new(),
            ..key
        };
        assert_eq!(key2.type_display(), "ED25519");
    }
}
