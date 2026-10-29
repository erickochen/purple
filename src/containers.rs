use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{error, info};

use serde::{Deserialize, Serialize};

use crate::ssh_context::{OwnedSshContext, SshContext};

// ---------------------------------------------------------------------------
// ContainerInfo model
// ---------------------------------------------------------------------------

/// Metadata for a single container (from `docker ps -a` / `podman ps -a`).
///
/// Deserialization is tolerant of both docker and podman JSON shapes.
/// Docker uses `ID` plus scalar `Names`/`Ports`; podman uses `Id` plus
/// `Names` as an array and `Ports` as an array of objects. The custom
/// helpers below coerce both into the docker-shaped scalar fields the
/// rest of purple (UI, cache, MCP) already understands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerInfo {
    #[serde(rename = "ID", alias = "Id")]
    pub id: String,
    #[serde(rename = "Names", deserialize_with = "deserialize_names_field")]
    pub names: String,
    #[serde(rename = "Image")]
    pub image: String,
    #[serde(rename = "State")]
    pub state: String,
    #[serde(rename = "Status", default)]
    pub status: String,
    // `default` covers the missing-key case directly via Default::default()
    // and bypasses `deserialize_with`. The custom deserializer therefore
    // only runs when `Ports` is present (scalar, array or explicit null).
    #[serde(
        rename = "Ports",
        deserialize_with = "deserialize_ports_field",
        default
    )]
    pub ports: String,
}

/// Accept `Names` as either a scalar string (docker) or an array of
/// strings (podman). Multiple names join with `,` to match docker's
/// own comma-joined rendering. Unexpected shapes (number, object,
/// null) propagate as serde errors; `parse_container_ps` drops the
/// offending row via `.ok()`, which is the right behaviour for a row
/// that has lost its identity.
fn deserialize_names_field<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NamesField {
        Scalar(String),
        Array(Vec<String>),
    }
    match NamesField::deserialize(deserializer)? {
        NamesField::Scalar(s) => Ok(s),
        NamesField::Array(arr) => Ok(arr.join(",")),
    }
}

/// Accept `Ports` as either a scalar string (docker) or an array of
/// port objects (podman). Podman entries are rendered into the same
/// `host_ip:host_port->container_port/proto` form docker emits, so
/// downstream UI rendering stays uniform. An explicit JSON null is
/// tolerated and produces an empty string: podman uses `null` to mean
/// "no ports published", which is semantically valid and the row must
/// remain visible.
fn deserialize_ports_field<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum PortsField {
        Scalar(String),
        Array(Vec<PodmanPort>),
    }
    match Option::<PortsField>::deserialize(deserializer)? {
        Some(PortsField::Scalar(s)) => Ok(s),
        Some(PortsField::Array(arr)) => Ok(format_podman_ports(&arr)),
        None => Ok(String::new()),
    }
}

#[derive(Deserialize)]
struct PodmanPort {
    #[serde(default)]
    host_ip: String,
    #[serde(default)]
    container_port: u32,
    #[serde(default)]
    host_port: u32,
    #[serde(default = "podman_port_default_range")]
    range: u32,
    #[serde(default)]
    protocol: String,
}

fn podman_port_default_range() -> u32 {
    1
}

fn format_podman_ports(ports: &[PodmanPort]) -> String {
    // ~24 chars per typical port entry. Pre-allocating avoids the
    // intermediate Vec<String> + repeated re-allocations that the prior
    // map/collect/join chain produced for compose stacks with many
    // published ports.
    let mut out = String::with_capacity(ports.len().saturating_mul(24));
    for (i, p) in ports.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_podman_port(p, &mut out);
    }
    out
}

fn write_podman_port(p: &PodmanPort, out: &mut String) {
    use std::fmt::Write as _;
    let protocol = if p.protocol.is_empty() {
        "tcp"
    } else {
        p.protocol.as_str()
    };
    if p.host_port != 0 {
        // Podman emits an empty `host_ip` for both IPv4 wildcard and IPv6
        // wildcard binds. Omit the prefix when unknown rather than
        // mis-claim IPv4. Concrete addresses (e.g. 127.0.0.1, ::1) render
        // verbatim with the docker `addr:port->...` form.
        if !p.host_ip.is_empty() {
            let _ = write!(out, "{}:", p.host_ip);
        }
        if p.range > 1 {
            let _ = write!(
                out,
                "{}-{}->",
                p.host_port,
                p.host_port.saturating_add(p.range.saturating_sub(1))
            );
        } else {
            let _ = write!(out, "{}->", p.host_port);
        }
    }
    if p.range > 1 {
        let _ = write!(
            out,
            "{}-{}",
            p.container_port,
            p.container_port.saturating_add(p.range.saturating_sub(1))
        );
    } else {
        let _ = write!(out, "{}", p.container_port);
    }
    let _ = write!(out, "/{protocol}");
}

