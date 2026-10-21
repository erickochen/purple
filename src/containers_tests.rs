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

// -- parse_container_ps: podman format ----------------------------------
// Podman's `ps --format '{{json .}}'` differs from docker on three keys:
// `Id` (lowercase d), `Names` is an array, `Ports` is an array of port
// objects (or null). These tests pin the tolerant deserialization.

#[test]
fn parse_ps_podman_single_running() {
    let line = r#"{"Id":"826abc60f2e5","Names":["purple-test-nginx"],"Image":"docker.io/library/nginx:alpine","State":"running","Status":"","Ports":[{"host_ip":"","container_port":80,"host_port":8081,"range":1,"protocol":"tcp"}]}"#;
    let r = parse_container_ps(line);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].id, "826abc60f2e5");
    assert_eq!(r[0].names, "purple-test-nginx");
    assert_eq!(r[0].image, "docker.io/library/nginx:alpine");
    assert_eq!(r[0].state, "running");
    assert_eq!(r[0].status, "");
    // Empty host_ip on podman covers both IPv4 and IPv6 wildcard binds;
    // we omit the prefix rather than mis-claim IPv4.
    assert_eq!(r[0].ports, "8081->80/tcp");
}

#[test]
fn parse_ps_podman_names_array_multiple_joined() {
    let line = r#"{"Id":"x","Names":["primary","secondary","tertiary"],"Image":"img","State":"running","Status":""}"#;
    assert_eq!(
        parse_container_ps(line)[0].names,
        "primary,secondary,tertiary"
    );
}

#[test]
fn parse_ps_podman_names_empty_array() {
    let line = r#"{"Id":"x","Names":[],"Image":"img","State":"running","Status":""}"#;
    assert_eq!(parse_container_ps(line)[0].names, "");
}

#[test]
fn parse_ps_podman_ports_null() {
    let line =
        r#"{"Id":"x","Names":["n"],"Image":"img","State":"running","Status":"","Ports":null}"#;
    assert_eq!(parse_container_ps(line)[0].ports, "");
}

#[test]
fn parse_ps_podman_ports_field_absent() {
    let line = r#"{"Id":"x","Names":["n"],"Image":"img","State":"running","Status":""}"#;
    assert_eq!(parse_container_ps(line)[0].ports, "");
}

#[test]
fn parse_ps_podman_ports_empty_array() {
    let line = r#"{"Id":"x","Names":["n"],"Image":"img","State":"running","Status":"","Ports":[]}"#;
    assert_eq!(parse_container_ps(line)[0].ports, "");
}

#[test]
fn parse_ps_podman_ports_exposed_not_published() {
    // host_port == 0 => container-only exposure
    let line = r#"{"Id":"x","Names":["n"],"Image":"img","State":"running","Status":"","Ports":[{"host_ip":"","container_port":6379,"host_port":0,"range":1,"protocol":"tcp"}]}"#;
    assert_eq!(parse_container_ps(line)[0].ports, "6379/tcp");
}

#[test]
fn parse_ps_podman_ports_published_with_host_ip() {
    let line = r#"{"Id":"x","Names":["n"],"Image":"img","State":"running","Status":"","Ports":[{"host_ip":"127.0.0.1","container_port":5432,"host_port":5432,"range":1,"protocol":"tcp"}]}"#;
    assert_eq!(
        parse_container_ps(line)[0].ports,
        "127.0.0.1:5432->5432/tcp"
    );
}

#[test]
fn parse_ps_podman_ports_range_published() {
    let line = r#"{"Id":"x","Names":["n"],"Image":"img","State":"running","Status":"","Ports":[{"host_ip":"","container_port":8000,"host_port":8000,"range":3,"protocol":"udp"}]}"#;
    assert_eq!(
        parse_container_ps(line)[0].ports,
        "8000-8002->8000-8002/udp"
    );
}

#[test]
fn parse_ps_podman_ports_multiple_objects_joined() {
    let line = r#"{"Id":"x","Names":["n"],"Image":"img","State":"running","Status":"","Ports":[{"host_ip":"","container_port":80,"host_port":8080,"range":1,"protocol":"tcp"},{"host_ip":"","container_port":443,"host_port":8443,"range":1,"protocol":"tcp"}]}"#;
    assert_eq!(
        parse_container_ps(line)[0].ports,
        "8080->80/tcp, 8443->443/tcp"
    );
}

#[test]
fn parse_ps_podman_ports_default_protocol_tcp() {
    // protocol missing/empty => default tcp (matches docker convention)
    let line = r#"{"Id":"x","Names":["n"],"Image":"img","State":"running","Status":"","Ports":[{"host_ip":"","container_port":80,"host_port":8080,"range":1,"protocol":""}]}"#;
    assert_eq!(parse_container_ps(line)[0].ports, "8080->80/tcp");
}

#[test]
fn parse_ps_podman_stopped_container() {
    // Created/stopped containers have State="created" or "exited" with no
    // published ports. Status is often empty on podman.
    let line = r#"{"Id":"a092e6927446","Names":["purple-test-stopped"],"Image":"docker.io/library/alpine:latest","State":"created","Status":"","Ports":null}"#;
    let r = parse_container_ps(line);
    assert_eq!(r[0].id, "a092e6927446");
    assert_eq!(r[0].names, "purple-test-stopped");
    assert_eq!(r[0].state, "created");
    assert_eq!(r[0].ports, "");
}

#[test]
fn parse_ps_podman_three_containers_ndjson() {
    // Mirrors the live `podman ps -a --format '{{json .}}'` shape across
    // running/running/created, with and without published ports.
    let lines = [
        r#"{"Id":"826abc60f2e5","Names":["purple-test-nginx"],"Image":"docker.io/library/nginx:alpine","State":"running","Status":"","Ports":[{"host_ip":"","container_port":80,"host_port":8081,"range":1,"protocol":"tcp"}]}"#,
        r#"{"Id":"04bfa4272e1e","Names":["purple-test-redis"],"Image":"docker.io/library/redis:alpine","State":"running","Status":"","Ports":null}"#,
        r#"{"Id":"a092e6927446","Names":["purple-test-stopped"],"Image":"docker.io/library/alpine:latest","State":"created","Status":"","Ports":null}"#,
    ];
    let r = parse_container_ps(&lines.join("\n"));
    assert_eq!(r.len(), 3);
    assert_eq!(r[0].names, "purple-test-nginx");
    assert_eq!(r[0].ports, "8081->80/tcp");
    assert_eq!(r[1].names, "purple-test-redis");
    assert_eq!(r[1].ports, "");
    assert_eq!(r[2].state, "created");
    assert_eq!(r[2].ports, "");
}

#[test]
fn parse_ps_podman_names_null_drops_row() {
    // `Names: null` is a corruption signal: a container has lost its
    // identity. The strict NamesField deserializer rejects null,
    // parse_container_ps' .ok() filter then drops the row entirely
    // rather than render a nameless entry in the UI.
    let line =
        r#"{"Id":"x","Names":null,"Image":"img","State":"running","Status":"","Ports":null}"#;
    assert_eq!(parse_container_ps(line).len(), 0);
}

