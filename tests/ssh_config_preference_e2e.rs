//! End-to-end tests for choosing the SSH config path.
//!
//! Spawns the real binary against a temp HOME. Unlike the XDG fixture, `run`
//! passes no `--config`, so the binary sees only the `ssh_config` preference
//! and `PURPLE_SSH_CONFIG`. Tests that need the flag pass it as an argument.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const NOT_FOUND: &str = "SSH config not found at";

fn purple_bin() -> &'static str {
    env!("CARGO_BIN_EXE_purple")
}

struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    pref_config: PathBuf,
    env_config: PathBuf,
    flag_config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("purple_ssh_config_")
            .tempdir()
            .unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join(".purple")).unwrap();
        let write_config = |name: &str, alias: &str| {
            let path = root.path().join(name);
            std::fs::write(
                &path,
                format!("Host {alias}\n    HostName {alias}.example.com\n"),
            )
            .unwrap();
            path
        };
        let pref_config = write_config("pref_config", "from-pref");
        let env_config = write_config("env_config", "from-env");
        let flag_config = write_config("flag_config", "from-flag");
        Self {
            _root: root,
            home,
            pref_config,
            env_config,
            flag_config,
        }
    }

    fn set_preference(&self, value: &str) {
        std::fs::write(
            self.home.join(".purple/preferences"),
            format!("ssh_config={value}\n"),
        )
        .unwrap();
    }

    fn command(&self, env: &[(&str, &Path)]) -> Command {
        let mut cmd = Command::new(purple_bin());
        cmd.env_clear()
            .env("HOME", &self.home)
            .env("PATH", std::env::var("PATH").unwrap_or_default());
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd
    }

    /// Run purple with the given extra environment and arguments.
    fn run(&self, env: &[(&str, &Path)], args: &[&str]) -> Output {
        self.command(env)
            .args(args)
            .output()
            .expect("failed to spawn purple binary")
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn e2e_preference_is_honored() {
    let fixture = Fixture::new();
    fixture.set_preference(&fixture.pref_config.display().to_string());
    let output = fixture.run(&[], &["--list"]);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).contains("from-pref"), "{output:?}");
}

#[test]
fn e2e_env_var_wins_over_preference() {
    let fixture = Fixture::new();
    fixture.set_preference(&fixture.pref_config.display().to_string());
    let output = fixture.run(&[("PURPLE_SSH_CONFIG", &fixture.env_config)], &["--list"]);
    assert!(output.status.success(), "{output:?}");
    let listed = stdout(&output);
    assert!(listed.contains("from-env"), "{output:?}");
    assert!(!listed.contains("from-pref"), "{output:?}");
}

#[test]
fn e2e_flag_wins_over_env_var_and_preference() {
    let fixture = Fixture::new();
    fixture.set_preference(&fixture.pref_config.display().to_string());
    let flag = fixture.flag_config.display().to_string();
    let output = fixture.run(
        &[("PURPLE_SSH_CONFIG", &fixture.env_config)],
        &["--config", &flag, "--list"],
    );
    assert!(output.status.success(), "{output:?}");
    let listed = stdout(&output);
    assert!(listed.contains("from-flag"), "{output:?}");
    assert!(!listed.contains("from-env"), "{output:?}");
    assert!(!listed.contains("from-pref"), "{output:?}");
}

#[test]
fn e2e_preference_expands_tilde() {
    let fixture = Fixture::new();
    std::fs::copy(&fixture.pref_config, fixture.home.join("alt")).unwrap();
    fixture.set_preference("~/alt");
    let output = fixture.run(&[], &["--list"]);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).contains("from-pref"), "{output:?}");
}

#[test]
fn e2e_missing_preference_path_prints_one_notice() {
    let fixture = Fixture::new();
    let missing = fixture.home.join("nope");
    fixture.set_preference(&missing.display().to_string());
    let output = fixture.run(&[], &["--list"]);
    assert!(output.status.success(), "{output:?}");
    let err = stderr(&output);
    assert_eq!(err.matches(NOT_FOUND).count(), 1, "notice once: {err}");
    assert!(err.contains(&missing.display().to_string()), "{err}");
    assert!(err.contains("(from preference ssh_config)"), "{err}");
    assert!(
        stdout(&output).contains("No hosts configured"),
        "{output:?}"
    );
}

