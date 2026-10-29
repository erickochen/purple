use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::fs_util;

/// A saved command snippet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snippet {
    pub name: String,
    pub command: String,
    pub description: String,
    /// Run the command through the remote user's bash or zsh with `-i` so
    /// `~/.bashrc` or `~/.zshrc` (and with it PATH additions like nvm) is loaded.
    pub interactive: bool,
}

impl Snippet {
    /// The command string handed to ssh: the plain command, or the command
    /// wrapped in an interactive shell when `interactive` is set.
    pub fn remote_command(&self) -> String {
        if self.interactive {
            wrap_interactive(&self.command)
        } else {
            self.command.clone()
        }
    }
}

/// POSIX dispatcher for [`wrap_interactive`]. `$1` is the command, `$2` the
/// interactive script. Other login shells run the command themselves, as they
/// would a plain snippet. Without a `$SHELL`, `/bin/sh` runs it.
const INTERACTIVE_DISPATCH: &str = "case \"${SHELL##*/}\" in bash|zsh) \
     exec 3>&2 4<&0; exec \"$SHELL\" -ic \"$2\" \"${SHELL##*/}\" \"$1\" 2>/dev/null </dev/null;; \
     ?*) exec \"$SHELL\" -c \"$1\" \"${SHELL##*/}\";; \
     *) eval \"$1\";; esac";

/// Runs inside the interactive bash or zsh. The shell itself reads from
/// `/dev/null` and writes its own noise (bash job-control warnings) there, so
/// a syntax error can never make it run stdin. The subshell gives the command
/// the real stderr and stdin back via fds 3 and 4 and keeps bash from echoing
/// `exit`. `eval` makes a syntax error in the command visible.
const INTERACTIVE_SCRIPT: &str = "( exec 2>&3 3>&- 0<&4 4<&-; eval \"$1\" )";

/// Wrap `command` so the remote user's bash or zsh runs it interactively. The
/// command travels as its own argument so no quoting nests. The outer line
/// also parses in fish and tcsh.
pub fn wrap_interactive(command: &str) -> String {
    format!(
        "exec /bin/sh -c {} sh {} {}",
        portable_escape(INTERACTIVE_DISPATCH),
        portable_escape(command),
        portable_escape(INTERACTIVE_SCRIPT)
    )
}

/// Wrap a POSIX `script` so `/bin/sh` runs it whatever the remote login
/// shell is. fish and tcsh hand the single-quoted script over verbatim
/// instead of parsing `if` or `{ }` themselves.
pub fn wrap_posix(script: &str) -> String {
    format!("exec /bin/sh -c {}", portable_escape(script))
}

/// Single-quote `s` so POSIX shells, fish and tcsh all read it back verbatim.
/// fish treats a backslash inside single quotes as an escape and tcsh expands
/// `!` there, so quotes, backslashes and `!` are written outside the quotes.
fn portable_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("'\\''"),
            '\\' => out.push_str("'\\\\'"),
            '!' => out.push_str("'\\!'"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
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
    /// Default target host aliases per snippet name, persisted as the optional
    /// `hosts=` line. Pre-selected when the run picker opens. Kept out of the
    /// `Snippet` struct so existing call sites that build snippets stay
    /// untouched; the association is by name.
    pub targets: HashMap<String, Vec<String>>,
}

fn config_path(paths: Option<&crate::runtime::env::Paths>) -> Option<PathBuf> {
    paths.map(crate::runtime::env::Paths::snippets_file)
}

