use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result};
use log::{debug, error, warn};

use crate::ssh_config::model::SshConfigFile;

/// A password source option for the picker overlay.
pub struct PasswordSourceOption {
    pub label: &'static str,
    pub value: &'static str,
    pub hint: &'static str,
}

pub const PASSWORD_SOURCES: &[PasswordSourceOption] = &[
    PasswordSourceOption {
        label: "OS Keychain",
        value: "keychain",
        hint: "keychain",
    },
    PasswordSourceOption {
        label: "1Password",
        value: "op://",
        hint: "op://Vault/Item/field",
    },
    PasswordSourceOption {
        label: "Bitwarden",
        value: "bw:",
        hint: "bw:item-name",
    },
    PasswordSourceOption {
        label: "pass",
        value: "pass:",
        hint: "pass:path/to/entry",
    },
    // Vault KV secrets engine (key/value store). Distinct from the Vault SSH
    // secrets engine used for signed SSH certificates, which has its own
    // "Vault SSH role" field on the host form.
    PasswordSourceOption {
        label: "HashiCorp Vault KV",
        value: "vault:",
        hint: "vault:secret/path#field",
    },
    PasswordSourceOption {
        label: "Custom command",
        value: "cmd:",
        hint: "cmd %a %h",
    },
    PasswordSourceOption {
        label: "None",
        value: "",
        hint: "(remove)",
    },
];

/// Handle an SSH_ASKPASS invocation. Called when purple is invoked as an askpass program.
/// Reads the password source from the host's `# purple:askpass` comment and retrieves it.
pub fn handle() -> Result<()> {
    // Initialize file-only logging for askpass subprocess
    // verbose is determined by PURPLE_LOG env var only (no CLI flag in subprocess)
    crate::logging::init(false, false);

    let alias = std::env::var("PURPLE_HOST_ALIAS").unwrap_or_default();
    let config_path = std::env::var("PURPLE_CONFIG_PATH").unwrap_or_default();

    // Check the prompt (argv[1]) to skip passphrase and host key verification prompts
    let prompt = std::env::args().nth(1).unwrap_or_default();
    let prompt_lower = prompt.to_ascii_lowercase();
    if prompt_lower.contains("passphrase")
        || prompt_lower.contains("yes/no")
        || prompt_lower.contains("(yes/no/")
    {
        // Not a password prompt. Exit with error so SSH falls back to interactive.
        std::process::exit(1);
    }

    if alias.is_empty() || config_path.is_empty() {
        std::process::exit(1);
    }

    // Retry detection: if we've been called recently for this alias, the password was wrong.
    // Exit with error so SSH falls back to interactive prompt.
    let marker = marker_path(&alias);
    if let Some(marker_path) = &marker {
        if is_recent_marker(marker_path) {
            debug!("Askpass retry detected for {alias}");
            // Clean up and bail
            let _ = std::fs::remove_file(marker_path);
            std::process::exit(1);
        }
        // Create marker for retry detection
        if let Err(e) = std::fs::create_dir_all(marker_path.parent().unwrap()) {
            debug!("[config] Failed to create askpass marker directory: {e}");
        }
        if let Err(e) = std::fs::write(marker_path, b"") {
            debug!("[config] Failed to write askpass marker: {e}");
        }
    }

    // Parse config and find askpass source
    let config =
        SshConfigFile::parse(&PathBuf::from(&config_path)).context("Failed to parse SSH config")?;

    let source = find_askpass_source(&config, &alias);

    let source = match source {
        Some(s) => s,
        None => std::process::exit(1),
    };

    debug!("Askpass invoked for alias={alias} source={source}");

    // Retrieve password
    let hostname = find_hostname(&config, &alias);
    match retrieve_password(&source, &alias, &hostname) {
        Ok(password) => {
            debug!("Askpass retrieved password for {alias} via {source}");
            print!("{}", password);
            Ok(())
        }
        Err(err) => {
            warn!("[external] Password retrieval failed via {source}");
            debug!("[external] Password retrieval detail: {err}");
            // Clean up marker on failure
            if let Some(m) = &marker {
                let _ = std::fs::remove_file(m);
            }
            std::process::exit(1);
        }
    }
}

/// Find the askpass source for a host. Checks per-host config, then global default.
fn find_askpass_source(config: &SshConfigFile, alias: &str) -> Option<String> {
    // Per-host source
    for entry in config.host_entries() {
        if entry.alias == alias {
            if let Some(ref source) = entry.askpass {
                return Some(source.clone());
            }
        }
    }
    // Global default from preferences file
    load_askpass_default_direct()
}

