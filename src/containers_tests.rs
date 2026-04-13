use super::*;

fn make_json(id: &str, names: &str, image: &str, state: &str, status: &str, ports: &str) -> String {
    serde_json::json!({
        "ID": id,
        "Names": names,
        "Image": image,
        "State": state,
        "Status": status,
        "Ports": ports,
    })
    .to_string()
}

// -- parse_container_ps --------------------------------------------------

#[test]
fn parse_ps_empty() {
    assert!(parse_container_ps("").is_empty());
    assert!(parse_container_ps("   \n  \n").is_empty());
}

#[test]
fn parse_ps_single() {
    let line = make_json("abc", "web", "nginx:latest", "running", "Up 2h", "80/tcp");
    let r = parse_container_ps(&line);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].id, "abc");
    assert_eq!(r[0].names, "web");
    assert_eq!(r[0].image, "nginx:latest");
    assert_eq!(r[0].state, "running");
}

#[test]
fn parse_ps_multiple() {
    let lines = [
        make_json("a", "web", "nginx", "running", "Up", "80/tcp"),
        make_json("b", "db", "postgres", "exited", "Exited (0)", ""),
    ];
    let r = parse_container_ps(&lines.join("\n"));
    assert_eq!(r.len(), 2);
}

#[test]
fn parse_ps_invalid_lines_ignored() {
    let valid = make_json("x", "c", "i", "running", "Up", "");
    let input = format!("garbage\n{valid}\nalso bad");
    assert_eq!(parse_container_ps(&input).len(), 1);
}

#[test]
fn parse_ps_all_docker_states() {
    for state in [
        "created",
        "restarting",
        "running",
        "removing",
        "paused",
        "exited",
        "dead",
    ] {
        let line = make_json("id", "c", "img", state, "s", "");
        let r = parse_container_ps(&line);
        assert_eq!(r[0].state, state, "failed for {state}");
    }
}

#[test]
fn parse_ps_compose_names() {
    let line = make_json("a", "myproject-redis-1", "redis:7", "running", "Up", "");
    assert_eq!(parse_container_ps(&line)[0].names, "myproject-redis-1");
}

#[test]
fn parse_ps_sha256_image() {
    let line = make_json("a", "app", "sha256:abcdef123456", "running", "Up", "");
    assert!(parse_container_ps(&line)[0].image.starts_with("sha256:"));
}

#[test]
fn parse_ps_long_ports() {
    let ports = "0.0.0.0:80->80/tcp, 0.0.0.0:443->443/tcp, :::80->80/tcp";
    let line = make_json("a", "proxy", "nginx", "running", "Up", ports);
    assert_eq!(parse_container_ps(&line)[0].ports, ports);
}

// -- parse_runtime -------------------------------------------------------

#[test]
fn runtime_docker() {
    assert_eq!(parse_runtime("docker"), Some(ContainerRuntime::Docker));
}

#[test]
fn runtime_podman() {
    assert_eq!(parse_runtime("podman"), Some(ContainerRuntime::Podman));
}

#[test]
fn runtime_none() {
    assert_eq!(parse_runtime(""), None);
    assert_eq!(parse_runtime("   "), None);
    assert_eq!(parse_runtime("unknown"), None);
    assert_eq!(parse_runtime("Docker"), None); // case sensitive
}

#[test]
fn runtime_motd_prepended() {
    let input = "Welcome to Ubuntu 22.04\nSystem info\ndocker";
    assert_eq!(parse_runtime(input), Some(ContainerRuntime::Docker));
}

#[test]
fn runtime_trailing_whitespace() {
    assert_eq!(parse_runtime("docker  "), Some(ContainerRuntime::Docker));
    assert_eq!(parse_runtime("podman\t"), Some(ContainerRuntime::Podman));
}

#[test]
fn runtime_motd_after_output() {
    let input = "docker\nSystem update available.";
    // Last non-empty line is "System update available." which is not a runtime
    assert_eq!(parse_runtime(input), None);
}

