use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::app::TunnelFormBaseline;
use crate::app::forms::TunnelForm;
use crate::tunnel::{ActiveTunnel, TunnelRule};

/// How long an exited tunnel waits for its parser to reach the end of
/// stderr. A `ProxyCommand` that outlives ssh can hold the pipe open, so the
/// wait is bounded.
const EXIT_DRAIN_GRACE: std::time::Duration = std::time::Duration::from_secs(1);

/// A tunnel whose ssh ended, as `TunnelState::poll` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelExit {
    pub alias: String,
    /// Toast text for the exit.
    pub message: String,
    pub is_error: bool,
    /// ssh gave up on authentication before the tunnel came up.
    pub auth_refused: bool,
    /// The target let ssh in before it exited.
    pub authenticated: bool,
}

/// Sort order for the tunnels overview screen. Cycled with `s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TunnelSortMode {
    /// Recently-used hosts first (uses `app.history.last_connected`). Active
    /// tunnels rank by `started_at` so they always sit above idle ones.
    #[default]
    MostRecent,
    /// Alphabetical by host alias, ascending.
    AlphaHostname,
}

impl TunnelSortMode {
    pub fn next(self) -> Self {
        match self {
            TunnelSortMode::MostRecent => TunnelSortMode::AlphaHostname,
            TunnelSortMode::AlphaHostname => TunnelSortMode::MostRecent,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TunnelSortMode::MostRecent => "most recent",
            TunnelSortMode::AlphaHostname => "A-Z hostname",
        }
    }
}
use crate::tunnel_live::{
    ChannelEventKind, ClientPeer, LsofMessage, LsofPollerHandle, PEER_VIZ_BUCKETS, ParserMessage,
    PortConflict, TunnelLiveSnapshot,
};

/// Tunnel-owned state grouped off the `App` god-struct. Contains the rule
/// list, the edit form, the live child-process map, form baseline for the
/// dirty check, the pending delete index and the cached per-host summary
/// strings. Pure state container; behaviour lives on `App` or on dedicated
/// methods here.
pub struct TunnelState {
    pub(in crate::app) list: Vec<TunnelRule>,
    pub(in crate::app) form: TunnelForm,
    pub(in crate::app) active: HashMap<String, ActiveTunnel>,
    pub(in crate::app) form_baseline: Option<TunnelFormBaseline>,
    pub(in crate::app) pending_delete: Option<usize>,
    pub(in crate::app) summaries_cache: HashMap<String, String>,
    /// Sort mode for the tunnels overview screen. Cycled by `s`.
    pub(in crate::app) sort_mode: TunnelSortMode,

    // Live-data layer. Receivers are drained from `poll()`. The Sender
    // halves are cloned into per-tunnel parser threads when start_tunnel
    // succeeds.
    pub(in crate::app) parser_tx: Sender<ParserMessage>,
    pub(in crate::app) parser_rx: Receiver<ParserMessage>,
    pub(in crate::app) lsof_tx: Sender<LsofMessage>,
    pub(in crate::app) lsof_rx: Receiver<LsofMessage>,

    // Last lsof snapshot, keyed by tunnel bind port. Replaced wholesale
    // on every successful poll so closed sockets disappear.
    pub(in crate::app) clients: HashMap<u16, Vec<ClientPeer>>,
    pub(in crate::app) conflicts: HashMap<u16, PortConflict>,
    pub(in crate::app) last_lsof_at: Option<Instant>,

    /// Per-peer rolling braille history, pushed once per lsof poll
    /// arrival from `poll()`. Keyed by `(bind_port, peer.src)` so it
    /// survives wholesale `clients` replacements as long as the peer is
    /// still in the new snapshot. Each cell carries one
    /// `current_rx_bps + current_tx_bps` snapshot from a single lsof
    /// poll, so the visible window covers `PEER_VIZ_BUCKETS` polls
    /// (~24-48s on Linux/macOS).
    pub(in crate::app) peer_viz: HashMap<(u16, String), [u64; PEER_VIZ_BUCKETS]>,
    /// Wall-clock of the most recent `push_peer_viz` rotation, and the
    /// one before it. The renderer divides `(now - last) / (last - prev)`
    /// to derive a smooth phase in `[0, 1]` that drifts the wave
    /// leftward by exactly one bucket between pushes — adaptive to the
    /// actual poll interval (which varies on macOS due to nettop
    /// overhead).
    pub(in crate::app) peer_viz_last_push: Option<Instant>,
    pub(in crate::app) peer_viz_prev_push: Option<Instant>,

