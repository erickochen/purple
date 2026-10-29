use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use log::{debug, error, info, warn};

use crate::ssh_launcher::SshLauncher;

/// Result of an SSH connection attempt.
#[derive(Debug)]
pub struct ConnectResult {
    pub status: std::process::ExitStatus,
    pub stderr_output: String,
}

/// Returns true if the process is running inside a tmux session, per the
/// resolved environment (so tests inject `TMUX` via `Env` instead of mutating
/// the process env).
#[cfg(unix)]
pub fn is_in_tmux(env: &crate::runtime::env::Env) -> bool {
    env.in_tmux()
}

/// Returns true if the current process is running inside a tmux session.
#[cfg(not(unix))]
pub fn is_in_tmux(_env: &crate::runtime::env::Env) -> bool {
    false
}

/// Open an SSH connection in a new tmux window.
/// Returns immediately after the window is created. The SSH session runs
/// asynchronously in the new window. Returns an error if tmux is not
/// available or the window cannot be created.
///
/// This path deliberately does not wire up SSH_ASKPASS. The caller in `main.rs`
/// guards this with `askpass.is_none()`, because an askpass-backed host needs an
/// inherited stdin (so purple's askpass subprocess can print back to the ssh
/// parent) and that inheritance does not survive the `tmux new-window` fork.
/// Hosts with a password source therefore keep using the suspend-TUI `connect()`
/// flow instead.
pub fn connect_tmux_window(
    alias: &str,
    config_path: &Path,
    has_active_tunnel: bool,
    launcher: &SshLauncher,
) -> Result<()> {
    info!("[external] SSH connection via tmux: {alias}");

    let config_str = config_path
        .to_str()
        .context("SSH config path is not valid UTF-8")?;

    let args = tmux_window_args(alias, launcher, config_str, has_active_tunnel, alias, None);

    debug!("[external] tmux args: {:?}", args);

    let status = Command::new("tmux")
        .args(&args)
        .status()
        .with_context(|| format!("Failed to launch tmux new-window for '{alias}'"))?;

    if status.success() {
        info!("[external] tmux window created: {alias}");
        Ok(())
    } else {
        let code = status.code().unwrap_or(-1);
        error!("[external] tmux new-window failed for {alias} (exit {code})");
        anyhow::bail!("tmux new-window exited with code {code}")
    }
}

/// Argument list for `tmux new-window` that runs the launcher inside a fresh
/// window. `remote_command` adds `-t` plus the command, as the exec path needs.
fn tmux_window_args(
    window_label: &str,
    launcher: &SshLauncher,
    config_str: &str,
    has_active_tunnel: bool,
    alias: &str,
    remote_command: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = ["new-window", "-n", window_label, "--"]
        .into_iter()
        .map(str::to_string)
        .collect();
    args.extend(launcher.words().iter().cloned());
    args.push("-F".to_string());
    args.push(config_str.to_string());
    if remote_command.is_some() {
        args.push("-t".to_string());
    }
    if has_active_tunnel {
        args.push("-o".to_string());
        args.push("ClearAllForwardings=yes".to_string());
    }
    args.push("--".to_string());
    args.push(alias.to_string());
    if let Some(cmd) = remote_command {
        args.push(cmd.to_string());
    }
    args
}

/// RAII guard that restores the signal mask when dropped.
/// Ensures SIGINT/SIGTSTP are unmasked even on early return or error.
#[cfg(unix)]
struct SignalMaskGuard {
    old: libc::sigset_t,
}

#[cfg(unix)]
impl SignalMaskGuard {
    /// Block SIGINT and SIGTSTP, saving the previous mask for restore on drop.
    fn block_interactive() -> Self {
        // SAFETY: `old` and `mask` are stack-allocated `sigset_t`s zeroed before
        // use. The libc sigset / sigprocmask calls only read/write these
        // pointers, which are valid for the duration of this block. `old` is
        // moved into `Self` so the mask can be restored on drop.
        unsafe {
            let mut old: libc::sigset_t = std::mem::zeroed();
            let mut mask: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            libc::sigaddset(&mut mask, libc::SIGINT);
            libc::sigaddset(&mut mask, libc::SIGTSTP);
            libc::sigprocmask(libc::SIG_BLOCK, &mask, &mut old);
            Self { old }
        }
    }
}

