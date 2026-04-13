use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::fs_util;

/// A saved command snippet.
#[derive(Debug, Clone, PartialEq)]
pub struct Snippet {
    pub name: String,
    pub command: String,
    pub description: String,
}

/// Result of running a snippet on a host.
pub struct SnippetResult {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Snippet storage backed by ~/.purple/snippets (INI-style).
#[derive(Debug, Clone, Default)]
pub struct SnippetStore {
    pub snippets: Vec<Snippet>,
    /// Override path for save(). None uses the default ~/.purple/snippets.
    pub path_override: Option<PathBuf>,
}

fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".purple/snippets"))
}

impl SnippetStore {
    /// Load snippets from ~/.purple/snippets.
    /// Returns empty store if file doesn't exist (normal first-use).
    pub fn load() -> Self {
        let path = match config_path() {
            Some(p) => p,
            None => return Self::default(),
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                log::warn!("[config] Could not read {}: {}", path.display(), e);
                return Self::default();
            }
        };
        Self::parse(&content)
    }

    /// Parse INI-style snippet config.
    pub fn parse(content: &str) -> Self {
        let mut snippets = Vec::new();
        let mut current: Option<Snippet> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if let Some(snippet) = current.take() {
                    if !snippet.command.is_empty()
                        && !snippets.iter().any(|s: &Snippet| s.name == snippet.name)
                    {
                        snippets.push(snippet);
                    }
                }
                let name = trimmed[1..trimmed.len() - 1].trim().to_string();
                if snippets.iter().any(|s| s.name == name) {
                    current = None;
                    continue;
                }
                current = Some(Snippet {
                    name,
                    command: String::new(),
                    description: String::new(),
                });
            } else if let Some(ref mut snippet) = current {
                if let Some((key, value)) = trimmed.split_once('=') {
                    let key = key.trim();
                    // Trim whitespace around key but preserve value content
                    // (only trim leading whitespace after '=', not trailing)
                    let value = value.trim_start().to_string();
                    match key {
                        "command" => snippet.command = value,
                        "description" => snippet.description = value,
                        _ => {}
                    }
                }
            }
        }
        if let Some(snippet) = current {
            if !snippet.command.is_empty() && !snippets.iter().any(|s| s.name == snippet.name) {
                snippets.push(snippet);
            }
        }
        Self {
            snippets,
            path_override: None,
        }
    }

    /// Save snippets to ~/.purple/snippets (atomic write, chmod 600).
    pub fn save(&self) -> io::Result<()> {
        if crate::demo_flag::is_demo() {
            return Ok(());
        }
        let path = match &self.path_override {
            Some(p) => p.clone(),
            None => match config_path() {
                Some(p) => p,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "Could not determine home directory",
                    ));
                }
            },
        };

        let mut content = String::new();
        for (i, snippet) in self.snippets.iter().enumerate() {
            if i > 0 {
                content.push('\n');
            }
            content.push_str(&format!("[{}]\n", snippet.name));
            content.push_str(&format!("command={}\n", snippet.command));
            if !snippet.description.is_empty() {
                content.push_str(&format!("description={}\n", snippet.description));
            }
        }

        fs_util::atomic_write(&path, content.as_bytes())
    }

    /// Get a snippet by name.
    pub fn get(&self, name: &str) -> Option<&Snippet> {
        self.snippets.iter().find(|s| s.name == name)
    }

    /// Add or replace a snippet.
    pub fn set(&mut self, snippet: Snippet) {
        if let Some(existing) = self.snippets.iter_mut().find(|s| s.name == snippet.name) {
            *existing = snippet;
        } else {
            self.snippets.push(snippet);
        }
    }

    /// Remove a snippet by name.
    pub fn remove(&mut self, name: &str) {
        self.snippets.retain(|s| s.name != name);
    }
}

/// Validate a snippet name: non-empty, no leading/trailing whitespace,
/// no `#`, no `[`, no `]`, no control characters.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Snippet name cannot be empty.".to_string());
    }
    if name != name.trim() {
        return Err("Snippet name cannot have leading or trailing whitespace.".to_string());
    }
    if name.contains('#') || name.contains('[') || name.contains(']') {
        return Err("Snippet name cannot contain #, [ or ].".to_string());
    }
    if name.contains(|c: char| c.is_control()) {
        return Err("Snippet name cannot contain control characters.".to_string());
    }
    Ok(())
}

