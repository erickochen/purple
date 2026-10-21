//! Live-data layer for active SSH tunnels. Two worker types:
//!
//! - One stderr-parser thread per tunnel that reads `ssh -v` output and
//!   emits `ChannelEvent`s when ssh logs `debug1: channel N: new` and
//!   `debug1: channel N: free` lines. Also keeps the last few stderr
//!   lines so `poll_tunnels()` can surface a meaningful exit reason.
//!
//! - One shared lsof poller that runs `lsof -iTCP -P -n` every 2s,
//!   filters by the active tunnel bind ports and emits `LsofMessage`
//!   snapshots with connected clients and port conflicts.
//!
//! All communication is via `std::sync::mpsc` channels. No tokio.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
#[cfg(target_os = "macos")]
use std::process::Stdio;
use std::process::{ChildStderr, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Maximum number of channel events kept in the per-tunnel ringbuffer.
pub const MAX_EVENTS: usize = 50;
/// Bucket width of the rolling activity history, in seconds. Two seconds
/// matches the lsof poll interval, so the sparkline is fed at the same
/// rate it is sampled. Each bucket carries the peak concurrent client
/// count observed within its window.
pub const BUCKET_SECS: u64 = 2;

/// Number of buckets in the rolling activity history. Combined with
/// `BUCKET_SECS` this is `HISTORY_BUCKETS * BUCKET_SECS = 300s ≈ 5 min`
/// of history. Wide window so sparse traffic still has visible content
/// across the chart. Bucket `HISTORY_BUCKETS - 1` is "now".
pub const HISTORY_BUCKETS: usize = 150;
/// Number of stderr lines kept per tunnel for exit-reason display.
pub const STDERR_BUFFER_LINES: usize = 10;
/// Maximum number of clients reported per bind port. The card only
/// renders a handful and lsof can list hundreds of peers on a busy
/// load balancer, so we cap to keep allocation bounded.
pub const MAX_CLIENTS_PER_PORT: usize = 64;

/// Channel-open or -close event observed in `ssh -v` stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelEventKind {
    Open,
    Close,
}

/// What kind of ssh channel this event belongs to. The bracketed token
/// after `channel N: new` in `ssh -v` output ("port listener",
/// "direct-tcpip", "dynamic-tcpip" etc.) maps onto these variants.
///
/// Only `Direct`, `Forwarded` and `Dynamic` represent end-user traffic;
/// the rest are ssh-internal bookkeeping (the listener that binds the
/// local port, mux master channels, agent forwarding) and are filtered
/// out before rendering the EVENTS card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    /// `direct-tcpip` — outgoing client connection through a LocalForward.
    Direct,
    /// `forwarded-tcpip` — incoming client connection arriving via a
    /// RemoteForward.
    Forwarded,
    /// `dynamic-tcpip` — single SOCKS request through a DynamicForward.
    Dynamic,
    /// Anything else (`port listener`, `client-session`, `mux-master`,
    /// `auth-agent@openssh.com`, `x11`, …).
    Other,
}

impl ChannelKind {
    /// Parse the bracketed token from a `channel N: new [<type>]` line.
    pub fn from_bracket(token: &str) -> Self {
        match token {
            "direct-tcpip" => Self::Direct,
            "forwarded-tcpip" => Self::Forwarded,
            "dynamic-tcpip" => Self::Dynamic,
            _ => Self::Other,
        }
    }

    /// True if this kind represents end-user traffic that should appear
    /// in the EVENTS card.
    pub fn is_user_visible(self) -> bool {
        matches!(self, Self::Direct | Self::Forwarded | Self::Dynamic)
    }
}

#[derive(Debug, Clone)]
pub struct ChannelEvent {
    pub at: Instant,
    pub channel_id: u32,
    pub kind: ChannelEventKind,
    /// Bracketed channel type from the `new` line, or the kind that
    /// was recorded when the channel opened (for `Close` events).
    /// `None` if neither side could be parsed (defensive — the
    /// regression suite expects every event to have a kind).
    pub channel_kind: Option<ChannelKind>,
    /// For `Close` events, the matching open time (so the UI can render
    /// the channel duration). `None` if the open was missed.
    pub opened_at: Option<Instant>,
}

/// Number of buckets in the per-client throughput history. The renderer
/// uses `VIZ_TICK_MS` cadence (one shift per 100ms), so 12 cells covers
/// the most recent ~1.2 seconds. The latest `current_rx_bps + current_tx_bps`
/// reading is pushed into the rightmost cell every tick, which gives the
/// braille wave continuous leftward motion at terminal frame rate even
/// when underlying lsof samples land only every few seconds.
pub const PEER_VIZ_BUCKETS: usize = 12;

/// One peer observed by the lsof poller as connected to a forwarded port.
#[derive(Debug, Clone)]
pub struct ClientPeer {
    pub src: String,
    pub process: String,
    /// Owner pid of the client socket. Surfaced in the CLIENTS card so
    /// users can correlate a connection with a local process.
    pub pid: u32,
    pub since: Instant,
    /// On macOS, the user-facing app that "owns" this socket according
    /// to the kernel-tracked responsible-pid. Lets the CLIENTS card show
    /// `Safari` for a `WebKit.Networking` XPC daemon, or `Ghostty` for
    /// a `psql` started from a Ghostty terminal. `None` when responsible
    /// pid resolution is unavailable, equals self, or the lookup failed.
    pub responsible_app: Option<String>,
    /// Most-recent rx/tx bytes-per-second sample, derived by diffing
    /// per-socket cumulative byte counters between lsof polls. Zero
    /// until a second sample arrives. macOS uses a per-pid fallback
    /// when per-socket counters are not available.
    pub current_rx_bps: u64,
    pub current_tx_bps: u64,
    /// Cumulative byte counters at the most recent sample. Used to diff
    /// against the next sample. `None` until the first sample arrives.
    pub bytes_rcvd: Option<u64>,
    pub bytes_sent: Option<u64>,
    /// Time of the most recent sample. `None` until the first sample.
    pub last_sample_at: Option<Instant>,
}

/// Another process bound to the same port as our tunnel. Detected when
/// lsof shows a LISTEN row on a tunnel bind port owned by a pid that
/// is not our tunnel pid. Reserved for future port-conflict surfacing
/// (see `app::tunnel_state::TunnelsState::conflicts`); the minimal
/// option-A detail panel does not show conflicts inline.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PortConflict {
    pub port: u16,
    pub process: String,
    pub pid: u32,
}

/// Per-tunnel live state. Lives on `ActiveTunnel` so a HashMap remove()
/// drops the state and joins the parser thread (no zijkanaal map that
/// could leak after `tunnels.active.remove()`).
pub struct TunnelLiveState {
    pub events: VecDeque<ChannelEvent>,
    pub opens_history: [u8; HISTORY_BUCKETS],
    /// Wall-clock time of the most recent rotate. Used by `rotate_if_due`
    /// to advance the history buckets one slot per elapsed minute.
    pub history_last_rotate: Instant,
    pub peak_concurrent: u32,
    pub total_opens: u32,
    pub last_event_at: Option<Instant>,
    pub active_channels: u32,
    /// Map of currently-open channel id -> open time + kind, used to
    /// compute duration and restore the kind when a `Close` arrives.
    pub channel_open: HashMap<u32, (Instant, ChannelKind)>,
    /// Filled by `poll_tunnels()` when the child exits unexpectedly.
    pub last_exit: Option<(i32, String)>,
    /// Last few stderr lines, written by the parser thread, read by
    /// `poll_tunnels()` on exit to compose `last_exit`.
    pub stderr_buffer: Arc<Mutex<VecDeque<String>>>,
    /// Joined when the `ActiveTunnel` is dropped.
    pub parser_thread: Option<JoinHandle<()>>,
    /// Set to true on `ActiveTunnel` drop so the parser thread can exit
    /// promptly even if stderr is still readable. The pipe close from
    /// `child.kill()` is the primary signal; this is a belt-and-braces.
    pub parser_stop: Arc<AtomicBool>,
    /// Set by the parser thread once ssh says the target let it in.
    pub authenticated: Arc<AtomicBool>,
    /// Set by the parser thread when ssh gave up on authentication.
    pub auth_refused: Arc<AtomicBool>,
    /// Set by the parser thread once it read stderr to the end, so an exit
    /// is judged on every line ssh wrote.
    pub drained: Arc<AtomicBool>,
    /// When `poll` first saw the child gone.
    pub exited_at: Option<Instant>,