#[cfg(unix)]
impl Drop for SignalMaskGuard {
    fn drop(&mut self) {
        // SAFETY: `self.old` is a valid `sigset_t` captured by
        // `block_interactive`. `pending` is zeroed before `sigpending` writes
        // to it. `libc::signal` is called with valid signal numbers. The
        // sigprocmask call restores the previously-saved mask, which is still
        // live for the duration of this drop.
        unsafe {
            // Discard any pending SIGINT/SIGTSTP that arrived while masked.
            // Without this, queued signals would fire immediately on unmask and
            // kill/suspend purple before the TUI can be restored.
            let mut pending: libc::sigset_t = std::mem::zeroed();
            libc::sigpending(&mut pending);
            let has_sigint = libc::sigismember(&pending, libc::SIGINT) == 1;
            let has_sigtstp = libc::sigismember(&pending, libc::SIGTSTP) == 1;
            // Temporarily ignore pending signals so they're consumed on unmask.
            if has_sigint {
                libc::signal(libc::SIGINT, libc::SIG_IGN);
            }
            if has_sigtstp {
                libc::signal(libc::SIGTSTP, libc::SIG_IGN);
            }
            libc::sigprocmask(libc::SIG_SETMASK, &self.old, std::ptr::null_mut());
            // Restore default handlers after pending signals are consumed.
            if has_sigint {
                libc::signal(libc::SIGINT, libc::SIG_DFL);
            }
            if has_sigtstp {
                libc::signal(libc::SIGTSTP, libc::SIG_DFL);
            }
        }
    }
}

/// Spawn `cmd`, mask interactive signals in the parent, tee SSH's
/// stderr to the real stderr while capturing it for error detection,
/// then wait for the child to exit. Both `connect` and
/// `connect_with_remote_command` build their `Command` independently
/// (different argv) and delegate the spawn/wait/tee plumbing here so
/// the long stderr-buffer + signal-guard sequence lives in one place.
///
/// `log_label` is interpolated into the started/ended/failed log lines
/// so a reader can tell host-login from container-exec at a glance.
///
/// Exit 255 is ssh's own failure (auth, network, host-key). Any other code is
/// either a remote-command exit status or a signal-kill (`-1`), both routine.
pub(crate) fn is_ssh_transport_failure(code: i32) -> bool {
    code == 255
}

/// Intended log severity for an ssh exit code. The single source of truth for
/// the started/ended/failed routing in `spawn_ssh_and_wait` and the MCP
/// `run_command` tool, so the error-vs-debug decision is testable as data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SshExitClass {
    /// Exit 0: the command ran and succeeded.
    Success,
    /// Exit 255: ssh's own transport failure (auth, network, host-key). Error.
    TransportFailure,
    /// Any other non-zero code: the remote command's own status. Routine.
    RemoteStatus,
}

pub(crate) fn classify_ssh_exit(code: i32) -> SshExitClass {
    if code == 0 {
        SshExitClass::Success
    } else if is_ssh_transport_failure(code) {
        SshExitClass::TransportFailure
    } else {
        SshExitClass::RemoteStatus
    }
}

/// Log level for a failed ssh-backed remote command. Only ssh's own transport
/// failure (exit 255) is an actionable error; a routine non-zero remote status
/// is debug noise. Call sites that run a remote command and branch on the exit
/// code (container list/actions, ...) route their failure log through here so
/// the error level stays reserved for genuine connection failures.
pub(crate) fn remote_exit_log_level(code: i32) -> log::Level {
    match classify_ssh_exit(code) {
        SshExitClass::TransportFailure => log::Level::Error,
        SshExitClass::Success | SshExitClass::RemoteStatus => log::Level::Debug,
    }
}

fn spawn_ssh_and_wait(
    mut cmd: Command,
    alias: &str,
    log_label: &str,
    program: &str,
) -> Result<ConnectResult> {
    cmd.stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::piped());

    // Reset signal mask in the child process so SSH receives Ctrl+C
    // normally. We mask signals in the parent AFTER spawn so the
    // child doesn't inherit the blocked mask.
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            let mut mask: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            libc::sigprocmask(libc::SIG_SETMASK, &mask, std::ptr::null_mut());
            Ok(())
        });
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to launch {program} {log_label} for '{alias}'"))?;

    // Mask SIGINT/SIGTSTP in purple AFTER spawn so SSH doesn't inherit
    // the blocked mask. The guard restores the mask on drop (even on
    // early return).
    #[cfg(unix)]
    let _signal_guard = SignalMaskGuard::block_interactive();

    let stderr_pipe = child.stderr.take().expect("stderr was piped");
    let stderr_thread = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let mut captured = Vec::new();
        let mut buf = [0u8; 4096];
        let mut reader = stderr_pipe;
        let mut stderr_out = std::io::stderr();
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = stderr_out.write_all(&buf[..n]);
                    let _ = stderr_out.flush();
                    captured.extend_from_slice(&buf[..n]);
                }
                Err(_) => break,
            }
        }
        String::from_utf8_lossy(&captured).to_string()
    });

    let status = child
        .wait()
        .with_context(|| format!("Failed to wait for {program} {log_label} for '{alias}'"))?;
    let stderr_output = stderr_thread.join().unwrap_or_else(|_| {
        warn!("[purple] Stderr capture thread panicked for {alias}");
        String::new()
    });

    let code = status.code().unwrap_or(-1);
    match classify_ssh_exit(code) {
        SshExitClass::Success => {
            info!("[external] SSH {} ended: {alias} (exit 0)", log_label);
        }
        SshExitClass::TransportFailure => {
            error!("[external] SSH {} failed: {alias} (exit {code})", log_label);
            if !stderr_output.is_empty() {
                let stderr = stderr_output.trim();
                let lower = stderr.to_lowercase();
                if lower.contains("are too open") || lower.contains("bad permissions") {
                    warn!("[config] SSH key permission issue: {stderr}");
                } else {
                    debug!("[external] SSH stderr: {stderr}");
                }
            }
        }
        SshExitClass::RemoteStatus => {
            // Remote-command status or a signal-kill: routine flow, not a
            // must-act error.
            debug!("[external] SSH {} ended: {alias} (exit {code})", log_label);
        }
    }

    Ok(ConnectResult {
        status,
        stderr_output,
    })
}