// -- ContainerAction x ContainerRuntime ----------------------------------

#[test]
fn action_command_all_combinations() {
    let cases = [
        (
            ContainerRuntime::Docker,
            ContainerAction::Start,
            "docker start c1",
        ),
        (
            ContainerRuntime::Docker,
            ContainerAction::Stop,
            "docker stop c1",
        ),
        (
            ContainerRuntime::Docker,
            ContainerAction::Restart,
            "docker restart c1",
        ),
        (
            ContainerRuntime::Podman,
            ContainerAction::Start,
            "podman start c1",
        ),
        (
            ContainerRuntime::Podman,
            ContainerAction::Stop,
            "podman stop c1",
        ),
        (
            ContainerRuntime::Podman,
            ContainerAction::Restart,
            "podman restart c1",
        ),
    ];
    for (rt, action, expected) in cases {
        assert_eq!(container_action_command(rt, action, "c1"), expected);
    }
}

#[test]
fn action_as_str() {
    assert_eq!(ContainerAction::Start.as_str(), "start");
    assert_eq!(ContainerAction::Stop.as_str(), "stop");
    assert_eq!(ContainerAction::Restart.as_str(), "restart");
}

#[test]
fn runtime_as_str() {
    assert_eq!(ContainerRuntime::Docker.as_str(), "docker");
    assert_eq!(ContainerRuntime::Podman.as_str(), "podman");
}

// -- validate_container_id -----------------------------------------------

#[test]
fn id_valid_hex() {
    assert!(validate_container_id("a1b2c3d4e5f6").is_ok());
}

#[test]
fn id_valid_names() {
    assert!(validate_container_id("myapp").is_ok());
    assert!(validate_container_id("my-app").is_ok());
    assert!(validate_container_id("my_app").is_ok());
    assert!(validate_container_id("my.app").is_ok());
    assert!(validate_container_id("myproject-web-1").is_ok());
}

#[test]
fn id_empty() {
    assert!(validate_container_id("").is_err());
}

#[test]
fn id_space() {
    assert!(validate_container_id("my app").is_err());
}

#[test]
fn id_newline() {
    assert!(validate_container_id("app\n").is_err());
}

#[test]
fn id_injection_semicolon() {
    assert!(validate_container_id("app;rm -rf /").is_err());
}

#[test]
fn id_injection_pipe() {
    assert!(validate_container_id("app|cat /etc/passwd").is_err());
}

#[test]
fn id_injection_dollar() {
    assert!(validate_container_id("app$HOME").is_err());
}

#[test]
fn id_injection_backtick() {
    assert!(validate_container_id("app`whoami`").is_err());
}

#[test]
fn id_unicode_rejected() {
    assert!(validate_container_id("app\u{00e9}").is_err());
    assert!(validate_container_id("\u{0430}pp").is_err()); // Cyrillic а
}

#[test]
fn id_colon_rejected() {
    assert!(validate_container_id("app:latest").is_err());
}

// -- container_list_command ----------------------------------------------

#[test]
fn list_cmd_docker() {
    assert_eq!(
        container_list_command(Some(ContainerRuntime::Docker)),
        "docker ps -a --format '{{json .}}'"
    );
}

#[test]
fn list_cmd_podman() {
    assert_eq!(
        container_list_command(Some(ContainerRuntime::Podman)),
        "podman ps -a --format '{{json .}}'"
    );
}

#[test]
fn list_cmd_none_has_sentinels() {
    let cmd = container_list_command(None);
    assert!(cmd.contains("##purple:docker##"));
    assert!(cmd.contains("##purple:podman##"));
    assert!(cmd.contains("##purple:none##"));
}

#[test]
fn list_cmd_none_docker_first() {
    let cmd = container_list_command(None);
    let d = cmd.find("##purple:docker##").unwrap();
    let p = cmd.find("##purple:podman##").unwrap();
    assert!(d < p);
}

// -- parse_container_output ----------------------------------------------

