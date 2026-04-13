use std::io::{BufRead, Write};
use std::path::Path;

use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ssh_config::model::{SshConfigFile, is_host_pattern};

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

/// Verify that an alias exists in the SSH config. Returns error Value if not found.
fn verify_alias_exists(alias: &str, config_path: &Path) -> Result<(), Value> {
    let config = match SshConfigFile::parse(config_path) {
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
    let mut child = match std::process::Command::new("ssh")
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

    let timeout = std::time::Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child
                    .stdout
                    .take()
                    .map(|mut s| {
                        let mut buf = String::new();
                        if let Err(e) = std::io::Read::read_to_string(&mut s, &mut buf) {
                            warn!("[external] Failed to read SSH stdout pipe: {e}");
                        }
                        buf
                    })
                    .unwrap_or_default();
                let stderr = child
                    .stderr
                    .take()
                    .map(|mut s| {
                        let mut buf = String::new();
                        if let Err(e) = std::io::Read::read_to_string(&mut s, &mut buf) {
                            warn!("[external] Failed to read SSH stderr pipe: {e}");
                        }
                        buf
                    })
                    .unwrap_or_default();
                return Ok((status.code().unwrap_or(-1), stdout, stderr));
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    if let Err(e) = child.kill() {
                        warn!("[external] Failed to kill timed-out SSH process: {e}");
                    }
                    let _ = child.wait();
                    warn!("[external] MCP SSH command timed out after {timeout_secs}s");
                    return Err(mcp_tool_error(&format!(
                        "SSH command timed out after {timeout_secs} seconds"
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(mcp_tool_error(&format!("Failed to wait for ssh: {e}"))),
        }
    }
}

/// Dispatch a JSON-RPC method to the appropriate handler.
pub(crate) fn dispatch(method: &str, params: Option<Value>, config_path: &Path) -> JsonRpcResponse {
    match method {
        "initialize" => handle_initialize(),
        "tools/list" => handle_tools_list(),
        "tools/call" => handle_tools_call(params, config_path),
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

fn handle_tools_list() -> JsonRpcResponse {
    let tools = serde_json::json!({
        "tools": [
            {
                "name": "list_hosts",
                "description": "List all SSH hosts available to connect to. Returns alias, hostname, user, port, tags and provider for each host. Use the tag parameter to filter by tag, provider tag or provider name (fuzzy match). Call this first to discover available hosts.",
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
        ]
    });
    JsonRpcResponse::success(None, tools)
}

fn handle_tools_call(params: Option<Value>, config_path: &Path) -> JsonRpcResponse {
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

    let result = match tool_name {
        "list_hosts" => tool_list_hosts(&args, config_path),
        "get_host" => tool_get_host(&args, config_path),
        "run_command" => tool_run_command(&args, config_path),
        "list_containers" => tool_list_containers(&args, config_path),
        "container_action" => tool_container_action(&args, config_path),
        _ => mcp_tool_error(&format!("Unknown tool: {tool_name}")),
    };

    JsonRpcResponse::success(None, result)
}

fn tool_list_hosts(args: &Value, config_path: &Path) -> Value {
    let config = match SshConfigFile::parse(config_path) {
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

    let json_str = serde_json::to_string_pretty(&hosts).unwrap_or_default();
    mcp_tool_result(&json_str)
}

fn tool_get_host(args: &Value, config_path: &Path) -> Value {
    let alias = match args.get("alias").and_then(|a| a.as_str()) {
        Some(a) => a,
        None => return mcp_tool_error("Missing required parameter: alias"),
    };

    let config = match SshConfigFile::parse(config_path) {
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

            let json_str = serde_json::to_string_pretty(&host).unwrap_or_default();
            mcp_tool_result(&json_str)
        }
        None => mcp_tool_error(&format!("Host not found: {alias}")),
    }
}

fn tool_run_command(args: &Value, config_path: &Path) -> Value {
    let alias = match args.get("alias").and_then(|a| a.as_str()) {
        Some(a) if !a.is_empty() => a,
        _ => return mcp_tool_error("Missing required parameter: alias"),
    };
    let command = match args.get("command").and_then(|c| c.as_str()) {
        Some(c) if !c.is_empty() => c,
        _ => return mcp_tool_error("Missing required parameter: command"),
    };
    let timeout_secs = args.get("timeout").and_then(|t| t.as_u64()).unwrap_or(30);

    if let Err(e) = verify_alias_exists(alias, config_path) {
        return e;
    }

    info!("MCP tool: ssh_exec alias={alias} command={command}");
    match ssh_exec(alias, config_path, command, timeout_secs) {
        Ok((exit_code, stdout, stderr)) => {
            if exit_code != 0 {
                error!("[external] MCP ssh_exec failed: alias={alias} exit={exit_code}");
            }
            let result = serde_json::json!({
                "exit_code": exit_code,
                "stdout": stdout,
                "stderr": stderr
            });
            let json_str = serde_json::to_string_pretty(&result).unwrap_or_default();
            mcp_tool_result(&json_str)
        }
        Err(e) => e,
    }
}

fn tool_list_containers(args: &Value, config_path: &Path) -> Value {
    let alias = match args.get("alias").and_then(|a| a.as_str()) {
        Some(a) if !a.is_empty() => a,
        _ => return mcp_tool_error("Missing required parameter: alias"),
    };

    if let Err(e) = verify_alias_exists(alias, config_path) {
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
        Ok((runtime, containers)) => {
            let containers_json: Vec<Value> = containers
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
            let result = serde_json::json!({
                "runtime": runtime.as_str(),
                "containers": containers_json,
            });
            let json_str = serde_json::to_string_pretty(&result).unwrap_or_default();
            mcp_tool_result(&json_str)
        }
        Err(e) => mcp_tool_error(&e),
    }
}

fn tool_container_action(args: &Value, config_path: &Path) -> Value {
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

    if let Err(e) = verify_alias_exists(alias, config_path) {
        return e;
    }

    // First detect runtime
    let detect_cmd = crate::containers::container_list_command(None);

    let (detect_exit, detect_stdout, _detect_stderr) =
        match ssh_exec(alias, config_path, &detect_cmd, 30) {
            Ok(r) => r,
            Err(e) => return e,
        };

    if detect_exit != 0 {
        return mcp_tool_error("Failed to detect container runtime");
    }

    let runtime = match crate::containers::parse_container_output(&detect_stdout, None) {
        Ok((rt, _)) => rt,
        Err(e) => return mcp_tool_error(&format!("Failed to detect container runtime: {e}")),
    };

    let action_command = crate::containers::container_action_command(runtime, action, container_id);

    let (action_exit, _action_stdout, action_stderr) =
        match ssh_exec(alias, config_path, &action_command, 30) {
            Ok(r) => r,
            Err(e) => return e,
        };

    if action_exit == 0 {
        let result = serde_json::json!({
            "success": true,
            "message": format!("Container {container_id} {}ed", action_str),
        });
        let json_str = serde_json::to_string_pretty(&result).unwrap_or_default();
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
pub fn run(config_path: &Path) -> anyhow::Result<()> {
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
            debug!("MCP notification: {}", request.method);
            continue;
        }

        debug!("MCP request: method={}", request.method);
        let mut response = dispatch(&request.method, request.params, config_path);
        debug!(
            "MCP response: method={} success={}",
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