/// Validate a snippet command: non-empty, no control characters (except tab).
pub fn validate_command(command: &str) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err("Command cannot be empty.".to_string());
    }
    if command.contains(|c: char| c.is_control() && c != '\t') {
        return Err("Command cannot contain control characters.".to_string());
    }
    Ok(())
}

// =========================================================================
// Parameter support
// =========================================================================

/// A parameter found in a snippet command template.
#[derive(Debug, Clone, PartialEq)]
pub struct SnippetParam {
    pub name: String,
    pub default: Option<String>,
}

/// Shell-escape a string with single quotes (POSIX).
/// Internal single quotes are escaped as `'\''`.
pub fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Parse `{{name}}` and `{{name:default}}` from a command string.
/// Returns params in order of first appearance, deduplicated. Max 20 params.
pub fn parse_params(command: &str) -> Vec<SnippetParam> {
    let mut params = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let bytes = command.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 3 < len {
        if bytes[i] == b'{' && bytes.get(i + 1) == Some(&b'{') {
            if let Some(end) = command[i + 2..].find("}}") {
                let inner = &command[i + 2..i + 2 + end];
                let (name, default) = if let Some((n, d)) = inner.split_once(':') {
                    (n.to_string(), Some(d.to_string()))
                } else {
                    (inner.to_string(), None)
                };
                if validate_param_name(&name).is_ok() && !seen.contains(&name) && params.len() < 20
                {
                    seen.insert(name.clone());
                    params.push(SnippetParam { name, default });
                }
                i = i + 2 + end + 2;
                continue;
            }
        }
        i += 1;
    }
    params
}

/// Validate a parameter name: non-empty, alphanumeric/underscore/hyphen only.
/// Rejects `{`, `}`, `'`, whitespace and control chars.
pub fn validate_param_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Parameter name cannot be empty.".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "Parameter name '{}' contains invalid characters.",
            name
        ));
    }
    Ok(())
}

/// Substitute parameters into a command template (single-pass).
/// All values (user-provided and defaults) are shell-escaped.
pub fn substitute_params(
    command: &str,
    values: &std::collections::HashMap<String, String>,
) -> String {
    let mut result = String::with_capacity(command.len());
    let bytes = command.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if i + 3 < len && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end) = command[i + 2..].find("}}") {
                let inner = &command[i + 2..i + 2 + end];
                let (name, default) = if let Some((n, d)) = inner.split_once(':') {
                    (n, Some(d))
                } else {
                    (inner, None)
                };
                let value = values
                    .get(name)
                    .filter(|v| !v.is_empty())
                    .map(|v| v.as_str())
                    .or(default)
                    .unwrap_or("");
                result.push_str(&shell_escape(value));
                i = i + 2 + end + 2;
                continue;
            }
        }
        // Properly decode UTF-8 character (not byte-level cast)
        let ch = command[i..].chars().next().unwrap();
        result.push(ch);
        i += ch.len_utf8();
    }
    result
}

// =========================================================================
// Output sanitization
// =========================================================================

/// Strip ANSI escape sequences and C1 control codes from output.
/// Handles CSI, OSC, DCS, SOS, PM and APC sequences plus the C1 range 0x80-0x9F.
pub fn sanitize_output(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                match chars.peek() {
                    Some('[') => {
                        chars.next();
                        // CSI: consume until 0x40-0x7E
                        while let Some(&ch) = chars.peek() {
                            chars.next();
                            if ('\x40'..='\x7e').contains(&ch) {
                                break;
                            }
                        }
                    }
                    Some(']') | Some('P') | Some('X') | Some('^') | Some('_') => {
                        chars.next();
                        // OSC/DCS/SOS/PM/APC: consume until ST (ESC\) or BEL
                        consume_until_st(&mut chars);
                    }
                    _ => {
                        // Single ESC + one char
                        chars.next();
                    }
                }
            }
            c if ('\u{0080}'..='\u{009F}').contains(&c) => {
                // C1 control codes: skip
            }
            c if c.is_control() && c != '\n' && c != '\t' => {
                // Other control chars (except newline/tab): skip
            }
            _ => out.push(c),
        }
    }
    out
}