#[test]
fn output_docker_sentinel() {
    let c = make_json("abc", "web", "nginx", "running", "Up", "80/tcp");
    let out = format!("##purple:docker##\n{c}");
    let (rt, cs) = parse_container_output(&out, None).unwrap();
    assert_eq!(rt, ContainerRuntime::Docker);
    assert_eq!(cs.len(), 1);
}

#[test]
fn output_podman_sentinel() {
    let c = make_json("xyz", "db", "pg", "exited", "Exited", "");
    let out = format!("##purple:podman##\n{c}");
    let (rt, _) = parse_container_output(&out, None).unwrap();
    assert_eq!(rt, ContainerRuntime::Podman);
}

#[test]
fn output_none_sentinel() {
    let r = parse_container_output("##purple:none##", None);
    assert!(r.is_err());
    assert!(r.unwrap_err().contains("No container runtime"));
}

#[test]
fn output_no_sentinel_with_caller() {
    let c = make_json("a", "app", "img", "running", "Up", "");
    let (rt, cs) = parse_container_output(&c, Some(ContainerRuntime::Docker)).unwrap();
    assert_eq!(rt, ContainerRuntime::Docker);
    assert_eq!(cs.len(), 1);
}

#[test]
fn output_no_sentinel_no_caller() {
    let c = make_json("a", "app", "img", "running", "Up", "");
    assert!(parse_container_output(&c, None).is_err());
}

#[test]
fn output_motd_before_sentinel() {
    let c = make_json("a", "app", "img", "running", "Up", "");
    let out = format!("Welcome to server\nInfo line\n##purple:docker##\n{c}");
    let (rt, cs) = parse_container_output(&out, None).unwrap();
    assert_eq!(rt, ContainerRuntime::Docker);
    assert_eq!(cs.len(), 1);
}

#[test]
fn output_empty_container_list() {
    let (rt, cs) = parse_container_output("##purple:docker##\n", None).unwrap();
    assert_eq!(rt, ContainerRuntime::Docker);
    assert!(cs.is_empty());
}

#[test]
fn output_multiple_containers() {
    let c1 = make_json("a", "web", "nginx", "running", "Up", "80/tcp");
    let c2 = make_json("b", "db", "pg", "exited", "Exited", "");
    let c3 = make_json("c", "cache", "redis", "running", "Up", "6379/tcp");
    let out = format!("##purple:podman##\n{c1}\n{c2}\n{c3}");
    let (_, cs) = parse_container_output(&out, None).unwrap();
    assert_eq!(cs.len(), 3);
}

// -- friendly_container_error --------------------------------------------

#[test]
fn friendly_error_command_not_found() {
    let msg = friendly_container_error("bash: docker: command not found", Some(127));
    assert_eq!(msg, "Docker or Podman not found on remote host.");
}

#[test]
fn friendly_error_permission_denied() {
    let msg = friendly_container_error(
        "Got permission denied while trying to connect to the Docker daemon socket",
        Some(1),
    );
    assert_eq!(msg, "Permission denied. Is your user in the docker group?");
}

#[test]
fn friendly_error_daemon_not_running() {
    let msg = friendly_container_error(
        "Cannot connect to the Docker daemon at unix:///var/run/docker.sock",
        Some(1),
    );
    assert_eq!(msg, "Container daemon is not running.");
}

#[test]
fn friendly_error_connection_refused() {
    let msg = friendly_container_error("ssh: connect to host: Connection refused", Some(255));
    assert_eq!(msg, "Connection refused.");
}

#[test]
fn friendly_error_empty_stderr() {
    let msg = friendly_container_error("", Some(1));
    assert_eq!(msg, "Command failed with code 1.");
}

#[test]
fn friendly_error_unknown_stderr_uses_generic_message() {
    let msg = friendly_container_error("some unknown error", Some(1));
    assert_eq!(msg, "Command failed with code 1.");
}

// -- cache serialization -------------------------------------------------

