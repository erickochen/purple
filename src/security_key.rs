//! Security key (FIDO2) PINs for ssh runs that have no terminal.
//!
//! OpenSSH asks for a key's PIN through askpass. A background run answers it
//! from a PIN typed in the TUI, carried in environment variables for that
//! one run. A wrong PIN costs an attempt on the key, so every run that
//! carried PINs records a refusal. askpass reads that record before it hands
//! the PIN out again.

use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::runtime::env::{Env, Paths};

/// Shortest PIN CTAP2 accepts. A shorter one can never be right and would
/// still cost an attempt on the key.
pub const PIN_MIN_CHARS: usize = 4;

/// Longest PIN CTAP2 accepts, in bytes.
pub const PIN_MAX_BYTES: usize = 63;

/// PINs one run carries at most. One per key file, and nobody uses more.
const MAX_PINS_PER_RUN: usize = 8;

/// How long a request or hand-out marker counts. Covers a slow touch on top
/// of a two-step file browser listing.
const MARKER_MAX_AGE: Duration = Duration::from_secs(120);

/// Bytes of the key path digest used in a refusal marker name.
const KEY_DIGEST_BYTES: usize = 16;

/// Filename prefix shared by every PIN marker, so the retry marker sweep
/// can leave them alone.
pub(crate) const PIN_MARKER_PREFIX: &str = ".askpass_pin.";

/// Names the flow that may ask the TUI for a missing PIN.
pub(crate) const FLOW_VAR: &str = "PURPLE_ASKPASS_SK_FLOW";
const KEY_VAR_PREFIX: &str = "PURPLE_ASKPASS_SK_KEY_";
const PIN_VAR_PREFIX: &str = "PURPLE_ASKPASS_SK_PIN_";

/// First OpenSSH release that prefixes a keyboard-interactive prompt with
/// `(user@host) `. Before it a server could send a prompt that reads like a
/// PIN prompt, so purple hands out no PIN there.
const SAFE_PROMPT_OPENSSH: (u32, u32) = (8, 5);

/// A PIN typed for one key file.
#[derive(Clone, PartialEq, Eq)]
pub struct SecurityKeyPin {
    pub key_path: String,
    pub pin: String,
}

// Hand-written so a stray `{:?}` can never print the PIN.
impl std::fmt::Debug for SecurityKeyPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecurityKeyPin")
            .field("key_path", &self.key_path)
            .finish_non_exhaustive()
    }
}

/// A flow that can ask for a missing PIN and run again with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinFlow {
    Tunnel,
    FileBrowser,
    KeyPush,
}

impl PinFlow {
    pub fn as_str(self) -> &'static str {
        match self {
            PinFlow::Tunnel => "tunnel",
            PinFlow::FileBrowser => "file_browser",
            PinFlow::KeyPush => "key_push",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "tunnel" => Some(PinFlow::Tunnel),
            "file_browser" => Some(PinFlow::FileBrowser),
            "key_push" => Some(PinFlow::KeyPush),
            _ => None,
        }
    }
}

/// The PINs one ssh run may hand out, plus the flow that asks for a missing
/// one. Runs without a flow (containers, snippets) only reuse PINs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecurityKeyRun {
    pub pins: Vec<SecurityKeyPin>,
    pub flow: Option<PinFlow>,
}

/// A run that carries no PIN and asks for none.
pub static NO_SECURITY_KEY: SecurityKeyRun = SecurityKeyRun {
    pins: Vec::new(),
    flow: None,
};

impl SecurityKeyRun {
    /// True when this run hands out a PIN for `key_path`.
    pub fn carries(&self, key_path: &str) -> bool {
        self.pins.iter().any(|p| p.key_path == key_path)
    }
}

/// The parts of OpenSSH's `Enter PIN for <type> key <path>: ` prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinPrompt<'a> {
    pub key_type: &'a str,
    pub key_path: &'a str,
}

/// Recognize the PIN prompt ssh shows for a FIDO2 key file. Anchored at the
/// start: a keyboard-interactive prompt begins with `(user@host) ` and the
/// agent's own prompt names a fingerprint, so neither matches.
pub fn parse_pin_prompt(prompt: &str) -> Option<PinPrompt<'_>> {
    let rest = prompt.strip_prefix("Enter PIN for ")?;
    let rest = rest.trim_end().strip_suffix(':')?;
    let (key_type, key_path) = rest.split_once(" key ")?;
    if key_type.contains(char::is_whitespace) || !is_security_key_type(key_type) {
        return None;
    }
    if key_path.is_empty()
        || key_path.contains('\n')
        || key_path.starts_with("SHA256:")
        || key_path.starts_with("MD5:")
    {
        return None;
    }
    Some(PinPrompt { key_type, key_path })
}

