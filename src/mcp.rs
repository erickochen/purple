use std::fs::{File, OpenOptions};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::messages;
use crate::ssh_config::model::{SshConfigFile, is_host_pattern};

/// Tools allowed when the server is started with `--read-only`.
/// State-changing tools (`run_command`, `container_action`) are excluded.
const READ_ONLY_TOOLS: &[&str] = &["list_hosts", "get_host", "list_containers"];

/// Runtime options for the MCP server. Built from CLI flags by `main`.
#[derive(Debug, Clone, Default)]
pub struct McpOptions {
    /// When true, only read-only tools are exposed; state-changing tools are
    /// removed from `tools/list` and rejected from `tools/call`.
    pub read_only: bool,
    /// Path for the audit log. `None` disables audit logging.
    pub audit_log_path: Option<PathBuf>,
}

/// Context passed to dispatch and tool handlers. Holds the SSH config path,
/// runtime options, and an optional audit log handle.
///
/// Fields are crate-visible so call sites inside `mcp` can read them, but
/// the type itself is `pub` so `main` and the in-module tests can construct
/// one.
pub struct McpContext {
    pub(crate) config_path: PathBuf,
    pub(crate) options: McpOptions,
    pub(crate) audit: Option<AuditLog>,
    pub(crate) env: std::sync::Arc<crate::runtime::env::Env>,
}

impl McpContext {
    pub fn new(
        config_path: PathBuf,
        options: McpOptions,
        env: std::sync::Arc<crate::runtime::env::Env>,
    ) -> Self {
        let audit = options
            .audit_log_path
            .as_deref()
            .and_then(|path| match AuditLog::open(path) {
                Ok(log) => Some(log),
                Err(e) => {
                    let body = messages::mcp_audit_init_failed(&path.display(), &e);
                    eprintln!("{body}");
                    warn!("[purple] {body}");
                    None
                }
            });
        Self {
            config_path,
            options,
            audit,
            env,
        }
    }

    /// True when a tool name may be called in the current mode.
    fn is_tool_allowed(&self, tool: &str) -> bool {
        !self.options.read_only || READ_ONLY_TOOLS.contains(&tool)
    }
}

/// Append-only JSON Lines audit log. One file handle, serialised by a
/// `Mutex` so concurrent writes from future multi-threaded clients stay
/// atomic on POSIX. Each entry records timestamp, tool name, sanitised
/// arguments, and outcome.
pub struct AuditLog {
    file: Mutex<File>,
}

impl AuditLog {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
            // Note: we do not chmod the parent. It may be a user-chosen
            // location (e.g. /tmp) that we have no business locking down.
            // The log file itself is set to 0o600 below.
        }
        // Refuse to open a path that already exists as a symlink: an attacker
        // who can pre-create a symlink in a writable location could redirect
        // the log into a sensitive file (cron, ssh authorized_keys, etc.).
        if let Ok(meta) = std::fs::symlink_metadata(path)
            && meta.file_type().is_symlink()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "audit log path is a symlink; refusing to open",
            ));
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        // Restrict to owner read/write. The log can carry host aliases and
        // tool arguments and must not be world-readable. Best-effort: if
        // chmod fails (rare on supported platforms) we still proceed.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    /// Append one audit entry. Failures are logged but never propagated:
    /// audit-log write errors must not break the JSON-RPC response loop.
    /// Args are redacted before logging so shell command bodies and other
    /// fields that may carry secrets are not persisted.
    //
    // Append-only log: `fs_util::atomic_write` would truncate-and-replace,
    // which destroys prior entries. Direct `writeln!` + `flush` is correct
    // here. POSIX guarantees small (<PIPE_BUF) writes against an `O_APPEND`
    // fd are atomic, so concurrent writers do not interleave lines.
    pub fn record(&self, tool: &str, args: &Value, outcome: AuditOutcome) {
        let entry = serde_json::json!({
            "ts": iso8601_now(),
            "tool": tool,
            "args": redact_args_for_audit(tool, args),
            "outcome": outcome.label(),
            "reason": outcome.reason(),
        });
        let line = match serde_json::to_string(&entry) {
            Ok(s) => s,
            Err(e) => {
                warn!("[purple] {}", messages::mcp_audit_write_failed(&e));
                // (return below)
                return;
            }
        };
        let mut guard = match self.file.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Err(e) = writeln!(*guard, "{line}") {
            warn!("[purple] {}", messages::mcp_audit_write_failed(&e));
            return;
        }
        if let Err(e) = guard.flush() {
            warn!("[purple] {}", messages::mcp_audit_write_failed(&e));
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AuditOutcome {
    Allowed,
    Denied,
    Error,
}

impl AuditOutcome {
    fn label(self) -> &'static str {
        match self {
            AuditOutcome::Allowed => "allowed",
            AuditOutcome::Denied => "denied",
            AuditOutcome::Error => "error",
        }
    }
    fn reason(self) -> Option<&'static str> {
        match self {
            AuditOutcome::Denied => Some("read-only mode"),
            _ => None,
        }
    }
}