/// Consume chars until String Terminator (ESC\) or BEL (\x07).
fn consume_until_st(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(&ch) = chars.peek() {
        if ch == '\x07' {
            chars.next();
            break;
        }
        if ch == '\x1b' {
            chars.next();
            if chars.peek() == Some(&'\\') {
                chars.next();
            }
            break;
        }
        chars.next();
    }
}

// =========================================================================
// Background snippet execution
// =========================================================================

/// Maximum lines stored per host. Reader continues draining beyond this
/// to prevent child from blocking on a full pipe buffer.
const MAX_OUTPUT_LINES: usize = 10_000;

/// RAII guard that kills the process group on drop.
/// Uses SIGTERM first, then escalates to SIGKILL after a brief wait.
pub struct ChildGuard {
    inner: std::sync::Mutex<Option<std::process::Child>>,
    pgid: i32,
}

impl ChildGuard {
    fn new(child: std::process::Child) -> Self {
        // i32::try_from avoids silent overflow for PIDs > i32::MAX.
        // Fallback -1 makes killpg a harmless no-op on overflow.
        // In practice Linux caps PIDs well below i32::MAX.
        let pgid = i32::try_from(child.id()).unwrap_or(-1);
        Self {
            inner: std::sync::Mutex::new(Some(child)),
            pgid,
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let mut lock = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref mut child) = *lock {
            // Already exited? Skip kill entirely (PID may be recycled).
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            // SAFETY: self.pgid was set by setpgid(0,0) in pre_exec and is
            // valid for the lifetime of this SnippetChild. kill() with a
            // negative PID sends the signal to the entire process group.
            // ESRCH (process already exited) is the expected race; the
            // return value is intentionally ignored.
            #[cfg(unix)]
            unsafe {
                libc::kill(-self.pgid, libc::SIGTERM);
            }
            // Poll for up to 500ms
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
            loop {
                if let Ok(Some(_)) = child.try_wait() {
                    return;
                }
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            // SAFETY: same invariants as the SIGTERM call above.
            #[cfg(unix)]
            unsafe {
                libc::kill(-self.pgid, libc::SIGKILL);
            }
            // Fallback: direct kill in case setpgid failed in pre_exec
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Read lines from a pipe. Stores up to `MAX_OUTPUT_LINES` but continues
/// draining the pipe after that to prevent the child from blocking.
fn read_pipe_capped<R: io::Read>(reader: R) -> String {
    use io::BufRead;
    let mut reader = io::BufReader::new(reader);
    let mut output = String::new();
    let mut line_count = 0;
    let mut capped = false;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break, // EOF
            Ok(_) => {
                if !capped {
                    if line_count < MAX_OUTPUT_LINES {
                        if line_count > 0 {
                            output.push('\n');
                        }
                        // Strip trailing newline (and \r for CRLF)
                        if buf.last() == Some(&b'\n') {
                            buf.pop();
                            if buf.last() == Some(&b'\r') {
                                buf.pop();
                            }
                        }
                        // Lossy conversion handles non-UTF-8 output
                        output.push_str(&String::from_utf8_lossy(&buf));
                        line_count += 1;
                    } else {
                        output.push_str("\n[Output truncated at 10,000 lines]");
                        capped = true;
                    }
                }
                // If capped, keep reading but discard to drain the pipe
            }
            Err(_) => break,
        }
    }
    output
}

/// Build the base SSH command with shared options for snippet execution.
/// Sets -F, ConnectTimeout, ControlMaster/ControlPath and ClearAllForwardings.
/// Also configures askpass and Bitwarden session env vars.
fn base_ssh_command(
    alias: &str,
    config_path: &Path,
    command: &str,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    has_active_tunnel: bool,
) -> Command {
    let mut cmd = Command::new("ssh");
    cmd.arg("-F")
        .arg(config_path)
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg("-o")
        .arg("ControlMaster=no")
        .arg("-o")
        .arg("ControlPath=none");

    if has_active_tunnel {
        cmd.arg("-o").arg("ClearAllForwardings=yes");
    }

    cmd.arg("--").arg(alias).arg(command);

    if askpass.is_some() {
        crate::askpass_env::configure_ssh_command(&mut cmd, alias, config_path);
    }

    if let Some(token) = bw_session {
        cmd.env("BW_SESSION", token);
    }

    cmd
}

/// Build the SSH Command for a snippet execution with piped I/O.
fn build_snippet_command(
    alias: &str,
    config_path: &Path,
    command: &str,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    has_active_tunnel: bool,
) -> Command {
    let mut cmd = base_ssh_command(
        alias,
        config_path,
        command,
        askpass,
        bw_session,
        has_active_tunnel,
    );
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Isolate child into its own process group so we can kill the
    // entire tree without affecting purple itself.
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }

    cmd
}

/// Execute a single host: spawn SSH, read output, wait, send result.
fn execute_host(
    run_id: u64,
    ctx: &crate::ssh_context::SshContext<'_>,
    command: &str,
    tx: &std::sync::mpsc::Sender<crate::event::AppEvent>,
) -> Option<std::sync::Arc<ChildGuard>> {
    let alias = ctx.alias;
    let mut cmd = build_snippet_command(
        alias,
        ctx.config_path,
        command,
        ctx.askpass,
        ctx.bw_session,
        ctx.has_tunnel,
    );

    match cmd.spawn() {
        Ok(child) => {
            let guard = std::sync::Arc::new(ChildGuard::new(child));

            // Take stdout/stderr BEFORE wait to avoid pipe deadlock
            let stdout_pipe = {
                let mut lock = guard.inner.lock().unwrap_or_else(|e| e.into_inner());
                lock.as_mut().and_then(|c| c.stdout.take())
            };
            let stderr_pipe = {
                let mut lock = guard.inner.lock().unwrap_or_else(|e| e.into_inner());
                lock.as_mut().and_then(|c| c.stderr.take())
            };

            // Spawn reader threads
            let stdout_handle = std::thread::spawn(move || match stdout_pipe {
                Some(pipe) => read_pipe_capped(pipe),
                None => String::new(),
            });
            let stderr_handle = std::thread::spawn(move || match stderr_pipe {
                Some(pipe) => read_pipe_capped(pipe),
                None => String::new(),
            });

            // Join readers BEFORE wait to guarantee all output is received
            let stdout_text = stdout_handle.join().unwrap_or_else(|_| {
                log::warn!("[purple] Snippet stdout reader thread panicked");
                String::new()
            });
            let stderr_text = stderr_handle.join().unwrap_or_else(|_| {
                log::warn!("[purple] Snippet stderr reader thread panicked");
                String::new()
            });

            // Now wait for the child to exit, then take it out of the
            // guard so Drop won't kill a potentially recycled PID.
            let exit_code = {
                let mut lock = guard.inner.lock().unwrap_or_else(|e| e.into_inner());
                let status = lock.as_mut().and_then(|c| c.wait().ok());
                let _ = lock.take(); // Prevent ChildGuard::drop from killing recycled PID
                status.and_then(|s| {
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        s.code().or_else(|| s.signal().map(|sig| 128 + sig))
                    }
                    #[cfg(not(unix))]
                    {
                        s.code()
                    }
                })
            };

            let _ = tx.send(crate::event::AppEvent::SnippetHostDone {
                run_id,
                alias: alias.to_string(),
                stdout: sanitize_output(&stdout_text),
                stderr: sanitize_output(&stderr_text),
                exit_code,
            });

            Some(guard)
        }
        Err(e) => {
            let _ = tx.send(crate::event::AppEvent::SnippetHostDone {
                run_id,
                alias: alias.to_string(),
                stdout: String::new(),
                stderr: format!("Failed to launch ssh: {}", e),
                exit_code: None,
            });
            None
        }
    }
}