/// Try to parse one NDJSON line into `ContainerInfo`. Returns `None`
/// for blank/non-JSON lines (MOTD/banner) without logging. JSON-shaped
/// lines that fail to match the schema log at debug level so missing
/// containers can be correlated to a concrete parse error rather than
/// guessed from a shrunken list.
fn try_parse_container_line(trimmed: &str) -> Option<ContainerInfo> {
    if trimmed.is_empty() {
        return None;
    }
    match serde_json::from_str(trimmed) {
        Ok(c) => Some(c),
        Err(e) if trimmed.starts_with('{') => {
            log::debug!(
                "[external] container parse: dropped JSON line: {} (err: {})",
                &trimmed[..trimmed.len().min(120)],
                e
            );
            None
        }
        Err(_) => None,
    }
}

/// Parse NDJSON output from `docker ps --format '{{json .}}'` or
/// `podman ps --format '{{json .}}'`. Used by tests and the public
/// crate API exposed via `lib.rs`; the live SSH path streams through
/// `parse_container_output` directly, so the binary build sees this
/// helper as unused and the lint must be silenced.
#[allow(dead_code)]
pub fn parse_container_ps(output: &str) -> Vec<ContainerInfo> {
    output
        .lines()
        .filter_map(|line| try_parse_container_line(line.trim()))
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
        return Err(crate::i18n::text(crate::messages::CONTAINER_ID_EMPTY).to_string());
    }
    for c in id.chars() {
        if !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.' {
            return Err(crate::messages::container_id_invalid_char(c));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Combined SSH command + output parsing
// ---------------------------------------------------------------------------

/// Build the SSH command string for listing containers. Output is the
/// container NDJSON, then the `##purple:engine##` sentinel, then the
/// daemon version on its own line. The version subcall is suffixed with
/// `|| true` so its failure cannot mask a `docker ps` error: the chain
/// surfaces ps's exit code, while a missing version line just yields
/// `engine_version: None` downstream.
///
/// The script runs through `/bin/sh` via [`crate::snippet::wrap_posix`], so
/// a fish or tcsh login shell hands it over instead of parsing it.
///
/// - `Some(Docker)` / `Some(Podman)`: direct listing for the known runtime.
/// - `None`: combined detection + listing with sentinel markers in one SSH call.
pub fn container_list_command(runtime: Option<ContainerRuntime>) -> String {
    crate::snippet::wrap_posix(container_list_script(runtime))
}

/// The POSIX script behind [`container_list_command`], before wrapping.
fn container_list_script(runtime: Option<ContainerRuntime>) -> &'static str {
    match runtime {
        Some(ContainerRuntime::Docker) => concat!(
            "docker ps -a --format '{{json .}}' && ",
            "echo '##purple:engine##' && ",
            "{ docker version --format '{{.Server.Version}}' 2>/dev/null || true; }"
        ),
        Some(ContainerRuntime::Podman) => concat!(
            "podman ps -a --format '{{json .}}' && ",
            "echo '##purple:engine##' && ",
            "{ podman version --format '{{.Server.Version}}' 2>/dev/null || true; }"
        ),
        None => concat!(
            "if command -v docker >/dev/null 2>&1; then ",
            "echo '##purple:docker##' && docker ps -a --format '{{json .}}' && ",
            "echo '##purple:engine##' && ",
            "{ docker version --format '{{.Server.Version}}' 2>/dev/null || true; }; ",
            "elif command -v podman >/dev/null 2>&1; then ",
            "echo '##purple:podman##' && podman ps -a --format '{{json .}}' && ",
            "echo '##purple:engine##' && ",
            "{ podman version --format '{{.Server.Version}}' 2>/dev/null || true; }; ",
            "else echo '##purple:none##'; fi"
        ),
    }
}

/// Parsed result of a container listing command. `engine_version` is the
/// daemon's `Server.Version` (best-effort, `None` when the version sub-call
/// failed or the remote runtime predates the engine sentinel).
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerListing {
    pub runtime: ContainerRuntime,
    pub engine_version: Option<String>,
    pub containers: Vec<ContainerInfo>,
}