/// `ED25519-SK`, `ECDSA-SK` and their `-CERT` forms, as `sshkey_type` names them.
fn is_security_key_type(key_type: &str) -> bool {
    key_type.ends_with("-SK") || key_type.ends_with("-SK-CERT")
}

/// Why a security key did not sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReason {
    /// The key turned the PIN down or none was given.
    WrongPin,
    /// The key did not answer: it locks after three wrong PINs in a row and
    /// a touch can time out.
    NoAnswer,
    /// No key was plugged in.
    NotFound,
    Other,
}

impl RefusalReason {
    fn as_str(self) -> &'static str {
        match self {
            RefusalReason::WrongPin => "wrong_pin",
            RefusalReason::NoAnswer => "no_answer",
            RefusalReason::NotFound => "not_found",
            RefusalReason::Other => "other",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "wrong_pin" => RefusalReason::WrongPin,
            "no_answer" => RefusalReason::NoAnswer,
            "not_found" => RefusalReason::NotFound,
            _ => RefusalReason::Other,
        }
    }

    /// Map the `ssh_err` text ssh appends to a signing failure.
    fn from_ssh_error(text: &str) -> Self {
        if text.contains("incorrect passphrase") {
            RefusalReason::WrongPin
        } else if text.contains("invalid format") {
            RefusalReason::NoAnswer
        } else if text.contains("not found") {
            RefusalReason::NotFound
        } else {
            RefusalReason::Other
        }
    }
}

/// A security key file ssh failed to sign with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignFailure {
    pub key_path: String,
    pub reason: RefusalReason,
}

/// Read one `sign_and_send_pubkey: signing failed for <type> "<path>": <why>`
/// line. Only security key files count. A key held by an agent is the
/// agent's business.
pub fn parse_sign_failure(line: &str) -> Option<SignFailure> {
    const MARKER: &str = "signing failed for ";
    let rest = &line[line.find(MARKER)? + MARKER.len()..];
    let (key_type, rest) = rest.split_once(" \"")?;
    if !is_security_key_type(key_type) {
        return None;
    }
    let end = rest.rfind('"')?;
    let (key_path, tail) = (&rest[..end], &rest[end + 1..]);
    if key_path.is_empty() || tail.starts_with(" from agent") {
        return None;
    }
    Some(SignFailure {
        key_path: key_path.to_string(),
        reason: RefusalReason::from_ssh_error(tail),
    })
}

/// Every security key signing failure in an ssh run's stderr.
pub fn sign_failures(stderr: &str) -> Vec<SignFailure> {
    stderr.lines().filter_map(parse_sign_failure).collect()
}

/// Hand the run's PINs and flow to ssh. Only the env carries them, never argv.
pub(crate) fn configure(cmd: &mut Command, run: &SecurityKeyRun) {
    for (i, pin) in run.pins.iter().take(MAX_PINS_PER_RUN).enumerate() {
        cmd.env(format!("{KEY_VAR_PREFIX}{i}"), &pin.key_path)
            .env(format!("{PIN_VAR_PREFIX}{i}"), &pin.pin);
    }
    if let Some(flow) = run.flow {
        cmd.env(FLOW_VAR, flow.as_str());
    }
}

/// The PIN this run carries for exactly `key_path`, if any.
pub(crate) fn pin_from_env<'a>(env: &'a Env, key_path: &str) -> Option<&'a str> {
    (0..MAX_PINS_PER_RUN)
        .map_while(|i| env.var(&format!("{KEY_VAR_PREFIX}{i}")).map(|k| (i, k)))
        .find(|(_, k)| *k == key_path)
        .and_then(|(i, _)| env.var(&format!("{PIN_VAR_PREFIX}{i}")))
}

/// The flow this run may ask a missing PIN for.
pub(crate) fn flow_from_env(env: &Env) -> Option<PinFlow> {
    env.var(FLOW_VAR).and_then(PinFlow::parse)
}

