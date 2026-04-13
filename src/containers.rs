use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{error, info};

use serde::{Deserialize, Serialize};

use crate::ssh_context::{OwnedSshContext, SshContext};

// ---------------------------------------------------------------------------
// ContainerInfo model
// ---------------------------------------------------------------------------

/// Metadata for a single container (from `docker ps -a` / `podman ps -a`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerInfo {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Names")]
    pub names: String,
    #[serde(rename = "Image")]
    pub image: String,
    #[serde(rename = "State")]
    pub state: String,
    #[serde(rename = "Status")]
    pub status: String,
    #[serde(rename = "Ports")]
    pub ports: String,
}

/// Parse NDJSON output from `docker ps --format '{{json .}}'`.
/// Invalid lines are silently ignored (MOTD lines, blank lines, etc.).
pub fn parse_container_ps(output: &str) -> Vec<ContainerInfo> {
    output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            serde_json::from_str(trimmed).ok()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// ContainerRuntime
// ---------------------------------------------------------------------------

/// Supported container runtimes.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ContainerRuntime {
    Docker,
    Podman,
}

impl ContainerRuntime {
    /// Returns the CLI binary name.
    pub fn as_str(&self) -> &'static str {
        match self {
            ContainerRuntime::Docker => "docker",
            ContainerRuntime::Podman => "podman",
        }
    }
}

/// Detect runtime from command output by matching the LAST non-empty trimmed
/// line. Only "docker" or "podman" are accepted. MOTD-resilient.
/// Currently unused (sentinel-based detection handles this inline) but kept
/// as a public utility for potential future two-step detection paths.
#[allow(dead_code)]
pub fn parse_runtime(output: &str) -> Option<ContainerRuntime> {
    let last = output
        .lines()
        .rev()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())?;
    match last {
        "docker" => Some(ContainerRuntime::Docker),
        "podman" => Some(ContainerRuntime::Podman),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// ContainerAction
// ---------------------------------------------------------------------------

/// Actions that can be performed on a container.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
}

impl ContainerAction {
    /// Returns the CLI sub-command string.
    pub fn as_str(&self) -> &'static str {
        match self {
            ContainerAction::Start => "start",
            ContainerAction::Stop => "stop",
            ContainerAction::Restart => "restart",
        }
    }
}

/// Build the shell command to perform an action on a container.
pub fn container_action_command(
    runtime: ContainerRuntime,
    action: ContainerAction,
    container_id: &str,
) -> String {
    format!("{} {} {}", runtime.as_str(), action.as_str(), container_id)
}

// ---------------------------------------------------------------------------
// Container ID validation
// ---------------------------------------------------------------------------