/// Parse the stdout of a container listing command.
///
/// When sentinels are present (combined detection run): extract runtime from
/// the sentinel line, parse remaining lines as NDJSON. When `caller_runtime`
/// is provided (subsequent run with known runtime): parse all lines as NDJSON.
/// In both cases, `##purple:engine##` splits the listing from the optional
/// trailing daemon version line.
pub fn parse_container_output(
    output: &str,
    caller_runtime: Option<ContainerRuntime>,
) -> Result<ContainerListing, String> {
    let runtime = match output
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("##purple:") && (*l != "##purple:engine##"))
    {
        Some("##purple:none##") => {
            return Err(crate::i18n::text(crate::messages::CONTAINER_RUNTIME_MISSING).to_string());
        }
        Some("##purple:docker##") => ContainerRuntime::Docker,
        Some("##purple:podman##") => ContainerRuntime::Podman,
        Some(other) => return Err(crate::messages::container_unknown_sentinel(other)),
        None => match caller_runtime {
            Some(rt) => rt,
            None => return Err("No sentinel found and no runtime provided.".to_string()),
        },
    };

    // Bound the version capture to the first non-empty post-sentinel line.
    // A trailing logout banner or MOTD after `docker version` would
    // otherwise concat into the cached engine_version and surface as
    // "25.0.3\n-- session closed --" in the Runtime field.
    let mut engine_version: Option<String> = None;
    let mut after_engine = false;
    let mut containers: Vec<ContainerInfo> = Vec::new();
    // Stream-parse each NDJSON line during the sentinel sweep so we never
    // build an intermediate copy of the listing block. At 1000 containers
    // that intermediate buffer would cost ~300 KB and an extra `.lines()`
    // walk; this loop is O(lines) with zero auxiliary allocation.
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed == "##purple:engine##" {
            after_engine = true;
            continue;
        }
        if trimmed.starts_with("##purple:") {
            continue;
        }
        if after_engine {
            if !trimmed.is_empty() && engine_version.is_none() {
                engine_version = Some(trimmed.to_string());
            }
            continue;
        }
        if let Some(c) = try_parse_container_line(trimmed) {
            containers.push(c);
        }
    }

    // Fedora CoreOS, podman-machine and other distros symlink `docker` to
    // `podman`. Detection picks the docker branch but the JSON shape is
    // pure podman (array `Names`, lowercase `Id`). When that happens we
    // relabel the runtime so downstream consumers (MCP runtime field, host
    // detail label, sort/filter by runtime) match reality.
    let runtime = if matches!(runtime, ContainerRuntime::Docker) && looks_like_podman(output) {
        log::debug!(
            "[external] container detection: docker sentinel emitted podman-shaped JSON, relabeling runtime to Podman"
        );
        ContainerRuntime::Podman
    } else {
        runtime
    };

    log::debug!(
        "[external] container listing parsed: runtime={:?} version={:?} containers={}",
        runtime,
        engine_version,
        containers.len()
    );
    Ok(ContainerListing {
        runtime,
        engine_version,
        containers,
    })
}

/// Heuristic: does the raw `ps` output look like podman JSON?
/// Podman emits `"Names":[` (array) and `"Id":` (lowercase d) for every row.
/// Docker emits `"Names":"` (string) and `"ID":` (uppercase D). We sample the
/// first JSON-shaped non-sentinel line. The check is fast (substring scan)
/// and only matters when the docker sentinel was emitted by a podman shim.
/// Accepts both `"Names":[` and `"Names": [` (pretty-printed) so handwritten
/// test fixtures and any intermediate JSON formatter cannot defeat the
/// detector silently.
fn looks_like_podman(output: &str) -> bool {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("##purple:") || !trimmed.starts_with('{') {
            continue;
        }
        return trimmed.contains("\"Names\":[") || trimmed.contains("\"Names\": [");
    }
    false
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
    if lower.contains("remote host identification has changed")
        || (lower.contains("host key for") && lower.contains("has changed"))
    {
        log::debug!("[external] Host key CHANGED detected; returning HOST_KEY_CHANGED toast");
        crate::i18n::text(crate::messages::HOST_KEY_CHANGED).to_string()
    } else if lower.contains("host key verification failed")
        || lower.contains("no matching host key")
        || lower.contains("no ed25519 host key is known")
        || lower.contains("no rsa host key is known")
        || lower.contains("no ecdsa host key is known")
        || lower.contains("host key is not known")
    {
        log::debug!("[external] Host key UNKNOWN detected; returning HOST_KEY_UNKNOWN toast");
        crate::i18n::text(crate::messages::HOST_KEY_UNKNOWN).to_string()
    } else if lower.contains("command not found") {
        crate::i18n::text(crate::messages::CONTAINER_RUNTIME_NOT_FOUND).to_string()
    } else if crate::connection::auth_refused(stderr)
        && !crate::security_key::sign_failures(stderr).is_empty()
    {
        // The login itself failed on the key. Checked before the generic
        // refusal, because the docker group is not the answer here.
        log::debug!("[external] Security key did not sign; returning the PIN hint");
        crate::i18n::text(crate::messages::CONTAINER_SECURITY_KEY_PIN).to_string()
    } else if lower.contains("permission denied") || lower.contains("got permission denied") {
        crate::i18n::text(crate::messages::CONTAINER_PERMISSION_DENIED).to_string()
    } else if lower.contains("cannot connect to the docker daemon")
        || lower.contains("cannot connect to podman")
    {
        crate::i18n::text(crate::messages::CONTAINER_DAEMON_NOT_RUNNING).to_string()
    } else if lower.contains("connection refused") {
        crate::i18n::text(crate::messages::CONTAINER_CONNECTION_REFUSED).to_string()
    } else if lower.contains("no route to host") || lower.contains("network is unreachable") {
        crate::i18n::text(crate::messages::CONTAINER_HOST_UNREACHABLE).to_string()
    } else {
        crate::messages::container_command_failed(code.unwrap_or(1))
    }
}

