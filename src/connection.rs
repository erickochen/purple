use std::process::Command;

use anyhow::{Context, Result};

/// Launch an SSH connection to the given host alias.
/// Uses the system `ssh` binary with inherited stdin/stdout/stderr.
pub fn connect(alias: &str) -> Result<std::process::ExitStatus> {
    let status = Command::new("ssh")
        .arg("--")
        .arg(alias)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to launch ssh for '{}'", alias))?;
    Ok(status)
}