    // The single shared lsof poller. Lazily started on first tunnel
    // start; lives until App::Drop. The bind_ports list is cloned on
    // every poll iteration so updates are eventually consistent.
    pub(in crate::app) lsof: Option<LsofPollerHandle>,

    /// Demo / test seed. When `App.demo_mode == true` the detail panel
    /// reads from this map instead of the live counters, so visual
    /// regression tests are byte-deterministic.
    pub(in crate::app) demo_live_snapshots: HashMap<String, TunnelLiveSnapshot>,
}

impl Default for TunnelState {
    fn default() -> Self {
        let (parser_tx, parser_rx) = std::sync::mpsc::channel::<ParserMessage>();
        let (lsof_tx, lsof_rx) = std::sync::mpsc::channel::<LsofMessage>();
        Self {
            list: Vec::new(),
            form: TunnelForm::new(),
            active: HashMap::new(),
            form_baseline: None,
            pending_delete: None,
            summaries_cache: HashMap::new(),
            sort_mode: TunnelSortMode::default(),
            parser_tx,
            parser_rx,
            lsof_tx,
            lsof_rx,
            clients: HashMap::new(),
            conflicts: HashMap::new(),
            last_lsof_at: None,
            peer_viz: HashMap::new(),
            peer_viz_last_push: None,
            peer_viz_prev_push: None,
            lsof: None,
            demo_live_snapshots: HashMap::new(),
        }
    }
}

impl Drop for TunnelState {
    fn drop(&mut self) {
        if let Some(mut handle) = self.lsof.take() {
            handle.shutdown();
        }
    }
}

impl TunnelState {
    pub fn list(&self) -> &[TunnelRule] {
        &self.list
    }

    pub fn list_mut(&mut self) -> &mut Vec<TunnelRule> {
        &mut self.list
    }

    pub fn form(&self) -> &TunnelForm {
        &self.form
    }

    pub fn form_mut(&mut self) -> &mut TunnelForm {
        &mut self.form
    }

    pub fn reset_form(&mut self) {
        self.form = TunnelForm::new();
    }

    /// Load this host's tunnel directives from the SSH config into `list`.
    pub fn load_directives(
        &mut self,
        config: &crate::ssh_config::model::SshConfigFile,
        alias: &str,
    ) {
        self.list = config.find_tunnel_directives(alias);
    }

    /// True if the tunnel form differs from its captured baseline.
    pub fn form_is_dirty(&self) -> bool {
        match &self.form_baseline {
            Some(b) => {
                self.form.tunnel_type != b.tunnel_type
                    || self.form.bind_port != b.bind_port
                    || self.form.remote_host != b.remote_host
                    || self.form.remote_port != b.remote_port
                    || self.form.bind_address != b.bind_address
            }
            None => false,
        }
    }

    pub fn active(&self) -> &HashMap<String, ActiveTunnel> {
        &self.active
    }

    pub fn active_get(&self, alias: &str) -> Option<&ActiveTunnel> {
        self.active.get(alias)
    }

    pub fn active_get_mut(&mut self, alias: &str) -> Option<&mut ActiveTunnel> {
        self.active.get_mut(alias)
    }

    pub fn active_contains(&self, alias: &str) -> bool {
        self.active.contains_key(alias)
    }

    pub fn active_insert(&mut self, alias: String, tunnel: ActiveTunnel) {
        self.active.insert(alias, tunnel);
    }