/// Strip fields that may carry secrets before persisting tool args to the
/// audit log. The log is a security record, not a debugger; the value of an
/// audited entry is "what tool was called, on which host, with what
/// outcome", not the literal command body.
///
/// For `run_command` the entire args value is replaced with a marker when it
/// is not the expected object shape, since a malformed client could send a
/// string or array containing the secret in any position.
fn redact_args_for_audit(tool: &str, args: &Value) -> Value {
    if tool != "run_command" {
        return args.clone();
    }
    let mut redacted = args.clone();
    match redacted.as_object_mut() {
        Some(obj) => {
            if obj.contains_key("command") {
                obj.insert(
                    "command".to_string(),
                    Value::String("<redacted>".to_string()),
                );
            }
        }
        None => {
            // Non-object payload: we cannot reason about which subfield holds
            // the command body, so redact the whole thing.
            redacted = Value::String("<redacted: non-object args>".to_string());
        }
    }
    redacted
}

/// RFC 3339 / ISO 8601 UTC timestamp with second precision built from
/// `SystemTime`. Avoids pulling in chrono for a single format use.
fn iso8601_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_iso8601_utc(secs)
}

fn format_iso8601_utc(secs: u64) -> String {
    let days_since_epoch = secs / 86_400;
    let day_secs = secs % 86_400;
    let hour = day_secs / 3600;
    let minute = (day_secs % 3600) / 60;
    let second = day_secs % 60;
    let (year, month, day) = civil_from_days(days_since_epoch as i64);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's date algorithm. Converts days since 1970-01-01 (UTC)
/// to a (year, month, day) triple. Public-domain, gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

/// Resolve the default audit log path (`mcp-audit.log` in the state
/// directory). `None` when the home directory is unknown, which disables
/// auditing without stopping the server.
pub fn default_audit_log_path(paths: Option<&crate::runtime::env::Paths>) -> Option<PathBuf> {
    match paths {
        Some(p) => Some(p.mcp_audit_log()),
        None => {
            warn!("[purple] {}", messages::MCP_AUDIT_HOME_DIR_UNAVAILABLE);
            None
        }
    }
}

/// A JSON-RPC 2.0 request.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcResponse {
    fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<Value>, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

/// Helper to build an MCP tool result (success).
fn mcp_tool_result(text: &str) -> Value {
    serde_json::json!({
        "content": [{"type": "text", "text": text}]
    })
}

/// Helper to build an MCP tool error result.
fn mcp_tool_error(text: &str) -> Value {
    serde_json::json!({
        "content": [{"type": "text", "text": text}],
        "isError": true
    })
}

/// Surface a missing config file as an explicit MCP error rather than letting
/// the parser silently produce an empty config (the failure mode that caused
/// the .mcpb-with-unexpanded-${HOME} bug to return `[]` instead of erroring).
fn require_config_exists(config_path: &Path) -> Result<(), Value> {
    if !config_path.exists() {
        return Err(mcp_tool_error(&messages::mcp_config_file_not_found(
            &config_path.display(),
        )));
    }
    Ok(())
}

/// Verify that an alias exists in the SSH config. Returns error Value if not found.
fn verify_alias_exists(
    alias: &str,
    config_path: &Path,
    env: &crate::runtime::env::Env,
) -> Result<(), Value> {
    require_config_exists(config_path)?;
    let config = match SshConfigFile::parse_with_env(config_path, env) {
        Ok(c) => c,
        Err(e) => return Err(mcp_tool_error(&format!("Failed to parse SSH config: {e}"))),
    };
    let exists = config.host_entries().iter().any(|h| h.alias == alias);
    if !exists {
        return Err(mcp_tool_error(&format!("Host not found: {alias}")));
    }
    Ok(())
}

/// Run an SSH command with a timeout. Returns (exit_code, stdout, stderr).
fn ssh_exec(
    alias: &str,
    config_path: &Path,
    command: &str,
    timeout_secs: u64,
) -> Result<(i32, String, String), Value> {
    let config_str = config_path.to_string_lossy();
    let child = match std::process::Command::new("ssh")
        .args([
            "-F",
            &config_str,
            "-o",
            "ConnectTimeout=10",
            "-o",
            "BatchMode=yes",
            "--",
            alias,
            command,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return Err(mcp_tool_error(&format!("Failed to spawn ssh: {e}"))),
    };

    // Wait with timeout via mpsc instead of busy-polling. The waiter thread
    // owns the child and reads its stdout/stderr to completion via
    // `wait_with_output`. The main thread blocks on `recv_timeout`. On
    // timeout we kill the orphan process by PID via `kill(1)` since we no
    // longer hold a `Child` handle here. POSIX-only path is acceptable: the
    // project runs on macOS and Linux.
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(std::time::Duration::from_secs(timeout_secs)) {
        Ok(Ok(out)) => {
            let exit = out.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            Ok((exit, stdout, stderr))
        }
        Ok(Err(e)) => Err(mcp_tool_error(&format!("Failed to wait for ssh: {e}"))),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("kill")
                    .arg("-TERM")
                    .arg(pid.to_string())
                    .status();
            }
            warn!("[external] MCP SSH command timed out after {timeout_secs}s (pid {pid})");
            Err(mcp_tool_error(&format!(
                "SSH command timed out after {timeout_secs} seconds"
            )))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(mcp_tool_error("ssh waiter thread disconnected"))
        }
    }
}