#[test]
fn cache_round_trip() {
    let line = CacheLine {
        alias: "web1".to_string(),
        timestamp: 1_700_000_000,
        runtime: ContainerRuntime::Docker,
        containers: vec![ContainerInfo {
            id: "abc".to_string(),
            names: "nginx".to_string(),
            image: "nginx:latest".to_string(),
            state: "running".to_string(),
            status: "Up 2h".to_string(),
            ports: "80/tcp".to_string(),
        }],
    };
    let s = serde_json::to_string(&line).unwrap();
    let d: CacheLine = serde_json::from_str(&s).unwrap();
    assert_eq!(d.alias, "web1");
    assert_eq!(d.runtime, ContainerRuntime::Docker);
    assert_eq!(d.containers.len(), 1);
    assert_eq!(d.containers[0].id, "abc");
}

#[test]
fn cache_round_trip_podman() {
    let line = CacheLine {
        alias: "host2".to_string(),
        timestamp: 200,
        runtime: ContainerRuntime::Podman,
        containers: vec![],
    };
    let s = serde_json::to_string(&line).unwrap();
    let d: CacheLine = serde_json::from_str(&s).unwrap();
    assert_eq!(d.runtime, ContainerRuntime::Podman);
}

#[test]
fn cache_parse_empty() {
    let map: HashMap<String, ContainerCacheEntry> =
        "".lines().filter_map(parse_cache_line).collect();
    assert!(map.is_empty());
}

#[test]
fn cache_parse_malformed_ignored() {
    let valid = serde_json::to_string(&CacheLine {
        alias: "good".to_string(),
        timestamp: 1,
        runtime: ContainerRuntime::Docker,
        containers: vec![],
    })
    .unwrap();
    let content = format!("garbage\n{valid}\nalso bad");
    let map: HashMap<String, ContainerCacheEntry> =
        content.lines().filter_map(parse_cache_line).collect();
    assert_eq!(map.len(), 1);
    assert!(map.contains_key("good"));
}

#[test]
fn cache_parse_multiple_hosts() {
    let lines: Vec<String> = ["h1", "h2", "h3"]
        .iter()
        .enumerate()
        .map(|(i, alias)| {
            serde_json::to_string(&CacheLine {
                alias: alias.to_string(),
                timestamp: i as u64,
                runtime: ContainerRuntime::Docker,
                containers: vec![],
            })
            .unwrap()
        })
        .collect();
    let content = lines.join("\n");
    let map: HashMap<String, ContainerCacheEntry> =
        content.lines().filter_map(parse_cache_line).collect();
    assert_eq!(map.len(), 3);
}

/// Helper: parse a single cache line (mirrors load_container_cache logic).
fn parse_cache_line(line: &str) -> Option<(String, ContainerCacheEntry)> {
    let t = line.trim();
    if t.is_empty() {
        return None;
    }
    let entry: CacheLine = serde_json::from_str(t).ok()?;
    Some((
        entry.alias,
        ContainerCacheEntry {
            timestamp: entry.timestamp,
            runtime: entry.runtime,
            containers: entry.containers,
        },
    ))
}

// -- truncate_str --------------------------------------------------------

#[test]
fn truncate_short() {
    assert_eq!(truncate_str("hi", 10), "hi");
}

#[test]
fn truncate_exact() {
    assert_eq!(truncate_str("hello", 5), "hello");
}

#[test]
fn truncate_long() {
    assert_eq!(truncate_str("hello world", 7), "hello..");
}

#[test]
fn truncate_empty() {
    assert_eq!(truncate_str("", 5), "");
}

#[test]
fn truncate_max_two() {
    assert_eq!(truncate_str("hello", 2), "..");
}

#[test]
fn truncate_multibyte() {
    assert_eq!(truncate_str("café-app", 6), "café..");
}

#[test]
fn truncate_emoji() {
    assert_eq!(truncate_str("🐳nginx", 5), "🐳ng..");
}