/// Spawn background snippet execution on multiple hosts.
/// The coordinator thread drives sequential or parallel host iteration.
#[allow(clippy::too_many_arguments)]
pub fn spawn_snippet_execution(
    run_id: u64,
    askpass_map: Vec<(String, Option<String>)>,
    config_path: PathBuf,
    command: String,
    bw_session: Option<String>,
    tunnel_aliases: std::collections::HashSet<String>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tx: std::sync::mpsc::Sender<crate::event::AppEvent>,
    parallel: bool,
) {
    let total = askpass_map.len();
    let max_concurrent: usize = 20;

    std::thread::Builder::new()
        .name("snippet-coordinator".into())
        .spawn(move || {
            let guards: std::sync::Arc<std::sync::Mutex<Vec<std::sync::Arc<ChildGuard>>>> =
                std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

            if parallel && total > 1 {
                // Slot-based semaphore for concurrency limiting
                let (slot_tx, slot_rx) = std::sync::mpsc::channel::<()>();
                for _ in 0..max_concurrent.min(total) {
                    let _ = slot_tx.send(());
                }

                let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let mut worker_handles = Vec::new();

                for (alias, askpass) in askpass_map {
                    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }

                    // Wait for a slot, checking cancel periodically
                    loop {
                        match slot_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                            Ok(()) => break,
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                                    break;
                                }
                            }
                            Err(_) => break, // channel closed
                        }
                    }

                    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }

                    let config_path = config_path.clone();
                    let command = command.clone();
                    let bw_session = bw_session.clone();
                    let has_tunnel = tunnel_aliases.contains(&alias);
                    let tx = tx.clone();
                    let slot_tx = slot_tx.clone();
                    let guards = guards.clone();
                    let completed = completed.clone();
                    let total = total;

                    let handle = std::thread::spawn(move || {
                        // RAII guard: release semaphore slot even on panic
                        struct SlotRelease(Option<std::sync::mpsc::Sender<()>>);
                        impl Drop for SlotRelease {
                            fn drop(&mut self) {
                                if let Some(tx) = self.0.take() {
                                    let _ = tx.send(());
                                }
                            }
                        }
                        let _slot = SlotRelease(Some(slot_tx));

                        let host_ctx = crate::ssh_context::SshContext {
                            alias: &alias,
                            config_path: &config_path,
                            askpass: askpass.as_deref(),
                            bw_session: bw_session.as_deref(),
                            has_tunnel,
                        };
                        let guard = execute_host(run_id, &host_ctx, &command, &tx);

                        // Insert guard BEFORE checking cancel so it can be cleaned up
                        if let Some(g) = guard {
                            guards.lock().unwrap_or_else(|e| e.into_inner()).push(g);
                        }

                        let c = completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        let _ = tx.send(crate::event::AppEvent::SnippetProgress {
                            run_id,
                            completed: c,
                            total,
                        });
                        // _slot dropped here, releasing semaphore
                    });
                    worker_handles.push(handle);
                }

                // Wait for all workers to finish
                for handle in worker_handles {
                    let _ = handle.join();
                }
            } else {
                // Sequential execution
                for (i, (alias, askpass)) in askpass_map.into_iter().enumerate() {
                    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }

                    let has_tunnel = tunnel_aliases.contains(&alias);
                    let host_ctx = crate::ssh_context::SshContext {
                        alias: &alias,
                        config_path: &config_path,
                        askpass: askpass.as_deref(),
                        bw_session: bw_session.as_deref(),
                        has_tunnel,
                    };
                    let guard = execute_host(run_id, &host_ctx, &command, &tx);

                    if let Some(g) = guard {
                        guards.lock().unwrap_or_else(|e| e.into_inner()).push(g);
                    }

                    let _ = tx.send(crate::event::AppEvent::SnippetProgress {
                        run_id,
                        completed: i + 1,
                        total,
                    });
                }
            }

            let _ = tx.send(crate::event::AppEvent::SnippetAllDone { run_id });
            // Guards dropped here, cleaning up any remaining children
        })
        .expect("failed to spawn snippet coordinator");
}

/// Run a snippet on a single host via SSH.
/// When `capture` is true, stdout/stderr are piped and returned in the result.
/// When `capture` is false, stdout/stderr are inherited (streamed to terminal
/// in real-time) and the returned strings are empty.
pub fn run_snippet(
    alias: &str,
    config_path: &Path,
    command: &str,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    capture: bool,
    has_active_tunnel: bool,
) -> anyhow::Result<SnippetResult> {
    let mut cmd = base_ssh_command(
        alias,
        config_path,
        command,
        askpass,
        bw_session,
        has_active_tunnel,
    );
    cmd.stdin(Stdio::inherit());

    if capture {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    }

    if capture {
        let output = cmd
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to run ssh for '{}': {}", alias, e))?;

        Ok(SnippetResult {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    } else {
        let status = cmd
            .status()
            .map_err(|e| anyhow::anyhow!("Failed to run ssh for '{}': {}", alias, e))?;

        Ok(SnippetResult {
            status,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

#[cfg(test)]
#[path = "snippet_tests.rs"]
mod tests;
