//! Security key PINs typed in the TUI, held in memory until purple exits.
//!
//! A typed PIN first goes out as a trial with the operation that asked for
//! it. It joins the confirmed set, which every later run may hand out, only
//! once a run proved it: askpass handed it out, the run succeeded and the key
//! refused nothing. A wrong PIN costs one attempt per typing, never more.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;

use super::{App, PendingRetry, Screen};
use crate::security_key::{self as sk, PinFlow, RefusalReason, SecurityKeyPin, SecurityKeyRun};

/// A PIN typed for one key and one operation, not proven yet.
#[derive(Clone, PartialEq, Eq)]
pub struct PinTrial {
    pub alias: String,
    pub key_path: String,
    pub pin: String,
    pub flow: PinFlow,
}

// Hand-written so a stray `{:?}` can never print the PIN.
impl std::fmt::Debug for PinTrial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinTrial")
            .field("alias", &self.alias)
            .field("key_path", &self.key_path)
            .field("flow", &self.flow)
            .finish_non_exhaustive()
    }
}

/// Proven PINs by key path, plus the one trial in flight.
#[derive(Default)]
pub struct SecurityKeyPins {
    confirmed: HashMap<String, String>,
    trial: Option<PinTrial>,
}

// Hand-written so a stray `{:?}` can never print a PIN.
impl std::fmt::Debug for SecurityKeyPins {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut keys: Vec<&String> = self.confirmed.keys().collect();
        keys.sort();
        f.debug_struct("SecurityKeyPins")
            .field("confirmed", &keys)
            .field("trial", &self.trial)
            .finish()
    }
}

impl SecurityKeyPins {
    /// Put a freshly typed PIN on trial. It replaces any earlier trial.
    pub fn start_trial(&mut self, trial: PinTrial) {
        self.trial = Some(trial);
    }

    pub fn trial(&self) -> Option<&PinTrial> {
        self.trial.as_ref()
    }

    fn take_trial_for(&mut self, alias: &str, flow: PinFlow) -> Option<PinTrial> {
        if self
            .trial
            .as_ref()
            .is_some_and(|t| t.alias == alias && t.flow == flow)
        {
            self.trial.take()
        } else {
            None
        }
    }

    /// The proven PIN for `key_path`, if there is one.
    pub fn confirmed_pin(&self, key_path: &str) -> Option<&str> {
        self.confirmed.get(key_path).map(String::as_str)
    }

    /// Every proven PIN the key has not refused since, plus the trial when it
    /// belongs to `trial_for`.
    fn pins_for(
        &self,
        paths: Option<&crate::runtime::env::Paths>,
        trial_for: Option<(&str, PinFlow)>,
    ) -> Vec<SecurityKeyPin> {
        let mut pins: Vec<SecurityKeyPin> = self
            .confirmed
            .iter()
            .filter(|(key, _)| sk::refusal_for(paths, key).is_none())
            .map(|(key, pin)| SecurityKeyPin {
                key_path: key.clone(),
                pin: pin.clone(),
            })
            .collect();
        if let (Some(trial), Some((alias, flow))) = (&self.trial, trial_for)
            && trial.alias == alias
            && trial.flow == flow
        {
            pins.retain(|p| p.key_path != trial.key_path);
            pins.push(SecurityKeyPin {
                key_path: trial.key_path.clone(),
                pin: trial.pin.clone(),
            });
        }
        pins.sort_by(|a, b| a.key_path.cmp(&b.key_path));
        pins
    }

    /// The run for `flow` on `alias`. Callers check the OpenSSH gate first.
    pub(crate) fn run_for(
        &self,
        paths: Option<&crate::runtime::env::Paths>,
        alias: &str,
        flow: PinFlow,
    ) -> SecurityKeyRun {
        SecurityKeyRun {
            pins: self.pins_for(paths, Some((alias, flow))),
            flow: Some(flow),
        }
    }

    /// Drop the trial of `flow`, only on `alias` when one is given. Returns
    /// true when one went.
    pub fn drop_trial(&mut self, flow: PinFlow, alias: Option<&str>) -> bool {
        if self
            .trial
            .as_ref()
            .is_some_and(|t| t.flow == flow && alias.is_none_or(|a| t.alias == a))
        {
            self.trial = None;
            return true;
        }
        false
    }

