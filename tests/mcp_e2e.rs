use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn send_and_receive(
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
    request: &serde_json::Value,
) -> serde_json::Value {
    let json = serde_json::to_string(request).unwrap();
    writeln!(stdin, "{json}").unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    serde_json::from_str(line.trim()).unwrap()
}

fn send_notification(stdin: &mut impl Write, request: &serde_json::Value) {
    let json = serde_json::to_string(request).unwrap();
    writeln!(stdin, "{json}").unwrap();
    stdin.flush().unwrap();
}

#[test]
fn mcp_e2e_full_session() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_purple"))
        .args(["--config", "tests/fixtures/mcp_test_config", "mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start purple mcp");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    // 1. Initialize
    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1.0"}
            }
        }),
    );
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(resp["result"]["serverInfo"]["name"], "purple");

    // 2. Initialized notification (no response expected)
    send_notification(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    );

    // 3. List tools
    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }),
    );
    assert_eq!(resp["id"], 2);
    let tools = resp["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);

    // 4. Call list_hosts
    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "list_hosts",
                "arguments": {}
            }
        }),
    );
    assert_eq!(resp["id"], 3);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<serde_json::Value> = serde_json::from_str(text).unwrap();
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0]["alias"], "web-1");

    // 5. Call get_host
    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "get_host",
                "arguments": {"alias": "web-1"}
            }
        }),
    );
    assert_eq!(resp["id"], 4);
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let host: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(host["hostname"], "10.0.1.5");
    assert_eq!(host["provider"], "aws");

    // 6. Unknown method
    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "bogus/method"
        }),
    );
    assert_eq!(resp["id"], 5);
    assert_eq!(resp["error"]["code"], -32601);

    // Close stdin to signal EOF
    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
}

// Regression for the .mcpb-on-Claude-Desktop bug: Claude Desktop did not
// substitute `${HOME}` in mcp_config.args before spawning purple, so purple
// got `${HOME}/.ssh/config` literally and silently returned an empty host
// list. The unit tests for `expand_user_path` cover the function in isolation;
// this test covers the full subprocess chain that mirrors how the .mcpb
// bundle launches in production.
//
// Unix-only: on Windows `dirs::home_dir()` uses SHGetKnownFolderPath, which
// ignores HOME/USERPROFILE env overrides, so the subprocess always reads
// the real user profile and the test cannot redirect it to a tempdir.
#[cfg(unix)]
#[test]
fn mcp_subprocess_expands_literal_home_in_args() {
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let home = tmp.path();
    let ssh_dir = home.join(".ssh");
    std::fs::create_dir_all(&ssh_dir).unwrap();
    std::fs::write(
        ssh_dir.join("config"),
        "Host smoke-${HOME}-test\n  HostName 10.99.0.1\n  User testuser\n",
    )
    .unwrap();

    let audit_log = home.join(".purple").join("mcp-audit.log");

    let mut child = Command::new(env!("CARGO_BIN_EXE_purple"))
        .env("HOME", home)
        .args([
            "--config",
            "${HOME}/.ssh/config",
            "mcp",
            "--read-only",
            "--audit-log",
            "${HOME}/.purple/mcp-audit.log",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start purple mcp");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1.0"}
            }
        }),
    );
    assert_eq!(resp["result"]["serverInfo"]["name"], "purple");

    send_notification(
        &mut stdin,
        &serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    );

    let resp = send_and_receive(
        &mut stdin,
        &mut stdout,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "list_hosts", "arguments": {}}
        }),
    );

    let result = &resp["result"];
    assert_ne!(
        result["isError"].as_bool(),
        Some(true),
        "literal ${{HOME}} arg must expand to real home; got error: {result}"
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<serde_json::Value> = serde_json::from_str(text).unwrap();
    assert_eq!(
        hosts.len(),
        1,
        "expected the single seeded host, got: {text}"
    );
    assert_eq!(hosts[0]["hostname"], "10.99.0.1");

    drop(stdin);
    child.wait().unwrap();

    assert!(
        audit_log.exists(),
        "audit log path with literal ${{HOME}} must expand and write to: {}",
        audit_log.display()
    );
}