#[test]
fn parse_ps_mixed_docker_and_podman_lines_in_same_output() {
    // Edge case where one host emits both shapes: e.g. a podman-docker
    // shim that wraps some calls but not others or a future podman that
    // changes shape between versions during a rolling upgrade.
    let docker_line = r#"{"ID":"deadbeef","Names":"web","Image":"nginx","State":"running","Status":"Up 1m","Ports":"0.0.0.0:80->80/tcp"}"#;
    let podman_line = r#"{"Id":"cafebabe","Names":["app"],"Image":"docker.io/library/redis","State":"running","Status":"","Ports":null}"#;
    let r = parse_container_ps(&format!("{docker_line}\n{podman_line}"));
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].id, "deadbeef");
    assert_eq!(r[0].names, "web");
    assert_eq!(r[0].ports, "0.0.0.0:80->80/tcp");
    assert_eq!(r[1].id, "cafebabe");
    assert_eq!(r[1].names, "app");
    assert_eq!(r[1].ports, "");
}

#[test]
fn parse_ps_both_id_fields_present_drops_row() {
    // When both `ID` and `Id` are present, serde treats them as duplicate
    // field assignments (rename + alias target the same struct field) and
    // raises a deserialization error. parse_container_ps' .ok() filter
    // then drops the row. No real-world runtime emits both keys; this
    // pins the defensive behaviour for the corrupted-input edge case so
    // a serde version that silently picks one value (and a different one
    // per release) cannot creep in unnoticed.
    let line =
        r#"{"ID":"upper","Id":"lower","Names":["n"],"Image":"img","State":"running","Status":""}"#;
    assert_eq!(parse_container_ps(line).len(), 0);
}

#[test]
fn parse_ps_podman_ports_range_zero_renders_as_single_port() {
    // `range: 0` is technically corrupt input (podman always emits >=1)
    // but the format helper must degrade gracefully to single-port form
    // rather than emit nonsensical `8080-8079->80-79` from saturating
    // arithmetic. Pinned to catch a future refactor that drops the
    // `range > 1` guard.
    let line = r#"{"Id":"x","Names":["n"],"Image":"img","State":"running","Status":"","Ports":[{"host_ip":"","container_port":80,"host_port":8080,"range":0,"protocol":"tcp"}]}"#;
    assert_eq!(parse_container_ps(line)[0].ports, "8080->80/tcp");
}

#[test]
fn parse_ps_docker_format_still_works() {
    // Regression guard: docker's scalar `Names`/`Ports` and uppercase `ID`
    // must continue to deserialize unchanged after the tolerant coercion.
    let line = r#"{"ID":"deadbeef","Names":"web,web-alt","Image":"nginx","State":"running","Status":"Up 5 minutes","Ports":"0.0.0.0:80->80/tcp"}"#;
    let r = parse_container_ps(line);
    assert_eq!(r[0].id, "deadbeef");
    assert_eq!(r[0].names, "web,web-alt");
    assert_eq!(r[0].ports, "0.0.0.0:80->80/tcp");
    assert_eq!(r[0].status, "Up 5 minutes");
}

#[test]
fn parse_output_podman_three_containers_with_sentinels() {
    // End-to-end: combined detection sentinel + podman NDJSON + engine
    // version. Reproduces the live SSH pipeline output shape.
    let out = "\
##purple:podman##\n\
{\"Id\":\"a\",\"Names\":[\"one\"],\"Image\":\"i\",\"State\":\"running\",\"Status\":\"\",\"Ports\":null}\n\
{\"Id\":\"b\",\"Names\":[\"two\"],\"Image\":\"i\",\"State\":\"exited\",\"Status\":\"\",\"Ports\":null}\n\
{\"Id\":\"c\",\"Names\":[\"three\"],\"Image\":\"i\",\"State\":\"created\",\"Status\":\"\",\"Ports\":null}\n\
##purple:engine##\n\
5.8.2";
    let listing = parse_container_output(out, None).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Podman);
    assert_eq!(listing.engine_version.as_deref(), Some("5.8.2"));
    assert_eq!(listing.containers.len(), 3);
    assert_eq!(listing.containers[0].names, "one");
    assert_eq!(listing.containers[2].state, "created");
}

#[test]
fn parse_output_fedora_coreos_docker_alias_relabels_to_podman() {
    // Fedora CoreOS / podman-machine ships `docker` as a symlink to
    // podman. Detection sees `docker` and emits the docker sentinel,
    // but the JSON shape is unmistakably podman (array `Names`). The
    // shape detector relabels the runtime so downstream consumers
    // (host detail label, MCP `runtime` field, group/filter by
    // runtime) reflect what's actually running on the remote.
    let out = "\
##purple:docker##\n\
{\"Id\":\"a\",\"Names\":[\"one\"],\"Image\":\"i\",\"State\":\"running\",\"Status\":\"\",\"Ports\":null}\n\
##purple:engine##\n\
5.8.2";
    let listing = parse_container_output(out, None).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Podman);
    assert_eq!(listing.containers.len(), 1);
    assert_eq!(listing.containers[0].names, "one");
}

#[test]
fn parse_output_docker_branch_with_docker_shape_stays_docker() {
    // Counterpart to the Fedora CoreOS relabel: a real docker host
    // emits docker sentinel AND docker-shaped JSON. No relabel.
    let out = "\
##purple:docker##\n\
{\"ID\":\"a\",\"Names\":\"one\",\"Image\":\"i\",\"State\":\"running\",\"Status\":\"Up 5m\",\"Ports\":\"0.0.0.0:80->80/tcp\"}\n\
##purple:engine##\n\
29.4.2";
    let listing = parse_container_output(out, None).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Docker);
}

#[test]
fn parse_output_docker_sentinel_empty_body_stays_docker() {
    // Edge case: docker sentinel with NO container lines. The shape
    // detector has nothing to inspect, must NOT relabel to Podman.
    // A future `looks_like_podman` that defaulted to `true` on empty
    // input would silently mislabel every fresh-cached docker host.
    let out = "##purple:docker##\n##purple:engine##\n29.4.2";
    let listing = parse_container_output(out, None).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Docker);
    assert_eq!(listing.containers.len(), 0);
}

#[test]
fn parse_output_pretty_printed_podman_json_relabels_to_podman() {
    // Pretty-printer in the middle (e.g. an SSH wrapper that pipes
    // through `jq`) would emit `"Names": [` with a space. The shape
    // detector must accept both compact and pretty forms.
    let out = "\
##purple:docker##\n\
{\"Id\": \"a\", \"Names\": [\"one\"], \"Image\": \"i\", \"State\": \"running\", \"Status\": \"\"}\n\
##purple:engine##\n\
5.8.2";
    let listing = parse_container_output(out, None).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Podman);
}

// -- looks_like_podman: direct unit tests -------------------------------
// Internal heuristic that drives the Fedora CoreOS relabel. Integration
// tests above exercise it via parse_container_output, but pinning each
// branch directly makes a regression in the detector trivially diagnosable.

#[test]
fn looks_like_podman_compact_array_names_returns_true() {
    let out =
        "{\"Id\":\"a\",\"Names\":[\"web\"],\"Image\":\"i\",\"State\":\"running\",\"Status\":\"\"}";
    assert!(super::looks_like_podman(out));
}