/// Dispatch a JSON-RPC method to the appropriate handler.
pub(crate) fn dispatch(method: &str, params: Option<Value>, ctx: &McpContext) -> JsonRpcResponse {
    match method {
        "initialize" => handle_initialize(),
        "tools/list" => handle_tools_list(ctx),
        "tools/call" => handle_tools_call(params, ctx),
        _ => JsonRpcResponse::error(None, -32601, format!("Method not found: {method}")),
    }
}

fn handle_initialize() -> JsonRpcResponse {
    JsonRpcResponse::success(
        None,
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "purple",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

fn handle_tools_list(ctx: &McpContext) -> JsonRpcResponse {
    let all_tools = all_tools_descriptor();
    let tools = if ctx.options.read_only {
        let filtered: Vec<Value> = all_tools
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|t| {
                        t.get("name")
                            .and_then(|n| n.as_str())
                            .map(|n| READ_ONLY_TOOLS.contains(&n))
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        serde_json::json!({ "tools": filtered })
    } else {
        serde_json::json!({ "tools": all_tools })
    };
    JsonRpcResponse::success(None, tools)
}

/// Static descriptor cached on first call. Building the JSON once instead of
/// per-request avoids ~90 lines of `serde_json::json!` allocation on every
/// `tools/list`. Returns a borrow; callers clone if they need ownership.
fn all_tools_descriptor() -> &'static Value {
    static DESCRIPTOR: OnceLock<Value> = OnceLock::new();
    DESCRIPTOR.get_or_init(build_all_tools_descriptor)
}

fn build_all_tools_descriptor() -> Value {
    serde_json::json!([
        {
            "name": "list_hosts",
            "description": "List all SSH hosts available to connect to. Returns alias, hostname, user, port, tags and provider for each host. Use the tag parameter to filter by tag, provider tag or provider name (fuzzy match). Call this first to discover available hosts.",
            "annotations": {
                "title": "List SSH hosts",
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            },
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tag": {
                        "type": "string",
                        "description": "Filter hosts by tag (fuzzy match against tags, provider_tags and provider name)"
                    }
                }
            }
        },
        {
            "name": "get_host",
            "description": "Get detailed information for a single SSH host including identity file, proxy jump, provider metadata, password source and tunnel count.",
            "annotations": {
                "title": "Get SSH host details",
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            },
            "inputSchema": {
                "type": "object",
                "properties": {
                    "alias": {
                        "type": "string",
                        "description": "The host alias to look up"
                    }
                },
                "required": ["alias"]
            }
        },
        {
            "name": "run_command",
            "description": "Run a shell command on a remote host via SSH. Non-interactive (BatchMode). Returns exit code, stdout and stderr. Suitable for diagnostic commands, not interactive programs.",
            "annotations": {
                "title": "Run shell command on SSH host",
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": false,
                "openWorldHint": true
            },
            "inputSchema": {
                "type": "object",
                "properties": {
                    "alias": {
                        "type": "string",
                        "description": "The host alias to connect to"
                    },
                    "command": {
                        "type": "string",
                        "description": "The command to execute"
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in seconds (default 30)",
                        "default": 30,
                        "minimum": 1,
                        "maximum": 300
                    }
                },
                "required": ["alias", "command"]
            }
        },
        {
            "name": "list_containers",
            "description": "List all Docker or Podman containers on a remote host via SSH. Auto-detects the container runtime. Returns container ID, name, image, state, status and ports.",
            "annotations": {
                "title": "List containers on SSH host",
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            },
            "inputSchema": {
                "type": "object",
                "properties": {
                    "alias": {
                        "type": "string",
                        "description": "The host alias to list containers for"
                    }
                },
                "required": ["alias"]
            }
        },
        {
            "name": "container_action",
            "description": "Start, stop or restart a Docker or Podman container on a remote host via SSH. Auto-detects the container runtime.",
            "annotations": {
                "title": "Start, stop or restart container",
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": false,
                "openWorldHint": false
            },
            "inputSchema": {
                "type": "object",
                "properties": {
                    "alias": {
                        "type": "string",
                        "description": "The host alias"
                    },
                    "container_id": {
                        "type": "string",
                        "description": "The container ID or name"
                    },
                    "action": {
                        "type": "string",
                        "description": "The action to perform",
                        "enum": ["start", "stop", "restart"]
                    }
                },
                "required": ["alias", "container_id", "action"]
            }
        }
    ])
}

