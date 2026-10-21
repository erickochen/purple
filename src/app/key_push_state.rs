//! Key-push state. Tracks the picker selection set, in-flight worker
//! handles and accumulated results between Screen transitions.
//!
//! Lives on `App` so the event loop can land per-host `KeyPushResult`
//! events into `results` regardless of which Screen is active (the user
//! may switch tabs while a push is running).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use ratatui::widgets::ListState;

use crate::key_push::{InflightChild, KeyPushResult};
use std::collections::{HashMap, HashSet, VecDeque};

/// A question the finished run left for the user, drained one at a time
/// once no run is in flight. Each entry carries the key its run was
/// pushing: a later run overwrites `KeyPushState::key_path`, so a host
/// still waiting here would otherwise be answered for the wrong key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyPushPrompt {
    /// Strict checking refused a host not in `known_hosts` yet.
    Trust { alias: String, key_path: String },
    /// ssh ended with `Permission denied` and the server takes a password.
    Password { alias: String, key_path: String },
    /// A security key that signs the login wants a PIN. `security_key` is
    /// that key file; `refused` says why it turned the last PIN down.
    SecurityKeyPin {
        alias: String,
        key_path: String,
        security_key: String,
        refused: Option<crate::security_key::RefusalReason>,
    },
}

impl KeyPushPrompt {
    pub fn alias(&self) -> &str {
        match self {
            KeyPushPrompt::Trust { alias, .. }
            | KeyPushPrompt::Password { alias, .. }
            | KeyPushPrompt::SecurityKeyPin { alias, .. } => alias,
        }
    }

    /// Path of the key this question belongs to, as `SshKeyInfo::display_path`
    /// spells it.
    pub fn key_path(&self) -> &str {
        match self {
            KeyPushPrompt::Trust { key_path, .. }
            | KeyPushPrompt::Password { key_path, .. }
            | KeyPushPrompt::SecurityKeyPin { key_path, .. } => key_path,
        }
    }
}

/// Push state owned by `App`. Empty between push runs.
#[derive(Default)]
pub struct KeyPushState {
    /// Aliases the user has selected in the picker. Modified by Space
    /// during `Screen::KeyPushPicker` and frozen into `committed` on Enter.
    pub selected: HashSet<String>,
    /// Snapshot of `selected` (in picker order) taken when the user
    /// presses Enter to open `Screen::ConfirmKeyPush`. Read by the
    /// confirm renderer and by `start_key_push`. Cleared on cancel or
    /// after the worker spawns. Keeps `Screen::ConfirmKeyPush` payload
    /// small.
    pub committed: Vec<String>,
    /// Cursor in the picker's host list. Indexes into the picker's
    /// visible host slice.
    pub list_state: ListState,
    /// Results accumulated as `AppEvent::KeyPushResult` lands. Drained
    /// when the run completes and the summary toast / sticky error is
    /// rendered.
    pub results: Vec<KeyPushResult>,
    /// What each landed host's run said about security key PINs, settled as
    /// its result arrives so the PIN markers are still fresh.
    pub pin_outcomes: HashMap<String, super::PinOutcome>,
    /// Total hosts the current run is targeting. Used to know when the
    /// run is "done" so the summary can fire exactly once.
    pub expected_count: usize,
    /// Cancel flag observed by every worker thread. Set on push-cancel,
    /// new push run, or App drop.
    pub cancel: Option<Arc<AtomicBool>>,
    /// JoinHandle for the worker pool. Joined on App drop.
    pub worker: Option<std::thread::JoinHandle<()>>,
    /// Monotonic run identifier. Bumped at the start of every push so
    /// stale `KeyPushResult` events from a previously-cancelled run can
    /// be dropped instead of contaminating the next run's accumulator.
    pub run_id: u64,
    /// The ssh child the worker is running right now. Cancel and shutdown
    /// kill it through here instead of waiting for the worker to notice the
    /// flag between hosts.
    pub inflight: InflightChild,
    /// Path of the key the current run pushes, as `SshKeyInfo::display_path`
    /// spells it. A retry after a dialog looks the key up by this path: the
    /// list is rebuilt after a successful push, so a position would not
    /// survive.
    pub key_path: String,
    /// True when the current run follows an accepted trust dialog, so its
    /// ssh may record a host key it has not seen before. Reset by every
    /// `start_run`, so it never carries into an ordinary push.
    pub trust_new_host_key: bool,
    /// Hosts of the finished run that still need an answer, in picker order.
    /// Drained one dialog at a time, never while a run is in flight.
    pub pending_prompts: VecDeque<KeyPushPrompt>,
}