impl SnippetStore {
    /// Load snippets from `~/.purple/snippets`, resolved from the injected
    /// `paths`. The resolved path is stored as `path_override` so a later
    /// `save()` writes back to the same location without re-resolving.
    /// Returns an empty store when the file does not exist (normal
    /// first-use) or when no home directory is known.
    pub fn load(paths: Option<&crate::runtime::env::Paths>) -> Self {
        let path = match config_path(paths) {
            Some(p) => p,
            None => return Self::default(),
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Self {
                    path_override: Some(path),
                    ..Self::default()
                };
            }
            Err(e) => {
                log::warn!("[config] Could not read {}: {}", path.display(), e);
                return Self {
                    path_override: Some(path),
                    ..Self::default()
                };
            }
        };
        Self {
            path_override: Some(path),
            ..Self::parse(&content)
        }
    }

    /// Parse INI-style snippet config.
    pub fn parse(content: &str) -> Self {
        let mut snippets = Vec::new();
        let mut targets: HashMap<String, Vec<String>> = HashMap::new();
        let mut current: Option<Snippet> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if let Some(snippet) = current.take()
                    && !snippet.command.is_empty()
                    && !snippets.iter().any(|s: &Snippet| s.name == snippet.name)
                {
                    snippets.push(snippet);
                }
                let name = trimmed[1..trimmed.len() - 1].trim().to_string();
                if snippets.iter().any(|s| s.name == name) {
                    current = None;
                    continue;
                }
                current = Some(Snippet {
                    name,
                    ..Snippet::default()
                });
            } else if let Some(ref mut snippet) = current
                && let Some((key, value)) = trimmed.split_once('=')
            {
                let key = key.trim();
                // Trim whitespace around key but preserve value content
                // (only trim leading whitespace after '=', not trailing)
                let value = value.trim_start().to_string();
                match key {
                    "command" => snippet.command = value,
                    "description" => snippet.description = value,
                    "interactive" => snippet.interactive = value.trim() == "true",
                    "hosts" => {
                        let aliases: Vec<String> = value
                            .split(',')
                            .map(|a| unescape_alias(a.trim()))
                            .filter(|a| !a.is_empty())
                            .collect();
                        if !aliases.is_empty() {
                            targets.insert(snippet.name.clone(), aliases);
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(snippet) = current
            && !snippet.command.is_empty()
            && !snippets.iter().any(|s| s.name == snippet.name)
        {
            snippets.push(snippet);
        }
        // Drop targets for sections that did not yield a snippet (no command,
        // or a duplicate name) so the map never holds orphans.
        targets.retain(|name, _| snippets.iter().any(|s| &s.name == name));
        Self {
            snippets,
            path_override: None,
            targets,
        }
    }

    /// Save snippets to ~/.purple/snippets (atomic write, chmod 600).
    pub fn save(&self) -> io::Result<()> {
        if crate::demo_flag::is_demo() {
            return Ok(());
        }
        let Some(path) = self.path_override.clone() else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Could not determine home directory",
            ));
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
            if snippet.interactive {
                content.push_str("interactive=true\n");
            }
            if let Some(hosts) = self.targets.get(&snippet.name)
                && !hosts.is_empty()
            {
                let joined = hosts
                    .iter()
                    .map(|a| escape_alias(a))
                    .collect::<Vec<_>>()
                    .join(",");
                content.push_str(&format!("hosts={joined}\n"));
            }
        }

        fs_util::atomic_write(&path, content.as_bytes())
    }

    /// Default target host aliases saved for `name` (empty when none).
    pub fn targets_for(&self, name: &str) -> &[String] {
        self.targets.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Set (or clear, when empty) the default target host aliases for `name`.
    pub fn set_targets(&mut self, name: &str, aliases: Vec<String>) {
        if aliases.is_empty() {
            self.targets.remove(name);
        } else {
            self.targets.insert(name.to_string(), aliases);
        }
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

    /// Remove a snippet by name, dropping its saved targets too.
    pub fn remove(&mut self, name: &str) {
        self.snippets.retain(|s| s.name != name);
        self.targets.remove(name);
    }
}

/// Percent-encode the comma (and the percent sign itself) in a host alias so it
/// survives the comma-joined `hosts=` line. A host whose SSH `Host` line uses a
/// comma (e.g. `Host web1,web2`) is a single concrete alias containing a comma;
/// without escaping it would shred into phantom aliases on reload.
fn escape_alias(alias: &str) -> String {
    alias.replace('%', "%25").replace(',', "%2C")
}

/// Inverse of [`escape_alias`]. Unescape order is the reverse of escape order so
/// a literal `%2C` in an alias round-trips intact.
fn unescape_alias(token: &str) -> String {
    token.replace("%2C", ",").replace("%25", "%")
}

/// Validate a snippet name: non-empty, no leading/trailing whitespace,
/// no `#`, no `[`, no `]`, no control characters.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err(crate::i18n::text(crate::messages::SNIPPET_NAME_EMPTY).to_string());
    }
    if name != name.trim() {
        return Err(crate::i18n::text(crate::messages::SNIPPET_NAME_WHITESPACE).to_string());
    }
    if name.contains('#') || name.contains('[') || name.contains(']') {
        return Err(crate::i18n::text(crate::messages::SNIPPET_NAME_INVALID_CHARS).to_string());
    }
    if name.contains(|c: char| c.is_control()) {
        return Err(crate::i18n::text(crate::messages::SNIPPET_NAME_CONTROL_CHARS).to_string());
    }
    Ok(())
}