/// True when `ssh -V` names an OpenSSH that cannot pass a server prompt off
/// as a PIN prompt. Unknown versions count as too old.
pub fn ssh_version_allows_pins(version: &str) -> bool {
    let Some(rest) = version.split("OpenSSH_").nth(1) else {
        return false;
    };
    let mut parts = rest.split(|c: char| !c.is_ascii_digit());
    let major = parts.next().and_then(|p| p.parse::<u32>().ok());
    let minor = parts.next().and_then(|p| p.parse::<u32>().ok());
    matches!((major, minor), (Some(ma), Some(mi)) if (ma, mi) >= SAFE_PROMPT_OPENSSH)
}

fn key_digest(key_path: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(key_path.as_bytes())
        .iter()
        .take(KEY_DIGEST_BYTES)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Contents of a marker written within `MARKER_MAX_AGE`.
fn read_fresh(path: &Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    // A clock that stepped back makes a marker look newer than now. It still
    // counts, since it guards PIN attempts.
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    if age >= MARKER_MAX_AGE {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn write_marker(path: &Path, body: &str) {
    if let Err(e) = crate::fs_util::atomic_write(path, body.as_bytes()) {
        log::warn!(
            "[config] security key: marker write failed path={}: {e}",
            path.display()
        );
    }
}

/// Record that the key refused. askpass hands out no PIN for it until a new
/// one is typed.
pub fn note_refusal(paths: Option<&Paths>, failure: &SignFailure) {
    let Some(paths) = paths else {
        return;
    };
    let path = paths.askpass_pin_refused_marker(&key_digest(&failure.key_path));
    write_marker(
        &path,
        &format!("{}\n{}\n", failure.reason.as_str(), failure.key_path),
    );
    log::debug!(
        "[purple] security key: refusal recorded for {} reason={}",
        failure.key_path,
        failure.reason.as_str()
    );
}

/// Longest stderr line the refusal scan keeps. ssh's sign failure line is
/// far shorter; the cap only bounds memory on a runaway line.
const SCAN_LINE_MAX: usize = 4096;

/// Passes ssh's stderr through and records a refusal of a key the run
/// carried a PIN for the moment its line arrives. A jump host that refused
/// the PIN is then on record before the target asks for it.
pub struct RefusalScan<R> {
    inner: R,
    paths: Option<Paths>,
    run: SecurityKeyRun,
    line: Vec<u8>,
}

impl<R: std::io::Read> RefusalScan<R> {
    pub fn new(inner: R, paths: Option<&Paths>, run: &SecurityKeyRun) -> Self {
        Self {
            inner,
            paths: paths.cloned(),
            run: run.clone(),
            line: Vec::new(),
        }
    }

    fn end_line(&mut self) {
        let text = String::from_utf8_lossy(&self.line);
        if let Some(failure) = parse_sign_failure(text.trim_end_matches('\r'))
            && self.run.carries(&failure.key_path)
        {
            note_refusal(self.paths.as_ref(), &failure);
        }
        self.line.clear();
    }
}

impl<R: std::io::Read> std::io::Read for RefusalScan<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if self.run.pins.is_empty() {
            return Ok(n);
        }
        for &byte in &buf[..n] {
            if byte == b'\n' {
                self.end_line();
            } else if self.line.len() < SCAN_LINE_MAX {
                self.line.push(byte);
            }
        }
        if n == 0 && !self.line.is_empty() {
            self.end_line();
        }
        Ok(n)
    }
}

/// Why the key refused the PIN purple last handed out for it, if it did.
pub fn refusal_for(paths: Option<&Paths>, key_path: &str) -> Option<RefusalReason> {
    let path = paths?.askpass_pin_refused_marker(&key_digest(key_path));
    let body = std::fs::read_to_string(path).ok()?;
    Some(RefusalReason::parse(body.lines().next().unwrap_or("")))
}

/// Forget a refusal, because the user typed a new PIN for the key.
pub fn clear_refusal(paths: Option<&Paths>, key_path: &str) {
    let Some(paths) = paths else {
        return;
    };
    if std::fs::remove_file(paths.askpass_pin_refused_marker(&key_digest(key_path))).is_ok() {
        log::debug!("[purple] security key: refusal cleared for {key_path}");
    }
}

/// Tell the TUI this run met a PIN prompt it had no PIN for.
pub(crate) fn arm_pin_request(paths: Option<&Paths>, flow: PinFlow, alias: &str, key_path: &str) {
    let Some(paths) = paths else {
        return;
    };
    write_marker(
        &paths.askpass_pin_request_marker(flow.as_str(), alias),
        key_path,
    );
    log::debug!(
        "[purple] security key: PIN requested alias={alias} flow={} key={key_path}",
        flow.as_str()
    );
}

/// Take the key path a run of `flow` for `alias` asked a PIN for.
pub fn take_pin_request(paths: Option<&Paths>, flow: PinFlow, alias: &str) -> Option<String> {
    let path = paths?.askpass_pin_request_marker(flow.as_str(), alias);
    let body = read_fresh(&path);
    let _ = std::fs::remove_file(&path);
    let key_path = body?.trim_end_matches('\n').to_string();
    (!key_path.is_empty()).then_some(key_path)
}

/// `(pid, key path)` pairs from a hand-out marker.
fn parse_hand_outs(body: &str) -> Vec<(u32, String)> {
    body.lines()
        .filter_map(|l| {
            let (pid, key) = l.split_once('\t')?;
            Some((pid.parse().ok()?, key.to_string()))
        })
        .collect()
}

/// True when the ssh process `pid` already received the PIN for `key_path`.
/// One ssh can sign twice with the same key file (the key plus its
/// certificate). A wrong PIN would then cost two attempts.
pub(crate) fn handed_out(paths: Option<&Paths>, alias: &str, pid: u32, key_path: &str) -> bool {
    let Some(paths) = paths else {
        return false;
    };
    read_fresh(&paths.askpass_pin_used_marker(alias)).is_some_and(|b| {
        parse_hand_outs(&b)
            .iter()
            .any(|(p, k)| *p == pid && k == key_path)
    })
}

/// Record that the ssh process `pid` received the PIN for `key_path`.
pub(crate) fn record_hand_out(paths: Option<&Paths>, alias: &str, pid: u32, key_path: &str) {
    let Some(paths) = paths else {
        return;
    };
    let path = paths.askpass_pin_used_marker(alias);
    let mut body = read_fresh(&path).unwrap_or_default();
    body.push_str(&format!("{pid}\t{key_path}\n"));
    write_marker(&path, &body);
    log::debug!("[purple] security key: PIN handed out alias={alias} key={key_path}");
}

/// Least time between two hand-outs of one key's PIN. A wrong PIN fails
/// before the key asks for a touch, so its refusal is on record well within.
const HAND_OUT_SPACING: Duration = Duration::from_secs(2);

/// Wait after a hand-out whose ssh already exited. Its signing is done, and
/// purple reads a refusal line as ssh writes it, so this only covers the
/// moment between the two.
const EXITED_HOLDER_GRACE: Duration = Duration::from_millis(250);

/// Let the ssh process `pid` have the PIN for `key_path`, queued behind any
/// other hand-out of the same key. Waits until the last one is
/// `HAND_OUT_SPACING` old (less once that ssh is gone). Returns false when
/// the key refused it meanwhile. Parallel runs on hosts sharing a key then
/// spend one attempt on a stale PIN, not one each.
pub(crate) fn claim_hand_out(paths: Option<&Paths>, alias: &str, pid: u32, key_path: &str) -> bool {
    let Some(paths) = paths else {
        return true;
    };
    let digest = key_digest(key_path);
    let _lock = KeyLock::acquire(&paths.askpass_pin_lock_marker(&digest));
    let last = paths.askpass_pin_last_marker(&digest);
    if let Some(wait) = spacing_left(&last) {
        log::debug!(
            "[purple] security key: PIN hand-out waits {}ms key={key_path}",
            wait.as_millis()
        );
        std::thread::sleep(wait);
    }
    if let Some(reason) = refusal_for(Some(paths), key_path) {
        log::debug!(
            "[purple] security key: PIN refused while waiting, withheld key={key_path} reason={}",
            reason.as_str()
        );
        return false;
    }
    write_marker(&last, &pid.to_string());
    record_hand_out(Some(paths), alias, pid, key_path);
    true
}

/// Time left before the hand-out recorded at `last` may be followed. The
/// marker holds the pid of the ssh that got the PIN.
fn spacing_left(last: &Path) -> Option<Duration> {
    let modified = std::fs::metadata(last).ok()?.modified().ok()?;
    // A clock that stepped back makes the marker look new: wait it out.
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    let holder_gone = std::fs::read_to_string(last)
        .ok()
        .and_then(|body| body.trim().parse::<u32>().ok())
        .is_some_and(|holder| !process_alive(holder));
    if holder_gone {
        // Counted from now: the holder may have exited only just now.
        return (age < HAND_OUT_SPACING).then_some(EXITED_HOLDER_GRACE);
    }
    HAND_OUT_SPACING.checked_sub(age).filter(|d| !d.is_zero())
}

/// True while process `pid` exists. Unknown counts as alive, so the caller
/// waits rather than risk a second attempt.
fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return true;
        };
        // SAFETY: signal 0 only checks that the process exists.
        if unsafe { libc::kill(pid, 0) } == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// Exclusive lock on a file, released when dropped.