/// Launch an SSH connection to the given host alias.
/// Runs the `launcher` program (plain `ssh` unless the user configured a
/// wrapper) with inherited stdin/stdout. Stderr is piped and forwarded to real
/// stderr in real time so the output is captured for error detection.
/// Passes `-F <config_path>` so the alias resolves against the correct config file.
/// When `askpass` is Some, delegates to `askpass_env::configure_ssh_command` to wire up
/// SSH_ASKPASS, SSH_ASKPASS_REQUIRE=force and the PURPLE_* env vars.
pub fn connect(
    alias: &str,
    config_path: &Path,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    has_active_tunnel: bool,
    launcher: &SshLauncher,
) -> Result<ConnectResult> {
    info!("[external] SSH connection started: {alias}");
    debug!(
        "[external] SSH command: {} -F {} -- {alias}",
        launcher.display(),
        config_path.display()
    );

    let mut cmd = launcher.command();
    cmd.arg("-F").arg(config_path);

    // When a tunnel is already running for this host, disable forwards in the
    // interactive session to avoid "Address already in use" bind conflicts.
    if has_active_tunnel {
        cmd.arg("-o").arg("ClearAllForwardings=yes");
    }

    cmd.arg("--").arg(alias);

    if askpass.is_some() {
        crate::askpass_env::configure_ssh_command(&mut cmd, alias, config_path);
    }

    if let Some(token) = bw_session {
        cmd.env("BW_SESSION", token);
    }

    spawn_ssh_and_wait(cmd, alias, "connection", launcher.program())
}

/// Launch an SSH connection that runs a single remote command in
/// interactive mode. Mirrors `connect()` exactly except for two
/// additions: `-t` to allocate a TTY (required for the remote shell
/// `docker exec` opens) and the trailing `remote_command` string passed
/// to ssh as one argv slot. The remote shell receives the string as a
/// single command line, so multi-token commands and shell operators
/// like `||` work naturally.
///
/// Used by the containers overview Enter handler: the `remote_command`
/// is built as `<runtime> exec -it <container_id> sh -c 'bash || sh'`
/// where `container_id` has already been validated to alphanumeric +
/// `-_.` so it cannot inject shell metacharacters.
#[allow(clippy::too_many_arguments)]
pub fn connect_with_remote_command(
    alias: &str,
    config_path: &Path,
    env: &crate::runtime::env::Env,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    has_active_tunnel: bool,
    remote_command: &str,
    launcher: &SshLauncher,
) -> Result<ConnectResult> {
    info!("[external] SSH exec started: {alias}");
    debug!(
        "[external] SSH command: {} -F {} -t -- {alias} {}",
        launcher.display(),
        config_path.display(),
        remote_command
    );

    // Renew the Vault SSH cert before exec'ing into a container so an
    // expired cert is refreshed, mirroring the interactive connect path.
    // No-op for non-vault hosts.
    crate::runtime::helpers::ensure_vault_cert_for_alias(env, alias, config_path);

    let mut cmd = launcher.command();
    cmd.arg("-F").arg(config_path).arg("-t");

    if has_active_tunnel {
        cmd.arg("-o").arg("ClearAllForwardings=yes");
    }

    cmd.arg("--").arg(alias).arg(remote_command);

    if askpass.is_some() {
        crate::askpass_env::configure_ssh_command(&mut cmd, alias, config_path);
    }

    if let Some(token) = bw_session {
        cmd.env("BW_SESSION", token);
    }

    spawn_ssh_and_wait(cmd, alias, "exec", launcher.program())
}

/// tmux variant of `connect_with_remote_command`. Opens a new tmux
/// window running `ssh -t <alias> <remote_command>` so the TUI stays
/// alive in the original window. Same askpass-incompatible caveat as
/// `connect_tmux_window`.
pub fn connect_tmux_window_with_remote_command(
    alias: &str,
    config_path: &Path,
    env: &crate::runtime::env::Env,
    has_active_tunnel: bool,
    remote_command: &str,
    window_label: &str,
    launcher: &SshLauncher,
) -> Result<()> {
    info!("[external] SSH exec via tmux: {alias}");

    // Renew the Vault SSH cert before exec'ing into a container so an
    // expired cert is refreshed, mirroring the interactive connect path.
    // No-op for non-vault hosts.
    crate::runtime::helpers::ensure_vault_cert_for_alias(env, alias, config_path);

    let config_str = config_path
        .to_str()
        .context("SSH config path is not valid UTF-8")?;

    let args = tmux_window_args(
        window_label,
        launcher,
        config_str,
        has_active_tunnel,
        alias,
        Some(remote_command),
    );

    debug!("[external] tmux exec args: {:?}", args);

    let status = Command::new("tmux")
        .args(&args)
        .status()
        .with_context(|| format!("Failed to launch tmux exec window for '{alias}'"))?;

    if status.success() {
        info!("[external] tmux exec window created: {alias}");
        Ok(())
    } else {
        let code = status.code().unwrap_or(-1);
        error!("[external] tmux exec window failed for {alias} (exit {code})");
        anyhow::bail!("tmux new-window exited with code {code}")
    }
}