    /// Rolling history of bytes per second received (downstream from
    /// remote to laptop). Same bucket layout as `opens_history`.
    pub rx_history: [u64; HISTORY_BUCKETS],
    /// Rolling history of bytes per second sent (upstream from laptop
    /// to remote).
    pub tx_history: [u64; HISTORY_BUCKETS],
    /// Aggregated bytes-per-second across every connected client. Set
    /// from the per-peer lsof samples in `TunnelState::poll` so this
    /// value matches what the roster's per-peer rows report.
    pub current_rx_bps: u64,
    pub current_tx_bps: u64,
    /// Peak observed since tunnel start. Tracks the running max of
    /// `current_rx_bps` / `current_tx_bps` across polls.
    pub peak_rx_bps: u64,
    pub peak_tx_bps: u64,
    /// Wall-clock of the most recent throughput aggregation that saw
    /// at least one sampled peer. `None` until the first sample
    /// arrives — UI shows `sampling…` in that gap.
    pub last_throughput_at: Option<Instant>,
}

impl TunnelLiveState {
    pub fn new(started_at: Instant) -> Self {
        Self {
            events: VecDeque::with_capacity(MAX_EVENTS),
            opens_history: [0u8; HISTORY_BUCKETS],
            history_last_rotate: started_at,
            peak_concurrent: 0,
            total_opens: 0,
            last_event_at: None,
            active_channels: 0,
            channel_open: HashMap::new(),
            last_exit: None,
            stderr_buffer: Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_BUFFER_LINES))),
            parser_thread: None,
            parser_stop: Arc::new(AtomicBool::new(false)),
            authenticated: Arc::new(AtomicBool::new(false)),
            auth_refused: Arc::new(AtomicBool::new(false)),
            drained: Arc::new(AtomicBool::new(false)),
            exited_at: None,
            rx_history: [0u64; HISTORY_BUCKETS],
            tx_history: [0u64; HISTORY_BUCKETS],
            current_rx_bps: 0,
            current_tx_bps: 0,
            peak_rx_bps: 0,
            peak_tx_bps: 0,
            last_throughput_at: None,
        }
    }

    /// Record an open or close event from the stderr parser thread.
    /// Updates counters and the bounded ringbuffer. The rolling
    /// `opens_history` is fed by `sample_activity` rather than by
    /// individual events: the sparkline tracks ongoing concurrency, not
    /// just channel-open bursts. End-user channels (Direct, Forwarded,
    /// Dynamic) bump `total_opens` and `peak_concurrent`; ssh-internal
    /// listeners and master channels stay in the ringbuffer for
    /// diagnostics only.
    pub fn record_event(&mut self, mut event: ChannelEvent) {
        self.rotate_if_due(event.at);
        match event.kind {
            ChannelEventKind::Open => {
                let kind = event.channel_kind.unwrap_or(ChannelKind::Other);
                if kind.is_user_visible() {
                    self.total_opens = self.total_opens.saturating_add(1);
                    self.active_channels = self.active_channels.saturating_add(1);
                    self.peak_concurrent = self.peak_concurrent.max(self.active_channels);
                }
                self.channel_open.insert(event.channel_id, (event.at, kind));
            }
            ChannelEventKind::Close => {
                if let Some((opened_at, kind)) = self.channel_open.remove(&event.channel_id) {
                    event.opened_at = Some(opened_at);
                    event.channel_kind = Some(kind);
                    if kind.is_user_visible() {
                        self.active_channels = self.active_channels.saturating_sub(1);
                    }
                }
            }
        }
        self.last_event_at = Some(event.at);
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
        log::debug!(
            "[purple] Tunnel live event: total_opens={} active={} peak={}",
            self.total_opens,
            self.active_channels,
            self.peak_concurrent
        );
    }

    /// Advance the rolling history if at least one bucket-width has
    /// elapsed since the previous rotate. Per-elapsed-bucket we shift
    /// left by one and push a fresh `0` at the right (now) edge.
    /// Rotates `opens_history`, `rx_history` and `tx_history` together
    /// so the three sparklines stay aligned in the UI.
    pub fn rotate_if_due(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.history_last_rotate);
        let ticks = elapsed.as_secs() / BUCKET_SECS;
        if ticks == 0 {
            return;
        }
        let shift = (ticks as usize).min(HISTORY_BUCKETS);
        if shift >= HISTORY_BUCKETS {
            self.opens_history.fill(0);
            self.rx_history.fill(0);
            self.tx_history.fill(0);
        } else {
            self.opens_history.rotate_left(shift);
            for slot in self.opens_history.iter_mut().rev().take(shift) {
                *slot = 0;
            }
            self.rx_history.rotate_left(shift);
            for slot in self.rx_history.iter_mut().rev().take(shift) {
                *slot = 0;
            }
            self.tx_history.rotate_left(shift);
            for slot in self.tx_history.iter_mut().rev().take(shift) {
                *slot = 0;
            }
        }
        self.history_last_rotate += Duration::from_secs(ticks * BUCKET_SECS);
    }

    /// Write the peak concurrent client count for the current bucket.
    /// Called once per `TunnelState::poll` after the lsof snapshot has
    /// been drained, with `concurrent` = `max(lsof_clients_for_alias,
    /// active_channels)`. Uses `max` rather than `+=` so repeated polls
    /// inside one bucket converge on the bucket's peak rather than
    /// double-counting.
    pub fn sample_activity(&mut self, concurrent: u32) {
        let sample = u8::try_from(concurrent).unwrap_or(u8::MAX);
        if let Some(last) = self.opens_history.last_mut() {
            *last = (*last).max(sample);
        }
    }
}

/// Message sent from a per-tunnel parser thread to the main loop drain.
#[derive(Debug, Clone)]
pub struct ParserMessage {
    pub alias: String,
    pub event: ChannelEvent,
}

/// Snapshot emitted by the lsof poller every poll cycle. Keys are the
/// tunnel bind ports.
#[derive(Debug, Clone)]
pub struct LsofMessage {
    pub at: Instant,
    pub clients: HashMap<u16, Vec<ClientPeer>>,
    pub conflicts: HashMap<u16, PortConflict>,
}

impl LsofMessage {
    pub fn empty(at: Instant) -> Self {
        Self {
            at,
            clients: HashMap::new(),
            conflicts: HashMap::new(),
        }
    }
}

/// Public wrapper around an in-flight lsof poller thread.
pub struct LsofPollerHandle {
    pub stop: Arc<AtomicBool>,
    pub bind_ports: Arc<Mutex<Vec<(String, u16, u32)>>>,
    pub thread: Option<JoinHandle<()>>,
}

