//! Container-related user-facing strings: per-runtime listing errors,
//! container library validation, exec/logs/restart/stop confirms,
//! stack restart, host-wide bulk actions, refresh batch progress and
//! SCP copy labels (the SCP transfer originates from the file browser
//! overlay so it shares the file-transfer subprocess context).

// ── Container library errors ────────────────────────────────────────
//
// Validation (`validate_container_id`) errors propagate via the
// `ContainerActionComplete` event and become toasts. The "no runtime"
// and "unknown sentinel" lines surface in the same path.

pub const CONTAINER_ID_EMPTY: &str = "Container ID must not be empty.";
pub const CONTAINER_RUNTIME_MISSING: &str = "No container runtime found. Install Docker or Podman.";

pub fn container_id_invalid_char(c: char) -> String {
    format!("Container ID contains invalid character: '{c}'")
}

pub fn container_unknown_sentinel(s: &str) -> String {
    format!("Unknown sentinel: {s}")
}

pub fn container_invalid_id(reason: &str) -> String {
    format!("Container exec blocked: {reason}")
}

/// Transient label shown on the file browser overlay while an scp transfer
/// is running. Singular form for a single source.
pub fn scp_copying_one(source: &str) -> String {
    format!("Copying {}...", source)
}

/// Transient label shown on the file browser overlay while an scp transfer
/// is running. Plural form when multiple files were selected at once.
pub fn scp_copying_many(count: usize) -> String {
    format!("Copying {} files...", count)
}

/// Toast shown when scp exited non-zero with no captured stderr to relay.
/// The exit code is the only signal we have left.
pub fn scp_failed_exit_code(code: i32) -> String {
    format!("Copy failed (exit code {}).", code)
}

/// Toast shown when the scp subprocess itself failed to spawn or wait
/// (e.g. binary missing, signal interrupted), distinct from a non-zero
/// exit which uses `scp_failed_exit_code`.
pub fn scp_spawn_failed(e: &impl std::fmt::Display) -> String {
    format!("scp failed: {}", e)
}

// ── Containers ──────────────────────────────────────────────────────

pub fn container_action_complete(action: &str) -> String {
    format!("Container {} complete.", action)
}

pub const HOST_KEY_UNKNOWN: &str = "Host key unknown. Connect first (Enter) to trust the host.";
pub const HOST_KEY_CHANGED: &str =
    "Host key changed. Possible tampering or server re-install. Clear with ssh-keygen -R.";

// User-friendly classifications of stderr from a remote `docker ps` /
// `podman ps`. The raw stderr is too technical and varies across
// distros; these phrasings give the user the actionable next step.
pub const CONTAINER_RUNTIME_NOT_FOUND: &str = "Docker or Podman not found on remote host.";
pub const CONTAINER_PERMISSION_DENIED: &str =
    "Permission denied. Is your user in the docker group?";
/// The login needs a security key PIN, which a container call cannot ask for.
pub const CONTAINER_SECURITY_KEY_PIN: &str = "This host signs in with a security key PIN. Enter it once from the file browser or a tunnel, then refresh.";
pub const CONTAINER_DAEMON_NOT_RUNNING: &str = "Container daemon is not running.";
pub const CONTAINER_CONNECTION_REFUSED: &str = "Connection refused.";
pub const CONTAINER_HOST_UNREACHABLE: &str = "Host unreachable.";

/// Generic fallback when none of the container error classifiers
/// matched. The exit code is the only signal we can show without
/// leaking unfiltered remote stderr.
pub fn container_command_failed(code: i32) -> String {
    format!("Command failed with code {}.", code)
}

/// `docker inspect` returned no JSON (empty array or empty stdout).
pub const CONTAINER_INSPECT_EMPTY: &str = "Inspect returned no data.";

/// `docker inspect` stdout was not valid JSON.
pub fn container_inspect_parse_failed(reason: &str) -> String {
    format!("Inspect parse failed: {}", reason)
}

// ── Container exec (Enter on containers overview) ──────────────────

/// User pressed Enter on a non-running container.
pub fn container_not_running(name: &str) -> String {
    format!("{} is not running. Cannot exec.", name)
}

/// Demo mode interactive guard.
pub const DEMO_CONTAINER_EXEC_DISABLED: &str = "Demo mode: container exec disabled.";

/// Tmux mode opened a new window for the exec session.
pub fn container_exec_opened_in_tmux(name: &str, alias: &str) -> String {
    format!("Opened {} on {} in tmux window.", name, alias)
}

/// Interactive shell exited cleanly.
pub fn container_exec_ended(name: &str) -> String {
    format!("Container shell ended: {}.", name)
}

/// Interactive shell failed with a parsed stderr reason.
pub fn container_exec_failed_with_reason(name: &str, reason: &str) -> String {
    format!("Container exec failed for {}: {}", name, reason)
}