/// Extract a concise reason from SSH stderr for display in the toast.
/// Joins all non-empty, non-banner lines with ` | ` so the full context
/// is visible. Truncates to 200 chars (char-safe) if needed.
pub fn stderr_summary(stderr: &str) -> Option<String> {
    let summary: String = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('@'))
        .filter(|l| {
            !(l.starts_with("Warning: Permanently added ")
                && l.ends_with("to the list of known hosts."))
        })
        .collect::<Vec<_>>()
        .join(" | ");
    if summary.is_empty() {
        return None;
    }
    if summary.len() > 200 {
        let truncated: String = summary.chars().take(197).collect();
        Some(format!("{truncated}..."))
    } else {
        Some(summary)
    }
}

/// True when ssh gave up on authentication and a typed password could still
/// help: the method list behind `Permission denied (` names `password` or
/// `keyboard-interactive`. A bare `(publickey)` is false, because the server
/// takes no password at all.
pub fn needs_password(stderr: &str) -> bool {
    stderr.split("Permission denied (").skip(1).any(|rest| {
        let methods = rest.split(')').next().unwrap_or("");
        methods
            .split(',')
            .map(str::trim)
            .any(|m| m == "password" || m == "keyboard-interactive")
    })
}

/// True when ssh gave up on authentication: some hop printed
/// `<user>@<host>: Permission denied (<methods>).`. A remote command that
/// fails with its own "permission denied" does not count.
pub fn auth_refused(stderr: &str) -> bool {
    stderr.lines().any(|l| l.contains(": Permission denied ("))
}

/// True when strict host key checking refused a host that is not in
/// `known_hosts` yet. A changed key prints the `@@@` banner plus an
/// `Offending` line instead and stays with `parse_host_key_error`.
pub fn is_unknown_host_key(stderr: &str) -> bool {
    stderr.contains("host key is known for") && stderr.contains("requested strict checking")
}

/// The host ssh named on its `Permission denied` line, without the user
/// part. OpenSSH writes `<user>@<host>: Permission denied (<methods>).`,
/// and on a ProxyJump chain that host is whichever hop refused. `None`
/// when no line carries the prefix.
pub fn denied_host(stderr: &str) -> Option<&str> {
    stderr
        .lines()
        .map(str::trim)
        .filter(|l| l.contains(": Permission denied ("))
        .filter_map(|l| {
            let prefix = l.split(": Permission denied (").next()?;
            prefix.rsplit('@').next().filter(|h| !h.is_empty())
        })
        .next_back()
}

/// The host strict checking refused, as ssh spells it on the
/// `No ... host key is known for <host> and you have requested strict
/// checking.` line. Carries the bracketed form for a non-default port,
/// so `[10.0.0.1]:2222` comes back whole.
pub fn unknown_host_key_host(stderr: &str) -> Option<&str> {
    stderr
        .lines()
        .map(str::trim)
        .find(|l| l.contains("host key is known for") && l.contains("requested strict checking"))
        .and_then(|l| {
            let start = l.find("host key is known for ")? + "host key is known for ".len();
            let rest = &l[start..];
            let end = rest.find(" and you have requested")?;
            Some(rest[..end].trim())
        })
        .filter(|h| !h.is_empty())
}

/// Strip ssh's `[host]:port` form down to the bare host, so a name parsed
/// out of stderr compares against a `HostName` directive.
pub fn bare_host(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|rest| rest.split(']').next())
        .unwrap_or(host)
}

