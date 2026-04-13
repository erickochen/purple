use super::*;

// --- Task 1: JSON-RPC types and parsing ---

#[test]
fn parse_valid_request() {
    let json = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.method, "initialize");
    assert_eq!(req.id, Some(Value::Number(1.into())));
}

#[test]
fn parse_notification_no_id() {
    let json = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
    assert!(req.id.is_none());
    assert!(req.params.is_none());
}

#[test]
fn parse_invalid_json() {
    let result: Result<JsonRpcRequest, _> = serde_json::from_str("not json");
    assert!(result.is_err());
}

#[test]
fn response_success_serialization() {
    let resp = JsonRpcResponse::success(Some(Value::Number(1.into())), Value::Bool(true));
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains(r#""result":true"#));
    assert!(!json.contains("error"));
}

#[test]
fn response_error_serialization() {
    let resp = JsonRpcResponse::error(
        Some(Value::Number(1.into())),
        -32601,
        "Method not found".to_string(),
    );
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("-32601"));
    assert!(!json.contains("result"));
}

// --- Task 2: MCP initialize and tools/list handlers ---

#[test]
fn test_handle_initialize() {
    let params = serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "test", "version": "1.0"}
    });
    let resp = dispatch(
        "initialize",
        Some(params),
        &std::path::PathBuf::from("/dev/null"),
    );
    let result = resp.result.unwrap();
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert!(result["capabilities"]["tools"].is_object());
    assert_eq!(result["serverInfo"]["name"], "purple");
}

#[test]
fn test_handle_tools_list() {
    let resp = dispatch("tools/list", None, &std::path::PathBuf::from("/dev/null"));
    let result = resp.result.unwrap();
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"list_hosts"));
    assert!(names.contains(&"get_host"));
    assert!(names.contains(&"run_command"));
    assert!(names.contains(&"list_containers"));
    assert!(names.contains(&"container_action"));
}

#[test]
fn test_handle_unknown_method() {
    let resp = dispatch("bogus/method", None, &std::path::PathBuf::from("/dev/null"));
    assert!(resp.error.is_some());
    assert_eq!(resp.error.unwrap().code, -32601);
}

// --- Task 3: list_hosts and get_host tool handlers ---

#[test]
fn tool_list_hosts_returns_all_concrete_hosts() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_hosts", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0]["alias"], "web-1");
    assert_eq!(hosts[1]["alias"], "db-1");
}

#[test]
fn tool_list_hosts_filter_by_tag() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"tag": "database"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_hosts", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0]["alias"], "db-1");
}

#[test]
fn tool_get_host_found() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "web-1"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "get_host", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let host: Value = serde_json::from_str(text).unwrap();
    assert_eq!(host["alias"], "web-1");
    assert_eq!(host["hostname"], "10.0.1.5");
    assert_eq!(host["user"], "deploy");
    assert_eq!(host["identity_file"], "~/.ssh/id_ed25519");
    assert_eq!(host["provider"], "aws");
}

#[test]
fn tool_get_host_not_found() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "nonexistent"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "get_host", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_get_host_missing_alias() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "get_host", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

// --- Task 4: run_command tool handler ---

#[test]
fn tool_run_command_missing_alias() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"command": "uptime"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "run_command", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_run_command_missing_command() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "web-1"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "run_command", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_run_command_empty_alias() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "", "command": "uptime"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "run_command", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_run_command_empty_command() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "web-1", "command": ""});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "run_command", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

// --- Task 5: list_containers and container_action tool handlers ---

#[test]
fn tool_list_containers_missing_alias() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_containers", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_container_action_missing_fields() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "web-1", "action": "start"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "container_action", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_container_action_invalid_action() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "web-1", "container_id": "abc", "action": "destroy"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "container_action", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_container_action_invalid_container_id() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args =
        serde_json::json!({"alias": "web-1", "container_id": "abc;rm -rf /", "action": "start"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "container_action", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

// --- Protocol-level tests ---

#[test]
fn tools_call_missing_params() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let resp = dispatch("tools/call", None, &config_path);
    assert!(resp.result.is_none());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(err.message.contains("missing params"));
}

#[test]
fn tools_call_missing_tool_name() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"arguments": {}})),
        &config_path,
    );
    assert!(resp.result.is_none());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(err.message.contains("missing tool name"));
}