struct KeyLock {
    _file: Option<std::fs::File>,
}

impl KeyLock {
    fn acquire(path: &Path) -> Self {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = match std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)
        {
            Ok(file) => file,
            Err(e) => {
                log::warn!(
                    "[config] security key: PIN lock unavailable path={}: {e}",
                    path.display()
                );
                return Self { _file: None };
            }
        };
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            // SAFETY: the descriptor belongs to `file`, which outlives the call.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
                log::warn!(
                    "[config] security key: PIN lock failed path={}: {}",
                    path.display(),
                    std::io::Error::last_os_error()
                );
            }
        }
        Self { _file: Some(file) }
    }
}

/// Take the key paths whose PIN went out to a run for `alias`.
pub fn take_handed_out_keys(paths: Option<&Paths>, alias: &str) -> Vec<String> {
    let Some(paths) = paths else {
        return Vec::new();
    };
    let path = paths.askpass_pin_used_marker(alias);
    let body = read_fresh(&path).unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    parse_hand_outs(&body).into_iter().map(|(_, k)| k).collect()
}

/// What one key press does to a PIN typed on the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PinInput {
    Typed,
    Erased,
    Ignored,
    Submit,
    Cancel,
}

/// Apply one key press to the PIN buffer. Ctrl-C and Ctrl-D cancel like Esc.
/// Raw mode turns them into plain key presses.
pub(crate) fn step_pin_input(buf: &mut String, key: &KeyEvent) -> PinInput {
    if key.kind == KeyEventKind::Release {
        return PinInput::Ignored;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Enter => PinInput::Submit,
        KeyCode::Esc => PinInput::Cancel,
        KeyCode::Char('c') | KeyCode::Char('d') if ctrl => PinInput::Cancel,
        KeyCode::Backspace => {
            if buf.pop().is_some() {
                PinInput::Erased
            } else {
                PinInput::Ignored
            }
        }
        KeyCode::Char(c)
            if !ctrl
                && !key.modifiers.contains(KeyModifiers::ALT)
                && !c.is_control()
                && buf.len() + c.len_utf8() <= PIN_MAX_BYTES =>
        {
            buf.push(c);
            PinInput::Typed
        }
        _ => PinInput::Ignored,
    }
}