/// Validate a container ID or name.
/// Accepts ASCII alphanumeric, hyphen, underscore, dot.
/// Rejects empty, non-ASCII, shell metacharacters, colon.
pub fn validate_container_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("Container ID must not be empty.".to_string());
    }
    for c in id.chars() {
        if !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.' {
            return Err(format!("Container ID contains invalid character: '{c}'"));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Combined SSH command + output parsing
// ---------------------------------------------------------------------------

/// Build the SSH command string for listing containers.
///
/// - `Some(Docker)` / `Some(Podman)`: direct listing for the known runtime.
/// - `None`: combined detection + listing with sentinel markers in one SSH call.
pub fn container_list_command(runtime: Option<ContainerRuntime>) -> String {
    match runtime {
        Some(ContainerRuntime::Docker) => "docker ps -a --format '{{json .}}'".to_string(),
        Some(ContainerRuntime::Podman) => "podman ps -a --format '{{json .}}'".to_string(),
        None => concat!(
            "if command -v docker >/dev/null 2>&1; then ",
            "echo '##purple:docker##' && docker ps -a --format '{{json .}}'; ",
            "elif command -v podman >/dev/null 2>&1; then ",
            "echo '##purple:podman##' && podman ps -a --format '{{json .}}'; ",
            "else echo '##purple:none##'; fi"
        )
        .to_string(),
    }
}

/// Parse the stdout of a container listing command.
///
/// When sentinels are present (combined detection run): extract runtime from
/// the sentinel line, parse remaining lines as NDJSON. When `caller_runtime`
/// is provided (subsequent run with known runtime): parse all lines as NDJSON.
pub fn parse_container_output(
    output: &str,
    caller_runtime: Option<ContainerRuntime>,
) -> Result<(ContainerRuntime, Vec<ContainerInfo>), String> {
    if let Some(sentinel_line) = output.lines().find(|l| l.trim().starts_with("##purple:")) {
        let sentinel = sentinel_line.trim();
        if sentinel == "##purple:none##" {
            return Err("No container runtime found. Install Docker or Podman.".to_string());
        }
        let runtime = if sentinel == "##purple:docker##" {
            ContainerRuntime::Docker
        } else if sentinel == "##purple:podman##" {
            ContainerRuntime::Podman
        } else {
            return Err(format!("Unknown sentinel: {sentinel}"));
        };
        let containers: Vec<ContainerInfo> = output
            .lines()
            .filter(|l| !l.trim().starts_with("##purple:"))
            .filter_map(|line| {
                let t = line.trim();
                if t.is_empty() {
                    return None;
                }
                serde_json::from_str(t).ok()
            })
            .collect();
        return Ok((runtime, containers));
    }

    match caller_runtime {
        Some(rt) => Ok((rt, parse_container_ps(output))),
        None => Err("No sentinel found and no runtime provided.".to_string()),
    }
}

// ---------------------------------------------------------------------------
// SSH fetch functions
// ---------------------------------------------------------------------------

/// Error from a container listing operation. Preserves the detected runtime
/// even when the `ps` command fails so it can be cached for future calls.
#[derive(Debug)]
pub struct ContainerError {
    pub runtime: Option<ContainerRuntime>,
    pub message: String,
}

impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Translate SSH stderr into a user-friendly error message.
fn friendly_container_error(stderr: &str, code: Option<i32>) -> String {
    let lower = stderr.to_lowercase();
    if lower.contains("command not found") {
        "Docker or Podman not found on remote host.".to_string()
    } else if lower.contains("permission denied") || lower.contains("got permission denied") {
        "Permission denied. Is your user in the docker group?".to_string()
    } else if lower.contains("cannot connect to the docker daemon")
        || lower.contains("cannot connect to podman")
    {
        "Container daemon is not running.".to_string()
    } else if lower.contains("connection refused") {
        "Connection refused.".to_string()
    } else if lower.contains("no route to host") || lower.contains("network is unreachable") {
        "Host unreachable.".to_string()
    } else {
        format!("Command failed with code {}.", code.unwrap_or(1))
    }
}

/// Fetch container list synchronously via SSH.
/// Follows the `fetch_remote_listing` pattern.
pub fn fetch_containers(
    ctx: &SshContext<'_>,
    cached_runtime: Option<ContainerRuntime>,
) -> Result<(ContainerRuntime, Vec<ContainerInfo>), ContainerError> {
    let command = container_list_command(cached_runtime);
    let result = crate::snippet::run_snippet(
        ctx.alias,
        ctx.config_path,
        &command,
        ctx.askpass,
        ctx.bw_session,
        true,
        ctx.has_tunnel,
    );
    let alias = ctx.alias;
    match result {
        Ok(r) if r.status.success() => {
            parse_container_output(&r.stdout, cached_runtime).map_err(|e| {
                error!("[external] Container list parse failed: alias={alias}: {e}");
                ContainerError {
                    runtime: cached_runtime,
                    message: e,
                }
            })
        }
        Ok(r) => {
            let stderr = r.stderr.trim().to_string();
            let msg = friendly_container_error(&stderr, r.status.code());
            error!("[external] Container fetch failed: alias={alias}: {msg}");
            Err(ContainerError {
                runtime: cached_runtime,
                message: msg,
            })
        }
        Err(e) => {
            error!("[external] Container fetch failed: alias={alias}: {e}");
            Err(ContainerError {
                runtime: cached_runtime,
                message: e.to_string(),
            })
        }
    }
}

/// Spawn a background thread to fetch container listings.
/// Follows the `spawn_remote_listing` pattern.
pub fn spawn_container_listing<F>(
    ctx: OwnedSshContext,
    cached_runtime: Option<ContainerRuntime>,
    send: F,
) where
    F: FnOnce(String, Result<(ContainerRuntime, Vec<ContainerInfo>), ContainerError>)
        + Send
        + 'static,
{
    std::thread::spawn(move || {
        let borrowed = SshContext {
            alias: &ctx.alias,
            config_path: &ctx.config_path,
            askpass: ctx.askpass.as_deref(),
            bw_session: ctx.bw_session.as_deref(),
            has_tunnel: ctx.has_tunnel,
        };
        let result = fetch_containers(&borrowed, cached_runtime);
        send(ctx.alias, result);
    });
}

/// Spawn a background thread to perform a container action (start/stop/restart).
/// Validates the container ID before executing.
pub fn spawn_container_action<F>(
    ctx: OwnedSshContext,
    runtime: ContainerRuntime,
    action: ContainerAction,
    container_id: String,
    send: F,
) where
    F: FnOnce(String, ContainerAction, Result<(), String>) + Send + 'static,
{
    std::thread::spawn(move || {
        if let Err(e) = validate_container_id(&container_id) {
            send(ctx.alias, action, Err(e));
            return;
        }
        let alias = &ctx.alias;
        info!(
            "Container action: {} container={container_id} alias={alias}",
            action.as_str()
        );
        let command = container_action_command(runtime, action, &container_id);
        let result = crate::snippet::run_snippet(
            alias,
            &ctx.config_path,
            &command,
            ctx.askpass.as_deref(),
            ctx.bw_session.as_deref(),
            true,
            ctx.has_tunnel,
        );
        match result {
            Ok(r) if r.status.success() => send(ctx.alias, action, Ok(())),
            Ok(r) => {
                let err = friendly_container_error(r.stderr.trim(), r.status.code());
                error!(
                    "[external] Container {} failed: alias={alias} container={container_id}: {err}",
                    action.as_str()
                );
                send(ctx.alias, action, Err(err));
            }
            Err(e) => {
                error!(
                    "[external] Container {} failed: alias={alias} container={container_id}: {e}",
                    action.as_str()
                );
                send(ctx.alias, action, Err(e.to_string()));
            }
        }
    });
}

// ---------------------------------------------------------------------------
// JSON lines cache
// ---------------------------------------------------------------------------

/// A cached container listing for a single host.
#[derive(Debug, Clone)]
pub struct ContainerCacheEntry {
    pub timestamp: u64,
    pub runtime: ContainerRuntime,
    pub containers: Vec<ContainerInfo>,
}

/// Serde helper for a single JSON line in the cache file.
#[derive(Serialize, Deserialize)]
struct CacheLine {
    alias: String,
    timestamp: u64,
    runtime: ContainerRuntime,
    containers: Vec<ContainerInfo>,
}

/// Load container cache from `~/.purple/container_cache.jsonl`.
/// Malformed lines are silently ignored. Duplicate aliases: last-write-wins.
pub fn load_container_cache() -> HashMap<String, ContainerCacheEntry> {
    let mut map = HashMap::new();
    let Some(home) = dirs::home_dir() else {
        return map;
    };
    let path = home.join(".purple").join("container_cache.jsonl");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return map;
    };
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<CacheLine>(trimmed) {
            map.insert(
                entry.alias,
                ContainerCacheEntry {
                    timestamp: entry.timestamp,
                    runtime: entry.runtime,
                    containers: entry.containers,
                },
            );
        }
    }
    map
}