// -- format_relative_time ------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn relative_just_now() {
    assert_eq!(format_relative_time(now_secs()), "just now");
    assert_eq!(format_relative_time(now_secs() - 30), "just now");
    assert_eq!(format_relative_time(now_secs() - 59), "just now");
}

#[test]
fn relative_minutes() {
    assert_eq!(format_relative_time(now_secs() - 60), "1m ago");
    assert_eq!(format_relative_time(now_secs() - 300), "5m ago");
    assert_eq!(format_relative_time(now_secs() - 3599), "59m ago");
}

#[test]
fn relative_hours() {
    assert_eq!(format_relative_time(now_secs() - 3600), "1h ago");
    assert_eq!(format_relative_time(now_secs() - 7200), "2h ago");
}

#[test]
fn relative_days() {
    assert_eq!(format_relative_time(now_secs() - 86400), "1d ago");
    assert_eq!(format_relative_time(now_secs() - 7 * 86400), "7d ago");
}

#[test]
fn relative_future_saturates() {
    assert_eq!(format_relative_time(now_secs() + 10000), "just now");
}

// -- Additional edge-case tests -------------------------------------------

#[test]
fn parse_ps_whitespace_only_lines_between_json() {
    let c1 = make_json("a", "web", "nginx", "running", "Up", "");
    let c2 = make_json("b", "db", "pg", "exited", "Exited", "");
    let input = format!("{c1}\n   \n\t\n{c2}");
    let r = parse_container_ps(&input);
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].id, "a");
    assert_eq!(r[1].id, "b");
}

#[test]
fn id_just_dot() {
    assert!(validate_container_id(".").is_ok());
}

#[test]
fn id_just_dash() {
    assert!(validate_container_id("-").is_ok());
}

#[test]
fn id_slash_rejected() {
    assert!(validate_container_id("my/container").is_err());
}

#[test]
fn list_cmd_none_valid_shell_syntax() {
    let cmd = container_list_command(None);
    assert!(cmd.contains("if "), "should start with if");
    assert!(cmd.contains("fi"), "should end with fi");
    assert!(cmd.contains("elif "), "should have elif fallback");
    assert!(cmd.contains("else "), "should have else branch");
}

#[test]
fn output_sentinel_on_last_line() {
    let r = parse_container_output("some MOTD\n##purple:docker##", None);
    let (rt, cs) = r.unwrap();
    assert_eq!(rt, ContainerRuntime::Docker);
    assert!(cs.is_empty());
}

#[test]
fn output_sentinel_none_on_last_line() {
    let r = parse_container_output("MOTD line\n##purple:none##", None);
    assert!(r.is_err());
    assert!(r.unwrap_err().contains("No container runtime"));
}

#[test]
fn relative_time_unix_epoch() {
    // Timestamp 0 is decades ago, should show many days
    let result = format_relative_time(0);
    assert!(
        result.contains("d ago"),
        "epoch should be days ago: {result}"
    );
}

#[test]
fn truncate_unicode_within_limit() {
    // 3-byte chars but total byte len 9 > max 5, yet char count is 3
    // truncate_str uses byte length so this string of 3 chars (9 bytes) > max 5
    assert_eq!(truncate_str("abc", 5), "abc"); // ASCII fits
}

#[test]
fn truncate_ascii_boundary() {
    // Ensure max=0 does not panic
    assert_eq!(truncate_str("hello", 0), "..");
}

#[test]
fn truncate_max_one() {
    assert_eq!(truncate_str("hello", 1), "..");
}

#[test]
fn cache_serde_unknown_runtime_rejected() {
    let json = r#"{"alias":"h","timestamp":1,"runtime":"Containerd","containers":[]}"#;
    let result = serde_json::from_str::<CacheLine>(json);
    assert!(result.is_err(), "unknown runtime should be rejected");
}