/// Fetch container list synchronously via SSH.
/// Follows the `fetch_remote_listing` pattern.
pub fn fetch_containers(
    ctx: &SshContext<'_>,
    cached_runtime: Option<ContainerRuntime>,
) -> Result<ContainerListing, ContainerError> {
    let command = container_list_command(cached_runtime);
    let result = crate::snippet::run_snippet_ctx(ctx, &command, true);
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
            // A non-zero remote exit (docker missing, daemon down, ...) is a
            // routine remote status, not an ssh connection failure: keep it out
            // of the error level.
            log::log!(
                crate::connection::remote_exit_log_level(r.status.code().unwrap_or(-1)),
                "[external] Container fetch failed: alias={alias}: {msg}"
            );
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
    F: FnOnce(String, Result<ContainerListing, ContainerError>) + Send + 'static,
{
    std::thread::spawn(move || {
        let result = fetch_containers(&ctx.borrow(), cached_runtime);
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
            log::debug!(
                "[purple] container action {} blocked on alias={}: invalid container_id: {}",
                action.as_str(),
                ctx.alias,
                e
            );
            send(ctx.alias, action, Err(e));
            return;
        }
        let alias = &ctx.alias;
        info!(
            "[external] Container action: {} container={container_id} alias={alias}",
            action.as_str()
        );
        let command = container_action_command(runtime, action, &container_id);
        let result = crate::snippet::run_snippet_ctx(&ctx.borrow(), &command, true);
        match result {
            Ok(r) if r.status.success() => send(ctx.alias, action, Ok(())),
            Ok(r) => {
                let err = friendly_container_error(r.stderr.trim(), r.status.code());
                // Routine remote exit (already stopped, no such container, ...)
                // stays out of the error level; only an ssh transport failure
                // (255) is a genuine connection error.
                log::log!(
                    crate::connection::remote_exit_log_level(r.status.code().unwrap_or(-1)),
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
// ContainerInspect: subset of `docker inspect` output we surface in the UI
// ---------------------------------------------------------------------------

/// Parsed subset of `docker inspect <id>` (or `podman inspect`). Only the
/// fields purple's container detail panel renders are extracted; the rest
/// of the JSON document is discarded so cache size stays bounded.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ContainerInspect {
    pub exit_code: i32,
    pub oom_killed: bool,
    pub started_at: String,
    pub finished_at: String,
    pub created_at: String,
    /// `Some("healthy" | "unhealthy" | "starting")` when the image defines
    /// a HEALTHCHECK. `None` when no healthcheck is configured.
    pub health: Option<String>,
    pub restart_count: u32,
    pub command: Option<Vec<String>>,
    pub entrypoint: Option<Vec<String>>,
    pub env_count: usize,
    pub mount_count: usize,
    pub networks: Vec<NetworkInfo>,
    // Audit-relevant fields surfaced in the right-side detail panel.
    pub image_digest: Option<String>,
    pub restart_policy: Option<String>,
    pub user: Option<String>,
    pub privileged: bool,
    pub readonly_rootfs: bool,
    pub apparmor_profile: Option<String>,
    pub seccomp_profile: Option<String>,
    pub cap_add: Vec<String>,
    pub cap_drop: Vec<String>,
    pub mounts: Vec<MountInfo>,
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
    // Lifecycle / runtime details surfaced in the LIFECYCLE card.
    pub pid: Option<u32>,
    pub stop_signal: Option<String>,
    pub stop_timeout: Option<u32>,
    // App identity from OCI image labels (visible in APP card).
    pub image_version: Option<String>,
    pub image_revision: Option<String>,
    pub image_source: Option<String>,
    pub working_dir: Option<String>,
    pub hostname: Option<String>,
    // Resource constraints (RESOURCES card). 0 / None means unlimited.
    pub memory_limit: Option<u64>,
    pub cpu_limit_nanos: Option<u64>,
    pub pids_limit: Option<i64>,
    pub log_driver: Option<String>,
    // Network mode (NETWORK card): bridge / host / none / container:xyz.
    pub network_mode: Option<String>,
    // Healthcheck definition + recent stats (HEALTH card when present).
    pub health_test: Option<Vec<String>>,
    pub health_interval_ns: Option<u64>,
    pub health_failing_streak: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub name: String,
    pub ip_address: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MountInfo {
    pub source: String,
    pub destination: String,
    pub read_only: bool,
}

/// Build the SSH command string for inspecting a single container.
///
/// Callers MUST validate `container_id` with `validate_container_id` before
/// reaching this builder. Restricted to crate visibility so a future caller
/// outside this crate cannot accidentally skip the guard.
pub(crate) fn container_inspect_command(runtime: ContainerRuntime, container_id: &str) -> String {
    format!("{} inspect {}", runtime.as_str(), container_id)
}

/// Translate a non-zero docker/podman exit code into a short
/// human-readable hint. Returns `None` for codes without a well-known
/// meaning so the UI can fall back to the bare number. Exit 0 has no
/// entry because the detail panel only annotates failed exits.
/// Sources: docker docs + Linux signal table.
pub fn exit_code_meaning(code: i32) -> Option<&'static str> {
    match code {
        1 => Some("application error"),
        125 => Some("docker run failed"),
        126 => Some("command not executable"),
        127 => Some("command not found"),
        130 => Some("interrupted (SIGINT)"),
        137 => Some("killed (SIGKILL / OOM)"),
        139 => Some("segfault (SIGSEGV)"),
        143 => Some("terminated (SIGTERM)"),
        _ => None,
    }
}

/// Parse `docker inspect <id>` stdout into `ContainerInspect`. The command
/// always returns a JSON array; we take the first element. Missing fields
/// degrade to defaults rather than fail so a partial response still
/// renders something useful.
pub fn parse_container_inspect(output: &str) -> Result<ContainerInspect, String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Err(crate::i18n::text(crate::messages::CONTAINER_INSPECT_EMPTY).to_string());
    }
    let value: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|e| crate::messages::container_inspect_parse_failed(&e.to_string()))?;
    let entry = value
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| crate::i18n::text(crate::messages::CONTAINER_INSPECT_EMPTY).to_string())?;

    let state = &entry["State"];
    let config = &entry["Config"];
    let network_settings = &entry["NetworkSettings"];

    let exit_code = state["ExitCode"].as_i64().unwrap_or(0) as i32;
    // Podman 5.x and docker both emit `OOMKilled`. Podman 3.x (still the
    // packaged default on Ubuntu 22.04 LTS) emits `OomKilled`. Try both so
    // OOM-killed containers surface in the ATTENTION card regardless of
    // remote runtime version.
    let oom_killed = state["OOMKilled"]
        .as_bool()
        .or_else(|| state["OomKilled"].as_bool())
        .unwrap_or(false);
    let started_at = state["StartedAt"].as_str().unwrap_or("").to_string();
    let finished_at = state["FinishedAt"].as_str().unwrap_or("").to_string();
    let health = state
        .get("Health")
        .and_then(|h| h.get("Status"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    let restart_count = entry["RestartCount"].as_u64().unwrap_or(0) as u32;

    let command = config["Cmd"].as_array().map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect()
    });
    let entrypoint = config["Entrypoint"].as_array().map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect()
    });
    let env_count = config["Env"].as_array().map(|arr| arr.len()).unwrap_or(0);
    let mount_count = entry["Mounts"].as_array().map(|arr| arr.len()).unwrap_or(0);

    let networks = network_settings
        .get("Networks")
        .and_then(|n| n.as_object())
        .map(|map| {
            map.iter()
                .map(|(name, cfg)| NetworkInfo {
                    name: name.clone(),
                    ip_address: cfg
                        .get("IPAddress")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let host_config = &entry["HostConfig"];

    let image_digest = entry["Image"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let restart_policy = host_config
        .get("RestartPolicy")
        .and_then(|p| p.get("Name"))
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty() && *s != "no")
        .map(|s| s.to_string());
    let user = config["User"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let privileged = host_config["Privileged"].as_bool().unwrap_or(false);
    let readonly_rootfs = host_config["ReadonlyRootfs"].as_bool().unwrap_or(false);
    let apparmor_profile = host_config["AppArmorProfile"]
        .as_str()
        .or_else(|| entry["AppArmorProfile"].as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let seccomp_profile = host_config["SecurityOpt"].as_array().and_then(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str())
            .find_map(|s| s.strip_prefix("seccomp=").map(|v| v.to_string()))
    });
    let cap_add = host_config["CapAdd"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let cap_drop = host_config["CapDrop"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mounts = entry["Mounts"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|m| MountInfo {
                    source: m["Source"].as_str().unwrap_or("").to_string(),
                    destination: m["Destination"].as_str().unwrap_or("").to_string(),
                    read_only: !m["RW"].as_bool().unwrap_or(true),
                })
                .collect()
        })
        .unwrap_or_default();
    let labels = config.get("Labels").and_then(|l| l.as_object());
    let label = |key: &str| {
        labels
            .and_then(|l| l.get(key))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };
    let compose_project = label("com.docker.compose.project");
    let compose_service = label("com.docker.compose.service");
    let image_version = label("org.opencontainers.image.version");
    let image_revision = label("org.opencontainers.image.revision");
    let image_source = label("org.opencontainers.image.source");

    let created_at = entry["Created"].as_str().unwrap_or("").to_string();
    // State.Pid is `0` when the container is not running. Drop the zero so
    // the UI does not render a misleading "pid 0" row for exited rows.
    let pid = state["Pid"].as_u64().filter(|n| *n > 0).map(|n| n as u32);
    let hostname = config["Hostname"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let working_dir = config["WorkingDir"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let stop_signal = config["StopSignal"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let stop_timeout = config["StopTimeout"].as_u64().map(|n| n as u32);

    let network_mode = host_config["NetworkMode"]
        .as_str()
        .filter(|s| !s.is_empty() && *s != "default")
        .map(|s| s.to_string());
    // HostConfig.Memory is bytes, 0 = unlimited (drop). Same for NanoCpus.
    let memory_limit = host_config["Memory"].as_u64().filter(|n| *n > 0);
    let cpu_limit_nanos = host_config["NanoCpus"].as_u64().filter(|n| *n > 0);
    // PidsLimit is i64. 0 or -1 means unlimited; drop both.
    let pids_limit = host_config["PidsLimit"].as_i64().filter(|n| *n > 0);
    // LogConfig.Type defaults to "json-file" on docker. Always carry it
    // so the renderer can decide whether to surface "Logs" only when
    // non-default.
    let log_driver = host_config
        .get("LogConfig")
        .and_then(|l| l.get("Type"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let healthcheck = config.get("Healthcheck");
    let health_test = healthcheck
        .and_then(|h| h.get("Test"))
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty());
    let health_interval_ns = healthcheck
        .and_then(|h| h.get("Interval"))
        .and_then(|v| v.as_u64())
        .filter(|n| *n > 0);
    let health_failing_streak = state
        .get("Health")
        .and_then(|h| h.get("FailingStreak"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    Ok(ContainerInspect {
        exit_code,
        oom_killed,
        started_at,
        finished_at,
        created_at,
        health,
        restart_count,
        command,
        entrypoint,
        env_count,
        mount_count,
        networks,
        image_digest,
        restart_policy,
        user,
        privileged,
        readonly_rootfs,
        apparmor_profile,
        seccomp_profile,
        cap_add,
        cap_drop,
        mounts,
        compose_project,
        compose_service,
        pid,
        stop_signal,
        stop_timeout,
        image_version,
        image_revision,
        image_source,
        working_dir,
        hostname,
        memory_limit,
        cpu_limit_nanos,
        pids_limit,
        log_driver,
        network_mode,
        health_test,
        health_interval_ns,
        health_failing_streak,
    })
}

/// Parse a Docker `Up …` status string into a compact uptime label.
/// Returns `None` for any non-running state (Exited, Created, Restarting,
/// Paused without an `Up` prefix, empty). Cells render `<1m` for
/// sub-minute uptimes, `1m` / `5m` / `12h` / `5w` / `3mo` / `2y` otherwise.
/// Format follows Docker's `units.HumanDuration`.
pub fn parse_uptime_from_status(s: &str) -> Option<String> {
    let body = s.strip_prefix("Up ")?;
    let body = body.split('(').next()?.trim();
    if body == "Less than a second" {
        return Some("<1m".to_string());
    }
    if body == "About a minute" {
        return Some("1m".to_string());
    }
    if body == "About an hour" {
        return Some("1h".to_string());
    }
    let mut parts = body.split_whitespace();
    let count: u64 = parts.next()?.parse().ok()?;
    let unit = parts.next()?;
    let suffix = match unit {
        "second" | "seconds" => return Some("<1m".to_string()),
        "minute" | "minutes" => "m",
        "hour" | "hours" => "h",
        "day" | "days" => "d",
        "week" | "weeks" => "w",
        "month" | "months" => "mo",
        "year" | "years" => "y",
        _ => return None,
    };
    Some(format!("{count}{suffix}"))
}

/// Synchronously fetch + parse `container inspect`. Validates the
/// container ID before issuing the SSH call.
pub fn fetch_container_inspect(
    ctx: &SshContext<'_>,
    runtime: ContainerRuntime,
    container_id: &str,
) -> Result<ContainerInspect, String> {
    validate_container_id(container_id)?;
    let command = container_inspect_command(runtime, container_id);
    let result = crate::snippet::run_snippet_ctx(ctx, &command, true);
    match result {
        Ok(r) if r.status.success() => parse_container_inspect(&r.stdout),
        Ok(r) => Err(crate::messages::container_command_failed(
            r.status.code().unwrap_or(1),
        )),
        Err(e) => Err(e.to_string()),
    }
}

/// Spawn a background thread to run `container inspect`. Mirrors the
/// `spawn_container_listing` pattern so the call site looks identical.
pub fn spawn_container_inspect_listing<F>(
    ctx: OwnedSshContext,
    runtime: ContainerRuntime,
    container_id: String,
    send: F,
) where
    F: FnOnce(String, String, Result<ContainerInspect, String>) + Send + 'static,
{
    std::thread::spawn(move || {
        let result = fetch_container_inspect(&ctx.borrow(), runtime, &container_id);
        send(ctx.alias, container_id, result);
    });
}

/// Build the `<runtime> logs --tail <n> <id>` command. The
/// `--tail` cap is enforced server-side so the SSH stream stays
/// bounded even on a noisy container.
pub(crate) fn container_logs_command(
    runtime: ContainerRuntime,
    container_id: &str,
    tail: usize,
) -> String {
    format!("{} logs --tail {} {}", runtime.as_str(), tail, container_id)
}

/// Synchronously fetch logs and split into lines. Returns the raw
/// captured stdout split on `\n` so the renderer does not have to
/// re-parse. Empty trailing lines are dropped.
pub fn fetch_container_logs(
    ctx: &SshContext<'_>,
    runtime: ContainerRuntime,
    container_id: &str,
    tail: usize,
) -> Result<Vec<String>, String> {
    validate_container_id(container_id)?;
    let command = container_logs_command(runtime, container_id, tail);
    let result = crate::snippet::run_snippet_ctx(ctx, &command, true);
    match result {
        Ok(r) if r.status.success() => Ok(parse_log_output(&r.stdout, &r.stderr)),
        Ok(r) => Err(crate::messages::container_command_failed(
            r.status.code().unwrap_or(1),
        )),
        Err(e) => Err(e.to_string()),
    }
}

/// Merge stdout (app logs) and stderr (errors) into a single chronological
/// stream. Many container runtimes split levels across the two streams;
/// re-interleaving them is closer to what `docker logs` shows on a TTY.
/// Trailing blank lines are stripped from each stream before merging so a
/// stdout block that ends in a newline does not introduce a phantom empty
/// row between the two streams.
pub(crate) fn parse_log_output(stdout: &str, stderr: &str) -> Vec<String> {
    let mut lines: Vec<String> = stdout.lines().map(|s| s.to_string()).collect();
    while lines.last().map(|s| s.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    for s in stderr.lines() {
        lines.push(s.to_string());
    }
    while lines.last().map(|s| s.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines
}

/// Spawn a background thread to run `container logs`. Same shape as
/// `spawn_container_inspect_listing`. In demo mode the SSH call is
/// short-circuited with a deterministic synthetic log stream so the
/// logs viewer (and its `/` search) is exercisable without a remote.
pub fn spawn_container_logs_fetch<F>(
    ctx: OwnedSshContext,
    runtime: ContainerRuntime,
    container_id: String,
    container_name: String,
    tail: usize,
    send: F,
) where
    F: FnOnce(String, String, String, Result<Vec<String>, String>) + Send + 'static,
{
    if crate::demo_flag::is_demo() {
        let lines = demo_log_lines(&container_name, tail);
        log::debug!(
            "[purple] container_logs_fetch: demo short-circuit alias={} id={} lines={}",
            ctx.alias,
            container_id,
            lines.len()
        );
        send(ctx.alias, container_id, container_name, Ok(lines));
        return;
    }
    std::thread::spawn(move || {
        let result = fetch_container_logs(&ctx.borrow(), runtime, &container_id, tail);
        send(ctx.alias, container_id, container_name, result);
    });
}

/// Generate a deterministic stream of fake log lines for demo mode.
/// Mixes INFO / WARN / ERROR / DEBUG with realistic-looking content
/// (HTTP requests, DB pings, retries, timeouts) so the user can
/// usefully press `/` and find matches under `--demo`. Anchored to
/// `demo_flag::now_secs()` so timestamps stay stable across renders.
pub(crate) fn demo_log_lines(container_name: &str, tail: usize) -> Vec<String> {
    use std::time::{Duration, UNIX_EPOCH};
    // Cheap hash to fan out per-container variation without bringing
    // `rand` into the binary.
    let seed: u32 = container_name
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));

    // Templates rotate every line. Index 0 is the freshest log line.
    let templates: &[&str] = &[
        "INFO  [{}] handled GET /api/v1/health 200 in 14ms",
        "INFO  [{}] handled POST /api/v1/orders 201 in 38ms (user_id={user})",
        "DEBUG [{}] cache hit key=session:{user} ttl=3600",
        "INFO  [{}] handled GET /api/v1/users/{user} 200 in 11ms",
        "WARN  [{}] slow query detected duration=812ms statement=SELECT FROM orders",
        "INFO  [{}] connection pool size=12 idle=8 in_use=4",
        "DEBUG [{}] flushing metrics batch size=64",
        "INFO  [{}] handled GET /api/v1/inventory 200 in 22ms",
        "ERROR [{}] upstream timeout after 5000ms target=payments retry=1",
        "WARN  [{}] retrying request attempt=2 backoff=250ms",
        "INFO  [{}] handled POST /api/v1/login 200 in 31ms",
        "DEBUG [{}] gc cycle reclaimed=42MB took=18ms",
        "INFO  [{}] heartbeat ok rss=128MB cpu=4%",
        "ERROR [{}] failed to acquire lock resource=cache_warmer waiter=3",
        "INFO  [{}] handled DELETE /api/v1/sessions/{user} 204 in 9ms",
        "WARN  [{}] disk usage at 78% mount=/data threshold=80%",
        "INFO  [{}] handled GET /api/v1/search?q=widget 200 in 47ms",
        "DEBUG [{}] websocket ping rtt=12ms",
    ];

    // Always use `demo_flag::now_secs()` even outside demo mode: the
    // helper's OnceLock caches the first wallclock value, so repeated
    // calls within a process return the same instant. Branching on
    // `is_demo()` would let tests cross a second boundary and flake.
    let now = crate::demo_flag::now_secs();

    // Map each line to a timestamp working backwards from now. One log
    // every 3 seconds keeps the time range realistic for a 200-line tail.
    let mut lines = Vec::with_capacity(tail);
    for i in 0..tail {
        let template = templates[(i + seed as usize) % templates.len()];
        let user = 1000 + ((seed as usize + i * 7) % 50);
        let secs_back = (i as u64) * 3;
        let line_time = UNIX_EPOCH + Duration::from_secs(now.saturating_sub(secs_back));
        let ts = format_demo_timestamp(line_time);
        let body = template
            .replace("{}", container_name)
            .replace("{user}", &user.to_string());
        lines.push(format!("{} {}", ts, body));
    }
    // Render flush-top with the newest line at the bottom of the
    // viewport, matching real `docker logs --tail` output.
    lines.reverse();
    lines
}

fn format_demo_timestamp(t: std::time::SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Fast UTC breakdown without dragging in chrono. Good enough for a
    // demo timestamp where leap seconds / DST are irrelevant.
    let days_since_epoch = (secs / 86_400) as i64;
    let seconds_in_day = (secs % 86_400) as u32;
    let h = seconds_in_day / 3600;
    let m = (seconds_in_day % 3600) / 60;
    let s = seconds_in_day % 60;
    let (y, mo, d) = civil_from_days(days_since_epoch);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, m, s)
}

/// Convert days-since-1970-01-01 to (year, month, day). Howard Hinnant's
/// civil_from_days algorithm — proleptic Gregorian, 1970 = day 0.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

// ---------------------------------------------------------------------------
// JSON lines cache
// ---------------------------------------------------------------------------

/// A cached container listing for a single host. `engine_version` is the
/// daemon's `Server.Version` captured during the last refresh, surfaced in
/// the host detail panel; `None` means the version sub-call did not return
/// or the cache was written by an older purple build.
#[derive(Debug, Clone)]
pub struct ContainerCacheEntry {
    pub timestamp: u64,
    pub runtime: ContainerRuntime,
    pub engine_version: Option<String>,
    pub containers: Vec<ContainerInfo>,
}

/// Serde helper for a single JSON line in the cache file. `engine_version`
/// uses `serde(default)` so cache files written before this field existed
/// still deserialize cleanly.
#[derive(Serialize, Deserialize)]
struct CacheLine {
    alias: String,
    timestamp: u64,
    runtime: ContainerRuntime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    engine_version: Option<String>,
    containers: Vec<ContainerInfo>,
}

fn cache_path(paths: Option<&crate::runtime::env::Paths>) -> Option<std::path::PathBuf> {
    paths.map(crate::runtime::env::Paths::container_cache)
}

/// Load container cache from `~/.purple/container_cache.jsonl`.
/// Malformed lines are silently ignored. Duplicate aliases: last-write-wins.
pub fn load_container_cache(
    paths: Option<&crate::runtime::env::Paths>,
) -> HashMap<String, ContainerCacheEntry> {
    let mut map = HashMap::new();
    let Some(path) = cache_path(paths) else {
        return map;
    };
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
                    engine_version: entry.engine_version,
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
                    engine_version: entry.engine_version,
                    containers: entry.containers,
                },
            );
        }
    }
    map
}

/// Save container cache to `~/.purple/container_cache.jsonl` via atomic write.
pub fn save_container_cache(
    paths: Option<&crate::runtime::env::Paths>,
    cache: &HashMap<String, ContainerCacheEntry>,
) {
    if crate::demo_flag::is_demo() {
        return;
    }
    let Some(path) = cache_path(paths) else {
        return;
    };
    let mut lines = Vec::with_capacity(cache.len());
    for (alias, entry) in cache {
        let line = CacheLine {
            alias: alias.clone(),
            timestamp: entry.timestamp,
            runtime: entry.runtime,
            engine_version: entry.engine_version.clone(),
            containers: entry.containers.clone(),
        };
        if let Ok(s) = serde_json::to_string(&line) {
            lines.push(s);
        }
    }
    let content = lines.join("\n");
    log::debug!(
        "[purple] save_container_cache: {} host entries, {} bytes -> {}",
        cache.len(),
        content.len(),
        path.display()
    );
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

/// Format a duration in seconds as a compact label (`12s`, `5m`,
/// `2h`, `3d`). Used for the in-border staleness badge where width
/// is precious and the surrounding label (`synced`) already says
/// "ago" without the suffix.
pub fn format_uptime_short(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86400)
    }
}

/// Format a Unix timestamp as a human-readable relative time string.
/// Honours `demo_flag::now_secs()` when demo mode is active so visual
/// regression goldens stay byte-stable across long-running test
/// processes (same pattern as `history::format_time_ago`).
pub fn format_relative_time(timestamp: u64) -> String {
    let now = if crate::demo_flag::is_demo() {
        crate::demo_flag::now_secs()
    } else {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    };
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