impl LsofPollerHandle {
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// What the parser thread watches for while ssh authenticates.
#[derive(Debug, Clone, Default)]
pub struct AuthWatch {
    /// Host ssh names on the target's `Authenticated to` line. `None` takes
    /// any such line, which only fits a tunnel without a jump host: there a
    /// bastion authenticates first.
    pub target_host: Option<String>,
    /// PINs the run carries, so a refusal of one can be recorded.
    pub security_key: crate::security_key::SecurityKeyRun,
    pub paths: Option<crate::runtime::env::Paths>,
}

impl AuthWatch {
    /// True when `line` says the target let the tunnel in.
    pub fn is_target_authenticated(&self, line: &str) -> bool {
        let Some(host) = parse_authenticated_host(line) else {
            return false;
        };
        self.target_host
            .as_deref()
            .is_none_or(|target| target.eq_ignore_ascii_case(host))
    }
}

/// The host on an `Authenticated to <host> (...) using "<method>".` line,
/// which ssh prints with `-v` once a host accepted it.
pub fn parse_authenticated_host(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("Authenticated to ")?;
    let (host, _) = rest.split_once(" (")?;
    (!host.is_empty()).then_some(host)
}

/// Shared flags the parser thread raises.
#[derive(Clone)]
pub struct ParserFlags {
    pub stop: Arc<AtomicBool>,
    pub authenticated: Arc<AtomicBool>,
    pub auth_refused: Arc<AtomicBool>,
    pub drained: Arc<AtomicBool>,
}

/// Spawn the per-tunnel stderr parser thread. Reads the child's stderr
/// line-by-line, captures channel events into `tx` and stores the last
/// few raw lines in `stderr_buffer` for exit-reason display. It also
/// raises `authenticated` once the target let ssh in and records a
/// security key refusing a PIN the run carried.
pub fn spawn_parser_thread(
    stderr: ChildStderr,
    alias: String,
    tx: Sender<ParserMessage>,
    stderr_buffer: Arc<Mutex<VecDeque<String>>>,
    flags: ParserFlags,
    watch: AuthWatch,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name(format!("purple-tunnel-parser-{alias}"))
        .spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if flags.stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(line) = line else { break };
                if let Ok(mut buf) = stderr_buffer.lock() {
                    if buf.len() == STDERR_BUFFER_LINES {
                        buf.pop_front();
                    }
                    buf.push_back(line.clone());
                }
                if watch.is_target_authenticated(&line) {
                    flags.authenticated.store(true, Ordering::Relaxed);
                    log::debug!("[purple] Tunnel authenticated: alias={alias}");
                }
                if crate::connection::auth_refused(&line) {
                    flags.auth_refused.store(true, Ordering::Relaxed);
                }
                if let Some(failure) = crate::security_key::parse_sign_failure(&line)
                    && watch.security_key.carries(&failure.key_path)
                {
                    crate::security_key::note_refusal(watch.paths.as_ref(), &failure);
                }
                if let Some(event) = parse_channel_line(&line) {
                    let msg = ParserMessage {
                        alias: alias.clone(),
                        event,
                    };
                    if tx.send(msg).is_err() {
                        break;
                    }
                }
            }
            flags.drained.store(true, Ordering::Relaxed);
            log::debug!("[purple] Tunnel parser thread exit: alias={alias}");
        })
        .expect("spawn purple-tunnel-parser thread")
}

/// Parse one stderr line. Recognises both OpenSSH log formats:
///
/// Modern (8.x+, 9.x, 10.x): `channel N: new <ctype> [<rname>] ...`.
/// ctype sits BEFORE the brackets and the brackets carry the remote
/// name; the `(inactive timeout: T)` suffix added in 9.x is ignored.
///
/// Legacy/test: `channel N: new [<ctype>]`. Kept for backwards
/// compatibility with test fixtures and any patched ssh build that
/// emits the older shape.
///
/// Both formats also produce `channel N: free: ...` for Close.
/// All other lines return `None`.
pub fn parse_channel_line(line: &str) -> Option<ChannelEvent> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("debug1: channel ")?;
    let (id_str, after) = rest.split_once(':')?;
    let channel_id: u32 = id_str.trim().parse().ok()?;
    let after = after.trim_start();
    let (kind, channel_kind) = if let Some(after_new) = after.strip_prefix("new") {
        let after_new = after_new.trim_start();
        let ctype = if let Some(rest) = after_new.strip_prefix('[') {
            // Legacy `new [<ctype>]` — ctype is inside the brackets.
            rest.split_once(']').map(|(t, _)| t.trim().to_string())
        } else {
            // Modern `new <ctype> [<rname>] (…)` — first whitespace-
            // delimited token is the ctype; everything after is the
            // remote name plus optional parenthesised attributes.
            after_new
                .split_whitespace()
                .next()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
        };
        let chan_kind = ctype.as_deref().map(ChannelKind::from_bracket);
        (ChannelEventKind::Open, chan_kind)
    } else if after.starts_with("free") {
        (ChannelEventKind::Close, None)
    } else {
        return None;
    };
    Some(ChannelEvent {
        at: Instant::now(),
        channel_id,
        kind,
        channel_kind,
        opened_at: None,
    })
}

/// Spawn the shared lsof poller. Polls `lsof -iTCP -P -n` every 2s,
/// filters by the bind ports passed via `bind_ports`, and emits a
/// `LsofMessage` per poll. macOS + Linux only — purple is unix-only.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn spawn_lsof_poller(
    bind_ports: Arc<Mutex<Vec<(String, u16, u32)>>>,
    tx: Sender<LsofMessage>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("purple-tunnel-lsof".into())
        .spawn(move || {
            // Track first-seen times per (port, src) so the CLIENTS card
            // can render a real "age" value across polls.
            let mut first_seen: HashMap<(u16, String), Instant> = HashMap::new();
            // Cache responsible-pid lookups across polls so the same client
            // does not pay the FFI cost every 2s tick.
            let mut responsible_cache = ResponsibleAppCache::default();
            // Per-peer throughput history. Key matches `first_seen` so a
            // peer that goes away and comes back resets cleanly.
            let mut peer_state: HashMap<(u16, String), PeerSampleCache> = HashMap::new();
            while !stop.load(Ordering::Relaxed) {
                let ports: Vec<(String, u16, u32)> = match bind_ports.lock() {
                    Ok(g) => g.clone(),
                    Err(p) => p.into_inner().clone(),
                };
                if ports.is_empty() {
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
                let now = Instant::now();
                let mut msg = run_lsof_once(&ports, &mut first_seen, now);
                annotate_responsible_apps(&mut msg, &mut responsible_cache);
                annotate_peer_throughput(&mut msg, &mut peer_state, now);
                if tx.send(msg).is_err() {
                    break;
                }
                for _ in 0..20 {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
            log::debug!("[purple] Tunnel lsof poller thread exit");
        })
        .expect("spawn purple-tunnel-lsof thread")
}

/// Stub lsof poller for non-unix builds. Purple does not officially
/// support Windows but the cfg keeps the codebase compilable.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn spawn_lsof_poller(
    _bind_ports: Arc<Mutex<Vec<(String, u16, u32)>>>,
    _tx: Sender<LsofMessage>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(500));
        }
    })
}

// macOS-only FFI to the kernel-tracked "responsible pid" for a process.
// `responsibility_get_pid_responsible_for_pid` is exported by libsystem
// (libquarantine) and used by Activity Monitor and the TCC subsystem.
// Stable across macOS releases and reachable without root or special
// entitlements for processes owned by the calling user. Returns the
// pid that "speaks for" `pid` when the kernel attributes resource use,
// sandbox decisions or TCC prompts. That mapping is what lets us label
// a `WebKit.Networking` daemon as `Safari`, or a shell/`psql` started
// inside Ghostty as `Ghostty`. Returns `0` or a negative value when no
// responsibility chain exists (most system daemons), and `pid` itself
// when the process is its own responsible (terminal apps, foreground
// GUI apps).
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn responsibility_get_pid_responsible_for_pid(pid: libc::pid_t) -> libc::pid_t;
}

/// Resolve a client pid to a user-facing app name via the macOS
/// responsibility API. Returns `None` when:
///   * the lookup failed (eg. process exited or kernel returned <= 0),
///   * the responsible pid equals the client pid (self-responsible —
///     the process name from lsof is already the right label), or
///   * the responsible pid maps to the same name as the client process
///     (no extra signal to add).
#[cfg(target_os = "macos")]
fn lookup_responsible_app(pid: u32, client_process: &str) -> Option<String> {
    // SAFETY: the FFI takes a pid_t by value and returns one. No memory
    // is exchanged. Calling with an exited pid is well-defined: the
    // kernel returns -1 / 0 which we treat as "no responsible".
    let rpid = unsafe { responsibility_get_pid_responsible_for_pid(pid as libc::pid_t) };
    if rpid <= 0 {
        return None;
    }
    if rpid as u32 == pid {
        return None;
    }
    let name = process_name(rpid as u32)?;
    if name.eq_ignore_ascii_case(client_process) {
        return None;
    }
    Some(name)
}