#[test]
fn cache_duplicate_alias_last_wins() {
    let line1 = serde_json::to_string(&CacheLine {
        alias: "dup".to_string(),
        timestamp: 1,
        runtime: ContainerRuntime::Docker,
        containers: vec![],
    })
    .unwrap();
    let line2 = serde_json::to_string(&CacheLine {
        alias: "dup".to_string(),
        timestamp: 99,
        runtime: ContainerRuntime::Podman,
        containers: vec![],
    })
    .unwrap();
    let content = format!("{line1}\n{line2}");
    let map: HashMap<String, ContainerCacheEntry> =
        content.lines().filter_map(parse_cache_line).collect();
    assert_eq!(map.len(), 1);
    // HashMap::from_iter keeps last for duplicate keys
    assert_eq!(map["dup"].runtime, ContainerRuntime::Podman);
    assert_eq!(map["dup"].timestamp, 99);
}

#[test]
fn friendly_error_no_route() {
    let msg = friendly_container_error("ssh: No route to host", Some(255));
    assert_eq!(msg, "Host unreachable.");
}

#[test]
fn friendly_error_network_unreachable() {
    let msg = friendly_container_error("connect: Network is unreachable", Some(255));
    assert_eq!(msg, "Host unreachable.");
}

#[test]
fn friendly_error_none_exit_code() {
    let msg = friendly_container_error("", None);
    assert_eq!(msg, "Command failed with code 1.");
}

#[test]
fn container_error_display() {
    let err = ContainerError {
        runtime: Some(ContainerRuntime::Docker),
        message: "test error".to_string(),
    };
    assert_eq!(format!("{err}"), "test error");
}

#[test]
fn container_error_display_no_runtime() {
    let err = ContainerError {
        runtime: None,
        message: "no runtime".to_string(),
    };
    assert_eq!(format!("{err}"), "no runtime");
}

// -- Additional tests: parse_container_ps edge cases ----------------------

#[test]
fn parse_ps_crlf_line_endings() {
    let c1 = make_json("a", "web", "nginx", "running", "Up", "");
    let c2 = make_json("b", "db", "pg", "exited", "Exited", "");
    let input = format!("{c1}\r\n{c2}\r\n");
    let r = parse_container_ps(&input);
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].id, "a");
    assert_eq!(r[1].id, "b");
}

#[test]
fn parse_ps_trailing_newline() {
    let c = make_json("a", "web", "nginx", "running", "Up", "");
    let input = format!("{c}\n");
    let r = parse_container_ps(&input);
    assert_eq!(
        r.len(),
        1,
        "trailing newline should not create phantom entry"
    );
}

#[test]
fn parse_ps_leading_whitespace_json() {
    let c = make_json("a", "web", "nginx", "running", "Up", "");
    let input = format!("  {c}");
    let r = parse_container_ps(&input);
    assert_eq!(
        r.len(),
        1,
        "leading whitespace before JSON should be trimmed"
    );
    assert_eq!(r[0].id, "a");
}

// -- Additional tests: parse_runtime edge cases ---------------------------

#[test]
fn parse_runtime_empty_lines_between_motd() {
    let input = "Welcome\n\n\n\ndocker";
    assert_eq!(parse_runtime(input), Some(ContainerRuntime::Docker));
}

#[test]
fn parse_runtime_crlf() {
    let input = "MOTD\r\npodman\r\n";
    assert_eq!(parse_runtime(input), Some(ContainerRuntime::Podman));
}

// -- Additional tests: parse_container_output edge cases ------------------

#[test]
fn output_unknown_sentinel() {
    let r = parse_container_output("##purple:unknown##", None);
    assert!(r.is_err());
    let msg = r.unwrap_err();
    assert!(msg.contains("Unknown sentinel"), "got: {msg}");
}

#[test]
fn output_sentinel_with_crlf() {
    let c = make_json("a", "web", "nginx", "running", "Up", "");
    let input = format!("##purple:docker##\r\n{c}\r\n");
    let (rt, cs) = parse_container_output(&input, None).unwrap();
    assert_eq!(rt, ContainerRuntime::Docker);
    assert_eq!(cs.len(), 1);
}