fn handle_tools_call(params: Option<Value>, ctx: &McpContext) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(
                None,
                -32602,
                "Invalid params: missing params object".to_string(),
            );
        }
    };

    let tool_name = match params.get("name").and_then(|n| n.as_str()) {
        Some(n) => n,
        None => {
            return JsonRpcResponse::error(
                None,
                -32602,
                "Invalid params: missing tool name".to_string(),
            );
        }
    };

    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    if !ctx.is_tool_allowed(tool_name) {
        debug!("[purple] MCP tool denied (read-only mode): tool={tool_name}");
        let result = mcp_tool_error(messages::MCP_TOOL_DENIED_READ_ONLY);
        if let Some(audit) = ctx.audit.as_ref() {
            audit.record(tool_name, &args, AuditOutcome::Denied);
        }
        return JsonRpcResponse::success(None, result);
    }

    let result = match tool_name {
        "list_hosts" => tool_list_hosts(&args, &ctx.config_path, &ctx.env),
        "get_host" => tool_get_host(&args, &ctx.config_path, &ctx.env),
        "run_command" => tool_run_command(&args, &ctx.config_path, &ctx.env),
        "list_containers" => tool_list_containers(&args, &ctx.config_path, &ctx.env),
        "container_action" => tool_container_action(&args, &ctx.config_path, &ctx.env),
        _ => mcp_tool_error(&format!("Unknown tool: {tool_name}")),
    };

    if let Some(audit) = ctx.audit.as_ref() {
        let outcome = if result.get("isError").and_then(|v| v.as_bool()) == Some(true) {
            AuditOutcome::Error
        } else {
            AuditOutcome::Allowed
        };
        audit.record(tool_name, &args, outcome);
    }

    JsonRpcResponse::success(None, result)
}