#[test]
fn e2e_missing_default_config_stays_silent() {
    let fixture = Fixture::new();
    let output = fixture.run(&[], &["--list"]);
    assert!(output.status.success(), "{output:?}");
    assert!(!stderr(&output).contains(NOT_FOUND), "{output:?}");
}

#[test]
fn e2e_debug_log_names_the_source() {
    let fixture = Fixture::new();
    fixture.set_preference(&fixture.pref_config.display().to_string());
    let output = fixture.run(&[("PURPLE_LOG", Path::new("debug"))], &["--list"]);
    assert!(output.status.success(), "{output:?}");
    let log = std::fs::read_to_string(fixture.home.join(".purple/purple.log")).unwrap_or_default();
    assert!(
        log.contains(&format!(
            "[purple] ssh config from preference ssh_config: {}",
            fixture.pref_config.display()
        )),
        "source line missing from log:\n{log}"
    );
}

#[test]
fn e2e_config_flag_after_subcommand() {
    let fixture = Fixture::new();
    std::fs::write(
        &fixture.flag_config,
        "Host fwd\n    HostName 10.0.0.1\n    LocalForward 8080 localhost:80\n",
    )
    .unwrap();
    let flag = fixture.flag_config.display().to_string();
    let output = fixture.run(&[], &["tunnel", "list", "--config", &flag]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        !stderr(&output).contains("unexpected argument"),
        "{output:?}"
    );
    let listed = stdout(&output);
    assert!(listed.contains("fwd"), "{output:?}");
    assert!(listed.contains("8080"), "{output:?}");
}

// The startup notice promises the file appears on the first add, also when
// its directory does not exist yet.
#[test]
fn e2e_add_creates_the_configured_file_and_its_directory() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let target = fixture.home.join("newdir/sub/purple_config");
    fixture.set_preference(&target.display().to_string());
    let output = fixture.run(&[], &["add", "root@h.example:22", "--alias", "h"]);
    assert!(output.status.success(), "{output:?}");
    let written = std::fs::read_to_string(&target).expect("config created");
    assert!(written.contains("Host h"), "{written}");
    let mode = std::fs::metadata(fixture.home.join("newdir"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700, "created directory must be private");
}

// A relative Include in a config outside ~/.ssh reads from ~/.ssh, as ssh does.
#[test]
fn e2e_relative_include_resolves_from_home_ssh() {
    let fixture = Fixture::new();
    let ssh_dir = fixture.home.join(".ssh");
    std::fs::create_dir_all(&ssh_dir).unwrap();
    std::fs::write(
        ssh_dir.join("extra"),
        "Host from-ssh-dir\n    HostName 10.0.0.1\n",
    )
    .unwrap();
    let elsewhere = fixture.home.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(
        elsewhere.join("extra"),
        "Host from-config-dir\n    HostName 10.0.0.2\n",
    )
    .unwrap();
    std::fs::write(elsewhere.join("config"), "Include extra\n").unwrap();
    let config = elsewhere.join("config").display().to_string();
    let output = fixture.run(&[], &["--config", &config, "--list"]);
    assert!(output.status.success(), "{output:?}");
    let listed = stdout(&output);
    assert!(listed.contains("from-ssh-dir"), "{output:?}");
    assert!(!listed.contains("from-config-dir"), "{output:?}");
}

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

#[test]
fn e2e_mcp_server_reads_the_preference() {
    let fixture = Fixture::new();
    fixture.set_preference(&fixture.pref_config.display().to_string());
    let mut child = fixture
        .command(&[])
        .args(["mcp", "--no-audit"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start purple mcp");
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());

    let resp = send_and_receive(
        &mut stdin,
        &mut out,
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

    let resp = send_and_receive(
        &mut stdin,
        &mut out,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "list_hosts", "arguments": {}}
        }),
    );
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let hosts: Vec<serde_json::Value> = serde_json::from_str(text).unwrap();
    assert_eq!(hosts.len(), 1, "{text}");
    assert_eq!(hosts[0]["alias"], "from-pref");

    drop(stdin);
    assert!(child.wait().unwrap().success());
}