#[test]
fn tools_call_unknown_tool() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "nonexistent_tool", "arguments": {}})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Unknown tool")
    );
}

#[test]
fn tools_call_name_is_number_not_string() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": 42, "arguments": {}})),
        &config_path,
    );
    assert!(resp.result.is_none());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
}

#[test]
fn tools_call_no_arguments_field() {
    // arguments defaults to {} when missing
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_hosts"})),
        &config_path,
    );
    let result = resp.result.unwrap();
    // Should succeed - list_hosts with no args returns all hosts
    assert!(result.get("isError").is_none());
    let text = result["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
    assert_eq!(hosts.len(), 2);
}

// --- list_hosts additional tests ---

#[test]
fn tool_list_hosts_empty_config() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_empty_config");
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_hosts", "arguments": {}})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
    assert!(hosts.is_empty());
}

#[test]
fn tool_list_hosts_filter_by_provider_name() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"tag": "aws"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_hosts", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0]["alias"], "web-1");
}

#[test]
fn tool_list_hosts_filter_case_insensitive() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"tag": "PROD"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_hosts", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
    assert_eq!(hosts.len(), 2); // both web-1 and db-1 have "prod" tag
}

#[test]
fn tool_list_hosts_filter_no_match() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"tag": "nonexistent-tag"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_hosts", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
    assert!(hosts.is_empty());
}

#[test]
fn tool_list_hosts_filter_by_provider_tags() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_provider_tags_config");
    let args = serde_json::json!({"tag": "backend"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_hosts", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0]["alias"], "tagged-1");
}

#[test]
fn tool_list_hosts_stale_field_is_boolean() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_stale_config");
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_hosts", "arguments": {}})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
    let stale_host = hosts.iter().find(|h| h["alias"] == "stale-1").unwrap();
    let active_host = hosts.iter().find(|h| h["alias"] == "active-1").unwrap();
    assert_eq!(stale_host["stale"], true);
    assert_eq!(active_host["stale"], false);
}

#[test]
fn tool_list_hosts_output_fields() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_hosts", "arguments": {}})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
    let host = &hosts[0];
    // Verify all expected fields are present
    assert!(host.get("alias").is_some());
    assert!(host.get("hostname").is_some());
    assert!(host.get("user").is_some());
    assert!(host.get("port").is_some());
    assert!(host.get("tags").is_some());
    assert!(host.get("provider").is_some());
    assert!(host.get("stale").is_some());
    // Verify types
    assert!(host["port"].is_number());
    assert!(host["tags"].is_array());
    assert!(host["stale"].is_boolean());
}

// --- get_host additional tests ---

#[test]
fn tool_get_host_empty_alias() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": ""});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "get_host", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    // get_host doesn't check for empty string (unlike run_command), just does lookup
    // Empty string won't match any host
    assert!(
        result["isError"].as_bool().unwrap_or(false) || {
            let text = result["content"][0]["text"].as_str().unwrap_or("");
            text.contains("not found") || text.contains("Missing")
        }
    );
}

#[test]
fn tool_get_host_alias_is_number() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": 42});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "get_host", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_get_host_output_fields() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "web-1"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "get_host", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let host: Value = serde_json::from_str(text).unwrap();
    // Verify all expected fields
    assert_eq!(host["port"], 22);
    assert!(host["tags"].is_array());
    assert!(host["provider_tags"].is_array());
    assert!(host["provider_meta"].is_object());
    assert!(host["stale"].is_boolean());
    assert_eq!(host["stale"], false);
    assert_eq!(host["tunnel_count"], 0);
    // Verify provider_meta content
    assert_eq!(host["provider_meta"]["region"], "us-east-1");
    assert_eq!(host["provider_meta"]["instance"], "t3.micro");
}