/// Read askpass default directly from ~/.purple/preferences without depending on the
/// preferences module (which requires crate::app and isn't available in askpass subprocess).
fn load_askpass_default_direct() -> Option<String> {
    let path = dirs::home_dir()?.join(".purple/preferences");
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == "askpass" {
                let val = v.trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

/// Find the hostname for an alias (for %h substitution).
fn find_hostname(config: &SshConfigFile, alias: &str) -> String {
    for entry in config.host_entries() {
        if entry.alias == alias {
            return entry.hostname.clone();
        }
    }
    alias.to_string()
}

/// Retrieve a password from the given source.
fn retrieve_password(source: &str, alias: &str, hostname: &str) -> Result<String> {
    if source == "keychain" {
        return retrieve_from_keychain(alias);
    }
    if let Some(uri) = source.strip_prefix("op://") {
        return retrieve_from_1password(&format!("op://{}", uri));
    }
    if let Some(entry) = source.strip_prefix("pass:") {
        return retrieve_from_pass(entry);
    }
    if let Some(item_id) = source.strip_prefix("bw:") {
        return retrieve_from_bitwarden(item_id);
    }
    if let Some(rest) = source.strip_prefix("vault:") {
        return retrieve_from_vault(rest);
    }
    // Custom command (with or without cmd: prefix)
    let cmd = source.strip_prefix("cmd:").unwrap_or(source);
    retrieve_from_command(cmd, alias, hostname)
}

/// Retrieve from OS keychain (macOS: Keychain, Linux: secret-tool).
fn retrieve_from_keychain(alias: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("security")
            .args([
                "find-generic-password",
                "-a",
                alias,
                "-s",
                "purple-ssh",
                "-w",
            ])
            .output()
            .context("Failed to run security command")?;
        if !output.status.success() {
            anyhow::bail!("Keychain lookup failed");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let output = Command::new("secret-tool")
            .args(["lookup", "application", "purple-ssh", "host", alias])
            .output()
            .context("Failed to run secret-tool")?;
        if !output.status.success() {
            anyhow::bail!("Secret-tool lookup failed");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// Check if a password exists in the OS keychain for this alias.
pub fn keychain_has_password(alias: &str) -> bool {
    retrieve_from_keychain(alias).is_ok()
}

/// Retrieve a password from the OS keychain. Public for keychain migration on alias rename.
pub fn retrieve_keychain_password(alias: &str) -> Result<String> {
    retrieve_from_keychain(alias)
}

/// Store a password in the OS keychain.
pub fn store_in_keychain(alias: &str, password: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("security")
            .args([
                "add-generic-password",
                "-U",
                "-a",
                alias,
                "-s",
                "purple-ssh",
                "-w",
                password,
            ])
            .status()
            .context("Failed to run security command")?;
        if !status.success() {
            anyhow::bail!("Failed to store password in Keychain");
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut child = Command::new("secret-tool")
            .args([
                "store",
                "--label",
                &format!("purple-ssh: {}", alias),
                "application",
                "purple-ssh",
                "host",
                alias,
            ])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to run secret-tool")?;
        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            stdin.write_all(password.as_bytes())?;
        }
        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!("Failed to store password with secret-tool");
        }
        Ok(())
    }
}

/// Remove a password from the OS keychain.
pub fn remove_from_keychain(alias: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("security")
            .args(["delete-generic-password", "-a", alias, "-s", "purple-ssh"])
            .status()
            .context("Failed to run security command")?;
        if !status.success() {
            anyhow::bail!("No password found for '{}' in Keychain", alias);
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let status = Command::new("secret-tool")
            .args(["clear", "application", "purple-ssh", "host", alias])
            .status()
            .context("Failed to run secret-tool")?;
        if !status.success() {
            anyhow::bail!("Failed to remove password with secret-tool");
        }
        Ok(())
    }
}

/// Retrieve from 1Password CLI.
fn retrieve_from_1password(uri: &str) -> Result<String> {
    let result = Command::new("op")
        .args(["read", uri, "--no-newline"])
        .output();
    let output = match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            error!("[config] Password manager binary not found: op");
            return Err(e).context("Failed to run 1Password CLI (op)");
        }
        other => other.context("Failed to run 1Password CLI (op)")?,
    };
    if !output.status.success() {
        anyhow::bail!("1Password lookup failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Retrieve from pass (password-store). Returns the first line.
fn retrieve_from_pass(entry: &str) -> Result<String> {
    let result = Command::new("pass").args(["show", entry]).output();
    let output = match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            error!("[config] Password manager binary not found: pass");
            return Err(e).context("Failed to run pass");
        }
        other => other.context("Failed to run pass")?,
    };
    if !output.status.success() {
        anyhow::bail!("pass lookup failed");
    }
    let full = String::from_utf8_lossy(&output.stdout);
    Ok(full.lines().next().unwrap_or("").to_string())
}

/// Retrieve from Bitwarden CLI. The item_id can be an item ID or search term.
/// Uses `bw get password <item_id>` which requires an unlocked vault (BW_SESSION).
fn retrieve_from_bitwarden(item_id: &str) -> Result<String> {
    let result = Command::new("bw")
        .args(["get", "password", item_id])
        .output();
    let output = match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            error!("[config] Password manager binary not found: bw");
            return Err(e).context("Failed to run Bitwarden CLI (bw)");
        }
        other => other.context("Failed to run Bitwarden CLI (bw)")?,
    };
    if !output.status.success() {
        anyhow::bail!("Bitwarden lookup failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Retrieve from the HashiCorp Vault KV secrets engine via the `vault` CLI.
/// Spec format: `path#field` or just `path` (defaults to `password`).
/// Distinct from the Vault SSH secrets engine (see src/vault_ssh.rs), which
/// signs SSH certificates rather than storing passwords.
fn retrieve_from_vault(spec: &str) -> Result<String> {
    let (path, field) = match spec.rsplit_once('#') {
        Some((p, f)) => (p, f),
        None => (spec, "password"),
    };
    let result = Command::new("vault")
        .args(["kv", "get", &format!("-field={}", field), path])
        .output();
    let output = match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            error!("[config] Password manager binary not found: vault");
            return Err(e).context("Failed to run vault CLI");
        }
        other => other.context("Failed to run vault CLI")?,
    };
    if !output.status.success() {
        anyhow::bail!("Vault lookup failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Retrieve via custom command. Supports %h (hostname) and %a (alias) substitution.
/// Values are shell-escaped to prevent metacharacter injection.
fn retrieve_from_command(cmd: &str, alias: &str, hostname: &str) -> Result<String> {
    let safe_alias = crate::snippet::shell_escape(alias);
    let safe_hostname = crate::snippet::shell_escape(hostname);
    let expanded = cmd.replace("%a", &safe_alias).replace("%h", &safe_hostname);
    let output = Command::new("sh")
        .args(["-c", &expanded])
        .output()
        .context("Failed to run custom askpass command")?;
    if !output.status.success() {
        anyhow::bail!("Custom askpass command failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the path for the retry marker file.
/// Sanitizes the alias to prevent path traversal (replaces `/` and `\` with `_`).
fn marker_path(alias: &str) -> Option<PathBuf> {
    let safe = alias.replace(['/', '\\', '.'], "_");
    dirs::home_dir().map(|h| h.join(format!(".purple/.askpass_{}", safe)))
}

/// Check if a marker file exists and is recent (< 60 seconds old).
fn is_recent_marker(path: &PathBuf) -> bool {
    if let Ok(meta) = std::fs::metadata(path) {
        if let Ok(modified) = meta.modified() {
            if let Ok(elapsed) = SystemTime::now().duration_since(modified) {
                return elapsed.as_secs() < 60;
            }
        }
    }
    false
}

/// Clean up the retry marker file for an alias. Called after a successful connection.
pub fn cleanup_marker(alias: &str) {
    if let Some(path) = marker_path(alias) {
        let _ = std::fs::remove_file(path);
    }
}

/// Parse an askpass source string and return a description for display.
#[allow(dead_code)]
pub fn describe_source(source: &str) -> &str {
    if source == "keychain" {
        "OS Keychain"
    } else if source.starts_with("op://") {
        "1Password"
    } else if source.starts_with("pass:") {
        "pass"
    } else if source.starts_with("bw:") {
        "Bitwarden"
    } else if source.starts_with("vault:") {
        "HashiCorp Vault KV"
    } else {
        "Custom command"
    }
}

/// Bitwarden vault status.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BwStatus {
    Unlocked,
    Locked,
    NotAuthenticated,
    NotInstalled,
}

/// Parse the Bitwarden vault status from `bw status` JSON output.
fn parse_bw_status(stdout: &str) -> BwStatus {
    if let Some(status) = stdout
        .split("\"status\":")
        .nth(1)
        .and_then(|s| s.split('"').nth(1))
    {
        match status {
            "unlocked" => BwStatus::Unlocked,
            "locked" => BwStatus::Locked,
            "unauthenticated" => BwStatus::NotAuthenticated,
            _ => BwStatus::Locked,
        }
    } else {
        BwStatus::NotInstalled
    }
}

/// Check the Bitwarden vault status by running `bw status`.
pub fn bw_vault_status() -> BwStatus {
    let output = match Command::new("bw").arg("status").output() {
        Ok(o) => o,
        Err(_) => return BwStatus::NotInstalled,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_bw_status(&stdout)
}

/// Unlock the Bitwarden vault with the given master password.
/// Passes the password via env var to avoid exposure in `ps` output.
/// Returns the session token on success.
pub fn bw_unlock(password: &str) -> Result<String> {
    let output = Command::new("bw")
        .args(["unlock", "--passwordenv", "PURPLE_BW_MASTER", "--raw"])
        .env("PURPLE_BW_MASTER", password)
        .output()
        .context("Failed to run Bitwarden CLI (bw)")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Bitwarden unlock failed: {}", stderr.trim());
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        anyhow::bail!("Bitwarden unlock returned empty session token");
    }
    Ok(token)
}

#[cfg(test)]
#[path = "askpass_tests.rs"]
mod tests;