#[test]
fn looks_like_podman_pretty_printed_array_names_returns_true() {
    let out = "{\"Id\": \"a\", \"Names\": [\"web\"], \"State\": \"running\"}";
    assert!(super::looks_like_podman(out));
}

#[test]
fn looks_like_podman_docker_scalar_names_returns_false() {
    let out = "{\"ID\":\"a\",\"Names\":\"web\",\"Image\":\"i\",\"State\":\"running\"}";
    assert!(!super::looks_like_podman(out));
}

#[test]
fn looks_like_podman_empty_input_returns_false() {
    assert!(!super::looks_like_podman(""));
}

#[test]
fn looks_like_podman_only_sentinels_no_json_returns_false() {
    // Empty body with only sentinels (Fedora-like host with zero
    // containers). Must NOT relabel to Podman by default.
    let out = "##purple:docker##\n##purple:engine##\n29.4.2";
    assert!(!super::looks_like_podman(out));
}

#[test]
fn looks_like_podman_skips_motd_lines() {
    // MOTD or banner lines that do not start with `{` are skipped;
    // detector still finds the real JSON line below.
    let out = "Last login: Mon\nFedora CoreOS 41\n{\"Id\":\"a\",\"Names\":[\"web\"]}";
    assert!(super::looks_like_podman(out));
}

// -- parse_container_inspect: podman version compat ---------------------

#[test]
fn parse_container_inspect_oom_killed_both_casings() {
    // Podman 5.x + docker emit `OOMKilled`. Podman 3.x (Ubuntu 22.04
    // LTS default) emits `OomKilled`. Both must surface oom_killed=true
    // so the ATTENTION card flags OOM-killed containers regardless of
    // remote runtime version. Pins the alias chain so a refactor that
    // drops the fallback fails loudly here rather than silently in
    // production telemetry.
    let docker_shape = r#"[{"State":{"ExitCode":137,"OOMKilled":true,"StartedAt":"","FinishedAt":""},"Config":{},"NetworkSettings":{},"RestartCount":0,"Mounts":[]}]"#;
    let podman3_shape = r#"[{"State":{"ExitCode":137,"OomKilled":true,"StartedAt":"","FinishedAt":""},"Config":{},"NetworkSettings":{},"RestartCount":0,"Mounts":[]}]"#;
    let neither_key = r#"[{"State":{"ExitCode":0,"StartedAt":"","FinishedAt":""},"Config":{},"NetworkSettings":{},"RestartCount":0,"Mounts":[]}]"#;

    assert!(parse_container_inspect(docker_shape).unwrap().oom_killed);
    assert!(parse_container_inspect(podman3_shape).unwrap().oom_killed);
    assert!(!parse_container_inspect(neither_key).unwrap().oom_killed);
}

// -- validate_container_id: injection-vector rejection ------------------

#[test]
fn validate_container_id_accepts_typical_ids() {
    assert!(validate_container_id("abc123").is_ok());
    assert!(validate_container_id("826abc60f2e5").is_ok());
    assert!(validate_container_id("my-container_1.0").is_ok());
}

#[test]
fn validate_container_id_rejects_empty() {
    assert!(validate_container_id("").is_err());
}

#[test]
fn validate_container_id_rejects_shell_metacharacters() {
    // The defense-in-depth check in handle_pending_container_exec relies
    // on this rejection to keep crafted container names off the SSH
    // command line. Every char that has shell meaning must fail.
    for bad in [
        ";", "|", "&", "`", "$", "(", ")", "<", ">", "\\", "\"", "'", "\n", " ",
    ] {
        let id = format!("abc{bad}def");
        assert!(
            validate_container_id(&id).is_err(),
            "metachar '{bad}' must be rejected"
        );
    }
}

#[test]
fn validate_container_id_rejects_colon() {
    // Colon would split addressable forms like `pod:container`.
    assert!(validate_container_id("qemu:300").is_err());
}

#[test]
fn validate_container_id_rejects_non_ascii() {
    assert!(validate_container_id("café").is_err());
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
fn list_cmd_docker_includes_engine_sentinel() {
    let cmd = container_list_command(Some(ContainerRuntime::Docker));
    assert!(cmd.contains("docker ps -a --format '{{json .}}'"));
    assert!(cmd.contains("##purple:engine##"));
    assert!(cmd.contains("docker version --format '{{.Server.Version}}'"));
}

#[test]
fn list_cmd_podman_includes_engine_sentinel() {
    let cmd = container_list_command(Some(ContainerRuntime::Podman));
    assert!(cmd.contains("podman ps -a --format '{{json .}}'"));
    assert!(cmd.contains("##purple:engine##"));
    assert!(cmd.contains("podman version --format '{{.Server.Version}}'"));
}

#[test]
fn list_cmd_none_has_sentinels() {
    let cmd = container_list_command(None);
    assert!(cmd.contains("##purple:docker##"));
    assert!(cmd.contains("##purple:podman##"));
    assert!(cmd.contains("##purple:none##"));
    assert!(cmd.contains("##purple:engine##"));
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
    let listing = parse_container_output(&out, None).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Docker);
    assert_eq!(listing.containers.len(), 1);
}

#[test]
fn output_podman_sentinel() {
    let c = make_json("xyz", "db", "pg", "exited", "Exited", "");
    let out = format!("##purple:podman##\n{c}");
    let listing = parse_container_output(&out, None).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Podman);
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
    let listing = parse_container_output(&c, Some(ContainerRuntime::Docker)).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Docker);
    assert_eq!(listing.containers.len(), 1);
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
    let listing = parse_container_output(&out, None).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Docker);
    assert_eq!(listing.containers.len(), 1);
}

#[test]
fn output_empty_container_list() {
    let listing = parse_container_output("##purple:docker##\n", None).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Docker);
    assert!(listing.containers.is_empty());
}