/// Look up the bare process name for a pid via libproc's `proc_name`,
/// which gives the truncated `comm` value (matches what `ps -o comm=`
/// returns without the full executable path). Used to label the
/// responsible-pid in the CLIENTS card.
#[cfg(target_os = "macos")]
fn process_name(pid: u32) -> Option<String> {
    unsafe extern "C" {
        fn proc_name(pid: libc::c_int, buffer: *mut libc::c_void, buffersize: u32) -> libc::c_int;
    }
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is a valid writeable region of `buf.len()` bytes.
    // `proc_name` writes a NUL-terminated string into it and returns
    // the byte count. Negative or zero means failure / unknown pid.
    let n = unsafe {
        proc_name(
            pid as libc::c_int,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
        )
    };
    if n <= 0 {
        return None;
    }
    let bytes = &buf[..(n as usize).min(buf.len())];
    let s = std::str::from_utf8(bytes).ok()?.trim_end_matches('\0');
    if s.is_empty() {
        None
    } else {
        Some(beautify_process(s))
    }
}

/// Linux: walk to the process's session leader via `/proc/PID/stat`
/// and use its comm as the user-facing app label. The session leader
/// is the process whose pid equals the session id; for shell-spawned
/// tools that leader is typically the terminal (`ghostty`, `konsole`,
/// `gnome-terminal-`); for GUI clients it is the app itself
/// (`dbeaver`, `firefox`, `code`). Generic — any process ancestry
/// rooted in a user session works without a hardcoded app list.
///
/// Returns `None` when the process exited mid-poll, when the session
/// leader equals the client pid (already its own app), or when the
/// leader's comm matches the client process name (no extra signal).
#[cfg(target_os = "linux")]
fn lookup_responsible_app(pid: u32, client_process: &str) -> Option<String> {
    let session_id = read_session_leader(pid)?;
    if session_id == pid {
        return None;
    }
    let comm = std::fs::read_to_string(format!("/proc/{}/comm", session_id)).ok()?;
    let name = comm.trim();
    if name.is_empty() || name.eq_ignore_ascii_case(client_process) {
        return None;
    }
    Some(beautify_process(name))
}