impl KeyPushState {
    /// Drop the worker handle gracefully. Called from `App::drop` so a
    /// panicking unwind cannot leave the push thread running with a
    /// dangling sender. The running child is killed first so the join
    /// returns within the SIGTERM grace window.
    pub fn shutdown(&mut self) {
        if let Some(ref cancel) = self.cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.inflight.terminate();
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }

    /// Reset picker-only state without touching in-flight worker. Called
    /// before opening the picker for a new key so the previous run's
    /// selection set does not bleed in.
    pub fn reset_picker(&mut self) {
        self.selected.clear();
        self.list_state.select(Some(0));
    }

    /// Begin a new push run. Clears the result accumulator, sets the
    /// expected host count and the path of the key being pushed, bumps the monotonic
    /// run_id (so any stale KeyPushResult events from an aborted previous
    /// run can be dropped), constructs a fresh cancel flag and an empty
    /// inflight slot and stores them on state. Returns the new run_id
    /// together with the cancel handle so the spawned worker can share it.
    pub fn start_run(
        &mut self,
        expected: usize,
        key_path: String,
        trust_new_host_key: bool,
    ) -> (u64, Arc<AtomicBool>) {
        self.results.clear();
        self.pin_outcomes.clear();
        self.expected_count = expected;
        self.key_path = key_path;
        self.trust_new_host_key = trust_new_host_key;
        self.run_id = self.run_id.wrapping_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        self.inflight = InflightChild::default();
        (self.run_id, cancel)
    }

    /// Completion path. The worker loop has finished naturally; clear the
    /// accumulators and join the worker handle. Safe to call when the
    /// worker has already exited.
    pub fn finish_run(&mut self) {
        self.results.clear();
        self.pin_outcomes.clear();
        self.expected_count = 0;
        self.selected.clear();
        self.cancel = None;
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }

    /// User-cancel path. The running child is killed and the cancel flag is
    /// raised for the worker. Accumulators and pending prompts are cleared
    /// and run_id is bumped so in-flight KeyPushResult events from the
    /// aborted worker arrive with a stale run_id and are dropped. The worker
    /// handle is intentionally NOT joined here so the UI does not block
    /// while the thread reaps the child.
    pub fn cancel_run(&mut self) {
        if let Some(ref cancel) = self.cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.inflight.terminate();
        self.results.clear();
        self.pin_outcomes.clear();
        self.expected_count = 0;
        self.cancel = None;
        self.selected.clear();
        self.pending_prompts.clear();
        self.run_id = self.run_id.wrapping_add(1);
    }