#[test]
fn tool_get_host_no_provider() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "db-1"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "get_host", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let host: Value = serde_json::from_str(text).unwrap();
    assert!(host["provider"].is_null());
    assert!(host["provider_meta"].as_object().unwrap().is_empty());
    assert_eq!(host["port"], 5432);
}

#[test]
fn tool_get_host_stale_is_boolean() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_stale_config");
    let args = serde_json::json!({"alias": "stale-1"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "get_host", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    let host: Value = serde_json::from_str(text).unwrap();
    assert_eq!(host["stale"], true);
}

#[test]
fn tool_get_host_case_sensitive() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "WEB-1"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "get_host", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

// --- run_command additional tests ---

#[test]
fn tool_run_command_nonexistent_alias() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "nonexistent-host", "command": "uptime"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "run_command", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("not found")
    );
}

#[test]
fn tool_run_command_alias_is_number() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": 42, "command": "uptime"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "run_command", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_run_command_command_is_number() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "web-1", "command": 123});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "run_command", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_run_command_timeout_is_string() {
    // timeout as string should be ignored, defaulting to 30
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args =
        serde_json::json!({"alias": "web-1", "command": "uptime", "timeout": "not-a-number"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "run_command", "arguments": args})),
        &config_path,
    );
    // This should not error on parsing - timeout defaults to 30
    // It will fail on SSH but not on input validation
    let result = resp.result.unwrap();
    // The alias exists so it will try SSH (which may fail), but no input validation error
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(!text.contains("Missing required parameter"));
}

// --- container_action additional tests ---

#[test]
fn tool_container_action_empty_alias() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "", "container_id": "abc", "action": "start"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "container_action", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_container_action_empty_container_id() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "web-1", "container_id": "", "action": "start"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "container_action", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_container_action_nonexistent_alias() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args =
        serde_json::json!({"alias": "nonexistent", "container_id": "abc", "action": "start"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "container_action", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("not found")
    );
}

#[test]
fn tool_container_action_uppercase_action() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "web-1", "container_id": "abc", "action": "START"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "container_action", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Invalid action")
    );
}

#[test]
fn tool_container_action_container_id_with_dots_and_hyphens() {
    // Valid container IDs can have dots, hyphens, underscores
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "web-1", "container_id": "my-container_v1.2", "action": "start"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "container_action", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    // Should NOT error on validation - container_id is valid
    // Will proceed to alias check and SSH (which may fail), but no validation error
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(!text.contains("invalid character"));
}

#[test]
fn tool_container_action_container_id_with_spaces() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args =
        serde_json::json!({"alias": "web-1", "container_id": "my container", "action": "start"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "container_action", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("invalid character")
    );
}

#[test]
fn tool_list_containers_missing_empty_alias() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": ""});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_containers", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
}

#[test]
fn tool_list_containers_nonexistent_alias() {
    let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
    let args = serde_json::json!({"alias": "nonexistent"});
    let resp = dispatch(
        "tools/call",
        Some(serde_json::json!({"name": "list_containers", "arguments": args})),
        &config_path,
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap());
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("not found")
    );
}

// --- initialize and tools/list output tests ---

#[test]
fn initialize_contains_version() {
    let resp = dispatch("initialize", None, &std::path::PathBuf::from("/dev/null"));
    let result = resp.result.unwrap();
    assert!(!result["serverInfo"]["version"].as_str().unwrap().is_empty());
}

#[test]
fn tools_list_schema_has_required_fields() {
    let resp = dispatch("tools/list", None, &std::path::PathBuf::from("/dev/null"));
    let result = resp.result.unwrap();
    let tools = result["tools"].as_array().unwrap();
    for tool in tools {
        assert!(tool["name"].is_string(), "Tool missing name");
        assert!(tool["description"].is_string(), "Tool missing description");
        assert!(tool["inputSchema"].is_object(), "Tool missing inputSchema");
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
}