    /// Drop a trial whose host left the config. Returns true when one went.
    pub fn prune_aliases(&mut self, valid: &HashSet<&str>) -> bool {
        if self
            .trial
            .as_ref()
            .is_some_and(|t| !valid.contains(t.alias.as_str()))
        {
            self.trial = None;
            return true;
        }
        false
    }
}

/// How a background run ended, as far as security key PINs care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEnd {
    /// The run got in and did its work.
    Connected,
    /// ssh gave up on authentication.
    AuthRefused,
    /// Anything else, such as the network or the remote command failing.
    OtherFailure,
}

impl RunEnd {
    /// Classify a run from whether it succeeded and what ssh left on stderr.
    pub fn from_run(success: bool, stderr: &str) -> Self {
        if success {
            RunEnd::Connected
        } else if crate::connection::auth_refused(stderr) {
            RunEnd::AuthRefused
        } else {
            RunEnd::OtherFailure
        }
    }
}

/// What a finished run says about security key PINs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinOutcome {
    NotInvolved,
    /// The run met a PIN prompt purple had no PIN for.
    Needed {
        key_path: String,
    },
    /// The key refused the PIN purple handed out. `connected` is true when
    /// the host let the run in another way.
    Rejected {
        key_path: String,
        reason: RefusalReason,
        connected: bool,
    },
}

impl App {
    /// True when the installed OpenSSH keeps server prompts apart from PIN
    /// prompts. Asked once per session.
    pub(crate) fn security_key_pins_allowed(&self) -> bool {
        *self.ssh_allows_security_key_pins.get_or_init(|| {
            let version = crate::logging::query_ssh_version().unwrap_or_default();
            let allowed = sk::ssh_version_allows_pins(&version);
            log::debug!("[purple] security key: PIN hand-out allowed={allowed} ssh={version}");
            allowed
        })
    }

    /// Seed the gate with the `ssh -V` output read at startup, so the TUI
    /// never runs it again.
    pub(crate) fn seed_ssh_version(&self, version: &str) {
        let allowed = sk::ssh_version_allows_pins(version);
        if self.ssh_allows_security_key_pins.set(allowed).is_ok() {
            log::debug!("[purple] security key: PIN hand-out allowed={allowed} ssh={version}");
        }
    }

    /// The proven PINs a run without a prompt of its own may hand out.
    pub(crate) fn security_key_confirmed_run(&self) -> SecurityKeyRun {
        if self.security_key_pins.confirmed.is_empty() || !self.security_key_pins_allowed() {
            return SecurityKeyRun::default();
        }
        SecurityKeyRun {
            pins: self.security_key_pins.pins_for(self.env.paths(), None),
            flow: None,
        }
    }

    /// The PINs plus the flow for a run of `flow` on `alias`, the trial
    /// included when it belongs to this run.
    pub(crate) fn security_key_run_for(&self, alias: &str, flow: PinFlow) -> SecurityKeyRun {
        if !self.security_key_pins_allowed() {
            return SecurityKeyRun::default();
        }
        self.security_key_pins
            .run_for(self.env.paths(), alias, flow)
    }

    /// Settle what a finished run of `flow` on `alias` says about PINs. A
    /// trial is confirmed or dropped, a proven PIN the key refused is
    /// forgotten and a PIN the run asked for is reported. A request counts
    /// only when the login itself was refused; otherwise it goes, so a later
    /// unrelated failure never opens a PIN prompt.
    pub(crate) fn settle_security_key_run(
        &mut self,
        alias: &str,
        flow: PinFlow,
        end: RunEnd,
    ) -> PinOutcome {
        let paths = self.env.paths().cloned();
        let paths = paths.as_ref();
        let success = end == RunEnd::Connected;
        let handed_out = sk::take_handed_out_keys(paths, alias);
        let requested = sk::take_pin_request(paths, flow, alias);
        if let Some(trial) = self.security_key_pins.take_trial_for(alias, flow) {
            if let Some(reason) = sk::refusal_for(paths, &trial.key_path) {
                log::debug!(
                    "[purple] security key: trial PIN refused alias={alias} key={} reason={reason:?}",
                    trial.key_path
                );
                return PinOutcome::Rejected {
                    key_path: trial.key_path,
                    reason,
                    connected: success,
                };
            }
            if success && handed_out.contains(&trial.key_path) {
                log::debug!(
                    "[purple] security key: PIN confirmed alias={alias} key={}",
                    trial.key_path
                );
                self.security_key_pins
                    .confirmed
                    .insert(trial.key_path, trial.pin);
                return PinOutcome::NotInvolved;
            }
            log::debug!(
                "[purple] security key: trial PIN unproven, dropped alias={alias} key={}",
                trial.key_path
            );
        }
        let refused: Vec<(String, RefusalReason)> = self
            .security_key_pins
            .confirmed
            .keys()
            .filter_map(|k| sk::refusal_for(paths, k).map(|r| (k.clone(), r)))
            .collect();
        // Every refused PIN goes, but only one this alias handed out is this
        // run's to report. A container or snippet run elsewhere may have
        // written the refusal.
        let mut reported: Option<(String, RefusalReason)> = None;
        for (key, reason) in refused {
            self.security_key_pins.confirmed.remove(&key);
            if handed_out.contains(&key) {
                log::debug!("[purple] security key: refused PIN forgotten alias={alias} key={key}");
                reported.get_or_insert((key, reason));
            } else {
                log::debug!(
                    "[purple] security key: PIN refused by another run, forgotten key={key}"
                );
            }
        }
        if let Some((key_path, reason)) = reported {
            return PinOutcome::Rejected {
                key_path,
                reason,
                connected: success,
            };
        }
        if end == RunEnd::AuthRefused
            && let Some(key_path) = requested
        {
            return PinOutcome::Needed { key_path };
        }
        PinOutcome::NotInvolved
    }