/// Validate a snippet command: non-empty, no control characters (except tab).
pub fn validate_command(command: &str) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err(crate::i18n::text(crate::messages::SNIPPET_COMMAND_EMPTY).to_string());
    }
    if command.contains(|c: char| c.is_control() && c != '\t') {
        return Err(crate::i18n::text(crate::messages::SNIPPET_COMMAND_CONTROL_CHARS).to_string());
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
        if bytes[i] == b'{'
            && bytes.get(i + 1) == Some(&b'{')
            && let Some(end) = command[i + 2..].find("}}")
        {
            let inner = &command[i + 2..i + 2 + end];
            let (name, default) = if let Some((n, d)) = inner.split_once(':') {
                (n.to_string(), Some(d.to_string()))
            } else {
                (inner.to_string(), None)
            };
            if validate_param_name(&name).is_ok() && !seen.contains(&name) && params.len() < 20 {
                seen.insert(name.clone());
                params.push(SnippetParam { name, default });
            }
            i = i + 2 + end + 2;
            continue;
        }
        i += 1;
    }
    params
}

/// Count the distinct valid `{{name}}` parameters in a command without
/// allocating the full parsed list. Equal to `parse_params(command).len()`;
/// used by the list row, which only needs the count, on every frame.
pub fn count_params(command: &str) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    let bytes = command.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 3 < len {
        if bytes[i] == b'{'
            && bytes.get(i + 1) == Some(&b'{')
            && let Some(end) = command[i + 2..].find("}}")
        {
            let inner = &command[i + 2..i + 2 + end];
            let name = inner.split_once(':').map_or(inner, |(n, _)| n);
            if validate_param_name(name).is_ok() && !seen.contains(&name) && seen.len() < 20 {
                seen.push(name);
            }
            i = i + 2 + end + 2;
            continue;
        }
        i += 1;
    }
    seen.len()
}