    /// Failure recovery after a failed worker spawn. Drops the cancel
    /// handle, zeroes the expected count, and clears the worker slot.
    /// Distinct from `finish_run`: the worker handle is None here (spawn
    /// failed), and the result accumulator is left intact for any caller
    /// that may want to surface it in the error path.
    pub fn clear_inflight_state(&mut self) {
        self.cancel = None;
        self.expected_count = 0;
        self.worker = None;
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn default_is_empty() {
        let s = KeyPushState::default();
        assert!(s.selected.is_empty());
        assert_eq!(s.list_state.selected(), None);
        assert!(s.results.is_empty());
        assert_eq!(s.expected_count, 0);
        assert!(s.cancel.is_none());
        assert!(s.worker.is_none());
    }

    #[test]
    fn reset_picker_clears_selection_and_resets_cursor() {
        let mut s = KeyPushState::default();
        s.selected.insert("host-a".to_string());
        s.selected.insert("host-b".to_string());
        s.list_state.select(Some(5));
        s.reset_picker();
        assert!(s.selected.is_empty());
        assert_eq!(s.list_state.selected(), Some(0));
    }

    #[test]
    fn reset_picker_leaves_inflight_state_alone() {
        // shutdown is the path that touches worker/cancel; reset_picker
        // is the picker-open path and must not interfere with a run that
        // is still finalising.
        let mut s = KeyPushState::default();
        s.cancel = Some(Arc::new(AtomicBool::new(false)));
        s.expected_count = 5;
        s.results.push(crate::key_push::KeyPushResult {
            alias: "h".into(),
            outcome: crate::key_push::KeyPushOutcome::Appended,
        });
        s.reset_picker();
        assert!(s.cancel.is_some());
        assert_eq!(s.expected_count, 5);
        assert_eq!(s.results.len(), 1);
    }

    #[test]
    fn shutdown_sets_cancel_flag() {
        let mut s = KeyPushState::default();
        let flag = Arc::new(AtomicBool::new(false));
        s.cancel = Some(flag.clone());
        s.shutdown();
        assert!(flag.load(Ordering::Relaxed));
    }

    #[test]
    fn shutdown_joins_worker_and_takes_handle() {
        let mut s = KeyPushState::default();
        let flag = Arc::new(AtomicBool::new(false));
        let cancel = flag.clone();
        // A trivial worker that observes the cancel flag and exits.
        let handle = std::thread::spawn(move || {
            while !cancel.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
        s.cancel = Some(flag);
        s.worker = Some(handle);
        s.shutdown();
        assert!(s.worker.is_none(), "worker handle should be taken");
    }

    #[test]
    fn shutdown_is_idempotent_with_no_worker() {
        let mut s = KeyPushState::default();
        // Should not panic when called on an empty state.
        s.shutdown();
        s.shutdown();
    }

    #[test]
    fn start_run_clears_results_sets_expected_and_stores_cancel() {
        let mut s = KeyPushState::default();
        s.results.push(crate::key_push::KeyPushResult {
            alias: "old".into(),
            outcome: crate::key_push::KeyPushOutcome::Appended,
        });

        let (_run_id, cancel) = s.start_run(4, "~/.ssh/id_a".into(), false);

        assert!(s.results.is_empty());
        assert_eq!(s.expected_count, 4);
        assert!(s.cancel.is_some());
        // Returned cancel arc points to the same flag stored on state.
        cancel.store(true, Ordering::Relaxed);
        assert!(s.cancel.as_ref().unwrap().load(Ordering::Relaxed));
    }

    #[test]
    fn start_run_bumps_run_id_and_returns_it() {
        let mut s = KeyPushState {
            run_id: 41,
            ..Default::default()
        };
        let (run_id, _cancel) = s.start_run(1, "~/.ssh/id_a".into(), false);
        assert_eq!(run_id, 42);
        assert_eq!(s.run_id, 42);
    }

    #[test]
    fn start_run_records_the_key_path_for_retries() {
        let mut s = KeyPushState::default();
        let _ = s.start_run(1, "~/.ssh/id_ed25519".into(), false);
        assert_eq!(s.key_path, "~/.ssh/id_ed25519");
    }

    #[test]
    fn start_run_records_and_resets_the_trust_flag() {
        let mut s = KeyPushState::default();
        let _ = s.start_run(1, "~/.ssh/id_a".into(), true);
        assert!(s.trust_new_host_key, "a trust retry carries the flag");
        let _ = s.start_run(1, "~/.ssh/id_a".into(), false);
        assert!(!s.trust_new_host_key, "an ordinary push does not");
    }

    #[test]
    fn start_run_preserves_picker_state_and_committed() {
        let mut s = KeyPushState::default();
        s.selected.insert("host-a".into());
        s.committed = vec!["host-a".into(), "host-b".into()];
        s.list_state.select(Some(2));

        let _ = s.start_run(2, "~/.ssh/id_a".into(), false);

        assert!(s.selected.contains("host-a"));
        assert_eq!(s.committed, vec!["host-a".to_string(), "host-b".into()]);
        assert_eq!(s.list_state.selected(), Some(2));
    }

    #[test]
    fn start_run_keeps_pending_prompts_from_the_previous_run() {
        // A retry for one host runs while the other hosts still wait for
        // their own dialog; starting that run must not drop them.
        let mut s = KeyPushState::default();
        s.pending_prompts.push_back(KeyPushPrompt::Password {
            alias: "h2".into(),
            key_path: "~/.ssh/id_b".into(),
        });
        let _ = s.start_run(1, "~/.ssh/id_a".into(), false);
        assert_eq!(s.pending_prompts.len(), 1);
    }

    #[test]
    fn cancel_run_sets_the_flag_and_drops_pending_prompts() {
        let mut s = KeyPushState::default();
        let (_id, cancel) = s.start_run(2, "~/.ssh/id_a".into(), false);
        s.pending_prompts.push_back(KeyPushPrompt::Trust {
            alias: "h".into(),
            key_path: "~/.ssh/id_a".into(),
        });
        s.cancel_run();
        assert!(cancel.load(Ordering::Relaxed), "worker must see the cancel");
        assert!(s.pending_prompts.is_empty());
    }

    #[test]
    fn key_push_prompt_alias_names_the_host() {
        let t = KeyPushPrompt::Trust {
            alias: "a".into(),
            key_path: "~/.ssh/id_a".into(),
        };
        assert_eq!(t.alias(), "a");
        assert_eq!(t.key_path(), "~/.ssh/id_a");
        let p = KeyPushPrompt::Password {
            alias: "b".into(),
            key_path: "~/.ssh/id_b".into(),
        };
        assert_eq!(p.alias(), "b");
        assert_eq!(p.key_path(), "~/.ssh/id_b");
    }

    #[test]
    fn finish_run_clears_run_accumulators() {
        let mut s = KeyPushState {
            expected_count: 5,
            cancel: Some(Arc::new(AtomicBool::new(false))),
            ..Default::default()
        };
        s.selected.insert("h".into());
        s.results.push(crate::key_push::KeyPushResult {
            alias: "h".into(),
            outcome: crate::key_push::KeyPushOutcome::Appended,
        });

        s.finish_run();

        assert!(s.results.is_empty());
        assert_eq!(s.expected_count, 0);
        assert!(s.selected.is_empty());
        assert!(s.cancel.is_none());
    }

    #[test]
    fn finish_run_joins_worker_and_takes_handle() {
        let mut s = KeyPushState::default();
        let handle = std::thread::spawn(|| {});
        s.worker = Some(handle);
        s.finish_run();
        assert!(s.worker.is_none(), "worker handle should be taken");
    }

    #[test]
    fn finish_run_preserves_committed_and_list_state() {
        let mut s = KeyPushState::default();
        s.committed = vec!["host-a".into()];
        s.list_state.select(Some(3));

        s.finish_run();

        assert_eq!(s.committed, vec!["host-a".to_string()]);
        assert_eq!(s.list_state.selected(), Some(3));
    }

    #[test]
    fn cancel_run_clears_accumulators_and_bumps_run_id() {
        let mut s = KeyPushState {
            expected_count: 3,
            run_id: 10,
            cancel: Some(Arc::new(AtomicBool::new(false))),
            ..Default::default()
        };
        s.selected.insert("h".into());
        s.results.push(crate::key_push::KeyPushResult {
            alias: "h".into(),
            outcome: crate::key_push::KeyPushOutcome::Appended,
        });

        s.cancel_run();

        assert!(s.results.is_empty());
        assert_eq!(s.expected_count, 0);
        assert!(s.cancel.is_none());
        assert!(s.selected.is_empty());
        assert_eq!(s.run_id, 11);
    }

    #[test]
    fn cancel_run_preserves_worker_handle() {
        // Cancel is async via the cancel flag; the worker thread drains
        // itself. We must not block the UI on join here.
        let mut s = KeyPushState::default();
        let handle = std::thread::spawn(|| {});
        s.worker = Some(handle);
        s.cancel_run();
        assert!(s.worker.is_some(), "cancel must not take the worker handle");
        // Clean up for the test harness.
        if let Some(h) = s.worker.take() {
            let _ = h.join();
        }
    }

    #[test]
    fn clear_inflight_state_drops_cancel_expected_count_worker() {
        let mut s = KeyPushState {
            expected_count: 7,
            cancel: Some(Arc::new(AtomicBool::new(false))),
            ..Default::default()
        };
        // Channel-based blocking thread so clear_inflight_state can drop
        // the JoinHandle without leaving a detached thread spinning in
        // the test harness. drop(tx) at the end signals the thread to
        // exit cleanly via the rx disconnect.
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let handle = std::thread::spawn(move || {
            let _ = rx.recv();
        });
        s.worker = Some(handle);

        s.clear_inflight_state();

        assert_eq!(s.expected_count, 0);
        assert!(s.cancel.is_none());
        assert!(s.worker.is_none());

        drop(tx);
    }

    #[test]
    fn clear_inflight_state_preserves_committed_run_id_and_results() {
        let mut s = KeyPushState {
            run_id: 9,
            ..Default::default()
        };
        s.committed = vec!["host".into()];
        s.results.push(crate::key_push::KeyPushResult {
            alias: "host".into(),
            outcome: crate::key_push::KeyPushOutcome::Appended,
        });

        s.clear_inflight_state();

        assert_eq!(s.run_id, 9);
        assert_eq!(s.committed, vec!["host".to_string()]);
        assert_eq!(s.results.len(), 1);
    }
}