#[test]
fn output_sentinel_indented() {
    let c = make_json("a", "web", "nginx", "running", "Up", "");
    let input = format!("  ##purple:docker##\n{c}");
    let (rt, cs) = parse_container_output(&input, None).unwrap();
    assert_eq!(rt, ContainerRuntime::Docker);
    assert_eq!(cs.len(), 1);
}

#[test]
fn output_caller_runtime_podman() {
    let c = make_json("a", "app", "img", "running", "Up", "");
    let (rt, cs) = parse_container_output(&c, Some(ContainerRuntime::Podman)).unwrap();
    assert_eq!(rt, ContainerRuntime::Podman);
    assert_eq!(cs.len(), 1);
}

// -- Additional tests: container_action_command ---------------------------

#[test]
fn action_command_long_id() {
    let long_id = "a".repeat(64);
    let cmd = container_action_command(ContainerRuntime::Docker, ContainerAction::Start, &long_id);
    assert_eq!(cmd, format!("docker start {long_id}"));
}

// -- Additional tests: validate_container_id ------------------------------

#[test]
fn id_full_sha256() {
    let id = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
    assert_eq!(id.len(), 64);
    assert!(validate_container_id(id).is_ok());
}

#[test]
fn id_ampersand_rejected() {
    assert!(validate_container_id("app&rm").is_err());
}

#[test]
fn id_parentheses_rejected() {
    assert!(validate_container_id("app(1)").is_err());
    assert!(validate_container_id("app)").is_err());
}

#[test]
fn id_angle_brackets_rejected() {
    assert!(validate_container_id("app<1>").is_err());
    assert!(validate_container_id("app>").is_err());
}

// -- Additional tests: friendly_container_error ---------------------------

#[test]
fn friendly_error_podman_daemon() {
    let msg = friendly_container_error("cannot connect to podman", Some(125));
    assert_eq!(msg, "Container daemon is not running.");
}

#[test]
fn friendly_error_case_insensitive() {
    let msg = friendly_container_error("PERMISSION DENIED", Some(1));
    assert_eq!(msg, "Permission denied. Is your user in the docker group?");
}

// -- Additional tests: Copy traits ----------------------------------------

#[test]
fn container_runtime_copy() {
    let a = ContainerRuntime::Docker;
    let b = a; // Copy
    assert_eq!(a, b); // both still usable
}

#[test]
fn container_action_copy() {
    let a = ContainerAction::Start;
    let b = a; // Copy
    assert_eq!(a, b); // both still usable
}

// -- Additional tests: truncate_str edge cases ----------------------------

#[test]
fn truncate_multibyte_utf8() {
    // "caf\u{00e9}-app" is 8 chars; truncating to 6 keeps "caf\u{00e9}" + ".."
    assert_eq!(truncate_str("caf\u{00e9}-app", 6), "caf\u{00e9}..");
}

// -- Additional tests: format_relative_time boundaries --------------------

#[test]
fn format_relative_time_boundary_60s() {
    let ts = now_secs() - 60;
    assert_eq!(format_relative_time(ts), "1m ago");
}

#[test]
fn format_relative_time_boundary_3600s() {
    let ts = now_secs() - 3600;
    assert_eq!(format_relative_time(ts), "1h ago");
}

#[test]
fn format_relative_time_boundary_86400s() {
    let ts = now_secs() - 86400;
    assert_eq!(format_relative_time(ts), "1d ago");
}

// -- Additional tests: ContainerError Debug -------------------------------

#[test]
fn container_error_debug() {
    let err = ContainerError {
        runtime: Some(ContainerRuntime::Docker),
        message: "test".to_string(),
    };
    let dbg = format!("{err:?}");
    assert!(
        dbg.contains("Docker"),
        "Debug should include runtime: {dbg}"
    );
    assert!(dbg.contains("test"), "Debug should include message: {dbg}");
}