/// Parse host key verification error from SSH stderr output.
/// Returns (hostname, known_hosts_path) if the error is a changed host key.
///
/// Uses two detection strategies:
/// 1. English string matching for hostname and known_hosts path extraction.
/// 2. Locale-independent fallback: the `@@@@@` warning banner is always present
///    regardless of locale, combined with a known_hosts path from "Offending" line.
///    When the English hostname line is missing, falls back to extracting the
///    hostname from the known_hosts file path.
pub fn parse_host_key_error(stderr: &str) -> Option<(String, String)> {
    // Primary: English locale detection
    let has_english_error = stderr.contains("Host key verification failed.");
    // Fallback: the @@@ banner is locale-independent and always present for host key errors
    let has_banner = stderr.contains("@@@@@@@@@@@@@@@");

    if !has_english_error && !has_banner {
        return None;
    }

    // Parse hostname from "Host key for <hostname> has changed"
    let hostname = stderr
        .lines()
        .find(|l| l.contains("Host key for") && l.contains("has changed"))
        .and_then(|l| {
            let start = l.find("Host key for ")? + "Host key for ".len();
            let rest = &l[start..];
            let end = rest.find(" has changed")?;
            Some(rest[..end].to_string())
        });

    // Parse known_hosts path from "Offending ... key in <path>:<line>"
    let known_hosts_path = stderr
        .lines()
        .find(|l| l.starts_with("Offending") && l.contains(" key in "))
        .and_then(|l| {
            let start = l.find(" key in ")? + " key in ".len();
            let rest = &l[start..];
            let end = rest.rfind(':')?;
            Some(rest[..end].to_string())
        });

    // We need at least the known_hosts path to be useful
    let known_hosts_path = known_hosts_path?;

    // If we couldn't parse the hostname (non-English locale), derive it from
    // the known_hosts path by running ssh-keygen -F would be complex.
    // Instead, use a reasonable default: the user will see the confirmation dialog
    // with the known_hosts path, which is the critical piece for the reset.
    let hostname = hostname.unwrap_or_else(|| "the remote host".to_string());

    Some((hostname, known_hosts_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_failure_is_only_code_255() {
        assert!(is_ssh_transport_failure(255));
        // Remote-command passthrough statuses are not ssh transport failures.
        assert!(!is_ssh_transport_failure(0));
        assert!(!is_ssh_transport_failure(1));
        assert!(!is_ssh_transport_failure(2));
        assert!(!is_ssh_transport_failure(254));
        assert!(!is_ssh_transport_failure(-1));
    }

    #[test]
    fn classify_ssh_exit_routes_only_255_to_error() {
        // Pins the error-vs-debug routing shared by spawn_ssh_and_wait and the
        // MCP run_command tool. A revert to "any non-zero is an error" fails here.
        assert_eq!(classify_ssh_exit(0), SshExitClass::Success);
        assert_eq!(classify_ssh_exit(255), SshExitClass::TransportFailure);
        // Routine remote-command statuses stay out of the error arm.
        assert_eq!(classify_ssh_exit(1), SshExitClass::RemoteStatus);
        assert_eq!(classify_ssh_exit(2), SshExitClass::RemoteStatus);
        assert_eq!(classify_ssh_exit(126), SshExitClass::RemoteStatus);
        assert_eq!(classify_ssh_exit(127), SshExitClass::RemoteStatus);
        assert_eq!(classify_ssh_exit(254), SshExitClass::RemoteStatus);
        // Signal-killed ssh (status.code() == None -> -1) is routine teardown.
        assert_eq!(classify_ssh_exit(-1), SshExitClass::RemoteStatus);
    }

    #[test]
    fn remote_exit_log_level_reserves_error_for_transport_failure() {
        // Pins the level every remote-command call site (container list/actions,
        // ...) uses for a failed exit. Only ssh's 255 transport failure is an
        // error; routine remote statuses are debug. A revert to "non-zero is an
        // error" fails here.
        assert_eq!(remote_exit_log_level(255), log::Level::Error);
        for code in [0, 1, 2, 126, 127, 130, 254, -1] {
            assert_eq!(
                remote_exit_log_level(code),
                log::Level::Debug,
                "exit {code} must not log at error level"
            );
        }
    }

    #[test]
    fn connect_fails_with_nonexistent_config() {
        // connect() should return an error when the config file doesn't exist and
        let result = connect(
            "nonexistent-host",
            Path::new("/tmp/__purple_test_nonexistent_config__"),
            None,
            None,
            false,
            &SshLauncher::default(),
        );
        // SSH should exit with a non-zero status (config file not found)
        assert!(result.is_ok()); // spawn succeeds, SSH exits with error
        let r = result.unwrap();
        assert!(!r.status.success());
    }

    #[test]
    fn connect_with_tunnel_flag_does_not_panic() {
        // Verify has_active_tunnel=true adds the ClearAllForwardings arg without panic.
        let result = connect(
            "nonexistent-host",
            Path::new("/tmp/__purple_test_nonexistent_config__"),
            None,
            None,
            true,
            &SshLauncher::default(),
        );
        assert!(result.is_ok());
        assert!(!result.unwrap().status.success());
    }

    #[test]
    fn connect_runs_the_configured_launcher() {
        // A launcher that echoes its argv proves purple's own arguments are
        // appended after the configured words and the exit code flows back.
        let launcher =
            SshLauncher::parse("sh -c 'printf \"%s\\n\" \"$@\" >&2; exit 7' sh").unwrap();
        let result = connect(
            "launcher-host",
            Path::new("/tmp/__purple_test_cfg__"),
            None,
            None,
            true,
            &launcher,
        )
        .unwrap();
        assert_eq!(result.status.code(), Some(7));
        let lines: Vec<&str> = result.stderr_output.lines().collect();
        assert_eq!(
            lines,
            [
                "-F",
                "/tmp/__purple_test_cfg__",
                "-o",
                "ClearAllForwardings=yes",
                "--",
                "launcher-host"
            ]
        );
    }

    #[test]
    fn connect_reports_missing_launcher_program() {
        let launcher = SshLauncher::parse("/nonexistent/purple-test-launcher ssh").unwrap();
        let err = connect(
            "launcher-host",
            Path::new("/tmp/__purple_test_cfg__"),
            None,
            None,
            false,
            &launcher,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to launch /nonexistent/purple-test-launcher connection"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn tmux_window_args_wrap_launcher_words() {
        let launcher = SshLauncher::parse("kitten ssh").unwrap();
        let args = tmux_window_args(
            "web1",
            &launcher,
            "/home/u/.ssh/config",
            false,
            "web1",
            None,
        );
        assert_eq!(
            args,
            [
                "new-window",
                "-n",
                "web1",
                "--",
                "kitten",
                "ssh",
                "-F",
                "/home/u/.ssh/config",
                "--",
                "web1"
            ]
        );
        let args = tmux_window_args(
            "web1/app",
            &SshLauncher::default(),
            "/home/u/.ssh/config",
            true,
            "web1",
            Some("docker exec -it abc sh"),
        );
        assert_eq!(
            args,
            [
                "new-window",
                "-n",
                "web1/app",
                "--",
                "ssh",
                "-F",
                "/home/u/.ssh/config",
                "-t",
                "-o",
                "ClearAllForwardings=yes",
                "--",
                "web1",
                "docker exec -it abc sh"
            ]
        );
    }

    #[test]
    fn connect_captures_stderr() {
        // SSH should produce some stderr output when failing.
        let result = connect(
            "nonexistent-host",
            Path::new("/tmp/__purple_test_nonexistent_config__"),
            None,
            None,
            false,
            &SshLauncher::default(),
        );
        assert!(result.is_ok());
        // SSH writes errors to stderr; we should have captured something
        // (either "Can't open user config file" or a connection error)
        let r = result.unwrap();
        assert!(
            !r.stderr_output.is_empty() || !r.status.success(),
            "SSH should produce stderr or fail"
        );
    }

    // --- needs_password / is_unknown_host_key tests ---
    //
    // The strings are what OpenSSH 10.3 prints for a password-only sshd
    // under BatchMode=yes and StrictHostKeyChecking=yes.

    #[test]
    fn needs_password_true_for_password_method() {
        assert!(needs_password(
            "test@localhost: Permission denied (password).\n"
        ));
    }

    #[test]
    fn needs_password_true_when_password_is_one_of_several_methods() {
        assert!(needs_password(
            "test@localhost: Permission denied (publickey,password).\n"
        ));
        assert!(needs_password(
            "test@localhost: Permission denied (publickey,keyboard-interactive).\n"
        ));
    }

    #[test]
    fn auth_refused_reads_only_the_line_ssh_prints() {
        assert!(auth_refused(
            "sign_and_send_pubkey: signing failed\nops@10.0.0.5: Permission denied (publickey).\n"
        ));
        // A remote command's own refusal is not an authentication failure.
        assert!(!auth_refused(
            "Got permission denied while trying to connect to the Docker daemon socket"
        ));
        assert!(!auth_refused("ls: /root: Permission denied"));
        assert!(!auth_refused(""));
    }

    #[test]
    fn needs_password_false_for_publickey_only() {
        // A typed password cannot help when the server takes none.
        assert!(!needs_password(
            "test@localhost: Permission denied (publickey).\n"
        ));
    }

    #[test]
    fn needs_password_false_for_unrelated_errors() {
        assert!(!needs_password(""));
        assert!(!needs_password(
            "ssh: connect to host example.com port 22: Connection refused\n"
        ));
        assert!(!needs_password("Permission denied, please try again.\n"));
    }

    #[test]
    fn needs_password_survives_the_post_quantum_warning_prefix() {
        let stderr = "** WARNING: connection is not using a post-quantum key exchange algorithm.\n\
                      ** This session may be vulnerable to \"store now, decrypt later\" attacks.\n\
                      test@10.0.0.5: Permission denied (publickey,password).\n";
        assert!(needs_password(stderr));
    }

    #[test]
    fn is_unknown_host_key_true_for_strict_checking_refusal() {
        let stderr = "No ED25519 host key is known for [localhost]:12222 and you have requested strict checking.\n\
                      Host key verification failed.\n";
        assert!(is_unknown_host_key(stderr));
    }

    #[test]
    fn is_unknown_host_key_false_for_changed_key_banner() {
        // A changed key is a security event: it keeps the reset dialog and
        // never gets an automatic retry.
        let stderr = "\
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
Offending ECDSA key in /Users/user/.ssh/known_hosts:55
Host key for example.com has changed and you have requested strict checking.
Host key verification failed.
";
        assert!(!is_unknown_host_key(stderr));
        assert!(parse_host_key_error(stderr).is_some());
    }

    #[test]
    fn is_unknown_host_key_false_for_plain_permission_denied() {
        assert!(!is_unknown_host_key(
            "test@localhost: Permission denied (password).\n"
        ));
        assert!(!is_unknown_host_key(""));
    }

    // --- denied_host / unknown_host_key_host / bare_host tests ---
    //
    // These three name the hop ssh refused, which is what decides whether a
    // dialog opens for the target or the run is reported as a jump host's
    // refusal. The fixtures are verbatim OpenSSH output.

    #[test]
    fn denied_host_takes_the_host_and_drops_the_user() {
        assert_eq!(
            denied_host(
                "nobodyuser@127.0.0.1: Permission denied (publickey,password,keyboard-interactive).\n"
            ),
            Some("127.0.0.1")
        );
    }

    #[test]
    fn denied_host_reads_a_line_carrying_no_user() {
        assert_eq!(
            denied_host("bastion.example.com: Permission denied (password).\n"),
            Some("bastion.example.com")
        );
    }

    #[test]
    fn denied_host_survives_a_warning_prefix_and_trailing_lines() {
        let stderr = "** WARNING: connection is not using a post-quantum key exchange algorithm.\n\
                      ops@10.0.0.5: Permission denied (publickey,password).\n\
                      Connection closed by 10.0.0.5 port 22\n";
        assert_eq!(denied_host(stderr), Some("10.0.0.5"));
    }

    #[test]
    fn denied_host_takes_the_last_line_when_several_carry_the_prefix() {
        // On a chain the hop that refused prints its own line. The last one
        // is the deepest ssh got.
        let stderr = "ops@bastion.example.com: Permission denied (password).\n\
                      ops@target.example.com: Permission denied (password).\n";
        assert_eq!(denied_host(stderr), Some("target.example.com"));
    }

    #[test]
    fn denied_host_is_none_without_the_line() {
        assert_eq!(denied_host(""), None);
        assert_eq!(denied_host("Permission denied, please try again.\n"), None);
        assert_eq!(
            denied_host("ssh: connect to host x port 22: Connection refused\n"),
            None
        );
    }

    #[test]
    fn unknown_host_key_host_keeps_the_bracketed_port_form() {
        let stderr = "No ED25519 host key is known for [127.0.0.1]:12222 and you have requested strict checking.\n\
                      Host key verification failed.\n";
        assert_eq!(unknown_host_key_host(stderr), Some("[127.0.0.1]:12222"));
    }

    #[test]
    fn unknown_host_key_host_reads_a_default_port_host() {
        let stderr =
            "No RSA host key is known for db.example.com and you have requested strict checking.\n";
        assert_eq!(unknown_host_key_host(stderr), Some("db.example.com"));
    }

    #[test]
    fn unknown_host_key_host_is_none_for_other_failures() {
        assert_eq!(unknown_host_key_host(""), None);
        assert_eq!(
            unknown_host_key_host("test@localhost: Permission denied (password).\n"),
            None
        );
    }

    #[test]
    fn bare_host_strips_the_bracketed_port() {
        assert_eq!(bare_host("[127.0.0.1]:12222"), "127.0.0.1");
        assert_eq!(bare_host("[2001:db8::1]:2222"), "2001:db8::1");
        assert_eq!(bare_host("db.example.com"), "db.example.com");
        assert_eq!(bare_host(""), "");
    }

    // --- parse_host_key_error tests ---

    #[test]
    fn parse_host_key_error_detects_changed_key() {
        let stderr = "\
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!
Someone could be eavesdropping on you right now (man-in-the-middle attack)!
It is also possible that a host key has just been changed.
The fingerprint for the ED25519 key sent by the remote host is
SHA256:ohwPXZbfBMvYWXnKefVYWVAcQsXKLMqaRKbXxRUVXqc.
Please contact your system administrator.
Add correct host key in /Users/user/.ssh/known_hosts to get rid of this message.
Offending ECDSA key in /Users/user/.ssh/known_hosts:55
Host key for example.com has changed and you have requested strict checking.
Host key verification failed.
";
        let result = parse_host_key_error(stderr);
        assert!(result.is_some());
        let (hostname, path) = result.unwrap();
        assert_eq!(hostname, "example.com");
        assert_eq!(path, "/Users/user/.ssh/known_hosts");
    }

    #[test]
    fn parse_host_key_error_returns_none_for_other_errors() {
        let stderr = "ssh: connect to host example.com port 22: Connection refused\n";
        assert!(parse_host_key_error(stderr).is_none());
    }

    #[test]
    fn parse_host_key_error_returns_none_for_empty() {
        assert!(parse_host_key_error("").is_none());
    }

    #[test]
    fn parse_host_key_error_handles_ip_address() {
        let stderr = "\
Offending ECDSA key in /home/user/.ssh/known_hosts:12
Host key for 10.0.0.1 has changed and you have requested strict checking.
Host key verification failed.
";
        let result = parse_host_key_error(stderr);
        assert!(result.is_some());
        let (hostname, path) = result.unwrap();
        assert_eq!(hostname, "10.0.0.1");
        assert_eq!(path, "/home/user/.ssh/known_hosts");
    }

    #[test]
    fn parse_host_key_error_handles_custom_known_hosts_path() {
        let stderr = "\
Offending RSA key in /etc/ssh/known_hosts:3
Host key for server.local has changed and you have requested strict checking.
Host key verification failed.
";
        let result = parse_host_key_error(stderr);
        assert!(result.is_some());
        let (hostname, path) = result.unwrap();
        assert_eq!(hostname, "server.local");
        assert_eq!(path, "/etc/ssh/known_hosts");
    }

    #[test]
    fn parse_host_key_error_handles_ipv6() {
        let stderr = "\
Offending ED25519 key in /Users/user/.ssh/known_hosts:7
Host key for ::1 has changed and you have requested strict checking.
Host key verification failed.
";
        let result = parse_host_key_error(stderr);
        assert!(result.is_some());
        let (hostname, _) = result.unwrap();
        assert_eq!(hostname, "::1");
    }

    #[test]
    fn connect_tmux_window_fails_gracefully_outside_tmux_session() {
        // When no tmux server is running (or tmux is absent), should return an error.
        // Skip if we're actually inside a live tmux session (the command would succeed).
        // Holds TMUX_LOCK so the env-mutating tests below cannot flip TMUX between
        // the guard read and the call to connect_tmux_window.
        let _guard = TMUX_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        if std::env::var("TMUX").is_ok() {
            return;
        }
        let result = connect_tmux_window(
            "test-host",
            Path::new("/tmp/__purple_test_nonexistent_config__"),
            false,
            &SshLauncher::default(),
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("tmux") || err.contains("No such file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn connect_tmux_window_with_tunnel_does_not_panic() {
        // Verify has_active_tunnel=true doesn't panic and fails gracefully.
        // Skip if inside a live tmux session. TMUX_LOCK prevents the env-mutating
        // tests from racing this guard read.
        let _guard = TMUX_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        if std::env::var("TMUX").is_ok() {
            return;
        }
        let result = connect_tmux_window(
            "tunnel-host",
            Path::new("/tmp/__purple_test_nonexistent_config__"),
            true,
            &SshLauncher::default(),
        );
        assert!(result.is_err());
    }

    /// Mutex to serialise tests that mutate the TMUX env var.
    static TMUX_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn is_in_tmux_returns_true_when_set() {
        let env = crate::runtime::env::Env::for_test("/tmp/x")
            .with_var("TMUX", "/tmp/tmux-1000/default,12345,0");
        assert!(is_in_tmux(&env));
    }

    #[test]
    fn is_in_tmux_returns_false_when_unset() {
        let env = crate::runtime::env::Env::for_test("/tmp/x");
        assert!(!is_in_tmux(&env));
    }

    // --- first_stderr_line tests ---

    #[test]
    fn stderr_summary_joins_all_lines() {
        let stderr = "channel 0: open failed: administratively prohibited: open failed\n\
                      stdio forwarding failed\n\
                      Connection closed by UNKNOWN port 65535\n";
        let result = stderr_summary(stderr);
        assert_eq!(
            result.as_deref(),
            Some(
                "channel 0: open failed: administratively prohibited: open failed | stdio forwarding failed | Connection closed by UNKNOWN port 65535"
            )
        );
    }

    #[test]
    fn stderr_summary_skips_banner_lines() {
        let stderr = "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
                      @    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n\
                      @@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n\
                      IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!\n";
        let result = stderr_summary(stderr);
        assert_eq!(
            result.as_deref(),
            Some("IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!")
        );
    }

    #[test]
    fn stderr_summary_omits_successful_host_key_save() {
        let notice =
            "Warning: Permanently added 'example.test' (ED25519) to the list of known hosts.\n";
        assert_eq!(stderr_summary(notice), None);
        assert_eq!(
            stderr_summary(&format!(
                "{notice}root@example.test: Permission denied (password).\n"
            ))
            .as_deref(),
            Some("root@example.test: Permission denied (password).")
        );
        assert_eq!(
            stderr_summary("Warning: Host key verification failed.\n").as_deref(),
            Some("Warning: Host key verification failed.")
        );
    }

    #[test]
    fn stderr_summary_returns_none_for_empty() {
        assert!(stderr_summary("").is_none());
        assert!(stderr_summary("   \n  \n").is_none());
        assert!(stderr_summary("@@@@@\n@@@@@\n").is_none());
    }

    #[test]
    fn stderr_summary_truncates_long_output() {
        let long = "x".repeat(250);
        let result = stderr_summary(&long).unwrap();
        assert_eq!(result.len(), 200);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn stderr_summary_truncates_multibyte_safely() {
        // Each '日' is 3 bytes. 100 chars = 300 bytes, exceeds the 200-char limit.
        let long = "日".repeat(100);
        let result = stderr_summary(&long).unwrap();
        assert!(result.ends_with("..."));
        // Must not panic and must be valid UTF-8
        assert!(result.len() <= 600); // 197 chars * 3 bytes + 3 bytes for "..."
    }

    #[test]
    fn stderr_summary_simple_errors() {
        assert_eq!(
            stderr_summary("Connection refused\n").as_deref(),
            Some("Connection refused")
        );
        assert_eq!(
            stderr_summary("Permission denied (publickey).\n").as_deref(),
            Some("Permission denied (publickey).")
        );
    }
}