#[test]
fn output_multiple_containers() {
    let c1 = make_json("a", "web", "nginx", "running", "Up", "80/tcp");
    let c2 = make_json("b", "db", "pg", "exited", "Exited", "");
    let c3 = make_json("c", "cache", "redis", "running", "Up", "6379/tcp");
    let out = format!("##purple:podman##\n{c1}\n{c2}\n{c3}");
    let listing = parse_container_output(&out, None).unwrap();
    assert_eq!(listing.containers.len(), 3);
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
fn friendly_error_security_key_pin_wins_over_the_generic_refusal() {
    // ssh ends this case with `Permission denied (publickey)`, so the docker
    // group hint would point the wrong way.
    let msg = friendly_container_error(
        "sign_and_send_pubkey: signing failed for ECDSA-SK \"/home/u/.ssh/id_sk\": incorrect passphrase supplied to decrypt private key\n\
         u@host: Permission denied (publickey).",
        Some(255),
    );
    assert_eq!(msg, crate::messages::CONTAINER_SECURITY_KEY_PIN);
}

#[test]
fn friendly_error_keeps_the_docker_hint_when_the_login_got_in_another_way() {
    // The key had no PIN, ssh logged in with another method and docker
    // itself refused: the PIN is not the problem.
    let msg = friendly_container_error(
        "sign_and_send_pubkey: signing failed for ECDSA-SK \"/home/u/.ssh/id_sk\": incorrect passphrase supplied to decrypt private key\n\
         Got permission denied while trying to connect to the Docker daemon socket",
        Some(1),
    );
    assert_eq!(msg, crate::messages::CONTAINER_PERMISSION_DENIED);
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
        engine_version: Some("25.0.3".to_string()),
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
    assert_eq!(d.engine_version.as_deref(), Some("25.0.3"));
    assert_eq!(d.containers.len(), 1);
    assert_eq!(d.containers[0].id, "abc");
}

#[test]
fn cache_round_trip_podman() {
    let line = CacheLine {
        alias: "host2".to_string(),
        timestamp: 200,
        runtime: ContainerRuntime::Podman,
        engine_version: None,
        containers: vec![],
    };
    let s = serde_json::to_string(&line).unwrap();
    let d: CacheLine = serde_json::from_str(&s).unwrap();
    assert_eq!(d.runtime, ContainerRuntime::Podman);
    assert!(d.engine_version.is_none());
    // None must omit the field on serialise so disk format stays compact.
    assert!(!s.contains("engine_version"));
}

#[test]
fn cache_legacy_line_without_engine_version_loads() {
    // Cache files written by older purple builds omit engine_version.
    // Backward compat: Option<String> + serde(default) makes them load
    // cleanly with engine_version = None.
    let legacy = r#"{"alias":"old","timestamp":100,"runtime":"Docker","containers":[]}"#;
    let d: CacheLine = serde_json::from_str(legacy).unwrap();
    assert_eq!(d.alias, "old");
    assert!(d.engine_version.is_none());
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
        engine_version: None,
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
                engine_version: None,
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
            engine_version: entry.engine_version,
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

/// Pin `format_relative_time` to its real-time branch. The function reads the
/// process-global demo flag: in demo mode it uses a frozen reference clock, so
/// a concurrent demo-mode test (visual suite, asset generation) flipping the
/// flag mid-assertion would skew a boundary value by a few seconds. Serialize
/// against those suites via `GLOBAL_TEST_LOCK` and disable demo so the function
/// uses live time, matching this module's `now_secs()`.
fn realtime_guard() -> std::sync::MutexGuard<'static, ()> {
    let g = crate::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::demo_flag::disable();
    g
}

#[test]
fn relative_just_now() {
    assert_eq!(format_relative_time(now_secs()), "just now");
    assert_eq!(format_relative_time(now_secs() - 30), "just now");
    assert_eq!(format_relative_time(now_secs() - 59), "just now");
}

#[test]
fn relative_minutes() {
    let _g = realtime_guard();
    assert_eq!(format_relative_time(now_secs() - 60), "1m ago");
    assert_eq!(format_relative_time(now_secs() - 300), "5m ago");
    assert_eq!(format_relative_time(now_secs() - 3599), "59m ago");
}

#[test]
fn relative_hours() {
    let _g = realtime_guard();
    assert_eq!(format_relative_time(now_secs() - 3600), "1h ago");
    assert_eq!(format_relative_time(now_secs() - 7200), "2h ago");
}

#[test]
fn relative_days() {
    let _g = realtime_guard();
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
    let listing = r.unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Docker);
    assert!(listing.containers.is_empty());
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
        engine_version: None,
        containers: vec![],
    })
    .unwrap();
    let line2 = serde_json::to_string(&CacheLine {
        alias: "dup".to_string(),
        timestamp: 99,
        runtime: ContainerRuntime::Podman,
        engine_version: None,
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
    let listing = parse_container_output(&input, None).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Docker);
    assert_eq!(listing.containers.len(), 1);
}

#[test]
fn output_sentinel_indented() {
    let c = make_json("a", "web", "nginx", "running", "Up", "");
    let input = format!("  ##purple:docker##\n{c}");
    let listing = parse_container_output(&input, None).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Docker);
    assert_eq!(listing.containers.len(), 1);
}

#[test]
fn output_caller_runtime_podman() {
    let c = make_json("a", "app", "img", "running", "Up", "");
    let listing = parse_container_output(&c, Some(ContainerRuntime::Podman)).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Podman);
    assert_eq!(listing.containers.len(), 1);
}

#[test]
fn output_engine_sentinel_extracts_version() {
    let c = make_json("a", "web", "nginx", "running", "Up", "");
    let out = format!("##purple:docker##\n{c}\n##purple:engine##\n25.0.3");
    let listing = parse_container_output(&out, None).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Docker);
    assert_eq!(listing.containers.len(), 1);
    assert_eq!(listing.engine_version.as_deref(), Some("25.0.3"));
}

#[test]
fn output_engine_sentinel_with_empty_version() {
    // Daemon down: docker version produced no stdout. Sentinel still
    // emitted but the line after it is empty. Parser must yield None.
    let c = make_json("a", "web", "nginx", "running", "Up", "");
    let out = format!("##purple:docker##\n{c}\n##purple:engine##\n");
    let listing = parse_container_output(&out, None).unwrap();
    assert_eq!(listing.containers.len(), 1);
    assert!(listing.engine_version.is_none());
}

#[test]
fn output_legacy_no_engine_sentinel_yields_none() {
    // Older purple SSH command shape (before engine sentinel) must
    // still parse cleanly with engine_version == None.
    let c = make_json("a", "web", "nginx", "running", "Up", "");
    let out = format!("##purple:docker##\n{c}");
    let listing = parse_container_output(&out, None).unwrap();
    assert_eq!(listing.containers.len(), 1);
    assert!(listing.engine_version.is_none());
}

#[test]
fn output_caller_runtime_with_engine_sentinel() {
    let c = make_json("a", "app", "img", "running", "Up", "");
    let out = format!("{c}\n##purple:engine##\n4.9.0");
    let listing = parse_container_output(&out, Some(ContainerRuntime::Podman)).unwrap();
    assert_eq!(listing.runtime, ContainerRuntime::Podman);
    assert_eq!(listing.containers.len(), 1);
    assert_eq!(listing.engine_version.as_deref(), Some("4.9.0"));
}

#[test]
fn output_engine_sentinel_only_no_runtime_sentinel_no_caller() {
    // engine sentinel alone is not a runtime sentinel; without a caller
    // hint there is nothing to identify the runtime, so parsing must err.
    let r = parse_container_output("##purple:engine##\n25.0.3", None);
    assert!(r.is_err());
}

#[test]
fn output_engine_version_caps_to_first_line_when_motd_follows() {
    // Some shells emit a logout banner or trailing MOTD after the
    // version subcall. The parser must capture only the first non-empty
    // post-sentinel line, otherwise the cached engine_version would
    // surface as "25.0.3\n-- session closed --" in the Runtime field.
    let c = make_json("a", "web", "nginx", "running", "Up", "");
    let out =
        format!("##purple:docker##\n{c}\n##purple:engine##\n25.0.3\nlogout\n-- session closed --");
    let listing = parse_container_output(&out, None).unwrap();
    assert_eq!(listing.engine_version.as_deref(), Some("25.0.3"));
}