/// Validate a parameter name: non-empty, alphanumeric/underscore/hyphen only.
/// Rejects `{`, `}`, `'`, whitespace and control chars.
pub fn validate_param_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err(crate::i18n::text(crate::messages::SNIPPET_PARAM_NAME_EMPTY).to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(crate::messages::snippet_param_name_invalid(name));
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
        if i + 3 < len
            && bytes[i] == b'{'
            && bytes[i + 1] == b'{'
            && let Some(end) = command[i + 2..].find("}}")
        {
            let inner = &command[i + 2..i + 2 + end];
            let (name, default) = if let Some((n, d)) = inner.split_once(':') {
                (n, Some(d))
            } else {
                (inner, None)
            };
            // Only a valid name is a parameter. parse_params and count_params
            // gate the same way, so an invalid placeholder (e.g. `{{a b}}`)
            // stays literal here instead of being silently rewritten to ''.
            if validate_param_name(name).is_ok() {
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
            '\u{009b}' => {
                // 8-bit CSI (the single-char form of ESC[): consume the
                // parameter bytes up to the final byte 0x40-0x7E so a colour or
                // cursor sequence does not leak its parameters into the TUI as
                // literal text. The terminator is bounded, so this never eats
                // arbitrary trailing output (unlike a consume-until-ST string
                // sequence, which the other C1 codes are left to strip
                // byte-by-byte instead).
                while let Some(&ch) = chars.peek() {
                    chars.next();
                    if ('\x40'..='\x7e').contains(&ch) {
                        break;
                    }
                }
            }
            c if ('\u{0080}'..='\u{009F}').contains(&c) => {
                // Other C1 control codes: skip the lone byte.
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

/// Exit status a POSIX shell returns when it cannot find the command.
pub const EXIT_COMMAND_NOT_FOUND: i32 = 127;

/// True when a run in a plain shell ended with "command not found", the case
/// the Interactive shell toggle usually solves.
pub fn not_found_hint_applies(interactive: bool, exit_code: Option<i32>) -> bool {
    !interactive && exit_code == Some(EXIT_COMMAND_NOT_FOUND)
}

/// Maximum lines stored per host. Reader continues draining beyond this
/// to prevent child from blocking on a full pipe buffer.
const MAX_OUTPUT_LINES: usize = 10_000;

/// How often [`ChildGuard::wait`] checks whether the child has exited. Short
/// enough that a finished push feels immediate, long enough that waiting on a
/// slow host costs no measurable CPU.
const WAIT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);

/// How long a child gets to act on SIGTERM before SIGKILL follows.
const TERM_GRACE: std::time::Duration = std::time::Duration::from_millis(500);

/// RAII guard that kills the process group on drop.
/// Uses SIGTERM first, then escalates to SIGKILL after a brief wait.
pub struct ChildGuard {
    inner: std::sync::Mutex<Option<std::process::Child>>,
    pgid: i32,
    /// When SIGTERM went out, if it has. The waiting thread reads this and
    /// escalates once the grace window is up, so the caller that asked for
    /// the kill does not have to stand there for it.
    terminated_at: std::sync::Mutex<Option<std::time::Instant>>,
}

impl ChildGuard {
    pub(crate) fn new(child: std::process::Child) -> Self {
        // i32::try_from avoids silent overflow for PIDs > i32::MAX. Fallback 0
        // is a sentinel the Drop guard treats as "no process group": it skips
        // the group signal entirely (a negative pgid built from 0 or 1 would
        // target our own group or PID 1/init) and falls back to child.kill().
        // In practice Linux caps PIDs well below i32::MAX.
        let pgid = i32::try_from(child.id()).unwrap_or(0);
        Self {
            inner: std::sync::Mutex::new(Some(child)),
            pgid,
            terminated_at: std::sync::Mutex::new(None),
        }
    }

    /// Send SIGKILL to the process group. Caller holds no lock on `inner`.
    #[cfg(unix)]
    fn kill_group(&self) {
        if self.pgid > 1 {
            // SAFETY: self.pgid was set by setpgid(0, 0) in pre_exec and is
            // valid for the lifetime of this guard. A negative PID signals
            // the whole group. ESRCH is the expected race and is ignored.
            unsafe {
                libc::kill(-self.pgid, libc::SIGKILL);
            }
        }
    }

    #[cfg(not(unix))]
    fn kill_group(&self) {}

    /// Wait for the child to exit and take it out of the guard so a later
    /// drop cannot signal a recycled PID. Returns `None` when the child was
    /// already taken or the wait failed.
    ///
    /// Polls rather than blocking inside the lock, so a `terminate` from the
    /// UI thread is not stuck behind this call. It also carries out the
    /// SIGKILL escalation: `terminate` only signals, and this is the thread
    /// that is standing here anyway.
    ///
    /// Sweeps the whole group once the child is reaped, whether the run was
    /// stopped or ended on its own. ssh goes away first while a
    /// `ProxyCommand` it spawned can linger, and that command inherits ssh's
    /// stderr, so a reader blocked on the pipe stays blocked long after ssh
    /// itself is gone. A group id stays reserved while any member is alive,
    /// so the sweep reaches the stragglers it is meant for. Where the group
    /// is already empty the signal does nothing, save for a window of a few
    /// instructions after the reap in which the kernel could hand that same
    /// id to a new group leader. `terminate` refuses the kill outright on
    /// that risk; here the alternative is a reader parked on a pipe for as
    /// long as the straggler lives, which is the worse of the two.
    pub(crate) fn wait(&self) -> Option<ExitStatus> {
        let mut escalated = false;
        loop {
            let reaped = {
                let mut lock = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                let child = lock.as_mut()?;
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let _ = lock.take();
                        Some(Some(status))
                    }
                    Ok(None) => None,
                    Err(_) => {
                        let _ = lock.take();
                        Some(None)
                    }
                }
            };
            if let Some(status) = reaped {
                self.kill_group();
                return status;
            }
            if !escalated && self.grace_expired() {
                escalated = true;
                self.kill_group();
            }
            std::thread::sleep(WAIT_POLL_INTERVAL);
        }
    }

    /// True once SIGTERM has been sent and the child has had its grace
    /// window to act on it.
    fn grace_expired(&self) -> bool {
        self.terminated_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|t| t.elapsed() >= TERM_GRACE)
    }

    /// Ask the process group to stop and return at once. SIGTERM goes out
    /// now; the SIGKILL that follows the grace window is left to whichever
    /// thread is in `wait`. Called from the UI thread on Esc and on quit, so
    /// it must not stand and wait. A no-op once the child exited.
    pub(crate) fn terminate(&self) {
        let mut lock = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(child) = lock.as_mut() else {
            return;
        };
        // Already exited? Skip the kill entirely, the PID may be recycled.
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        *self.terminated_at.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(std::time::Instant::now());
        // Group signal only for a valid pgid (> 1). pgid 0 is the overflow
        // sentinel; a negative pgid built from 0 or 1 would hit our own
        // group or PID 1 (init), so those skip to the direct child.kill().
        #[cfg(unix)]
        if self.pgid > 1 {
            // SAFETY: self.pgid was set by setpgid(0,0) in pre_exec and is
            // valid for the lifetime of this guard. kill() with a negative
            // PID signals the entire process group. ESRCH (already exited)
            // is the expected race and the return value is ignored.
            unsafe {
                libc::kill(-self.pgid, libc::SIGTERM);
            }
            return;
        }
        // Fallback: direct kill in case setpgid failed in pre_exec or the
        // pgid was the overflow sentinel. Nothing to escalate to afterwards.
        let _ = child.kill();
    }

    /// Stop the child and stay until it is gone. For `Drop`, where no other
    /// thread is left to carry out the escalation.
    fn terminate_and_reap(&self) {
        self.terminate();
        let deadline = std::time::Instant::now() + TERM_GRACE;
        loop {
            {
                let mut lock = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                let Some(child) = lock.as_mut() else {
                    return;
                };
                match child.try_wait() {
                    Ok(Some(_)) | Err(_) => {
                        let _ = lock.take();
                        return;
                    }
                    Ok(None) => {}
                }
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(WAIT_POLL_INTERVAL);
        }
        self.kill_group();
        let mut lock = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(child) = lock.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Nobody is left in `wait` to escalate, so the reap happens here.
        self.terminate_and_reap();
    }
}

/// Read a pipe into a String under two hard bounds: a total byte cap
/// (`SSH_OUTPUT_MAX_BYTES`) and a line cap (`MAX_OUTPUT_LINES`). The byte cap is
/// applied first via [`read_bounded`], which reads fixed chunks and drains the
/// remainder to a sink once the cap is hit, so a single newline-free megastream
/// from a hostile remote can never buffer unbounded memory. The bounded bytes
/// are then split into at most `MAX_OUTPUT_LINES` lines.
fn read_pipe_capped<R: io::Read>(reader: R, alias: &str, stream: &str) -> String {
    use io::BufRead;
    let mut br = io::BufReader::new(reader);
    let bounded = read_bounded(&mut br, SSH_OUTPUT_MAX_BYTES, alias, stream);

    let mut output = String::new();
    let mut line_count = 0;
    let mut rdr = io::BufReader::new(bounded.as_slice());
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match rdr.read_until(b'\n', &mut buf) {
            Ok(0) => break, // EOF
            Ok(_) => {
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
                    break;
                }
            }
            Err(_) => break,
        }
    }
    output
}