    pub fn active_remove(&mut self, alias: &str) -> Option<ActiveTunnel> {
        self.active.remove(alias)
    }

    pub fn drain_active(&mut self) -> std::collections::hash_map::Drain<'_, String, ActiveTunnel> {
        self.active.drain()
    }

    pub fn clear_active(&mut self) {
        self.active.clear();
    }

    pub fn pending_delete(&self) -> Option<usize> {
        self.pending_delete
    }

    pub fn take_pending_delete(&mut self) -> Option<usize> {
        self.pending_delete.take()
    }

    pub fn sort_mode(&self) -> TunnelSortMode {
        self.sort_mode
    }

    pub fn set_sort_mode(&mut self, mode: TunnelSortMode) {
        self.sort_mode = mode;
    }

    pub fn form_baseline(&self) -> Option<&TunnelFormBaseline> {
        self.form_baseline.as_ref()
    }

    pub fn set_form_baseline(&mut self, baseline: Option<TunnelFormBaseline>) {
        self.form_baseline = baseline;
    }

    pub fn demo_live_snapshots(&self) -> &HashMap<String, crate::tunnel_live::TunnelLiveSnapshot> {
        &self.demo_live_snapshots
    }

    pub fn demo_live_snapshots_mut(
        &mut self,
    ) -> &mut HashMap<String, crate::tunnel_live::TunnelLiveSnapshot> {
        &mut self.demo_live_snapshots
    }

    pub fn parser_tx(&self) -> Sender<ParserMessage> {
        self.parser_tx.clone()
    }

    pub fn clients(&self) -> &HashMap<u16, Vec<ClientPeer>> {
        &self.clients
    }

    pub fn peer_viz(&self) -> &HashMap<(u16, String), [u64; PEER_VIZ_BUCKETS]> {
        &self.peer_viz
    }

    pub fn peer_viz_last_push(&self) -> Option<Instant> {
        self.peer_viz_last_push
    }

    pub fn peer_viz_prev_push(&self) -> Option<Instant> {
        self.peer_viz_prev_push
    }

    pub fn summaries_cache(&self) -> &HashMap<String, String> {
        &self.summaries_cache
    }

    pub fn summaries_cache_mut(&mut self) -> &mut HashMap<String, String> {
        &mut self.summaries_cache
    }

    /// Open a delete confirmation for the tunnel at `idx`. The renderer
    /// reads `pending_delete` to draw the confirm overlay.
    pub fn request_delete(&mut self, idx: usize) {
        self.pending_delete = Some(idx);
    }

    /// Dismiss a pending delete confirmation. Idempotent.
    pub fn cancel_delete(&mut self) {
        self.pending_delete = None;
    }

    /// Ensure the shared lsof poller is running. Idempotent: a second
    /// call after the poller is already up is a noop. Caller is
    /// responsible for updating `bind_ports` afterwards.
    pub fn ensure_lsof_poller(&mut self) {
        if self.lsof.is_some() {
            return;
        }
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let bind_ports: Arc<Mutex<Vec<(String, u16, u32)>>> = Arc::new(Mutex::new(Vec::new()));
        let thread = crate::tunnel_live::spawn_lsof_poller(
            bind_ports.clone(),
            self.lsof_tx.clone(),
            stop.clone(),
        );
        self.lsof = Some(LsofPollerHandle {
            stop,
            bind_ports,
            thread: Some(thread),
        });
        log::debug!("[purple] Tunnel lsof poller started");
    }

    /// Replace the lsof poller's port list. Callers compute the
    /// `(alias, bind_port, tunnel_pid)` tuples from the SSH config
    /// directives because `ActiveTunnel` does not store the rule set
    /// directly. The poller picks up the new list on its next iteration.
    pub fn set_lsof_ports(&self, ports: Vec<(String, u16, u32)>) {
        if let Some(handle) = &self.lsof
            && let Ok(mut g) = handle.bind_ports.lock()
        {
            *g = ports;
        }
    }

    /// Drain the parser channel into per-tunnel live state, drain the
    /// lsof channel into the shared `clients` and `conflicts` maps,
    /// rotate per-tunnel history buckets and finally poll every active
    /// child for exit. Returns one `TunnelExit` per tunnel that ended so the
    /// outer loop can route it through `notify_*`.
    pub fn poll(&mut self) -> Vec<TunnelExit> {
        let now = Instant::now();
        // Drain channel events first so any pending opens are reflected
        // before we report exits.
        while let Ok(msg) = self.parser_rx.try_recv() {
            if let Some(tunnel) = self.active.get_mut(&msg.alias) {
                tunnel.live.record_event(msg.event);
            }
        }
        // Drain lsof snapshots — keep only the freshest one. Older
        // pending messages are discarded because they would just
        // overwrite each other.
        let mut latest_lsof: Option<LsofMessage> = None;
        while let Ok(msg) = self.lsof_rx.try_recv() {
            latest_lsof = Some(msg);
        }
        if let Some(msg) = latest_lsof {
            self.clients = msg.clients;
            self.conflicts = msg.conflicts;
            self.last_lsof_at = Some(msg.at);
            // Roll the per-peer braille history forward exactly once
            // per lsof arrival. The renderer derives a smooth phase
            // from the timestamp pair below to fill in motion between
            // pushes at terminal frame rate.
            self.push_peer_viz(now);
        }
        // Rotate per-tunnel history. Bucket width is `BUCKET_SECS`
        // (currently 2s), so this is effectively per-poll rotation.
        for tunnel in self.active.values_mut() {
            tunnel.live.rotate_if_due(now);
        }
        // Build a port → alias map once and reuse it for both the
        // throughput aggregation and the concurrent-activity sampling
        // below. Source: the lsof poller's `(alias, port, pid)` view of
        // the active bind ports.
        let port_to_alias: HashMap<u16, String> = self
            .lsof
            .as_ref()
            .and_then(|h| h.bind_ports.lock().ok().map(|g| g.clone()))
            .map(|v| v.into_iter().map(|(a, p, _)| (p, a)).collect())
            .unwrap_or_default();
        // Aggregate per-peer current bps into per-tunnel readouts. The
        // tunnel-level value is the honest sum of every connected
        // client's flow — it matches the per-peer numbers shown in the
        // roster. The previous SSH-process sampler counted both ends
        // of the loopback hop, which doubled the displayed speed.
        let mut bps_per_alias: HashMap<String, (u64, u64, bool)> = HashMap::new();
        for (port, peers) in &self.clients {
            let Some(alias) = port_to_alias.get(port) else {
                continue;
            };
            let entry = bps_per_alias
                .entry(alias.clone())
                .or_insert((0u64, 0u64, false));
            for peer in peers {
                entry.0 = entry.0.saturating_add(peer.current_rx_bps);
                entry.1 = entry.1.saturating_add(peer.current_tx_bps);
                if peer.last_sample_at.is_some() {
                    entry.2 = true;
                }
            }
        }
        for (alias, tunnel) in self.active.iter_mut() {
            let (rx, tx, ready) = bps_per_alias.get(alias).copied().unwrap_or((0, 0, false));
            tunnel.live.current_rx_bps = rx;
            tunnel.live.current_tx_bps = tx;
            tunnel.live.peak_rx_bps = tunnel.live.peak_rx_bps.max(rx);
            tunnel.live.peak_tx_bps = tunnel.live.peak_tx_bps.max(tx);
            if ready {
                tunnel.live.last_throughput_at = Some(now);
            }
        }
        // Sample concurrent activity per alias into the current bucket.
        // Source = max(lsof ESTABLISHED clients, ssh active channels)
        // summed across every bind_port that belongs to the alias. That
        // way the sparkline reflects ongoing concurrency (a long-lived
        // WebKit connection, a streaming HTTP/2 session) rather than
        // only short channel-open bursts.
        let mut concurrent_per_alias: HashMap<String, u32> = HashMap::new();
        for (port, peers) in &self.clients {
            if let Some(alias) = port_to_alias.get(port) {
                *concurrent_per_alias.entry(alias.clone()).or_insert(0) += peers.len() as u32;
            }
        }
        for (alias, tunnel) in self.active.iter_mut() {
            let lsof_count = concurrent_per_alias.get(alias).copied().unwrap_or(0);
            let sample = lsof_count.max(tunnel.live.active_channels);
            tunnel.live.sample_activity(sample);
        }

        if self.active.is_empty() {
            return Vec::new();
        }
        let mut exited = Vec::new();
        let mut to_remove = Vec::new();
        for (alias, tunnel) in &mut self.active {
            match tunnel.child.try_wait() {
                Ok(Some(status)) => {
                    // Give the parser a moment to read ssh's last lines, so
                    // the exit is judged on everything ssh wrote.
                    let exited_at = *tunnel.live.exited_at.get_or_insert(now);
                    if !tunnel.live.drained.load(Ordering::Relaxed)
                        && now.saturating_duration_since(exited_at) < EXIT_DRAIN_GRACE
                    {
                        continue;
                    }
                    // The parser thread holds child.stderr; ask the
                    // shared stderr ringbuffer for the last meaningful
                    // line instead of re-reading the pipe.
                    let stderr_msg = tunnel
                        .live
                        .stderr_buffer
                        .lock()
                        .ok()
                        .and_then(|b| b.iter().rev().find(|s| !s.trim().is_empty()).cloned())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                    let exit_code = status.code().unwrap_or(-1);
                    if !status.success() {
                        log::error!(
                            "[external] Tunnel exited unexpectedly: alias={alias} exit={exit_code}"
                        );
                        if let Some(ref err) = stderr_msg {
                            log::debug!("[external] Tunnel stderr: {}", err.trim());
                        }
                    }
                    let last_exit_line = stderr_msg
                        .clone()
                        .unwrap_or_else(|| format!("exit code {}", exit_code));
                    tunnel.live.last_exit = Some((exit_code, last_exit_line));
                    tunnel.live.parser_stop.store(true, Ordering::Relaxed);
                    // Mark all currently-open channels as closed so the
                    // active count drops to zero on exit.
                    if tunnel.live.active_channels > 0 {
                        let close_now = ChannelEventKind::Close;
                        let ids: Vec<u32> = tunnel.live.channel_open.keys().copied().collect();
                        for id in ids {
                            tunnel.live.record_event(crate::tunnel_live::ChannelEvent {
                                at: now,
                                channel_id: id,
                                kind: close_now,
                                channel_kind: None,
                                opened_at: None,
                            });
                        }
                    }
                    let (msg, is_error) = if status.success() {
                        (format!("Tunnel for {} closed.", alias), false)
                    } else if let Some(err) = stderr_msg {
                        (format!("Tunnel for {}: {}", alias, err), true)
                    } else {
                        (
                            format!("Tunnel for {} exited with code {}.", alias, exit_code),
                            true,
                        )
                    };
                    exited.push(TunnelExit {
                        alias: alias.clone(),
                        message: msg,
                        is_error,
                        auth_refused: tunnel.live.auth_refused.load(Ordering::Relaxed),
                        authenticated: tunnel.live.authenticated.load(Ordering::Relaxed),
                    });
                    to_remove.push(alias.clone());
                }
                Ok(None) => {}
                Err(e) => {
                    exited.push(TunnelExit {
                        alias: alias.clone(),
                        message: format!("Tunnel for {} lost: {}", alias, e),
                        is_error: true,
                        auth_refused: false,
                        authenticated: tunnel.live.authenticated.load(Ordering::Relaxed),
                    });
                    to_remove.push(alias.clone());
                }
            }
        }
        for alias in to_remove {
            self.active.remove(&alias);
        }
        exited
    }

    /// Push one bucket of per-peer braille history. Called exactly
    /// once per lsof arrival so the visible window encodes
    /// `PEER_VIZ_BUCKETS` consecutive poll snapshots — long enough to
    /// see the trend, short enough to react quickly to changes. The
    /// renderer fills in smooth motion between pushes via
    /// `peer_viz_last_push` / `peer_viz_prev_push`. Garbage-collects
    /// entries for peers that no longer appear in `self.clients`.
    pub fn push_peer_viz(&mut self, now: Instant) {
        let mut live: HashSet<(u16, String)> = HashSet::new();
        for (port, peers) in &self.clients {
            for peer in peers {
                let key = (*port, peer.src.clone());
                live.insert(key.clone());
                let combined = peer.current_rx_bps.saturating_add(peer.current_tx_bps);
                let history = self
                    .peer_viz
                    .entry(key)
                    .or_insert_with(|| [0u64; PEER_VIZ_BUCKETS]);
                history.rotate_left(1);
                history[PEER_VIZ_BUCKETS - 1] = combined;
            }
        }
        self.peer_viz.retain(|key, _| live.contains(key));
        self.peer_viz_prev_push = self.peer_viz_last_push;
        self.peer_viz_last_push = Some(now);
    }

    /// Drop demo-mode tunnel snapshots whose alias is no longer in
    /// `valid_aliases`. Called from `App::reload_hosts`. Outside demo
    /// the map stays empty so this is a no-op, but a demo workflow that
    /// renames or deletes a host should not leak the old snapshot.
    pub fn prune_orphans(&mut self, valid_aliases: &HashSet<&str>) {
        let pre = self.demo_live_snapshots.len();
        self.demo_live_snapshots
            .retain(|alias, _| valid_aliases.contains(alias.as_str()));
        let dropped = pre.saturating_sub(self.demo_live_snapshots.len());
        if dropped > 0 {
            log::debug!(
                "[purple] reload_hosts: dropped {dropped} orphan demo_live_snapshots entrie(s)"
            );
        }
    }

    /// Move the active-tunnel handle from `old` to `new` on host
    /// rename. Called from `App::migrate_alias_keyed_caches` before
    /// `reload_hosts`, whose prune step would otherwise drop entries
    /// still under the old alias. No-op when `old == new` or no
    /// active tunnel exists under `old`.
    pub fn migrate_alias(&mut self, old: &str, new: &str) {
        if old == new {
            return;
        }
        if let Some(t) = self.active.remove(old) {
            self.active.insert(new.to_string(), t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_empty() {
        let s = TunnelState::default();
        assert!(s.list.is_empty());
        assert!(s.active.is_empty());
        assert!(s.pending_delete.is_none());
        assert!(s.summaries_cache.is_empty());
    }

    #[test]
    fn poll_on_empty_returns_empty_vec() {
        // Fast path: no active tunnels means no exit events to report and
        // no child processes to reap. Spawning real ssh child processes
        // belongs in integration tests.
        let mut s = TunnelState::default();
        let result = s.poll();
        assert!(result.is_empty());
        assert!(s.active.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn an_exit_waits_for_the_parser_to_read_the_last_lines() {
        let mut s = TunnelState::default();
        // A grandchild keeps stderr open after the shell exits, so the
        // parser cannot reach the end of the pipe yet.
        let child = std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 10 >&2 & exit 3")
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn sh");
        let tunnel = crate::tunnel::ActiveTunnel::spawn(
            child,
            "h",
            s.parser_tx(),
            crate::tunnel_live::AuthWatch::default(),
        );
        s.active.insert("h".into(), tunnel);
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while s
            .active
            .get("h")
            .is_some_and(|t| t.live.exited_at.is_none())
            && Instant::now() < deadline
        {
            assert!(s.poll().is_empty(), "held back while the parser drains");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(s.active.contains_key("h"), "still waiting on the parser");
        // Pinned, so a slow machine cannot drift past the grace here.
        s.active.get_mut("h").expect("tunnel").live.exited_at = Some(Instant::now());
        assert!(s.poll().is_empty(), "inside the grace the exit waits");
        // Past the grace the exit is reported whether or not the pipe closed.
        s.active.get_mut("h").expect("tunnel").live.exited_at =
            Some(Instant::now() - EXIT_DRAIN_GRACE);
        let exits = s.poll();
        assert_eq!(exits.len(), 1);
        assert!(exits[0].is_error, "exit 3 is an error");
        assert!(!exits[0].auth_refused);
        assert!(s.active.is_empty());
    }

    fn make_peer(src: &str, rx: u64, tx: u64) -> ClientPeer {
        ClientPeer {
            src: src.to_string(),
            process: "curl".into(),
            pid: 1234,
            since: Instant::now(),
            responsible_app: None,
            current_rx_bps: rx,
            current_tx_bps: tx,
            bytes_rcvd: None,
            bytes_sent: None,
            last_sample_at: Some(Instant::now()),
        }
    }

    #[test]
    fn push_peer_viz_initialises_history_and_writes_combined_bps_to_rightmost_cell() {
        let mut s = TunnelState::default();
        s.clients
            .insert(8080, vec![make_peer("127.0.0.1:1", 1_000, 500)]);
        let now = Instant::now();
        s.push_peer_viz(now);
        let key = (8080u16, "127.0.0.1:1".to_string());
        let history = s.peer_viz.get(&key).expect("entry created on first push");
        assert_eq!(history[PEER_VIZ_BUCKETS - 1], 1_500);
        for cell in &history[..PEER_VIZ_BUCKETS - 1] {
            assert_eq!(*cell, 0);
        }
        assert_eq!(s.peer_viz_last_push, Some(now));
        assert_eq!(s.peer_viz_prev_push, None);
    }

    #[test]
    fn push_peer_viz_rotates_left_on_each_call() {
        let mut s = TunnelState::default();
        s.clients
            .insert(8080, vec![make_peer("127.0.0.1:1", 100, 0)]);
        let t0 = Instant::now();
        s.push_peer_viz(t0);
        // Update the bps reading and push again to simulate a second
        // lsof arrival.
        if let Some(peers) = s.clients.get_mut(&8080) {
            peers[0].current_rx_bps = 200;
        }
        let t1 = t0 + std::time::Duration::from_secs(2);
        s.push_peer_viz(t1);
        let key = (8080u16, "127.0.0.1:1".to_string());
        let history = s.peer_viz.get(&key).expect("entry exists");
        assert_eq!(history[PEER_VIZ_BUCKETS - 1], 200);
        assert_eq!(history[PEER_VIZ_BUCKETS - 2], 100);
        // Both timestamps populated so the renderer can derive a
        // smooth phase from the actual interval.
        assert_eq!(s.peer_viz_last_push, Some(t1));
        assert_eq!(s.peer_viz_prev_push, Some(t0));
    }

    #[test]
    fn push_peer_viz_garbage_collects_disappeared_peers() {
        let mut s = TunnelState::default();
        s.clients.insert(8080, vec![make_peer("127.0.0.1:1", 0, 0)]);
        let t0 = Instant::now();
        s.push_peer_viz(t0);
        assert!(
            s.peer_viz
                .contains_key(&(8080u16, "127.0.0.1:1".to_string()))
        );
        // Peer disappears from the lsof snapshot on the next poll.
        s.clients.clear();
        s.push_peer_viz(t0 + std::time::Duration::from_secs(2));
        assert!(s.peer_viz.is_empty());
    }

    #[test]
    fn request_delete_sets_pending_delete_to_some_idx() {
        let mut s = TunnelState::default();
        s.request_delete(3);
        assert_eq!(s.pending_delete, Some(3));
    }

    #[test]
    fn cancel_delete_clears_pending_delete() {
        let mut s = TunnelState::default();
        s.pending_delete = Some(2);
        s.cancel_delete();
        assert!(s.pending_delete.is_none());
    }

    #[test]
    fn request_delete_overwrites_existing_pending() {
        let mut s = TunnelState::default();
        s.pending_delete = Some(1);
        s.request_delete(7);
        assert_eq!(s.pending_delete, Some(7));
    }

    #[test]
    fn cancel_delete_is_idempotent_on_empty_pending() {
        let mut s = TunnelState::default();
        s.cancel_delete();
        s.cancel_delete();
        assert!(s.pending_delete.is_none());
    }

    fn empty_snapshot() -> crate::tunnel_live::TunnelLiveSnapshot {
        crate::tunnel_live::TunnelLiveSnapshot {
            uptime_secs: 0,
            active_channels: 0,
            peak_concurrent: 0,
            total_opens: 0,
            idle_secs: 0,
            rx_history: [0; crate::tunnel_live::HISTORY_BUCKETS],
            tx_history: [0; crate::tunnel_live::HISTORY_BUCKETS],
            current_rx_bps: 0,
            current_tx_bps: 0,
            peak_rx_bps: 0,
            peak_tx_bps: 0,
            throughput_ready: false,
            clients: vec![],
            events: vec![],
            currently_open: vec![],
            conflict: None,
            last_exit: None,
        }
    }

    #[test]
    fn prune_orphans_drops_unknown_demo_snapshots() {
        let mut s = TunnelState::default();
        s.demo_live_snapshots
            .insert("keep".to_string(), empty_snapshot());
        s.demo_live_snapshots
            .insert("drop".to_string(), empty_snapshot());

        let valid: HashSet<&str> = ["keep"].into_iter().collect();
        s.prune_orphans(&valid);

        assert!(s.demo_live_snapshots.contains_key("keep"));
        assert!(!s.demo_live_snapshots.contains_key("drop"));
    }

    #[test]
    fn migrate_alias_is_noop_on_empty_active_map() {
        // Constructing a real ActiveTunnel requires a spawned child
        // process; the active-map rename is covered by the
        // `migrate_alias_keyed_caches_*` integration tests, which build
        // a full App. Here we just verify the no-op contract on absent
        // and self-rename inputs.
        let mut s = TunnelState::default();
        s.migrate_alias("missing", "new");
        assert!(s.active.is_empty());
        s.migrate_alias("same", "same");
        assert!(s.active.is_empty());
    }

    fn state_matching_baseline() -> TunnelState {
        let mut s = TunnelState::default();
        s.form.tunnel_type = crate::tunnel::TunnelType::Local;
        s.form.bind_port = "8080".into();
        s.form.remote_host = "db.internal".into();
        s.form.remote_port = "5432".into();
        s.form.bind_address = "127.0.0.1".into();
        s.set_form_baseline(Some(TunnelFormBaseline {
            tunnel_type: crate::tunnel::TunnelType::Local,
            bind_port: "8080".into(),
            remote_host: "db.internal".into(),
            remote_port: "5432".into(),
            bind_address: "127.0.0.1".into(),
        }));
        s
    }

    #[test]
    fn form_is_dirty_is_false_without_a_baseline() {
        let mut s = TunnelState::default();
        s.form.bind_port = "9000".into();
        assert!(!s.form_is_dirty());
    }

    #[test]
    fn form_is_dirty_is_false_when_form_equals_baseline() {
        assert!(!state_matching_baseline().form_is_dirty());
    }

    fn assert_field_change_is_dirty(field: &str, mutate: impl FnOnce(&mut TunnelForm)) {
        let mut s = state_matching_baseline();
        mutate(&mut s.form);
        assert!(s.form_is_dirty(), "a change in {field} must read dirty");
    }

    #[test]
    fn form_is_dirty_detects_a_change_in_each_field() {
        assert_field_change_is_dirty("tunnel_type", |f| {
            f.tunnel_type = crate::tunnel::TunnelType::Remote;
        });
        assert_field_change_is_dirty("bind_port", |f| f.bind_port.push('1'));
        assert_field_change_is_dirty("remote_host", |f| f.remote_host.push('x'));
        assert_field_change_is_dirty("remote_port", |f| f.remote_port.push('1'));
        assert_field_change_is_dirty("bind_address", |f| f.bind_address.push('9'));
    }
}