#[test]
fn list_cmd_known_runtime_chains_with_and_so_ps_failure_propagates() {
    // Docker known-runtime command must short-circuit at `&&` so a `ps`
    // failure cannot be masked by a successful `version` subcall on the
    // remote shell. The trailing version step is wrapped in `|| true`
    // so its own failure stays best-effort.
    let docker = container_list_command(Some(ContainerRuntime::Docker));
    assert!(
        docker.contains(" && echo '##purple:engine##' && "),
        "docker command must use `&&` between ps and engine sentinel: {docker}"
    );
    assert!(
        docker.contains("|| true"),
        "version subcall must be wrapped so its failure does not surface: {docker}"
    );
    let podman = container_list_command(Some(ContainerRuntime::Podman));
    assert!(podman.contains(" && echo '##purple:engine##' && "));
    assert!(podman.contains("|| true"));
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
    let _g = realtime_guard();
    let ts = now_secs() - 60;
    assert_eq!(format_relative_time(ts), "1m ago");
}

#[test]
fn format_relative_time_boundary_3600s() {
    let _g = realtime_guard();
    let ts = now_secs() - 3600;
    assert_eq!(format_relative_time(ts), "1h ago");
}

#[test]
fn format_relative_time_boundary_86400s() {
    let _g = realtime_guard();
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

// -- Host key verification --------------------------------------------------

#[test]
fn friendly_error_host_key_verification_failed() {
    let msg = friendly_container_error("Host key verification failed.", Some(255));
    assert_eq!(msg, crate::messages::HOST_KEY_UNKNOWN);
}

#[test]
fn friendly_error_host_key_not_known() {
    let stderr = "No ED25519 host key is known for 10.30.0.51 and you have \
                  requested strict checking.";
    let msg = friendly_container_error(stderr, Some(255));
    assert_eq!(msg, crate::messages::HOST_KEY_UNKNOWN);
}

#[test]
fn friendly_error_host_key_rsa_not_known() {
    let msg = friendly_container_error("No RSA host key is known for example.com", Some(255));
    assert_eq!(msg, crate::messages::HOST_KEY_UNKNOWN);
}

#[test]
fn friendly_error_host_key_is_not_known() {
    let msg = friendly_container_error("This host key is not known by any other names.", Some(255));
    assert_eq!(msg, crate::messages::HOST_KEY_UNKNOWN);
}

#[test]
fn friendly_error_host_key_wins_over_other_matches() {
    // Stderr containing both a permission-denied fragment and a host-key
    // failure should route to the host-key message; host-key trust must
    // always be fixed first before any auth-level diagnosis.
    let stderr = "Host key verification failed.\nPermission denied (publickey)";
    let msg = friendly_container_error(stderr, Some(255));
    assert_eq!(msg, crate::messages::HOST_KEY_UNKNOWN);
}

#[test]
fn friendly_error_host_key_changed_remote_identification() {
    let stderr = "WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!\n\
                  IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!";
    let msg = friendly_container_error(stderr, Some(255));
    assert_eq!(msg, crate::messages::HOST_KEY_CHANGED);
}

#[test]
fn friendly_error_host_key_changed_has_changed_variant() {
    let stderr = "Host key for server.example.com has changed and \
                  you have requested strict checking.";
    let msg = friendly_container_error(stderr, Some(255));
    assert_eq!(msg, crate::messages::HOST_KEY_CHANGED);
}

#[test]
fn friendly_error_changed_wins_over_unknown() {
    // A stderr that contains both "verification failed" and "has changed"
    // must route to the CHANGED message. Changed-key is security-critical
    // and takes precedence over the generic "unknown" bucket.
    let stderr = "Host key for x has changed.\nHost key verification failed.";
    let msg = friendly_container_error(stderr, Some(255));
    assert_eq!(msg, crate::messages::HOST_KEY_CHANGED);
}

// -- parse_container_inspect ---------------------------------------------

fn sample_inspect_running() -> String {
    serde_json::json!([{
        "RestartCount": 0,
        "State": {
            "Status": "running",
            "Running": true,
            "ExitCode": 0,
            "OOMKilled": false,
            "StartedAt": "2026-05-09T08:00:00Z",
            "FinishedAt": "0001-01-01T00:00:00Z",
            "Health": {"Status": "healthy"}
        },
        "Config": {
            "Cmd": ["nginx", "-g", "daemon off;"],
            "Entrypoint": null,
            "Env": ["PATH=/usr/bin", "TZ=UTC", "FOO=bar"]
        },
        "Mounts": [{"Source": "/var/data", "Destination": "/data"}, {"Source": "/etc/cfg", "Destination": "/etc/cfg"}],
        "NetworkSettings": {
            "Networks": {
                "bridge": {"IPAddress": "172.17.0.5"}
            }
        }
    }])
    .to_string()
}

#[test]
fn parse_inspect_extracts_running_fields() {
    let r = parse_container_inspect(&sample_inspect_running()).expect("parse");
    assert_eq!(r.exit_code, 0);
    assert!(!r.oom_killed);
    assert_eq!(r.started_at, "2026-05-09T08:00:00Z");
    assert_eq!(r.health.as_deref(), Some("healthy"));
    assert_eq!(r.restart_count, 0);
    assert_eq!(r.command.as_ref().map(|c| c.len()), Some(3));
    assert_eq!(r.env_count, 3);
    assert_eq!(r.mount_count, 2);
    assert_eq!(r.networks.len(), 1);
    assert_eq!(r.networks[0].name, "bridge");
    assert_eq!(r.networks[0].ip_address, "172.17.0.5");
}

#[test]
fn parse_inspect_extracts_exited_fields() {
    let json = serde_json::json!([{
        "RestartCount": 3,
        "State": {
            "Status": "exited",
            "Running": false,
            "ExitCode": 137,
            "OOMKilled": true,
            "StartedAt": "2026-05-08T12:00:00Z",
            "FinishedAt": "2026-05-08T18:00:00Z"
        },
        "Config": {"Cmd": null, "Entrypoint": null, "Env": []},
        "Mounts": [],
        "NetworkSettings": {"Networks": {}}
    }])
    .to_string();
    let r = parse_container_inspect(&json).expect("parse");
    assert_eq!(r.exit_code, 137);
    assert!(r.oom_killed);
    assert_eq!(r.restart_count, 3);
    assert_eq!(r.health, None);
    assert!(r.command.is_none());
    assert_eq!(r.env_count, 0);
    assert_eq!(r.mount_count, 0);
}

#[test]
fn parse_inspect_empty_array_errors() {
    assert!(parse_container_inspect("[]").is_err());
}

#[test]
fn parse_inspect_empty_string_errors() {
    assert!(parse_container_inspect("").is_err());
    assert!(parse_container_inspect("   ").is_err());
}

#[test]
fn parse_inspect_invalid_json_errors() {
    assert!(parse_container_inspect("not json").is_err());
}

#[test]
fn parse_inspect_missing_fields_uses_defaults() {
    // Minimal valid object — every nested field absent. Parser must not
    // panic and must produce a usable but empty ContainerInspect.
    let r = parse_container_inspect("[{}]").expect("parse");
    assert_eq!(r.exit_code, 0);
    assert!(!r.oom_killed);
    assert_eq!(r.restart_count, 0);
    assert!(r.command.is_none());
    assert_eq!(r.env_count, 0);
    assert!(r.networks.is_empty());
}

// -- exit_code_meaning ---------------------------------------------------

#[test]
fn exit_code_meaning_known_codes() {
    assert!(exit_code_meaning(1).is_some());
    assert!(exit_code_meaning(137).unwrap().contains("OOM"));
    assert!(exit_code_meaning(143).unwrap().contains("SIGTERM"));
    assert!(exit_code_meaning(127).unwrap().contains("not found"));
}

#[test]
fn exit_code_meaning_unknown_returns_none() {
    // 0 has no entry: detail panel only annotates failed exits.
    assert!(exit_code_meaning(0).is_none());
    assert!(exit_code_meaning(42).is_none());
    assert!(exit_code_meaning(255).is_none());
}

// -- container_inspect_command -------------------------------------------

#[test]
fn inspect_command_uses_runtime_binary() {
    assert_eq!(
        container_inspect_command(ContainerRuntime::Docker, "abc"),
        "docker inspect abc"
    );
    assert_eq!(
        container_inspect_command(ContainerRuntime::Podman, "xyz"),
        "podman inspect xyz"
    );
}

// -- parse_container_inspect: audit fields -------------------------------

const FIXTURE_FULL_RUNNING: &str = r#"[{
  "Id":"86d03287fdf9aaaa",
  "Image":"sha256:a4f1e7c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7c91d",
  "Config":{
    "Image":"nginx:stable-alpine",
    "User":"root",
    "Cmd":["/bin/sh","-c","while :; do sleep 6h & wait; done"],
    "Env":["PATH=/usr/local/sbin:/usr/local/bin","NGINX_VERSION=1.27.0","PKG_RELEASE=1"],
    "Labels":{
      "com.docker.compose.project":"signalproxy-nl",
      "com.docker.compose.service":"nginx-terminate"
    }
  },
  "HostConfig":{
    "RestartPolicy":{"Name":"unless-stopped","MaximumRetryCount":0},
    "Privileged":false,
    "ReadonlyRootfs":false,
    "CapAdd":null,
    "CapDrop":["NET_RAW"],
    "AppArmorProfile":"docker-default",
    "SecurityOpt":["seccomp=default"]
  },
  "AppArmorProfile":"docker-default",
  "State":{"Status":"running","ExitCode":0,"StartedAt":"2026-04-02T19:46:58Z","Health":{"Status":"healthy"}},
  "RestartCount":0,
  "Mounts":[
    {"Type":"bind","Source":"/etc/letsencrypt","Destination":"/etc/letsencrypt","Mode":"rw","RW":true},
    {"Type":"volume","Name":"certs","Destination":"/etc/nginx/certs","Mode":"ro","RW":false},
    {"Type":"bind","Source":"/srv/nginx.conf","Destination":"/etc/nginx/nginx.conf","Mode":"ro","RW":false}
  ],
  "NetworkSettings":{"Networks":{"signal-tls-proxy_default":{"IPAddress":"172.18.0.3"}}}
}]"#;