/// Parse container cache from JSONL content string (for demo/test use).
pub fn parse_container_cache_content(content: &str) -> HashMap<String, ContainerCacheEntry> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<CacheLine>(trimmed) {
            map.insert(
                entry.alias,
                ContainerCacheEntry {
                    timestamp: entry.timestamp,
                    runtime: entry.runtime,
                    containers: entry.containers,
                },
            );
        }
    }
    map
}

/// Save container cache to `~/.purple/container_cache.jsonl` via atomic write.
pub fn save_container_cache(cache: &HashMap<String, ContainerCacheEntry>) {
    if crate::demo_flag::is_demo() {
        return;
    }
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let path = home.join(".purple").join("container_cache.jsonl");
    let mut lines = Vec::with_capacity(cache.len());
    for (alias, entry) in cache {
        let line = CacheLine {
            alias: alias.clone(),
            timestamp: entry.timestamp,
            runtime: entry.runtime,
            containers: entry.containers.clone(),
        };
        if let Ok(s) = serde_json::to_string(&line) {
            lines.push(s);
        }
    }
    let content = lines.join("\n");
    if let Err(e) = crate::fs_util::atomic_write(&path, content.as_bytes()) {
        log::warn!(
            "[config] Failed to write container cache {}: {e}",
            path.display()
        );
    }
}

// ---------------------------------------------------------------------------
// String truncation
// ---------------------------------------------------------------------------

/// Truncate a string to at most `max` characters. Appends ".." if truncated.
pub fn truncate_str(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let cut = max.saturating_sub(2);
        let end = s.char_indices().nth(cut).map(|(i, _)| i).unwrap_or(s.len());
        format!("{}..", &s[..end])
    }
}

// ---------------------------------------------------------------------------
// Relative time
// ---------------------------------------------------------------------------

/// Format a Unix timestamp as a human-readable relative time string.
pub fn format_relative_time(timestamp: u64) -> String {
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
    } else {
        format!("{}d ago", diff / 86400)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "containers_tests.rs"]
mod tests;