/// Interactive shell exited non-zero with no stderr reason.
pub fn container_exec_exited_with_code(name: &str, code: i32) -> String {
    format!("Container exec for {} exited with code {}.", name, code)
}

/// `Command::new("ssh").spawn()` failed.
pub fn container_exec_spawn_failed(name: &str) -> String {
    format!("Failed to launch ssh for container {}.", name)
}

/// Exec prompt rejected the typed command (control chars, newline).
pub const CONTAINER_EXEC_INVALID_COMMAND: &str =
    "Command rejected: control characters not allowed.";

// ── Container logs (l) ─────────────────────────────────────────────

/// Title shown in the logs overlay border for "logs are loading".
pub const CONTAINER_LOGS_LOADING: &str = "fetching logs…";

/// Title for "logs are ready". Uses the short relative-time format
/// (12s, 5m, 2h) so the badge stays compact regardless of staleness.
pub fn container_logs_fetched(secs_ago: u64) -> String {
    format!(
        "fetched {} ago",
        crate::containers::format_uptime_short(secs_ago)
    )
}

/// Title for "logs fetch failed".
pub fn container_logs_failed(reason: &str) -> String {
    format!("logs fetch failed: {}", reason)
}

/// Search position badge for the logs overlay: `3 of 12` while the
/// user navigates `/foo` matches with n/N.
pub fn container_logs_search_position(current: usize, total: usize) -> String {
    format!("{} of {}", current, total)
}

/// Search badge when the query has no hits in the current body.
pub const CONTAINER_LOGS_SEARCH_NO_MATCHES: &str = "no matches";

// ── Container restart/stop (K / S) ─────────────────────────────────

/// Confirm body line that summarises a destructive action's mechanics.
pub const CONTAINER_RESTART_BODY: &str =
    "Sends SIGTERM, waits 10s, then SIGKILL. Live connections will drop.";
pub const CONTAINER_STOP_BODY: &str = "Sends SIGTERM, waits 10s, then SIGKILL. Container will not restart unless its policy reschedules it.";

// ── Container stack restart (Ctrl-K) ───────────────────────────────

pub fn container_stack_unknown(name: &str) -> String {
    format!("Stack unknown for {}: open the detail panel first.", name)
}

pub fn container_stack_no_running(project: &str) -> String {
    format!("Stack {} has no running members to restart.", project)
}

pub const CONTAINER_STACK_RESTART_BODY: &str = "Restart cycles every running member one by one. Exited members are not touched. Live connections will drop.";

// ── Container host-wide bulk actions (K / S on a divider) ──────────

/// Body line on the bulk-restart-host confirm dialog. Same mechanics
/// as a single restart but spelled out so the user knows it walks the
/// host one container at a time.
pub const CONTAINER_HOST_RESTART_ALL_BODY: &str = "Restart cycles every running container on the host one by one. Exited containers are not touched. Live connections will drop.";

/// Body line on the bulk-stop-host confirm dialog.
pub const CONTAINER_HOST_STOP_ALL_BODY: &str = "Stops every running container on the host one by one. Exited containers are not touched. Restart policies may reschedule them.";

/// Footer toast when the user presses a single-target action key (l, e)
/// while the cursor is parked on a host-divider row. Steers the user
/// back to a container row instead of silently no-op'ing. `action` is
/// lowercased for sentence-case readability ("logs needs..." reads
/// better than "Logs applies...").
pub fn container_action_needs_single(action: &str) -> String {
    format!(
        "{} need a single container. Place the cursor on a container row.",
        action.to_lowercase()
    )
}

/// Toast when bulk K/S on a divider finds no running containers.
pub fn container_host_no_running(alias: &str) -> String {
    format!("No running containers on {}.", alias)
}

// ── Container refresh (r / R / a) ──────────────────────────────────

/// `r` keypress: single-host refresh started.
pub fn container_refreshing(alias: &str) -> String {
    format!("Refreshing {}…", alias)
}

/// `R` keypress while a previous batch is still in flight.
pub const REFRESH_BATCH_ALREADY_RUNNING: &str = "Refresh already in progress.";

/// `R` keypress on an empty container cache.
pub const REFRESH_NOTHING_TO_REFRESH: &str = "No cached hosts to refresh. Press 'a' to add a host.";

/// Batch progress readout shown in the status footer.
pub fn container_refresh_progress(done: usize, total: usize) -> String {
    format!("Refreshing {}/{} hosts…", done, total)
}

/// Batch completed.
pub fn container_refresh_complete(total: usize) -> String {
    format!(
        "Refreshed {} host{}.",
        total,
        if total == 1 { "" } else { "s" }
    )
}

/// Host picker: no hosts match the live query.
pub const CONTAINER_HOST_PICKER_NO_MATCH: &str = "No hosts match.";

/// Host picker: every host already has a cache entry.
pub const CONTAINER_HOST_PICKER_NOTHING_TO_ADD: &str =
    "All hosts already cached. Use 'r' or 'R' to refresh.";