#[test]
fn inspect_parses_image_digest_from_full_image_id() {
    let inspect = parse_container_inspect(FIXTURE_FULL_RUNNING).unwrap();
    assert_eq!(
        inspect.image_digest.as_deref(),
        Some("sha256:a4f1e7c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7c91d")
    );
}

#[test]
fn inspect_parses_restart_policy_unless_stopped() {
    let inspect = parse_container_inspect(FIXTURE_FULL_RUNNING).unwrap();
    assert_eq!(inspect.restart_policy.as_deref(), Some("unless-stopped"));
}

#[test]
fn inspect_parses_user_root() {
    let inspect = parse_container_inspect(FIXTURE_FULL_RUNNING).unwrap();
    assert_eq!(inspect.user.as_deref(), Some("root"));
}

#[test]
fn inspect_parses_privs_block() {
    let inspect = parse_container_inspect(FIXTURE_FULL_RUNNING).unwrap();
    assert!(!inspect.privileged);
    assert!(!inspect.readonly_rootfs);
    assert_eq!(inspect.apparmor_profile.as_deref(), Some("docker-default"));
    assert_eq!(inspect.seccomp_profile.as_deref(), Some("default"));
    assert!(inspect.cap_add.is_empty());
    assert_eq!(inspect.cap_drop, vec!["NET_RAW".to_string()]);
}

#[test]
fn inspect_parses_three_mounts_with_rw_ro() {
    let inspect = parse_container_inspect(FIXTURE_FULL_RUNNING).unwrap();
    assert_eq!(inspect.mounts.len(), 3);
    assert_eq!(inspect.mounts[0].source, "/etc/letsencrypt");
    assert_eq!(inspect.mounts[0].destination, "/etc/letsencrypt");
    assert!(!inspect.mounts[0].read_only);
    assert!(inspect.mounts[1].read_only);
    assert!(inspect.mounts[2].read_only);
}

#[test]
fn inspect_parses_compose_labels() {
    let inspect = parse_container_inspect(FIXTURE_FULL_RUNNING).unwrap();
    assert_eq!(inspect.compose_project.as_deref(), Some("signalproxy-nl"));
    assert_eq!(inspect.compose_service.as_deref(), Some("nginx-terminate"));
}

#[test]
fn inspect_handles_missing_audit_fields_gracefully() {
    let minimal = r#"[{
      "Id":"abc",
      "State":{"Status":"running","ExitCode":0,"StartedAt":"2026-01-01T00:00:00Z"},
      "Config":{"Image":"alpine"},
      "HostConfig":{}
    }]"#;
    let inspect = parse_container_inspect(minimal).unwrap();
    assert_eq!(inspect.image_digest.as_deref(), None);
    assert_eq!(inspect.restart_policy.as_deref(), None);
    assert_eq!(inspect.user.as_deref(), None);
    assert!(!inspect.privileged);
    assert!(!inspect.readonly_rootfs);
    assert_eq!(inspect.apparmor_profile.as_deref(), None);
    assert_eq!(inspect.seccomp_profile.as_deref(), None);
    assert!(inspect.cap_add.is_empty());
    assert!(inspect.cap_drop.is_empty());
    assert!(inspect.mounts.is_empty());
    assert_eq!(inspect.compose_project.as_deref(), None);
    assert_eq!(inspect.compose_service.as_deref(), None);
    // Mockup 2 fields all default to None / empty when absent.
    assert!(inspect.created_at.is_empty());
    assert_eq!(inspect.pid, None);
    assert_eq!(inspect.hostname, None);
    assert_eq!(inspect.working_dir, None);
    assert_eq!(inspect.network_mode, None);
    assert_eq!(inspect.memory_limit, None);
    assert_eq!(inspect.cpu_limit_nanos, None);
    assert_eq!(inspect.pids_limit, None);
    assert_eq!(inspect.image_version, None);
    assert_eq!(inspect.health_test, None);
}

// -- parse_container_inspect: Mockup 2 fields ----------------------------