    /// Drop the trial when the run that carries it was stopped and will never
    /// settle, so a new run asks for the PIN again instead of reusing it.
    pub(crate) fn abandon_security_key_trial(&mut self, flow: PinFlow, alias: Option<&str>) {
        if self.security_key_pins.drop_trial(flow, alias) {
            log::debug!(
                "[purple] security key: trial PIN dropped, run stopped flow={} alias={}",
                flow.as_str(),
                alias.unwrap_or("-")
            );
        }
    }

    /// Settle a tunnel trial once ssh says the target let the tunnel in. The
    /// tunnel stays up, so this cannot wait for an exit.
    pub(crate) fn settle_authenticated_tunnel_trial(&mut self) -> Option<(String, PinOutcome)> {
        let alias = self
            .security_key_pins
            .trial()
            .filter(|t| t.flow == PinFlow::Tunnel)?
            .alias
            .clone();
        let up = self
            .tunnels
            .active_get(&alias)
            .is_some_and(|t| t.live.authenticated.load(Ordering::Relaxed));
        if !up {
            return None;
        }
        let outcome = self.settle_security_key_run(&alias, PinFlow::Tunnel, RunEnd::Connected);
        Some((alias, outcome))
    }

    /// `key_path` the way the user would write it.
    pub(crate) fn security_key_label(&self, key_path: &str) -> String {
        match self.env.paths() {
            Some(paths) => paths.abbreviate_home(std::path::Path::new(key_path)),
            None => key_path.to_string(),
        }
    }

    /// True when a tunnel's PIN prompt may open: wherever any dialog may, and
    /// over the per-host tunnel list the start came from.
    pub(crate) fn can_open_tunnel_dialog(&self) -> bool {
        self.can_open_dialog()
            || (!self.dialog_open()
                && matches!(self.screen, Screen::TunnelList { .. })
                && self.tunnels.pending_delete().is_none())
    }

    /// Open the PIN prompt for `key_path` with `retry` queued behind it.
    /// `refused` names why the key turned the last PIN down, which the toast
    /// reports. `prefill` holds a PIN already proven for this key.
    pub(crate) fn open_security_key_pin_prompt(
        &mut self,
        alias: &str,
        key_path: &str,
        retry: PendingRetry,
        refused: Option<RefusalReason>,
        prefill: Option<String>,
    ) {
        log::debug!(
            "[purple] security key: PIN prompt open alias={alias} key={key_path} retry={} refused={refused:?}",
            retry.kind_label()
        );
        let mut state = super::PasswordPromptState::security_key_pin(
            alias.to_string(),
            key_path.to_string(),
            retry,
        );
        if let Screen::TunnelList { alias: list_alias } = &self.screen {
            state.return_to_tunnel_list = Some(list_alias.clone());
        }
        if let Some(pin) = prefill {
            state.input = pin;
        }
        self.password_prompt = Some(state);
        self.set_screen(Screen::PasswordPrompt);
        if let Some(reason) = refused {
            let label = self.security_key_label(key_path);
            self.notify_warning(crate::messages::askpass::pin_refused(&label, reason));
        }
    }
}

#[cfg(test)]
#[path = "security_key_pins_tests.rs"]
mod tests;