/// Leaves raw mode however the read ends.
struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

fn open_tty() -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().write(true).open("/dev/tty")
}

/// Ask for the PIN on the terminal, one star per character. Returns `None`
/// when the user cancels.
pub(crate) fn read_pin_from_tty(prompt: &str) -> std::io::Result<Option<String>> {
    let mut tty = open_tty()?;
    loop {
        write!(tty, "{prompt}")?;
        tty.flush()?;
        let pin = {
            crossterm::terminal::enable_raw_mode()?;
            let _raw = RawModeGuard;
            read_pin_keys(&mut tty)?
        };
        match pin {
            None => return Ok(None),
            Some(p) if p.chars().count() < PIN_MIN_CHARS => {
                writeln!(tty, "{}", crate::messages::askpass::pin_too_short())?;
            }
            Some(p) => return Ok(Some(p)),
        }
    }
}

fn read_pin_keys(tty: &mut std::fs::File) -> std::io::Result<Option<String>> {
    let mut buf = String::new();
    loop {
        let crossterm::event::Event::Key(key) = crossterm::event::read()? else {
            continue;
        };
        match step_pin_input(&mut buf, &key) {
            PinInput::Typed => write!(tty, "*")?,
            PinInput::Erased => write!(tty, "\x08 \x08")?,
            PinInput::Ignored => {}
            PinInput::Submit => {
                write!(tty, "\r\n")?;
                return Ok(Some(buf));
            }
            PinInput::Cancel => {
                write!(tty, "\r\n")?;
                return Ok(None);
            }
        }
        tty.flush()?;
    }
}

/// Show ssh's notice on the terminal. purple pipes ssh's stderr, so ssh
/// cannot print it there itself.
pub(crate) fn show_on_tty(text: &str) {
    if let Ok(mut tty) = open_tty() {
        let _ = write!(tty, "{text}\r\n");
    }
}

#[cfg(test)]
#[path = "security_key_tests.rs"]
mod tests;