/// Build the base SSH command with shared options for snippet execution.
/// Sets -F, ConnectTimeout, ControlMaster/ControlPath and ClearAllForwardings.
/// Also configures askpass and Bitwarden session env vars.
///
/// When `non_interactive` is true, the child can never reach the controlling
/// tty: `-o StrictHostKeyChecking=yes` turns an unknown host into an error
/// (`accept-new` when `trust_new_host_key` is set by a retry after the trust
/// dialog) and `-o BatchMode=yes` turns a password prompt into `Permission
/// denied` when no source and no session password can answer it. Background
/// fetches (container listings, file browser listings, captured snippet
/// output) pass `true`. Direct CLI use passes `false` so users retain normal
/// host-key trust-on-first-use behavior and ssh's own prompt.
#[allow(clippy::too_many_arguments)]
fn base_ssh_command(
    alias: &str,
    config_path: &Path,
    command: &str,
    askpass: Option<&str>,
    session_password: Option<&str>,
    bw_session: Option<&str>,
    has_active_tunnel: bool,
    non_interactive: bool,
    trust_new_host_key: bool,
    security_key: &crate::security_key::SecurityKeyRun,
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

    if non_interactive {
        let strict = if trust_new_host_key {
            "StrictHostKeyChecking=accept-new"
        } else {
            "StrictHostKeyChecking=yes"
        };
        cmd.arg("-o")
            .arg(strict)
            .arg("-o")
            .arg(crate::askpass_env::SINGLE_PASSWORD_PROMPT_OPT);
        if crate::askpass_env::needs_batch_mode(askpass, session_password) {
            cmd.arg("-o").arg(crate::askpass_env::BATCH_MODE_OPT);
        }
    }

    if has_active_tunnel {
        cmd.arg("-o").arg("ClearAllForwardings=yes");
    }

    cmd.arg("--").arg(alias).arg(command);

    crate::askpass_env::configure_auth(
        &mut cmd,
        alias,
        config_path,
        askpass,
        session_password,
        bw_session,
        non_interactive,
    );
    crate::security_key::configure(&mut cmd, security_key);

    cmd
}