const FIXTURE_MOCKUP2: &str = r#"[{
  "Id":"86d03287fdf9aaaa",
  "Created":"2026-04-02T19:46:55Z",
  "Image":"sha256:c9095e",
  "Config":{
    "Image":"nginx:stable-alpine",
    "Hostname":"86d03287fdf9",
    "WorkingDir":"/usr/share/nginx",
    "StopSignal":"SIGQUIT",
    "StopTimeout":30,
    "Labels":{
      "org.opencontainers.image.version":"1.27.3",
      "org.opencontainers.image.revision":"a4f9b22",
      "org.opencontainers.image.source":"github.com/nginxinc/docker-nginx",
      "com.docker.compose.project":"edge"
    },
    "Healthcheck":{
      "Test":["CMD","curl","-fs","http://localhost"],
      "Interval":30000000000,
      "Timeout":5000000000
    }
  },
  "HostConfig":{
    "NetworkMode":"bridge",
    "Memory":536870912,
    "NanoCpus":1500000000,
    "PidsLimit":200,
    "LogConfig":{"Type":"json-file","Config":{"max-size":"10m"}}
  },
  "State":{
    "Status":"running",
    "Pid":12345,
    "ExitCode":0,
    "StartedAt":"2026-04-02T19:46:58Z",
    "Health":{"Status":"unhealthy","FailingStreak":3}
  },
  "Mounts":[],
  "NetworkSettings":{"Networks":{}}
}]"#;

#[test]
fn inspect_parses_created_at() {
    let i = parse_container_inspect(FIXTURE_MOCKUP2).unwrap();
    assert_eq!(i.created_at, "2026-04-02T19:46:55Z");
}

#[test]
fn inspect_parses_pid_when_running() {
    let i = parse_container_inspect(FIXTURE_MOCKUP2).unwrap();
    assert_eq!(i.pid, Some(12345));
}

#[test]
fn inspect_drops_pid_zero() {
    let json = r#"[{"State":{"Pid":0,"Status":"exited","ExitCode":0,"StartedAt":""}}]"#;
    let i = parse_container_inspect(json).unwrap();
    assert_eq!(i.pid, None);
}

#[test]
fn inspect_parses_hostname_and_workdir() {
    let i = parse_container_inspect(FIXTURE_MOCKUP2).unwrap();
    assert_eq!(i.hostname.as_deref(), Some("86d03287fdf9"));
    assert_eq!(i.working_dir.as_deref(), Some("/usr/share/nginx"));
}

#[test]
fn inspect_parses_stop_signal_and_timeout() {
    let i = parse_container_inspect(FIXTURE_MOCKUP2).unwrap();
    assert_eq!(i.stop_signal.as_deref(), Some("SIGQUIT"));
    assert_eq!(i.stop_timeout, Some(30));
}

#[test]
fn inspect_parses_oci_image_labels() {
    let i = parse_container_inspect(FIXTURE_MOCKUP2).unwrap();
    assert_eq!(i.image_version.as_deref(), Some("1.27.3"));
    assert_eq!(i.image_revision.as_deref(), Some("a4f9b22"));
    assert_eq!(
        i.image_source.as_deref(),
        Some("github.com/nginxinc/docker-nginx")
    );
}

#[test]
fn inspect_parses_resource_limits() {
    let i = parse_container_inspect(FIXTURE_MOCKUP2).unwrap();
    assert_eq!(i.memory_limit, Some(536870912));
    assert_eq!(i.cpu_limit_nanos, Some(1500000000));
    assert_eq!(i.pids_limit, Some(200));
}

#[test]
fn inspect_drops_memory_zero_unlimited() {
    let json = r#"[{"HostConfig":{"Memory":0,"NanoCpus":0,"PidsLimit":0}}]"#;
    let i = parse_container_inspect(json).unwrap();
    assert_eq!(i.memory_limit, None);
    assert_eq!(i.cpu_limit_nanos, None);
    assert_eq!(i.pids_limit, None);
}

#[test]
fn inspect_drops_pids_limit_negative_one() {
    // docker emits -1 for "no limit" on some daemons; treat same as 0.
    let json = r#"[{"HostConfig":{"PidsLimit":-1}}]"#;
    let i = parse_container_inspect(json).unwrap();
    assert_eq!(i.pids_limit, None);
}

#[test]
fn inspect_parses_network_mode_and_log_driver() {
    let i = parse_container_inspect(FIXTURE_MOCKUP2).unwrap();
    assert_eq!(i.network_mode.as_deref(), Some("bridge"));
    assert_eq!(i.log_driver.as_deref(), Some("json-file"));
}

#[test]
fn inspect_drops_network_mode_default() {
    // "default" is a non-distinguishing value the renderer should not
    // bother to surface.
    let json = r#"[{"HostConfig":{"NetworkMode":"default"}}]"#;
    let i = parse_container_inspect(json).unwrap();
    assert_eq!(i.network_mode, None);
}

#[test]
fn inspect_parses_healthcheck_definition() {
    let i = parse_container_inspect(FIXTURE_MOCKUP2).unwrap();
    assert_eq!(
        i.health_test.as_deref(),
        Some(&["CMD", "curl", "-fs", "http://localhost"][..])
            .map(|a| a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .as_deref()
    );
    assert_eq!(i.health_interval_ns, Some(30_000_000_000));
}

#[test]
fn inspect_parses_health_failing_streak() {
    let i = parse_container_inspect(FIXTURE_MOCKUP2).unwrap();
    assert_eq!(i.health_failing_streak, Some(3));
}

// -- parse_uptime_from_status --------------------------------------------

#[test]
fn uptime_weeks() {
    assert_eq!(
        parse_uptime_from_status("Up 5 weeks (healthy)"),
        Some("5w".to_string())
    );
    assert_eq!(
        parse_uptime_from_status("Up 1 week"),
        Some("1w".to_string())
    );
}

#[test]
fn uptime_days() {
    assert_eq!(
        parse_uptime_from_status("Up 12 days"),
        Some("12d".to_string())
    );
}

#[test]
fn uptime_hours() {
    assert_eq!(
        parse_uptime_from_status("Up About an hour"),
        Some("1h".to_string())
    );
    assert_eq!(
        parse_uptime_from_status("Up 3 hours"),
        Some("3h".to_string())
    );
}

#[test]
fn uptime_minutes() {
    assert_eq!(
        parse_uptime_from_status("Up 16 minutes"),
        Some("16m".to_string())
    );
    assert_eq!(
        parse_uptime_from_status("Up About a minute"),
        Some("1m".to_string())
    );
}

#[test]
fn uptime_seconds() {
    assert_eq!(
        parse_uptime_from_status("Up 30 seconds"),
        Some("<1m".to_string())
    );
    assert_eq!(
        parse_uptime_from_status("Up Less than a second"),
        Some("<1m".to_string())
    );
}

#[test]
fn uptime_paused_still_running() {
    assert_eq!(
        parse_uptime_from_status("Up 5 minutes (Paused)"),
        Some("5m".to_string())
    );
}

#[test]
fn uptime_months() {
    assert_eq!(
        parse_uptime_from_status("Up 3 months"),
        Some("3mo".to_string())
    );
    assert_eq!(
        parse_uptime_from_status("Up 1 month"),
        Some("1mo".to_string())
    );
}

#[test]
fn uptime_years() {
    assert_eq!(
        parse_uptime_from_status("Up 2 years"),
        Some("2y".to_string())
    );
    assert_eq!(
        parse_uptime_from_status("Up 1 year"),
        Some("1y".to_string())
    );
}

#[test]
fn uptime_non_running_returns_none() {
    assert_eq!(parse_uptime_from_status("Exited (0) 2 days ago"), None);
    assert_eq!(parse_uptime_from_status("Created"), None);
    assert_eq!(
        parse_uptime_from_status("Restarting (1) 3 seconds ago"),
        None
    );
    assert_eq!(parse_uptime_from_status(""), None);
    assert_eq!(parse_uptime_from_status("not a docker status"), None);
}

// -- container_logs_command + parse_log_output ---------------------------

#[test]
fn logs_command_uses_runtime_and_tail() {
    assert_eq!(
        container_logs_command(ContainerRuntime::Docker, "abc", 200),
        "docker logs --tail 200 abc"
    );
    assert_eq!(
        container_logs_command(ContainerRuntime::Podman, "xyz", 50),
        "podman logs --tail 50 xyz"
    );
}

#[test]
fn logs_command_tail_zero_yields_zero_not_all() {
    // `--tail 0` returns no lines on docker/podman. Surface this
    // exactly so a caller passing 0 cannot accidentally trigger an
    // unbounded `--tail all` walk through every line in the
    // container's log file.
    assert_eq!(
        container_logs_command(ContainerRuntime::Docker, "abc", 0),
        "docker logs --tail 0 abc"
    );
}

#[test]
fn parse_log_output_combines_stdout_and_stderr_in_order() {
    let stdout = "stdout-line-1\nstdout-line-2\n";
    let stderr = "stderr-line-1\nstderr-line-2\n";
    let lines = parse_log_output(stdout, stderr);
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0], "stdout-line-1");
    assert_eq!(lines[1], "stdout-line-2");
    assert_eq!(lines[2], "stderr-line-1");
    assert_eq!(lines[3], "stderr-line-2");
}