/// Parse `/proc/PID/stat` and return field 5 (the session id). The
/// `comm` field is parenthesized and may contain spaces, so we slice
/// at the LAST `)` and tokenize what follows: `state ppid pgid sid ...`
/// → field index 3 of the post-comm slice is the session id.
#[cfg(target_os = "linux")]
fn read_session_leader(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    let close = stat.rfind(')')?;
    let after = stat[close + 1..].trim();
    let fields: Vec<&str> = after.split_whitespace().collect();
    fields.get(3).and_then(|s| s.parse().ok())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn lookup_responsible_app(_pid: u32, _client_process: &str) -> Option<String> {
    None
}

/// Cache of `pid -> Option<responsible_app>` so the lsof poller does not
/// re-resolve the same connection on every 2s tick. Keyed on pid; a
/// stale entry is at worst a stale label until the pid is reused, which
/// is acceptable for an observability surface. Size-bounded by the
/// per-port client cap times a generous fan-out — in practice the lsof
/// pid set turns over slowly.
#[derive(Default)]
struct ResponsibleAppCache {
    map: HashMap<u32, Option<String>>,
}

impl ResponsibleAppCache {
    fn resolve(&mut self, pid: u32, client_process: &str) -> Option<String> {
        if let Some(cached) = self.map.get(&pid) {
            return cached.clone();
        }
        let resolved = lookup_responsible_app(pid, client_process);
        self.map.insert(pid, resolved.clone());
        resolved
    }

    /// Drop entries for pids no longer present in `live`, so the cache
    /// cannot grow unbounded across many short-lived clients.
    fn retain_pids(&mut self, live: &std::collections::HashSet<u32>) {
        self.map.retain(|pid, _| live.contains(pid));
    }
}

/// Walk every client peer in `msg` and fill in `responsible_app` from
/// the macOS responsibility API. Cache hits are O(1); cache misses pay
/// one FFI plus one libproc call per pid. After resolution the cache is
/// trimmed to the still-live set of pids so it cannot grow unbounded.
fn annotate_responsible_apps(msg: &mut LsofMessage, cache: &mut ResponsibleAppCache) {
    let mut live: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for peers in msg.clients.values_mut() {
        for peer in peers.iter_mut() {
            live.insert(peer.pid);
            peer.responsible_app = cache.resolve(peer.pid, &peer.process);
        }
    }
    cache.retain_pids(&live);
}

/// Per-peer sampling state retained across lsof polls. Holds the last
/// observed cumulative byte counters so the next poll can derive a
/// bytes-per-second rate. The braille sparkline history is owned by
/// `TunnelState::peer_viz` on the main thread (ticked at 100ms).
#[derive(Debug, Clone, Default)]
struct PeerSampleCache {
    last_rcvd: u64,
    last_sent: u64,
    last_at: Option<Instant>,
}

/// Per-peer sample shape. Linux returns cumulative byte counters that
/// the diff path turns into a rate. macOS returns per-pid bps deltas
/// directly because the underlying nettop call already emits a 1s
/// delta sample. The annotator picks the right path per peer.
#[derive(Debug, Default)]
struct PerPeerSamples {
    /// Cumulative byte counters per client local port from
    /// `NETLINK_SOCK_DIAG` on Linux (see `crate::tcp_diag`). Netlink
    /// returns per-socket data without pid attribution, so we key on
    /// the local port alone and rely on the lsof-driven join in
    /// `annotate_peer_throughput` to attribute counters back to the
    /// owning client process.
    per_local_port_cumulative: HashMap<u16, (u64, u64)>,
    /// Bytes-per-second deltas per pid from `nettop` on macOS. The
    /// kernel does not expose per-socket counters here, so per-pid is
    /// the finest-grained truth available — adequate for the typical
    /// "one app, one tunnel client" case.
    per_pid_bps: HashMap<u32, (u64, u64)>,
}

/// Walk every client peer in `msg`, run a per-platform sampler, and
/// fill in `current_rx_bps`, `current_tx_bps`, and the cumulative
/// `bytes_rcvd`/`bytes_sent` where available. Stale cache entries are
/// pruned to the still-live peer set so the map cannot grow unbounded.
/// The braille sparkline history is owned by `TunnelState::peer_viz`
/// on the main thread; this function only refreshes the bps readout
/// that the main thread feeds into the rolling 100ms-tick history.
fn annotate_peer_throughput(
    msg: &mut LsofMessage,
    cache: &mut HashMap<(u16, String), PeerSampleCache>,
    now: Instant,
) {
    let samples = sample_peer_throughput();
    let mut live: std::collections::HashSet<(u16, String)> = std::collections::HashSet::new();

    for (port, peers) in msg.clients.iter_mut() {
        for peer in peers.iter_mut() {
            let key = (*port, peer.src.clone());
            live.insert(key.clone());
            let entry = cache.entry(key).or_default();
            let src_port = src_port_from(&peer.src);

            // Path A: Linux per-socket cumulative counters → diff against cache.
            let cumulative =
                src_port.and_then(|p| samples.per_local_port_cumulative.get(&p).copied());
            if let Some((rcvd, sent)) = cumulative {
                if let Some(prev_at) = entry.last_at {
                    let dt = now.saturating_duration_since(prev_at).as_secs_f64();
                    if dt > 0.0 {
                        let rx_bps = ((rcvd.saturating_sub(entry.last_rcvd)) as f64 / dt) as u64;
                        let tx_bps = ((sent.saturating_sub(entry.last_sent)) as f64 / dt) as u64;
                        peer.current_rx_bps = rx_bps;
                        peer.current_tx_bps = tx_bps;
                    }
                }
                entry.last_rcvd = rcvd;
                entry.last_sent = sent;
                entry.last_at = Some(now);
                peer.bytes_rcvd = Some(rcvd);
                peer.bytes_sent = Some(sent);
                peer.last_sample_at = Some(now);
                continue;
            }

            // Path B: macOS per-pid bps directly — no diff math needed.
            if let Some((rx_bps, tx_bps)) = samples.per_pid_bps.get(&peer.pid).copied() {
                peer.current_rx_bps = rx_bps;
                peer.current_tx_bps = tx_bps;
                entry.last_at = Some(now);
                peer.last_sample_at = Some(now);
            }
        }
    }

    cache.retain(|key, _| live.contains(key));
}

/// Run the per-platform peer-throughput sampler. On Linux this talks
/// directly to the kernel via `NETLINK_SOCK_DIAG` (see `crate::tcp_diag`)
/// and returns per-socket cumulative byte counters. On macOS it spawns
/// a short `nettop` subprocess that yields one 1-second delta sample
/// per pid. Returns an empty struct on unsupported platforms or when
/// the underlying mechanism fails — the caller falls back to
/// status-only display.
fn sample_peer_throughput() -> PerPeerSamples {
    let mut out = PerPeerSamples::default();
    #[cfg(target_os = "linux")]
    {
        out.per_local_port_cumulative = crate::tcp_diag::sample_per_local_port();
    }
    #[cfg(target_os = "macos")]
    {
        out.per_pid_bps = sample_nettop_per_pid_macos();
    }
    out
}

/// Spawn `nettop -P -L 2 -d -x -s 1` to capture one 1-second delta
/// sample per process and parse it into a `pid -> (rx_bps, tx_bps)`
/// map. Blocking call: returns after ~1.2 seconds. Falls back to an
/// empty map if the subprocess fails to spawn or parse.
///
/// `-d` enables delta mode (each emitted sample is the diff over the
/// previous interval). `-L 2` exits after two samples. The first
/// sample is cumulative-since-process-start (we drop it); the second
/// is the 1-second delta we want.
#[cfg(target_os = "macos")]
fn sample_nettop_per_pid_macos() -> HashMap<u32, (u64, u64)> {
    use std::time::Duration;
    let output = Command::new("/usr/bin/nettop")
        .args(["-P", "-d", "-x", "-s", "1", "-L", "2"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let text = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => return HashMap::new(),
    };
    // Discard the warmup-sample block (everything before the second
    // header) and parse the delta block. `nettop` re-emits the
    // `time,proc.pid,...` header before each new sample, so the second
    // header line marks the start of the delta block.
    let mut blocks: Vec<&str> = Vec::new();
    let mut start = 0;
    for (i, line) in text.lines().enumerate() {
        if line.starts_with("time,") {
            blocks.push(&text[start..]);
            start = text.lines().take(i).map(|l| l.len() + 1).sum();
        }
    }
    let _ = Duration::ZERO; // silence unused import on platforms with no Duration use
    let delta_block = if text.matches("\ntime,").count() >= 1 {
        // Find the second header line and slice from there.
        let first = text.find("time,").unwrap_or(0);
        let second_rel = text[first + 5..].find("\ntime,");
        match second_rel {
            Some(off) => &text[first + 5 + off + 1..],
            None => &text[first..],
        }
    } else {
        text.as_str()
    };
    let mut out: HashMap<u32, (u64, u64)> = HashMap::new();
    for line in delta_block.lines() {
        if let Some((pid, rx, tx)) = parse_nettop_csv_row_per_pid(line) {
            // nettop -d sample interval is 1s (`-s 1`), so the bytes
            // in this row are bytes-per-second already.
            let entry = out.entry(pid).or_insert((0, 0));
            entry.0 = entry.0.saturating_add(rx);
            entry.1 = entry.1.saturating_add(tx);
        }
    }
    out
}

/// Parse one `nettop -P -d -x` CSV row into `(pid, rx_bytes, tx_bytes)`.
/// Returns `None` for header rows, blanks, or rows with too few
/// columns. Mirrors `parse_nettop_csv_row` but does not filter on a
/// specific pid. macOS-only because the only caller is the nettop
/// sampler.
#[cfg(target_os = "macos")]
fn parse_nettop_csv_row_per_pid(line: &str) -> Option<(u32, u64, u64)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("time,") {
        return None;
    }
    let cols: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if cols.len() < 6 {
        return None;
    }
    let proc_pid = cols[1];
    let dot = proc_pid.rfind('.')?;
    let pid_str = &proc_pid[dot + 1..];
    let pid: u32 = pid_str.parse().ok()?;
    let rx: u64 = cols[4].parse().ok()?;
    let tx: u64 = cols[5].parse().ok()?;
    Some((pid, rx, tx))
}

/// Extract the port portion of an `addr:port` socket string.
fn src_port_from(src: &str) -> Option<u16> {
    src.rsplit_once(':').and_then(|(_, port)| port.parse().ok())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn run_lsof_once(
    ports: &[(String, u16, u32)],
    first_seen: &mut HashMap<(u16, String), Instant>,
    now: Instant,
) -> LsofMessage {
    // `+c 0` widens the COMMAND column to the full process name.
    // macOS defaults to 9 characters which collapses every Apple
    // framework process down to `com.apple`; Linux defaults to 15.
    // Neither is useful in the CLIENTS card.
    let output = Command::new("lsof")
        .args(["-iTCP", "-P", "-n", "-w", "+c", "0"])
        .output();
    let stdout = match output {
        Ok(o) if o.status.success() || !o.stdout.is_empty() => o.stdout,
        _ => return LsofMessage::empty(now),
    };
    let text = String::from_utf8_lossy(&stdout);
    parse_lsof_output(&text, ports, first_seen, now)
}

/// Parse `lsof -iTCP -P -n` output into a `LsofMessage`. Public for tests.
pub fn parse_lsof_output(
    text: &str,
    ports: &[(String, u16, u32)],
    first_seen: &mut HashMap<(u16, String), Instant>,
    now: Instant,
) -> LsofMessage {
    let mut clients: HashMap<u16, Vec<ClientPeer>> = HashMap::new();
    let mut conflicts: HashMap<u16, PortConflict> = HashMap::new();
    let bind_ports: Vec<u16> = ports.iter().map(|(_, p, _)| *p).collect();
    let tunnel_pids: Vec<u32> = ports.iter().map(|(_, _, pid)| *pid).collect();

    for line in text.lines().skip(1) {
        let row = match parse_lsof_row(line) {
            Some(r) => r,
            None => continue,
        };
        // LISTEN rows owned by another process on a tunnel bind port → conflict.
        if row.is_listen && bind_ports.contains(&row.local_port) && !tunnel_pids.contains(&row.pid)
        {
            conflicts
                .entry(row.local_port)
                .or_insert_with(|| PortConflict {
                    port: row.local_port,
                    process: row.command.clone(),
                    pid: row.pid,
                });
            continue;
        }
        if row.is_listen {
            continue;
        }
        // ESTABLISHED rows where the client process owns a socket whose
        // remote port is one of our bind ports. The client process != ssh.
        if let Some(remote_port) = row.remote_port
            && bind_ports.contains(&remote_port)
            && !tunnel_pids.contains(&row.pid)
        {
            let src = row.local_addr_port().unwrap_or_else(|| "?".to_string());
            let key = (remote_port, src.clone());
            let since = *first_seen.entry(key).or_insert(now);
            let entry = clients.entry(remote_port).or_default();
            if entry.len() >= MAX_CLIENTS_PER_PORT {
                continue;
            }
            entry.push(ClientPeer {
                src,
                process: beautify_process(&row.command),
                pid: row.pid,
                since,
                responsible_app: None,
                current_rx_bps: 0,
                current_tx_bps: 0,
                bytes_rcvd: None,
                bytes_sent: None,
                last_sample_at: None,
            });
        }
    }
    // Drop first-seen entries for sockets we no longer see, so their age
    // counters do not grow unbounded across reconnects.
    let live: std::collections::HashSet<(u16, String)> = clients
        .iter()
        .flat_map(|(port, peers)| peers.iter().map(move |p| (*port, p.src.clone())))
        .collect();
    first_seen.retain(|key, _| live.contains(key));

    LsofMessage {
        at: now,
        clients,
        conflicts,
    }
}

/// One row of `lsof -iTCP -P -n` output, fields we care about.
#[derive(Debug)]
struct LsofRow {
    command: String,
    pid: u32,
    is_listen: bool,
    local_addr: String,
    local_port: u16,
    remote_addr: Option<String>,
    remote_port: Option<u16>,
}

impl LsofRow {
    fn local_addr_port(&self) -> Option<String> {
        if let (Some(addr), Some(port)) = (self.remote_addr.as_deref(), self.remote_port) {
            // The CLIENTS card shows the *peer* of our tunnel bind port,
            // i.e. the side connecting in. From the client process row,
            // remote = our loopback bind. So the peer to show is the row's
            // local end.
            let _ = (addr, port);
        }
        Some(format!("{}:{}", self.local_addr, self.local_port))
    }
}

fn parse_lsof_row(line: &str) -> Option<LsofRow> {
    if line.trim().is_empty() {
        return None;
    }
    // lsof columns are whitespace-separated. Format:
    //   COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
    // NAME may contain spaces only inside parens (state). The address
    // tokens never contain spaces because we pass `-n` and `-P`.
    let mut fields = line.split_whitespace();
    let command = fields.next()?.to_string();
    let pid: u32 = fields.next()?.parse().ok()?;
    let _user = fields.next()?;
    let _fd = fields.next()?;
    let _ty = fields.next()?;
    let _dev = fields.next()?;
    let _size = fields.next()?;
    let _node = fields.next()?;
    let name = fields.next()?;
    let state = fields.next();
    if !name.contains(':') {
        return None;
    }
    let is_listen = matches!(state, Some(s) if s.contains("LISTEN"));
    let is_established = matches!(state, Some(s) if s.contains("ESTABLISHED"));
    if !is_listen && !is_established {
        return None;
    }
    let (local, remote) = match name.split_once("->") {
        Some((l, r)) => (l, Some(r)),
        None => (name, None),
    };
    let (local_addr, local_port) = split_addr_port(local)?;
    let (remote_addr, remote_port) = match remote {
        Some(r) => match split_addr_port(r) {
            Some((a, p)) => (Some(a), Some(p)),
            None => (None, None),
        },
        None => (None, None),
    };
    Some(LsofRow {
        command,
        pid,
        is_listen,
        local_addr,
        local_port,
        remote_addr,
        remote_port,
    })
}

/// Trim noise that lsof reports for macOS Apple-framework processes
/// so the CLIENTS card shows useful identifiers. Examples:
///   `com.apple.WebKit.Networking` → `WebKit.Networking`
///   `com.apple.Safari`            → `Safari`
///   `curl`                        → `curl`
/// Other names pass through unchanged.
pub fn beautify_process(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("com.apple.")
        && !rest.is_empty()
    {
        return rest.to_string();
    }
    raw.to_string()
}

/// Split a `host:port` token. Handles bracketed IPv6: `[::1]:8080`.
fn split_addr_port(s: &str) -> Option<(String, u16)> {
    if let Some(rest) = s.strip_prefix('[') {
        let end = rest.find(']')?;
        let addr = &rest[..end];
        let after = &rest[end + 1..];
        let port_str = after.strip_prefix(':')?;
        let port: u16 = port_str.parse().ok()?;
        return Some((addr.to_string(), port));
    }
    let colon = s.rfind(':')?;
    let addr = &s[..colon];
    let port: u16 = s[colon + 1..].parse().ok()?;
    Some((addr.to_string(), port))
}

// ---------------------------------------------------------------------------
// Snapshot path for demo and visual regression tests.
// ---------------------------------------------------------------------------

/// Deterministic snapshot of a tunnel's live state. Used in `--demo` and
/// in visual regression tests so goldens do not depend on wall-clock or
/// background workers.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TunnelLiveSnapshot {
    pub uptime_secs: u64,
    pub active_channels: u32,
    pub peak_concurrent: u32,
    pub total_opens: u32,
    pub idle_secs: u64,
    pub rx_history: [u64; HISTORY_BUCKETS],
    pub tx_history: [u64; HISTORY_BUCKETS],
    pub current_rx_bps: u64,
    pub current_tx_bps: u64,
    pub peak_rx_bps: u64,
    pub peak_tx_bps: u64,
    /// True when the throughput aggregator has produced at least one
    /// sample. UI shows `sampling…` until then.
    pub throughput_ready: bool,
    pub clients: Vec<DisplayClient>,
    pub events: Vec<DisplayEvent>,
    /// Currently-open channels at snapshot time. Each entry is
    /// `(channel_id, open_age_secs, kind)`. Drives the channel
    /// lifeline swimlane in the detail panel.
    pub currently_open: Vec<(u32, u64, ChannelKind)>,
    pub conflict: Option<PortConflict>,
    pub last_exit: Option<(i32, String)>,
}

#[derive(Debug, Clone)]
pub struct DisplayClient {
    /// Source `addr:port` of the connected client. Reserved for the
    /// expanded clients-list view; the heartbeat-dial dashboard does
    /// not show it inline.
    #[allow(dead_code)]
    pub src: String,
    pub process: String,
    pub age_secs: u64,
    /// Connecting process PID. Reserved for the expanded clients-list
    /// view.
    #[allow(dead_code)]
    pub pid: u32,
    /// User-facing app that owns this socket on macOS. `None` if equal
    /// to `process` or unavailable. See `ClientPeer::responsible_app`.
    pub responsible_app: Option<String>,
    /// Per-client throughput readouts. Zero when no sample has been
    /// taken yet, or when the platform sampler does not produce
    /// per-client data.
    pub current_rx_bps: u64,
    pub current_tx_bps: u64,
    /// Rolling history of combined rx+tx bytes-per-second. Cell 0 is
    /// the oldest sample, the last cell is "now".
    pub viz_history: [u64; PEER_VIZ_BUCKETS],
    /// True after the per-client sampler has produced at least one
    /// sample. Lets the renderer distinguish "0 B/s, idle" from
    /// "no sampler available".
    pub throughput_ready: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DisplayEvent {
    pub age_secs: u64,
    /// Surfaced through the runtime path for completeness; the
    /// renderer currently does not show channel ids because the
    /// numbers are noisy without context. Reserved for future use.
    #[allow(dead_code)]
    pub channel_id: u32,
    pub kind: ChannelEventKind,
    /// Filtered to `Direct | Forwarded | Dynamic` — internal channels
    /// are dropped by the UI before this struct is built, so this is
    /// always one of the user-visible kinds.
    pub channel_kind: ChannelKind,
    /// Open→close duration when known. Reserved for the expanded
    /// events-list view.
    #[allow(dead_code)]
    pub duration_secs: Option<u64>,
    /// Number of co-occurring events of the same kind/age that were
    /// folded into this row. `1` means "just one event"; higher values
    /// render as a `(3x)` suffix.
    pub count: u32,
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn user_open(channel_id: u32, at: Instant) -> ChannelEvent {
        ChannelEvent {
            at,
            channel_id,
            kind: ChannelEventKind::Open,
            channel_kind: Some(ChannelKind::Direct),
            opened_at: None,
        }
    }

    #[test]
    fn parse_channel_open_simple() {
        let ev = parse_channel_line("debug1: channel 0: new [direct-tcpip]").unwrap();
        assert_eq!(ev.channel_id, 0);
        assert_eq!(ev.kind, ChannelEventKind::Open);
        assert_eq!(ev.channel_kind, Some(ChannelKind::Direct));
    }

    #[test]
    fn parse_channel_open_records_listener_kind() {
        let ev = parse_channel_line("debug1: channel 1: new [port listener]").unwrap();
        assert_eq!(ev.channel_kind, Some(ChannelKind::Other));
    }

    #[test]
    fn parse_channel_open_records_dynamic_kind() {
        let ev = parse_channel_line("debug1: channel 4: new [dynamic-tcpip]").unwrap();
        assert_eq!(ev.channel_kind, Some(ChannelKind::Dynamic));
    }

    #[test]
    fn parse_channel_close_simple() {
        let ev = parse_channel_line("debug1: channel 12: free: blah blah").unwrap();
        assert_eq!(ev.channel_id, 12);
        assert_eq!(ev.kind, ChannelEventKind::Close);
        // Close lines do not carry the kind; the recorder fills it in
        // from the open-channel map.
        assert_eq!(ev.channel_kind, None);
    }

    #[test]
    fn parse_channel_with_leading_whitespace() {
        let ev = parse_channel_line("   debug1: channel 5: new [forwarded-tcpip]").unwrap();
        assert_eq!(ev.channel_id, 5);
        assert_eq!(ev.kind, ChannelEventKind::Open);
        assert_eq!(ev.channel_kind, Some(ChannelKind::Forwarded));
    }

    #[test]
    fn parse_channel_modern_openssh_format_with_inactive_timeout() {
        // OpenSSH 10.x: `channel N: new <ctype> [<rname>] (inactive timeout: T)`.
        // The ctype sits BEFORE the brackets; the brackets hold the
        // remote endpoint name (which we ignore). Without this branch
        // the parser silently set channel_kind=None and counters never
        // advanced past zero on real-world OpenSSH ≥ 9.x.
        let ev = parse_channel_line(
            "debug1: channel 3: new direct-tcpip [127.0.0.1:54321] (inactive timeout: 0)",
        )
        .unwrap();
        assert_eq!(ev.channel_id, 3);
        assert_eq!(ev.kind, ChannelEventKind::Open);
        assert_eq!(ev.channel_kind, Some(ChannelKind::Direct));
    }

    #[test]
    fn parse_channel_modern_openssh_format_forwarded() {
        let ev = parse_channel_line(
            "debug1: channel 7: new forwarded-tcpip [10.0.0.1:443] (inactive timeout: 0)",
        )
        .unwrap();
        assert_eq!(ev.channel_kind, Some(ChannelKind::Forwarded));
    }

    #[test]
    fn parse_channel_modern_openssh_format_dynamic() {
        let ev = parse_channel_line("debug1: channel 9: new dynamic-tcpip [client] (timeout: 5)")
            .unwrap();
        assert_eq!(ev.channel_kind, Some(ChannelKind::Dynamic));
    }

    #[test]
    fn parse_channel_modern_openssh_format_internal_listener_is_other() {
        // The local-port listener that ssh creates at startup uses
        // ctype "port-listener" or "listener" depending on version.
        // Either way it must NOT be promoted to a user-visible kind.
        let ev = parse_channel_line(
            "debug1: channel 0: new port-listener [::1:8080] (inactive timeout: 0)",
        )
        .unwrap();
        assert_eq!(ev.channel_kind, Some(ChannelKind::Other));
    }

    #[test]
    fn parse_channel_unrelated_line_returns_none() {
        assert!(parse_channel_line("debug1: client_input_global_request").is_none());
        assert!(parse_channel_line("not even ssh output").is_none());
        assert!(parse_channel_line("debug1: channel abc: new").is_none());
        assert!(parse_channel_line("debug1: channel 1: confirm").is_none());
    }

    #[test]
    fn reads_the_host_from_an_authenticated_line() {
        assert_eq!(
            parse_authenticated_host(
                "Authenticated to db.example.com ([10.0.0.5]:22) using \"publickey\"."
            ),
            Some("db.example.com")
        );
        assert_eq!(
            parse_authenticated_host("Authenticated to db (via proxy) using \"publickey\"."),
            Some("db")
        );
        assert_eq!(
            parse_authenticated_host("debug1: Authentication succeeded"),
            None
        );
    }

    #[test]
    fn behind_a_jump_host_only_the_target_counts_as_in() {
        // ssh passes `-v` to the hop, which authenticates first on the
        // same pipe.
        let watch = AuthWatch {
            target_host: Some("db.example.com".into()),
            ..AuthWatch::default()
        };
        assert!(!watch.is_target_authenticated(
            "Authenticated to bastion ([10.0.0.1]:22) using \"publickey\"."
        ));
        assert!(watch.is_target_authenticated(
            "Authenticated to DB.example.com (via proxy) using \"publickey\"."
        ));
        let direct = AuthWatch::default();
        assert!(direct.is_target_authenticated(
            "Authenticated to anything ([1.2.3.4]:22) using \"publickey\"."
        ));
    }

    #[cfg(unix)]
    #[test]
    fn the_parser_flags_authentication_and_records_a_refused_pin() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = crate::runtime::env::Paths::new(dir.path());
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(
                "printf '%s\\n' \
                 'sign_and_send_pubkey: signing failed for ED25519-SK \"/k\": incorrect passphrase supplied to decrypt private key' \
                 'Authenticated to web ([1.2.3.4]:22) using \"password\".' \
                 'ops@bastion: Permission denied (publickey).' >&2",
            )
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn sh");
        let stderr = child.stderr.take().expect("stderr");
        let flags = ParserFlags {
            stop: Arc::new(AtomicBool::new(false)),
            authenticated: Arc::new(AtomicBool::new(false)),
            auth_refused: Arc::new(AtomicBool::new(false)),
            drained: Arc::new(AtomicBool::new(false)),
        };
        let watch = AuthWatch {
            target_host: None,
            security_key: crate::security_key::SecurityKeyRun {
                pins: vec![crate::security_key::SecurityKeyPin {
                    key_path: "/k".into(),
                    pin: "0000".into(),
                }],
                flow: None,
            },
            paths: Some(paths.clone()),
        };
        let (tx, _rx) = std::sync::mpsc::channel();
        let handle = spawn_parser_thread(
            stderr,
            "web".into(),
            tx,
            Arc::new(Mutex::new(VecDeque::new())),
            flags.clone(),
            watch,
        );
        handle.join().expect("parser thread");
        let _ = child.wait();
        assert!(flags.drained.load(Ordering::Relaxed));
        assert!(flags.authenticated.load(Ordering::Relaxed));
        assert!(flags.auth_refused.load(Ordering::Relaxed));
        assert_eq!(
            crate::security_key::refusal_for(Some(&paths), "/k"),
            Some(crate::security_key::RefusalReason::WrongPin)
        );
    }

    #[test]
    fn record_event_open_increments_counters_for_user_visible_kinds() {
        let now = Instant::now();
        let mut state = TunnelLiveState::new(now);
        state.record_event(user_open(1, now));
        assert_eq!(state.total_opens, 1);
        assert_eq!(state.active_channels, 1);
        assert_eq!(state.peak_concurrent, 1);
        // Activity history is fed by sample_activity, not record_event.
        assert_eq!(state.opens_history[HISTORY_BUCKETS - 1], 0);
    }

    #[test]
    fn record_event_skips_counters_for_internal_channels() {
        // Listener / session / mux-master channels should NOT inflate
        // peak_concurrent or total_opens — only user-visible traffic
        // counts towards the activity figures.
        let now = Instant::now();
        let mut state = TunnelLiveState::new(now);
        state.record_event(ChannelEvent {
            at: now,
            channel_id: 0,
            kind: ChannelEventKind::Open,
            channel_kind: Some(ChannelKind::Other),
            opened_at: None,
        });
        assert_eq!(state.total_opens, 0);
        assert_eq!(state.active_channels, 0);
        assert_eq!(state.opens_history[HISTORY_BUCKETS - 1], 0);
        // The event is still kept in the ringbuffer for diagnostics.
        assert_eq!(state.events.len(), 1);
    }

    #[test]
    fn sample_activity_writes_peak_into_current_bucket() {
        let now = Instant::now();
        let mut state = TunnelLiveState::new(now);
        state.sample_activity(2);
        assert_eq!(state.opens_history[HISTORY_BUCKETS - 1], 2);
        // A lower sample inside the same bucket must not erase the peak.
        state.sample_activity(1);
        assert_eq!(state.opens_history[HISTORY_BUCKETS - 1], 2);
        // A higher sample raises the peak.
        state.sample_activity(5);
        assert_eq!(state.opens_history[HISTORY_BUCKETS - 1], 5);
    }

    #[test]
    fn sample_activity_clamps_to_u8_max() {
        let now = Instant::now();
        let mut state = TunnelLiveState::new(now);
        state.sample_activity(u32::MAX);
        assert_eq!(state.opens_history[HISTORY_BUCKETS - 1], u8::MAX);
    }

    #[test]
    fn record_event_close_pairs_with_open_for_duration() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(5);
        let mut state = TunnelLiveState::new(t0);
        state.record_event(user_open(7, t0));
        state.record_event(ChannelEvent {
            at: t1,
            channel_id: 7,
            kind: ChannelEventKind::Close,
            channel_kind: None,
            opened_at: None,
        });
        assert_eq!(state.active_channels, 0);
        let last = state.events.back().unwrap();
        assert_eq!(last.kind, ChannelEventKind::Close);
        assert_eq!(last.opened_at, Some(t0));
        // The close event picks up the kind that was recorded on open.
        assert_eq!(last.channel_kind, Some(ChannelKind::Direct));
    }

    #[test]
    fn record_event_caps_ringbuffer_at_max() {
        let now = Instant::now();
        let mut state = TunnelLiveState::new(now);
        for i in 0..(MAX_EVENTS as u32 + 5) {
            state.record_event(user_open(i, now));
        }
        assert_eq!(state.events.len(), MAX_EVENTS);
    }

    #[test]
    fn rotate_if_due_shifts_buckets_per_tick() {
        let t0 = Instant::now();
        let mut state = TunnelLiveState::new(t0);
        state.opens_history[HISTORY_BUCKETS - 1] = 7;
        state.rotate_if_due(t0 + Duration::from_secs(BUCKET_SECS));
        // After one rotate the value moved one slot left.
        assert_eq!(state.opens_history[HISTORY_BUCKETS - 2], 7);
        assert_eq!(state.opens_history[HISTORY_BUCKETS - 1], 0);
    }

    #[test]
    fn rotate_if_due_clamps_at_full_window() {
        let t0 = Instant::now();
        let mut state = TunnelLiveState::new(t0);
        state.opens_history[HISTORY_BUCKETS - 1] = 9;
        // Far beyond the window — rotation should clear it entirely.
        state.rotate_if_due(t0 + Duration::from_secs(BUCKET_SECS * HISTORY_BUCKETS as u64 * 4));
        assert!(state.opens_history.iter().all(|&v| v == 0));
    }

    #[test]
    fn rotate_if_due_noop_within_one_bucket() {
        let t0 = Instant::now();
        let mut state = TunnelLiveState::new(t0);
        state.opens_history[HISTORY_BUCKETS - 1] = 3;
        // Less than one BUCKET_SECS elapsed — no rotation should happen.
        state.rotate_if_due(t0 + Duration::from_millis(BUCKET_SECS * 1000 / 2));
        assert_eq!(state.opens_history[HISTORY_BUCKETS - 1], 3);
    }

    #[test]
    fn parse_lsof_listen_row() {
        let line = "ssh    12345 user 3u IPv4 0xabc 0t0 TCP 127.0.0.1:8080 (LISTEN)";
        let row = parse_lsof_row(line).unwrap();
        assert_eq!(row.command, "ssh");
        assert_eq!(row.pid, 12345);
        assert!(row.is_listen);
        assert_eq!(row.local_addr, "127.0.0.1");
        assert_eq!(row.local_port, 8080);
        assert!(row.remote_port.is_none());
    }

    #[test]
    fn parse_lsof_established_row() {
        let line =
            "curl   23456 user 4u IPv4 0xdef 0t0 TCP 127.0.0.1:54321->127.0.0.1:8080 (ESTABLISHED)";
        let row = parse_lsof_row(line).unwrap();
        assert_eq!(row.command, "curl");
        assert_eq!(row.pid, 23456);
        assert!(!row.is_listen);
        assert_eq!(row.local_port, 54321);
        assert_eq!(row.remote_port, Some(8080));
    }

    #[test]
    fn parse_lsof_other_states_skipped() {
        // CLOSE_WAIT, TIME_WAIT, etc. are not interesting for our display.
        let line = "x 1 u 0u IPv4 0 0t0 TCP 1.2.3.4:1->5.6.7.8:9 (CLOSE_WAIT)";
        assert!(parse_lsof_row(line).is_none());
    }

    #[test]
    fn parse_lsof_output_finds_clients_for_bind_port() {
        let txt = "\
COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
ssh    12345 u 3u IPv4 0xa 0t0 TCP 127.0.0.1:8080 (LISTEN)
curl   23456 u 4u IPv4 0xb 0t0 TCP 127.0.0.1:54321->127.0.0.1:8080 (ESTABLISHED)
ssh    12345 u 5u IPv4 0xc 0t0 TCP 127.0.0.1:8080->127.0.0.1:54321 (ESTABLISHED)
";
        let ports = vec![("foo".into(), 8080u16, 12345u32)];
        let mut seen = HashMap::new();
        let now = Instant::now();
        let msg = parse_lsof_output(txt, &ports, &mut seen, now);
        let peers = msg.clients.get(&8080).expect("clients on 8080");
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].process, "curl");
        assert_eq!(peers[0].pid, 23456);
        assert!(peers[0].src.contains("54321"));
        assert!(msg.conflicts.is_empty());
    }

    #[test]
    fn parse_lsof_output_detects_port_conflict() {
        let txt = "\
COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
nginx  99999 u 3u IPv4 0xa 0t0 TCP 127.0.0.1:8080 (LISTEN)
";
        let ports = vec![("foo".into(), 8080u16, 12345u32)];
        let mut seen = HashMap::new();
        let now = Instant::now();
        let msg = parse_lsof_output(txt, &ports, &mut seen, now);
        let conflict = msg.conflicts.get(&8080).expect("conflict on 8080");
        assert_eq!(conflict.process, "nginx");
        assert_eq!(conflict.pid, 99999);
    }

    #[test]
    fn parse_lsof_output_skips_own_listen() {
        let txt = "\
COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
ssh    12345 u 3u IPv4 0xa 0t0 TCP 127.0.0.1:8080 (LISTEN)
";
        let ports = vec![("foo".into(), 8080u16, 12345u32)];
        let mut seen = HashMap::new();
        let now = Instant::now();
        let msg = parse_lsof_output(txt, &ports, &mut seen, now);
        assert!(msg.conflicts.is_empty());
    }

    #[test]
    fn parse_lsof_output_first_seen_persists_across_polls() {
        let txt = "\
COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
ssh    12345 u 3u IPv4 0xa 0t0 TCP 127.0.0.1:8080 (LISTEN)
curl   23456 u 4u IPv4 0xb 0t0 TCP 127.0.0.1:54321->127.0.0.1:8080 (ESTABLISHED)
";
        let ports = vec![("foo".into(), 8080u16, 12345u32)];
        let mut seen = HashMap::new();
        let t0 = Instant::now();
        let msg1 = parse_lsof_output(txt, &ports, &mut seen, t0);
        let t1 = t0 + Duration::from_secs(5);
        let msg2 = parse_lsof_output(txt, &ports, &mut seen, t1);
        let p1 = &msg1.clients[&8080][0];
        let p2 = &msg2.clients[&8080][0];
        assert_eq!(p1.since, p2.since, "first_seen should be sticky");
    }

    #[test]
    fn split_addr_port_handles_ipv6_brackets() {
        let (a, p) = split_addr_port("[::1]:8080").unwrap();
        assert_eq!(a, "::1");
        assert_eq!(p, 8080);
    }

    #[test]
    fn split_addr_port_handles_ipv4() {
        let (a, p) = split_addr_port("127.0.0.1:8080").unwrap();
        assert_eq!(a, "127.0.0.1");
        assert_eq!(p, 8080);
    }

    #[test]
    fn beautify_process_strips_com_apple_prefix() {
        assert_eq!(
            beautify_process("com.apple.WebKit.Networking"),
            "WebKit.Networking"
        );
        assert_eq!(beautify_process("com.apple.Safari"), "Safari");
    }

    #[test]
    fn beautify_process_passes_other_names_through_unchanged() {
        assert_eq!(beautify_process("curl"), "curl");
        assert_eq!(beautify_process("nginx"), "nginx");
        assert_eq!(beautify_process("python3"), "python3");
    }

    #[test]
    fn beautify_process_does_not_strip_when_only_prefix() {
        // Edge case: the bare `com.apple.` string would otherwise
        // collapse to "" and disappear from the card. Keep the raw
        // value so the user at least sees that lsof reported something.
        assert_eq!(beautify_process("com.apple."), "com.apple.");
    }

    #[test]
    fn parse_lsof_output_unwraps_apple_framework_names() {
        let txt = "\
COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
ssh    12345 u 3u IPv4 0xa 0t0 TCP 127.0.0.1:8080 (LISTEN)
com.apple.WebKit.Networking 23456 u 4u IPv4 0xb 0t0 TCP 127.0.0.1:54321->127.0.0.1:8080 (ESTABLISHED)
";
        let ports = vec![("foo".into(), 8080u16, 12345u32)];
        let mut seen = HashMap::new();
        let now = Instant::now();
        let msg = parse_lsof_output(txt, &ports, &mut seen, now);
        let peers = msg.clients.get(&8080).expect("clients on 8080");
        assert_eq!(peers.len(), 1);
        // Process name is shown without the noisy `com.apple.` prefix.
        assert_eq!(peers[0].process, "WebKit.Networking");
    }
}