/// Build the SSH Command for a snippet execution with piped I/O.
fn build_snippet_command(ctx: &crate::ssh_context::SshContext<'_>, command: &str) -> Command {
    let mut cmd = base_ssh_command(
        ctx.alias,
        ctx.config_path,
        command,
        ctx.askpass,
        ctx.session_password,
        ctx.bw_session,
        ctx.has_tunnel,
        true,
        ctx.trust_new_host_key,
        ctx.security_key,
    );
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Isolate child into its own process group so we can kill the
    // entire tree without affecting purple itself.
    #[cfg(unix)]
    // SAFETY: the pre-fork callback runs between fork and the exec syscall in
    // the child; only async-signal-safe calls are permitted. `setpgid(0, 0)`
    // is async-signal-safe per POSIX and does not touch Rust runtime state.
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
    let mut cmd = build_snippet_command(ctx, command);

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
            }
            .map(|pipe| {
                crate::security_key::RefusalScan::new(pipe, ctx.env.paths(), ctx.security_key)
            });

            // Spawn reader threads
            let alias_out = alias.to_string();
            let stdout_handle = std::thread::spawn(move || match stdout_pipe {
                Some(pipe) => read_pipe_capped(pipe, &alias_out, "stdout"),
                None => String::new(),
            });
            let alias_err = alias.to_string();
            let stderr_handle = std::thread::spawn(move || match stderr_pipe {
                Some(pipe) => read_pipe_capped(pipe, &alias_err, "stderr"),
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
            log::warn!(
                "[external] snippet ssh spawn failed: run_id={} alias={} err={}",
                run_id,
                alias,
                e
            );
            let _ = tx.send(crate::event::AppEvent::SnippetHostDone {
                run_id,
                alias: alias.to_string(),
                stdout: String::new(),
                stderr: crate::messages::snippet_ssh_launch_failed(&e),
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
    env: std::sync::Arc<crate::runtime::env::Env>,
    command: String,
    bw_session: Option<String>,
    tunnel_aliases: std::collections::HashSet<String>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tx: std::sync::mpsc::Sender<crate::event::AppEvent>,
    parallel: bool,
    security_key: crate::security_key::SecurityKeyRun,
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
                    let env = std::sync::Arc::clone(&env);
                    let command = command.clone();
                    let bw_session = bw_session.clone();
                    let security_key = security_key.clone();
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
                            session_password: None,
                            bw_session: bw_session.as_deref(),
                            has_tunnel,
                            trust_new_host_key: false,
                            security_key: &security_key,
                            env: &env,
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
                        session_password: None,
                        bw_session: bw_session.as_deref(),
                        has_tunnel,
                        trust_new_host_key: false,
                        security_key: &security_key,
                        env: &env,
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
#[allow(clippy::too_many_arguments)]
pub fn run_snippet(
    alias: &str,
    config_path: &Path,
    env: &crate::runtime::env::Env,
    command: &str,
    askpass: Option<&str>,
    bw_session: Option<&str>,
    capture: bool,
    has_active_tunnel: bool,
) -> anyhow::Result<SnippetResult> {
    let ctx = crate::ssh_context::SshContext {
        alias,
        config_path,
        askpass,
        session_password: None,
        bw_session,
        has_tunnel: has_active_tunnel,
        trust_new_host_key: false,
        security_key: &crate::security_key::NO_SECURITY_KEY,
        env,
    };
    run_snippet_ctx(&ctx, command, capture)
}

/// `run_snippet` for callers that already hold an
/// [`SshContext`](crate::ssh_context::SshContext). The context
/// carries the session password and the trust flag, so background fetches
/// (file browser, containers) authenticate the way the TUI decided.
pub fn run_snippet_ctx(
    ctx: &crate::ssh_context::SshContext<'_>,
    command: &str,
    capture: bool,
) -> anyhow::Result<SnippetResult> {
    let alias = ctx.alias;
    // Renew the Vault SSH cert before connecting so container listing,
    // inspect, logs, actions and file-browser operations get a fresh cert
    // just like the interactive connect path does. No-op for non-vault hosts.
    crate::runtime::helpers::ensure_vault_cert_for_alias(ctx.env, alias, ctx.config_path);

    let mut cmd = base_ssh_command(
        alias,
        ctx.config_path,
        command,
        ctx.askpass,
        ctx.session_password,
        ctx.bw_session,
        ctx.has_tunnel,
        capture,
        ctx.trust_new_host_key,
        ctx.security_key,
    );

    if capture {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    } else {
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
    }

    if capture {
        let (status, stdout, stderr) =
            run_with_bounded_output(&mut cmd, alias, ctx.env.paths(), ctx.security_key)?;
        Ok(SnippetResult {
            status,
            stdout,
            stderr,
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

/// Hard cap on stdout/stderr captured from an SSH child process.
/// A hostile or malfunctioning remote can stream unbounded output and
/// blow up purple's memory. 16 MB is far above any legitimate
/// `docker inspect` / `docker logs --tail 200` / `ps -a` output (which
/// peak at hundreds of KB) and below what would meaningfully stress
/// the parse pipelines or terminal buffer.
pub const SSH_OUTPUT_MAX_BYTES: usize = 16 * 1024 * 1024;

/// Spawn a child with piped stdout/stderr and read each pipe with a
/// hard byte cap. Once the cap is hit the remaining bytes are drained
/// to a sink (so the child can exit cleanly) and a debug log records
/// the truncation. Returns the captured payload as lossy-UTF8 Strings
/// alongside the child's exit status.
fn run_with_bounded_output(
    cmd: &mut Command,
    alias: &str,
    paths: Option<&crate::runtime::env::Paths>,
    security_key: &crate::security_key::SecurityKeyRun,
) -> anyhow::Result<(ExitStatus, String, String)> {
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn ssh for '{}': {}", alias, e))?;
    let stdout = child.stdout.take();
    let stderr = child
        .stderr
        .take()
        .map(|pipe| crate::security_key::RefusalScan::new(pipe, paths, security_key));

    let alias_for_stdout = alias.to_string();
    let stdout_handle = std::thread::spawn(move || match stdout {
        Some(mut pipe) => {
            read_bounded(&mut pipe, SSH_OUTPUT_MAX_BYTES, &alias_for_stdout, "stdout")
        }
        None => Vec::new(),
    });
    let alias_for_stderr = alias.to_string();
    let stderr_handle = std::thread::spawn(move || match stderr {
        Some(mut pipe) => {
            read_bounded(&mut pipe, SSH_OUTPUT_MAX_BYTES, &alias_for_stderr, "stderr")
        }
        None => Vec::new(),
    });

    // Join the drain threads BEFORE waiting on the child. The threads
    // own the pipe read-ends; when they finish (cap hit or EOF) the
    // reader handle drops and closes its side of the pipe, which
    // unblocks any pending write on the remote child. Waiting on the
    // child first would deadlock the moment stdout exceeded the kernel
    // pipe buffer (typically 64 KB on Linux): the child blocks on
    // write, the parent blocks on wait, neither thread runs.
    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();
    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("ssh wait failed for '{}': {}", alias, e))?;

    Ok((
        status,
        String::from_utf8_lossy(&stdout_bytes).to_string(),
        String::from_utf8_lossy(&stderr_bytes).to_string(),
    ))
}

fn read_bounded<R: std::io::Read>(
    reader: &mut R,
    max: usize,
    alias: &str,
    stream: &str,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 * 1024);
    let mut chunk = [0u8; 8 * 1024];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if out.len() + n > max {
                    let allowed = max.saturating_sub(out.len());
                    out.extend_from_slice(&chunk[..allowed]);
                    log::warn!(
                        "[external] ssh {} for '{}' exceeded {} bytes; truncating remainder",
                        stream,
                        alias,
                        max
                    );
                    // Drain remaining bytes so the child can exit cleanly
                    // instead of blocking on a backpressured pipe.
                    let _ = std::io::copy(reader, &mut std::io::sink());
                    break;
                }
                out.extend_from_slice(&chunk[..n]);
            }
            Err(e) => {
                log::debug!("[external] ssh {stream} read error for '{alias}': {e}");
                break;
            }
        }
    }
    out
}

/// Static, execution-free blast-radius analysis of a snippet command for the
/// Snippets detail IMPACT card. Implemented in [`crate::snippet_impact`]
/// (quote-aware segmenter + curated capability table); re-exported here so the
/// `crate::snippet::analyze_command` path stays stable.
pub use crate::snippet_impact::{Category, CommandImpact, Finding, Severity, analyze_command};

/// Indices of snippets in `store` matching `query` by name, command or
/// description (case-insensitive). A `None` or empty query returns every index
/// in order. Shared by the host-list snippet picker and the Snippets tab.
pub fn filtered_indices(store: &SnippetStore, query: Option<&str>) -> Vec<usize> {
    match query {
        None | Some("") => (0..store.snippets.len()).collect(),
        Some(q) => store
            .snippets
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                crate::app::contains_ci(&s.name, q)
                    || crate::app::contains_ci(&s.command, q)
                    || crate::app::contains_ci(&s.description, q)
            })
            .map(|(i, _)| i)
            .collect(),
    }
}

#[cfg(test)]
#[path = "snippet_tests.rs"]
pub(crate) mod tests;