#[test]
fn parse_log_output_stderr_only_appended() {
    // Containers (Go/Rust binaries, k8s) often emit exclusively to
    // stderr. The renderer must show those lines, not silently drop
    // them when stdout is empty.
    let lines = parse_log_output("", "only-on-stderr\nanother\n");
    assert_eq!(lines, vec!["only-on-stderr", "another"]);
}

#[test]
fn parse_log_output_strips_all_trailing_blank_lines() {
    // Both streams ending in newline plus a flushed empty line each
    // would produce two trailing empties. The renderer should not
    // leave a blank tail.
    let stdout = "real-line\n\n";
    let stderr = "stderr-line\n\n";
    let lines = parse_log_output(stdout, stderr);
    assert_eq!(lines, vec!["real-line", "stderr-line"]);
}

#[test]
fn parse_log_output_empty_inputs_returns_empty() {
    assert!(parse_log_output("", "").is_empty());
}

// -- format_uptime_short -------------------------------------------------

#[test]
fn format_uptime_short_buckets() {
    assert_eq!(format_uptime_short(0), "0s");
    assert_eq!(format_uptime_short(45), "45s");
    assert_eq!(format_uptime_short(60), "1m");
    assert_eq!(format_uptime_short(3599), "59m");
    assert_eq!(format_uptime_short(3600), "1h");
    assert_eq!(format_uptime_short(86_399), "23h");
    assert_eq!(format_uptime_short(86_400), "1d");
    assert_eq!(format_uptime_short(7 * 86_400), "7d");
}

// -- demo_log_lines (demo short-circuit) ---------------------------------

#[test]
fn demo_log_lines_returns_requested_tail_count() {
    let lines = demo_log_lines("api", 200);
    assert_eq!(lines.len(), 200);
}

#[test]
fn demo_log_lines_are_deterministic_for_same_container() {
    let a = demo_log_lines("api", 50);
    let b = demo_log_lines("api", 50);
    assert_eq!(a, b, "same container name yields identical output");
}

#[test]
fn demo_log_lines_vary_between_containers() {
    let a = demo_log_lines("api", 30);
    let b = demo_log_lines("worker", 30);
    assert_ne!(a, b, "different containers must produce different streams");
}

#[test]
fn demo_log_lines_include_search_targets() {
    let lines = demo_log_lines("api", 200);
    let joined = lines.join("\n");
    // The user must be able to find these via `/` to demo search.
    assert!(
        joined.contains("ERROR"),
        "demo logs must include error rows"
    );
    assert!(joined.contains("WARN"), "demo logs must include warn rows");
    assert!(joined.contains("INFO"), "demo logs must include info rows");
}

#[test]
fn demo_log_lines_start_with_iso_timestamp() {
    let lines = demo_log_lines("api", 5);
    // Format: YYYY-MM-DD HH:MM:SS
    let first = &lines[0];
    assert_eq!(first.as_bytes()[4], b'-', "year-month separator at index 4");
    assert_eq!(first.as_bytes()[7], b'-', "month-day separator at index 7");
    assert_eq!(first.as_bytes()[10], b' ', "date-time gap at index 10");
    assert_eq!(
        first.as_bytes()[13],
        b':',
        "hour-minute separator at index 13"
    );
}

#[test]
fn civil_from_days_round_trip_known_dates() {
    // 1970-01-01 is day 0.
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    // 2026-05-11 is day 20585 (matches `date -d "2026-05-11" +%s` math).
    let secs: u64 = 1_778_457_600; // 2026-05-11 00:00:00 UTC
    let days = (secs / 86_400) as i64;
    assert_eq!(civil_from_days(days), (2026, 5, 11));
}

#[test]
fn civil_from_days_handles_leap_years() {
    // 2000-02-29 exists (divisible by 400). 2000-01-01 is day 10957.
    assert_eq!(civil_from_days(10_957 + 31 + 28), (2000, 2, 29));
    // 2024-02-29 exists (divisible by 4 not by 100). 2024-01-01 = day 19723.
    assert_eq!(civil_from_days(19_723 + 31 + 28), (2024, 2, 29));
}

#[test]
fn civil_from_days_handles_non_leap_centennial() {
    // 1900 is NOT a leap year (divisible by 100, not by 400).
    // 1900-01-01 = day -25_567. 1900-03-01 = +59 days (Jan 31 + Feb 28).
    assert_eq!(civil_from_days(-25_567 + 31 + 28), (1900, 3, 1));
}

#[test]
fn format_demo_timestamp_emits_iso_8601_seconds() {
    use std::time::{Duration, UNIX_EPOCH};
    // 2026-05-11 00:00:00 UTC = 1_778_457_600 (cross-checked via
    // `date -u -d "2026-05-11" +%s`). Add 19h53m47s = 71_627s.
    let t = UNIX_EPOCH + Duration::from_secs(1_778_457_600 + 71_627);
    assert_eq!(format_demo_timestamp(t), "2026-05-11 19:53:47");
}

#[test]
fn format_demo_timestamp_pads_single_digit_fields() {
    use std::time::{Duration, UNIX_EPOCH};
    // 2026-01-05 03:07:09 UTC.
    // 2026-01-01 base then +4 days. Seconds-in-day = 3*3600 + 7*60 + 9.
    let secs_2026_jan_1: u64 = 1_767_225_600; // verified: 2026-01-01 00:00:00 UTC
    let t = UNIX_EPOCH + Duration::from_secs(secs_2026_jan_1 + 4 * 86_400 + 3 * 3600 + 7 * 60 + 9);
    assert_eq!(format_demo_timestamp(t), "2026-01-05 03:07:09");
}