fn tool_list_hosts(args: &Value, config_path: &Path, env: &crate::runtime::env::Env) -> Value {
    if let Err(e) = require_config_exists(config_path) {
        return e;
    }
    let config = match SshConfigFile::parse_with_env(config_path, env) {
        Ok(c) => c,
        Err(e) => return mcp_tool_error(&format!("Failed to parse SSH config: {e}")),
    };

    let entries = config.host_entries();
    let tag_filter = args.get("tag").and_then(|t| t.as_str());

    let hosts: Vec<Value> = entries
        .iter()
        .filter(|entry| {
            // Skip host patterns (already filtered by host_entries, but be safe)
            if is_host_pattern(&entry.alias) {
                return false;
            }

            // Apply tag filter (fuzzy: substring match on tags, provider_tags, provider name)
            if let Some(tag) = tag_filter {
                let tag_lower = tag.to_lowercase();
                let matches_tags = entry
                    .tags
                    .iter()
                    .any(|t| t.to_lowercase().contains(&tag_lower));
                let matches_provider_tags = entry
                    .provider_tags
                    .iter()
                    .any(|t| t.to_lowercase().contains(&tag_lower));
                let matches_provider = entry
                    .provider
                    .as_ref()
                    .is_some_and(|p| p.to_lowercase().contains(&tag_lower));
                if !matches_tags && !matches_provider_tags && !matches_provider {
                    return false;
                }
            }

            true
        })
        .map(|entry| {
            serde_json::json!({
                "alias": entry.alias,
                "hostname": entry.hostname,
                "user": entry.user,
                "port": entry.port,
                "tags": entry.tags,
                "provider": entry.provider,
                "stale": entry.stale.is_some(),
            })
        })
        .collect();

    let json_str = serde_json::to_string_pretty(&hosts)
        .expect("serde_json::json! values are always serialisable");
    mcp_tool_result(&json_str)
}

fn tool_get_host(args: &Value, config_path: &Path, env: &crate::runtime::env::Env) -> Value {
    let alias = match args.get("alias").and_then(|a| a.as_str()) {
        Some(a) if !a.is_empty() => a,
        _ => return mcp_tool_error("Missing required parameter: alias"),
    };

    if let Err(e) = require_config_exists(config_path) {
        return e;
    }
    let config = match SshConfigFile::parse_with_env(config_path, env) {
        Ok(c) => c,
        Err(e) => return mcp_tool_error(&format!("Failed to parse SSH config: {e}")),
    };

    let entries = config.host_entries();
    let entry = entries.iter().find(|e| e.alias == alias);

    match entry {
        Some(entry) => {
            let meta: serde_json::Map<String, Value> = entry
                .provider_meta
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();

            let host = serde_json::json!({
                "alias": entry.alias,
                "hostname": entry.hostname,
                "user": entry.user,
                "port": entry.port,
                "identity_file": entry.identity_file,
                "proxy_jump": entry.proxy_jump,
                "tags": entry.tags,
                "provider_tags": entry.provider_tags,
                "provider": entry.provider,
                "provider_meta": meta,
                "askpass": entry.askpass,
                "tunnel_count": entry.tunnel_count,
                "stale": entry.stale.is_some(),
            });

            let json_str = serde_json::to_string_pretty(&host)
                .expect("serde_json::json! values are always serialisable");
            mcp_tool_result(&json_str)
        }
        None => mcp_tool_error(&format!("Host not found: {alias}")),
    }
}

fn tool_run_command(args: &Value, config_path: &Path, env: &crate::runtime::env::Env) -> Value {
    let alias = match args.get("alias").and_then(|a| a.as_str()) {
        Some(a) if !a.is_empty() => a,
        _ => return mcp_tool_error("Missing required parameter: alias"),
    };
    let command = match args.get("command").and_then(|c| c.as_str()) {
        Some(c) if !c.is_empty() => c,
        _ => return mcp_tool_error("Missing required parameter: command"),
    };
    // Clamp to the schema-advertised maximum. A misbehaving or compromised
    // client could otherwise hold the single-threaded MCP server for hours
    // by sending an unreasonably large timeout.
    let timeout_secs = args
        .get("timeout")
        .and_then(|t| t.as_u64())
        .unwrap_or(30)
        .clamp(1, 300);

    if let Err(e) = verify_alias_exists(alias, config_path, env) {
        return e;
    }

    // Do not log the command body. It can carry secrets (passwords, tokens)
    // passed as shell arguments. The audit log redacts the same field; the
    // application log must not be a side channel that leaks them.
    debug!("[purple] MCP tool: run_command alias={alias}");
    match ssh_exec(alias, config_path, command, timeout_secs) {
        Ok((exit_code, stdout, stderr)) => {
            // 255 is an ssh transport failure (auth, network, host-key) the
            // caller must act on; 1-254 is the remote command's own status,
            // already returned in the JSON result, so it stays at debug.
            match crate::connection::classify_ssh_exit(exit_code) {
                crate::connection::SshExitClass::TransportFailure => {
                    error!("[external] MCP ssh_exec failed: alias={alias} exit={exit_code}");
                }
                crate::connection::SshExitClass::RemoteStatus => {
                    debug!("[external] MCP ssh_exec exit: alias={alias} exit={exit_code}");
                }
                crate::connection::SshExitClass::Success => {}
            }
            let result = serde_json::json!({
                "exit_code": exit_code,
                "stdout": stdout,
                "stderr": stderr
            });
            let json_str = serde_json::to_string_pretty(&result)
                .expect("serde_json::json! values are always serialisable");
            mcp_tool_result(&json_str)
        }
        Err(e) => e,
    }
}

fn tool_list_containers(args: &Value, config_path: &Path, env: &crate::runtime::env::Env) -> Value {
    let alias = match args.get("alias").and_then(|a| a.as_str()) {
        Some(a) if !a.is_empty() => a,
        _ => return mcp_tool_error("Missing required parameter: alias"),
    };

    if let Err(e) = verify_alias_exists(alias, config_path, env) {
        return e;
    }

    // Build the combined detection + listing command
    let command = crate::containers::container_list_command(None);

    let (exit_code, stdout, stderr) = match ssh_exec(alias, config_path, &command, 30) {
        Ok(r) => r,
        Err(e) => return e,
    };

    if exit_code != 0 {
        return mcp_tool_error(&format!("SSH command failed: {}", stderr.trim()));
    }

    match crate::containers::parse_container_output(&stdout, None) {
        Ok(listing) => {
            let containers_json: Vec<Value> = listing
                .containers
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "id": c.id,
                        "name": c.names,
                        "image": c.image,
                        "state": c.state,
                        "status": c.status,
                        "ports": c.ports,
                    })
                })
                .collect();
            let mut result = serde_json::json!({
                "runtime": listing.runtime.as_str(),
                "containers": containers_json,
            });
            if let Some(v) = listing.engine_version {
                result["engine_version"] = serde_json::Value::String(v);
            }
            let json_str = serde_json::to_string_pretty(&result)
                .expect("serde_json::json! values are always serialisable");
            mcp_tool_result(&json_str)
        }
        Err(e) => mcp_tool_error(&e),
    }
}

fn tool_container_action(
    args: &Value,
    config_path: &Path,
    env: &crate::runtime::env::Env,
) -> Value {
    let alias = match args.get("alias").and_then(|a| a.as_str()) {
        Some(a) if !a.is_empty() => a,
        _ => return mcp_tool_error("Missing required parameter: alias"),
    };
    let container_id = match args.get("container_id").and_then(|c| c.as_str()) {
        Some(c) if !c.is_empty() => c,
        _ => return mcp_tool_error("Missing required parameter: container_id"),
    };
    let action_str = match args.get("action").and_then(|a| a.as_str()) {
        Some(a) => a,
        None => return mcp_tool_error("Missing required parameter: action"),
    };

    // Validate container ID (injection prevention)
    if let Err(e) = crate::containers::validate_container_id(container_id) {
        return mcp_tool_error(&e);
    }

    let action = match action_str {
        "start" => crate::containers::ContainerAction::Start,
        "stop" => crate::containers::ContainerAction::Stop,
        "restart" => crate::containers::ContainerAction::Restart,
        _ => {
            return mcp_tool_error(&format!(
                "Invalid action: {action_str}. Must be start, stop or restart"
            ));
        }
    };

    if let Err(e) = verify_alias_exists(alias, config_path, env) {
        return e;
    }

    // First detect runtime
    let detect_cmd = crate::containers::container_list_command(None);

    let (detect_exit, detect_stdout, detect_stderr) =
        match ssh_exec(alias, config_path, &detect_cmd, 30) {
            Ok(r) => r,
            Err(e) => return e,
        };

    if detect_exit != 0 {
        return mcp_tool_error(&format!(
            "Failed to detect container runtime: {}",
            detect_stderr.trim()
        ));
    }

    let runtime = match crate::containers::parse_container_output(&detect_stdout, None) {
        Ok(listing) => listing.runtime,
        Err(e) => return mcp_tool_error(&format!("Failed to detect container runtime: {e}")),
    };

    let action_command = crate::containers::container_action_command(runtime, action, container_id);

    let (action_exit, _action_stdout, action_stderr) =
        match ssh_exec(alias, config_path, &action_command, 30) {
            Ok(r) => r,
            Err(e) => return e,
        };

    if action_exit == 0 {
        let past = match action_str {
            "start" => "started",
            "stop" => "stopped",
            "restart" => "restarted",
            other => other,
        };
        let result = serde_json::json!({
            "success": true,
            "message": format!("Container {container_id} {past}"),
        });
        let json_str = serde_json::to_string_pretty(&result)
            .expect("serde_json::json! values are always serialisable");
        mcp_tool_result(&json_str)
    } else {
        mcp_tool_error(&format!(
            "Container action failed: {}",
            action_stderr.trim()
        ))
    }
}

/// Run the MCP server, reading JSON-RPC requests from stdin and writing
/// responses to stdout. Blocks until stdin is closed.
pub fn run(
    config_path: &Path,
    options: McpOptions,
    env: std::sync::Arc<crate::runtime::env::Env>,
) -> anyhow::Result<()> {
    info!(
        "[purple] MCP server starting (read_only={}, audit_log={})",
        options.read_only,
        options
            .audit_log_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "disabled".to_string())
    );
    let ctx = McpContext::new(config_path.to_path_buf(), options, env);

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let reader = stdin.lock();
    let mut writer = stdout.lock();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(_) => {
                let resp = JsonRpcResponse::error(None, -32700, "Parse error".to_string());
                let json = serde_json::to_string(&resp)?;
                writeln!(writer, "{json}")?;
                writer.flush()?;
                continue;
            }
        };

        // Notifications (no id) don't get responses
        if request.id.is_none() {
            debug!("[purple] MCP notification: {}", request.method);
            continue;
        }

        debug!("[purple] MCP request: method={}", request.method);
        let mut response = dispatch(&request.method, request.params, &ctx);
        debug!(
            "[purple] MCP response: method={} success={}",
            request.method,
            response.error.is_none()
        );
        response.id = request.id;

        let json = serde_json::to_string(&response)?;
        writeln!(writer, "{json}")?;
        writer.flush()?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
