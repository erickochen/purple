//! Key-push run accumulation. Each completed host pushes a `KeyPushResult`;
//! once `expected_count` has landed, `finalize_key_push` collapses them into
//! a single summary toast and refreshes the key list. Stale-run results
//! (after an abort) are dropped before they touch the accumulator.
//!
//! A host that ended in `Permission denied` on a password-taking server, or
//! that is not in `known_hosts` yet, is not a finished result: it joins the
//! prompt queue and the user answers one dialog per host, in picker order.

use crate::app::{App, KeyPushPrompt};

/// Handle `AppEvent::KeyPushResult`. Accumulates per-host outcomes and
/// fires the run-completion summary exactly once.
///
/// Events whose `run_id` no longer matches the current run are dropped
/// before they touch the accumulator: this happens when a worker that
/// was cancelled mid-batch sends its tail event after a new run has
/// already started. Without the guard the stale event would either
/// pollute the new run's tallies or trip `finalize` one event sooner
/// than the new run actually finished.
pub(crate) fn handle_key_push_result(
    app: &mut App,
    run_id: u64,
    result: crate::key_push::KeyPushResult,
) {
    if run_id != app.keys.push().run_id {
        log::debug!(
            "[purple] key_push: dropping stale result for alias={} (event run_id={} current={})",
            result.alias,
            run_id,
            app.keys.push().run_id
        );
        return;
    }
    let expected = app.keys.push().expected_count;
    if expected == 0 {
        // No run is in flight (cancel just zeroed expected_count); drop.
        return;
    }
    // Settle now: a long batch can outlast the PIN markers this host left.
    let pin = app.settle_security_key_run(
        &result.alias,
        crate::security_key::PinFlow::KeyPush,
        run_end(&result.outcome),
    );
    app.keys
        .push_mut()
        .pin_outcomes
        .insert(result.alias.clone(), pin);
    app.keys.push_mut().results.push(result);
    if app.keys.push().results.len() < expected {
        return;
    }
    finalize_key_push(app);
}

/// How a host's push ended, as far as its security key PIN is concerned.
fn run_end(outcome: &crate::key_push::KeyPushOutcome) -> crate::app::RunEnd {
    use crate::key_push::KeyPushOutcome;
    match outcome {
        KeyPushOutcome::Appended | KeyPushOutcome::AlreadyPresent => crate::app::RunEnd::Connected,
        KeyPushOutcome::NeedsPassword { .. } | KeyPushOutcome::AuthRefused(_) => {
            crate::app::RunEnd::AuthRefused
        }
        _ => crate::app::RunEnd::OtherFailure,
    }
}

/// Compute the summary toast / sticky overlay from the accumulated
/// `KeyPushResult` entries, queue the hosts that still need an answer, then
/// clear the run state. Called from `handle_key_push_result` once the
/// expected count is reached.
fn finalize_key_push(app: &mut App) {
    use crate::key_push::KeyPushOutcome;
    let mut appended = 0usize;
    let mut already = 0usize;
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut prompts: Vec<KeyPushPrompt> = Vec::new();

    let results = app.keys.push().results.clone();
    // Stamped onto every question this run leaves behind, so a later run
    // pushing a different key cannot answer for the wrong one.
    let run_key_path = app.keys.push().key_path.clone();
    let mut refused_but_connected: Vec<(String, String)> = Vec::new();
    for r in &results {
        // A security key PIN outranks whatever ssh printed last: the key
        // signs the login, so without the PIN nothing else can be answered.
        let pin = app
            .keys
            .push_mut()
            .pin_outcomes
            .remove(&r.alias)
            .unwrap_or(crate::app::PinOutcome::NotInvolved);
        match pin {
            crate::app::PinOutcome::Needed { key_path } => {
                prompts.push(KeyPushPrompt::SecurityKeyPin {
                    alias: r.alias.clone(),
                    key_path: run_key_path.clone(),
                    security_key: key_path,
                    refused: None,
                });
                continue;
            }
            crate::app::PinOutcome::Rejected {
                key_path,
                reason,
                connected: false,
            } => {
                prompts.push(KeyPushPrompt::SecurityKeyPin {
                    alias: r.alias.clone(),
                    key_path: run_key_path.clone(),
                    security_key: key_path,
                    refused: Some(reason),
                });
                continue;
            }
            crate::app::PinOutcome::Rejected {
                key_path,
                connected: true,
                ..
            } => refused_but_connected.push((r.alias.clone(), key_path)),
            _ => {}
        }
        match &r.outcome {
            KeyPushOutcome::Appended => appended += 1,
            KeyPushOutcome::AlreadyPresent => already += 1,
            KeyPushOutcome::Failed(msg) | KeyPushOutcome::AuthRefused(msg) => {
                failed.push((r.alias.clone(), msg.clone()))
            }
            KeyPushOutcome::UnknownHostKey { detail, host } => {
                if app.refusal_is_this_host(&r.alias, host.as_deref()) {
                    prompts.push(KeyPushPrompt::Trust {
                        alias: r.alias.clone(),
                        key_path: run_key_path.clone(),
                    });
                } else {
                    // A jump host on the way there is the one ssh does not
                    // know. Trusting it under the target's name would record
                    // the wrong key, so the hop is named instead.
                    log::debug!(
                        "[external] key_push: unknown host key came from a hop alias={} hop={:?} err={}",
                        r.alias,
                        host,
                        detail
                    );
                    failed.push((
                        r.alias.clone(),
                        crate::messages::host_key_trust::hop_is_unknown(
                            host.as_deref().unwrap_or(&r.alias),
                        ),
                    ));
                }
            }
            KeyPushOutcome::NeedsPassword { detail, host } => {
                let source = app.askpass_source_for(&r.alias);
                if !app.refusal_is_this_host(&r.alias, host.as_deref()) {
                    // A jump host refused, not the target. Asking under the
                    // target's name would store the bastion's password on
                    // the wrong host and leave the push failing anyway.
                    log::debug!(
                        "[external] key_push: a hop refused alias={} hop={:?} err={}",
                        r.alias,
                        host,
                        detail
                    );
                    failed.push((
                        r.alias.clone(),
                        crate::messages::askpass::hop_refused(host.as_deref().unwrap_or(&r.alias)),
                    ));
                } else if crate::askpass::take_withheld(app.env.paths(), &r.alias) {
                    // The server asked without naming itself on a connection
                    // that proxies, so the target cannot be told apart from
                    // the machine in front of it. A typed password would be
                    // held back for the same reason, so none is asked for.
                    log::debug!(
                        "[external] key_push: prompt named no host alias={} err={}",
                        r.alias,
                        detail
                    );
                    failed.push((
                        r.alias.clone(),
                        crate::messages::askpass::prompt_names_no_host(&r.alias),
                    ));
                } else if crate::app::source_allows_prompt(source.as_deref()) {
                    prompts.push(KeyPushPrompt::Password {
                        alias: r.alias.clone(),
                        key_path: run_key_path.clone(),
                    });
                } else {
                    // A configured vault or command answered and the server
                    // still said no, so the source itself did not deliver.
                    // Typing a password would neither unlock the vault nor
                    // correct the stored entry.
                    let label = crate::askpass::describe_source(source.as_deref().unwrap_or(""));
                    log::debug!(
                        "[external] key_push: source did not deliver alias={} source={} err={}",
                        r.alias,
                        label,
                        detail
                    );
                    failed.push((
                        r.alias.clone(),
                        crate::messages::askpass::source_did_not_deliver(&r.alias, label),
                    ));
                }
            }
        }
    }

    let done = appended + already;
    // Drop the "Pushing X to N hosts..." sticky progress before the
    // outcome toast lands; otherwise the footer would keep advertising
    // a push that already finished.
    app.status_center.clear_sticky_status();
    if !failed.is_empty() {
        if failed.len() == 1 && done == 0 && prompts.is_empty() {
            // One host, one reason. The reason itself says more than a
            // count plus a pointer at the log file.
            let (alias, why) = &failed[0];
            app.notify_sticky_error(crate::messages::key_push_single_failure(alias, why));
        } else if done == 0 && prompts.is_empty() {
            app.notify_sticky_error(crate::messages::key_push_all_failed(failed.len()));
        } else {
            // Partial-failure: name up to five failed aliases inline so the
            // user can act on the outcome without grepping the log file. The
            // toast goes sticky because the headline number alone hides which
            // hosts need follow-up.
            let mut body = crate::messages::key_push_partial_failure(done, failed.len());
            let preview: Vec<&str> = failed.iter().take(5).map(|(a, _)| a.as_str()).collect();
            if !preview.is_empty() {
                body.push_str(" Failed: ");
                body.push_str(&preview.join(", "));
                if failed.len() > preview.len() {
                    use std::fmt::Write;
                    let _ = write!(body, ", +{} more", failed.len() - preview.len());
                }
                body.push('.');
            }
            app.notify_sticky_error(body);
        }
    } else if !prompts.is_empty() {
        // The dialog usually says it too, but it waits while the user is in
        // a form, so the run always reports what it is waiting on.
        app.notify(crate::messages::key_push_pending_answers(
            done,
            prompts.len(),
        ));
    } else {
        app.notify(crate::messages::key_push_success(appended, already));
    }

    for (alias, msg) in &failed {
        // Remote failure is an external fault (the remote host's choice),
        // not a bug in purple. Tag it as such so log filters can split
        // [external] from [purple] like the rest of the codebase.
        log::warn!("[external] key_push: failed alias={} err={}", alias, msg);
    }
    for p in &prompts {
        log::debug!(
            "[purple] key_push: queued {} dialog for alias={}",
            match p {
                KeyPushPrompt::Trust { .. } => "trust",
                KeyPushPrompt::Password { .. } => "password",
                KeyPushPrompt::SecurityKeyPin { .. } => "security key PIN",
            },
            p.alias()
        );
    }
    for (alias, key_path) in &refused_but_connected {
        let label = app.security_key_label(key_path);
        app.notify_warning(crate::messages::askpass::pin_refused_but_connected(
            alias, &label,
        ));
    }

    // Refresh keys so linked_hosts picks up the newly-authorized aliases.
    // Honour the test override so suite runs never touch the real ~/.ssh.
    if appended > 0 {
        let ssh_dir = crate::ssh_keys::resolve_ssh_dir(app.env().paths());
        if let Some(dir) = ssh_dir {
            let keys =
                crate::ssh_keys::discover_keys(app.env().paths(), &dir, app.hosts_state.list());
            app.keys.set_list(keys);
            // Clamp the key-list cursor in case discover_keys returned a
            // shorter list (a key removed between push start and finalize
            // would otherwise leave the cursor pointing past the end).
            if let Some(sel) = app.keys.list_state().selected() {
                if app.keys.list().is_empty() {
                    app.keys.list_state_mut().select(None);
                } else if sel >= app.keys.list().len() {
                    let last = app.keys.list().len() - 1;
                    app.keys.list_state_mut().select(Some(last));
                }
            }
        }
    }

    // A terminal ssh run can leave a retry marker behind. This run is over,
    // so clear them and let the retry plus every later operation start
    // clean. The helper clears every marker in the state directory whatever
    // alias it is handed.
    crate::askpass::cleanup_marker(app.env.paths(), "");

    // Reset push state for the next run, then hand the first waiting host
    // its dialog. Queueing after `finish_run` matters: the retry it leads to
    // is refused while a run still looks in flight.
    app.keys.push_mut().finish_run();
    app.keys.push_mut().pending_prompts.extend(prompts);
    drain_next_key_push_prompt(app);
}

/// Open the dialog for the next host waiting on an answer. Returns true when
/// one opened. Nothing happens while a push is in flight, while another
/// dialog is waiting or while the user sits in a form: two questions are
/// never on screen together and neither takes the screen from typing. A
/// question whose key file disappeared meanwhile is dropped and the next
/// one is tried.
pub(crate) fn drain_next_key_push_prompt(app: &mut App) -> bool {
    if app.keys.push().expected_count > 0 {
        return false;
    }
    // A prompt whose screen moved on without it would read as a dialog that
    // is still waiting, and every later question would queue behind it.
    app.drop_stranded_password_prompt();
    // Another dialog is waiting, or the user is somewhere a question must
    // not appear, such as a form. The queue keeps its order and the tick
    // drains it once they are back on a page that can carry a dialog.
    if !app.can_open_dialog() {
        return false;
    }
    while let Some(prompt) = app.keys.push_mut().pending_prompts.pop_front() {
        let key_path = prompt.key_path().to_string();
        if !app.keys.list().iter().any(|k| k.display_path == key_path) {
            // Only this question falls: another host may still be waiting
            // on a key that is present.
            log::debug!(
                "[purple] key_push: dropping queued dialog for alias={}, key {} is gone",
                prompt.alias(),
                key_path
            );
            continue;
        }
        let alias = prompt.alias().to_string();
        let retry = crate::app::PendingRetry::KeyPush {
            key_path,
            alias: alias.clone(),
        };
        match prompt {
            KeyPushPrompt::Trust { .. } => app.open_host_key_trust(&alias, retry),
            KeyPushPrompt::Password { .. } => {
                let supplied = app.password_was_supplied_for(&alias);
                app.open_password_prompt(&alias, retry, supplied);
            }
            KeyPushPrompt::SecurityKeyPin {
                security_key,
                refused,
                ..
            } => {
                // An earlier host of this batch may have proven the PIN
                // meanwhile, so Enter alone runs this one again.
                let prefill = app
                    .security_key_pins
                    .confirmed_pin(&security_key)
                    .map(str::to_string);
                app.open_security_key_pin_prompt(&alias, &security_key, retry, refused, prefill);
            }
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::key_push::{KeyPushOutcome, KeyPushResult};
    use crate::ssh_config::model::SshConfigFile;

    fn make_app() -> App {
        make_app_with("")
    }

    /// An App whose SSH config is `config`. App::new auto-sandboxes the
    /// in-test env so it never touches the real ~/.purple/ or ~/.ssh/. The
    /// sandbox `.ssh` is created so finalize_key_push's `discover_keys`
    /// refresh runs against an empty dir and the appended>0 path stays
    /// exercisable.
    fn make_app_with(config: &str) -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(config),
            path: scratch.join("test_config"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        if let Some(ssh_dir) = crate::ssh_keys::resolve_ssh_dir(app.env().paths()) {
            std::fs::create_dir_all(&ssh_dir).unwrap();
        }
        // Tests assume a fresh run_id so they can fire results with run_id=1
        // without colliding with whatever default App::new set up.
        app.keys.push_mut().run_id = 1;
        app
    }

    /// An App with one host per alias plus a key in the list, so a queued
    /// dialog survives the "is the key still there" guard in the drain.
    fn app_with_hosts(config: &str) -> App {
        let mut app = make_app_with(config);
        app.keys.list_mut().push(crate::ssh_keys::SshKeyInfo {
            name: "id_test".into(),
            display_path: "~/.ssh/id_test".into(),
            key_type: "ED25519".into(),
            bits: "256".into(),
            fingerprint: String::new(),
            comment: String::new(),
            linked_hosts: vec![],
            bishop_art: String::new(),
            strength_score: 95,
            encrypted: false,
            agent_loaded: false,
            is_certificate: false,
            mtime_ts: None,
        });
        app
    }

    fn result(alias: &str, outcome: KeyPushOutcome) -> KeyPushResult {
        KeyPushResult {
            alias: alias.to_string(),
            outcome,
        }
    }

    /// A refusal ssh attributes to `host`. `None` stands for a refusal that
    /// names nobody.
    fn needs_password_from(host: Option<&str>) -> KeyPushOutcome {
        KeyPushOutcome::NeedsPassword {
            detail: "Permission denied (password).".into(),
            host: host.map(str::to_string),
        }
    }

    /// An unknown host key ssh attributes to `host`. `None` stands for one
    /// that names nobody.
    fn unknown_host_key_from(host: Option<&str>) -> KeyPushOutcome {
        KeyPushOutcome::UnknownHostKey {
            detail: "No ED25519 host key is known".into(),
            host: host.map(str::to_string),
        }
    }

    #[test]
    fn handle_result_does_not_finalize_below_expected() {
        let mut app = make_app();
        app.keys.push_mut().expected_count = 3;
        handle_key_push_result(&mut app, 1, result("h1", KeyPushOutcome::AlreadyPresent));
        assert_eq!(app.keys.push().results.len(), 1);
        assert_eq!(
            app.keys.push().expected_count,
            3,
            "should not finalize early"
        );
    }

    #[test]
    fn handle_result_skips_when_expected_zero() {
        // After a cancel the expected_count is zeroed; late-arriving
        // results from the worker must be dropped, not re-trigger the
        // finalize path.
        let mut app = make_app();
        app.keys.push_mut().expected_count = 0;
        handle_key_push_result(&mut app, 1, result("h1", KeyPushOutcome::Appended));
        assert!(app.keys.push().results.is_empty());
    }

    #[test]
    fn handle_result_drops_stale_run_id() {
        // A worker that was cancelled mid-batch can still emit results
        // tagged with the old run_id. After the user starts a new push,
        // run_id has been bumped: the stale events must not contaminate
        // the new run's accumulator.
        let mut app = make_app();
        app.keys.push_mut().expected_count = 2;
        app.keys.push_mut().run_id = 7;
        handle_key_push_result(&mut app, 6, result("h-stale", KeyPushOutcome::Appended));
        assert!(
            app.keys.push().results.is_empty(),
            "stale-run event must not push into the new run's results"
        );
    }

    #[test]
    fn finalize_all_already_present_emits_success_toast() {
        let mut app = make_app();
        app.keys.push_mut().expected_count = 2;
        app.keys
            .push_mut()
            .results
            .push(result("h1", KeyPushOutcome::AlreadyPresent));
        handle_key_push_result(&mut app, 1, result("h2", KeyPushOutcome::AlreadyPresent));
        // After finalize, accumulator state is cleared.
        assert_eq!(app.keys.push().expected_count, 0);
        assert!(app.keys.push().results.is_empty());
        assert!(app.keys.push().selected.is_empty());
        // Last status should be a non-sticky (toast) success.
        let toast = app.status_center.toast().expect("toast set");
        assert!(!toast.sticky, "fully-successful run is a plain toast");
    }

    #[test]
    fn finalize_all_failed_emits_sticky_error() {
        let mut app = make_app();
        app.keys.push_mut().expected_count = 2;
        app.keys
            .push_mut()
            .results
            .push(result("h1", KeyPushOutcome::Failed("oops".into())));
        handle_key_push_result(
            &mut app,
            1,
            result("h2", KeyPushOutcome::Failed("also bad".into())),
        );
        assert_eq!(app.keys.push().expected_count, 0);
        let status = app.status_center.status().expect("sticky status");
        assert!(
            status.sticky && status.is_error(),
            "all-failed should be sticky-error"
        );
    }

    #[test]
    fn finalize_partial_failure_is_sticky_and_names_failed_hosts() {
        let mut app = make_app();
        app.keys.push_mut().expected_count = 3;
        app.keys
            .push_mut()
            .results
            .push(result("h1", KeyPushOutcome::AlreadyPresent));
        app.keys
            .push_mut()
            .results
            .push(result("h2", KeyPushOutcome::Failed("bad".into())));
        handle_key_push_result(&mut app, 1, result("h3", KeyPushOutcome::AlreadyPresent));
        assert_eq!(app.keys.push().expected_count, 0);
        let status = app.status_center.status().expect("sticky status set");
        assert!(
            status.sticky && status.is_error(),
            "partial failure is sticky so the user sees which hosts failed"
        );
        assert!(
            status.text.contains("h2"),
            "failed alias must appear in body: {}",
            status.text
        );
    }

    // --- prompt queue: a host that needs an answer is not a finished result

    /// Land `results` as one complete run of `results.len()` hosts.
    fn run_with(app: &mut App, results: Vec<KeyPushResult>) {
        app.keys.push_mut().expected_count = results.len();
        app.keys.push_mut().key_path = "~/.ssh/id_test".to_string();
        for r in results {
            handle_key_push_result(app, 1, r);
        }
    }

    const SK: &str = "/home/u/.ssh/id_sk";

    fn arm_pin(app: &App, alias: &str) {
        crate::security_key::arm_pin_request(
            app.env().paths(),
            crate::security_key::PinFlow::KeyPush,
            alias,
            SK,
        );
    }

    #[test]
    fn a_host_that_wanted_a_pin_gets_the_pin_prompt_not_the_password_prompt() {
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\n");
        arm_pin(&app, "h1");
        run_with(&mut app, vec![result("h1", needs_password_from(None))]);
        assert_eq!(app.screen, crate::app::Screen::PasswordPrompt);
        let state = app.password_prompt.as_ref().expect("prompt");
        assert_eq!(state.security_key.as_deref(), Some(SK));
        assert_eq!(
            state.retry,
            crate::app::PendingRetry::KeyPush {
                key_path: "~/.ssh/id_test".into(),
                alias: "h1".into(),
            }
        );
    }

    #[test]
    fn a_pin_request_is_settled_when_its_host_lands_not_at_the_end_of_the_batch() {
        // The request marker ages out, and a batch with slow hosts behind
        // this one can take longer than that.
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\nHost h2\n  HostName 2.2.2.2\n");
        app.keys.push_mut().expected_count = 2;
        app.keys.push_mut().key_path = "~/.ssh/id_test".to_string();
        arm_pin(&app, "h1");
        handle_key_push_result(&mut app, 1, result("h1", needs_password_from(None)));
        assert_eq!(
            app.keys.push().pin_outcomes.get("h1"),
            Some(&crate::app::PinOutcome::Needed {
                key_path: SK.into()
            })
        );
        assert_eq!(
            crate::security_key::take_pin_request(
                app.env().paths(),
                crate::security_key::PinFlow::KeyPush,
                "h1"
            ),
            None,
            "the marker was read on arrival"
        );
        handle_key_push_result(&mut app, 1, result("h2", KeyPushOutcome::AlreadyPresent));
        let state = app.password_prompt.as_ref().expect("prompt for h1");
        assert_eq!(state.alias, "h1");
        assert_eq!(state.security_key.as_deref(), Some(SK));
        assert!(app.keys.push().pin_outcomes.is_empty());
    }

    #[test]
    fn a_pin_proven_earlier_in_the_batch_is_offered_in_the_next_prompt() {
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\nHost h2\n  HostName 2.2.2.2\n");
        arm_pin(&app, "h2");
        // h1 proves the PIN on its retry while h2 still waits in the queue.
        app.security_key_pins.start_trial(crate::app::PinTrial {
            alias: "h1".into(),
            key_path: SK.into(),
            pin: "4242".into(),
            flow: crate::security_key::PinFlow::KeyPush,
        });
        crate::security_key::record_hand_out(app.env().paths(), "h1", 1, SK);
        // AlreadyPresent rather than Appended: an append rebuilds the key
        // list from the empty sandbox and the queued question would go.
        run_with(
            &mut app,
            vec![
                result("h1", KeyPushOutcome::AlreadyPresent),
                result(
                    "h2",
                    KeyPushOutcome::AuthRefused("u@h: Permission denied (publickey).".into()),
                ),
            ],
        );
        assert_eq!(app.security_key_pins.confirmed_pin(SK), Some("4242"));
        let state = app.password_prompt.as_ref().expect("prompt for h2");
        assert_eq!(state.alias, "h2");
        assert_eq!(state.input, "4242", "Enter alone runs h2 again");
    }

    #[test]
    fn a_pin_the_key_refused_reopens_the_prompt_with_the_reason() {
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\n");
        app.security_key_pins.start_trial(crate::app::PinTrial {
            alias: "h1".into(),
            key_path: SK.into(),
            pin: "0000".into(),
            flow: crate::security_key::PinFlow::KeyPush,
        });
        crate::security_key::note_refusal(
            app.env().paths(),
            &crate::security_key::SignFailure {
                key_path: SK.into(),
                reason: crate::security_key::RefusalReason::WrongPin,
            },
        );
        run_with(
            &mut app,
            vec![result(
                "h1",
                KeyPushOutcome::AuthRefused("u@h: Permission denied (publickey).".into()),
            )],
        );
        let state = app.password_prompt.as_ref().expect("prompt");
        assert_eq!(state.security_key.as_deref(), Some(SK));
        assert!(
            state.input.is_empty(),
            "a refused PIN is never offered again"
        );
    }

    #[test]
    fn a_password_failure_opens_the_prompt_instead_of_reporting_a_failure() {
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\n");
        run_with(&mut app, vec![result("h1", needs_password_from(None))]);
        assert_eq!(app.screen, crate::app::Screen::PasswordPrompt);
        assert_eq!(
            app.password_prompt.as_ref().map(|s| s.alias.as_str()),
            Some("h1")
        );
        // The run says what it is waiting on even when the dialog is up,
        // because the dialog holds back while the user is in a form.
        let said = app
            .status_center
            .toast()
            .map(|t| t.text.clone())
            .or_else(|| app.status_center.status().map(|s| s.text.clone()))
            .expect("the run reports what it waits on");
        assert!(said.contains("1 host(s) need an answer"), "got: {said}");
    }

    #[test]
    fn an_unknown_host_key_opens_the_trust_dialog() {
        let mut app = app_with_hosts("Host h1\n  HostName db.example.com\n");
        run_with(&mut app, vec![result("h1", unknown_host_key_from(None))]);
        match &app.screen {
            crate::app::Screen::ConfirmHostKeyTrust {
                alias, hostname, ..
            } => {
                assert_eq!(alias, "h1");
                assert_eq!(hostname, "db.example.com");
            }
            other => panic!("expected the trust dialog, got {other:?}"),
        }
    }

    #[test]
    fn a_prompt_that_named_no_host_asks_for_nothing_and_explains() {
        // Behind a bastion, a server asking without naming itself cannot be
        // told apart from the bastion asking. The background run left a
        // marker saying so, and a password typed in a dialog would be held
        // back for the same reason, so no dialog opens.
        let mut app = app_with_hosts(
            "Host bast\n  HostName bastion.example.com\n\
             Host h1\n  HostName db.example.com\n  ProxyJump bast\n",
        );
        let marker = app
            .env()
            .paths()
            .expect("sandboxed paths")
            .askpass_withheld_marker("h1");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, b"").unwrap();

        // ssh names the target on its failure line, so without the marker
        // this is the shape that opens a dialog.
        run_with(
            &mut app,
            vec![result("h1", needs_password_from(Some("db.example.com")))],
        );
        assert!(
            app.password_prompt.is_none(),
            "a dialog here would ask for something that cannot be used"
        );
        assert!(
            !marker.exists(),
            "reading takes the marker, so it cannot suppress a later dialog"
        );
        let said = app
            .status_center
            .status()
            .map(|s| s.text.clone())
            .or_else(|| app.status_center.toast().map(|t| t.text.clone()))
            .expect("the reason is reported");
        assert!(said.contains("h1"), "got: {said}");
    }

    #[test]
    fn a_jump_host_refusal_does_not_ask_for_the_targets_password() {
        // ssh authenticates each hop, and the bastion is the one that said
        // no. A prompt under the target's name would store the bastion's
        // password on the wrong host and the push would still fail.
        let mut app = app_with_hosts(
            "Host bast\n  HostName bastion.example.com\n\
             Host h1\n  HostName db.example.com\n  ProxyJump bast\n",
        );
        run_with(
            &mut app,
            vec![result(
                "h1",
                needs_password_from(Some("bastion.example.com")),
            )],
        );
        assert_eq!(app.screen, crate::app::Screen::HostList);
        assert!(app.password_prompt.is_none(), "no dialog for the target");
        let said = app
            .status_center
            .status()
            .map(|s| s.text.clone())
            .or_else(|| app.status_center.toast().map(|t| t.text.clone()))
            .expect("the failure is reported");
        assert!(said.contains("h1"), "got: {said}");
    }

    #[test]
    fn the_targets_own_refusal_behind_a_jump_host_still_asks() {
        // Same chain, and this time the target itself is the one asking.
        let mut app = app_with_hosts(
            "Host bast\n  HostName bastion.example.com\n\
             Host h1\n  HostName db.example.com\n  ProxyJump bast\n",
        );
        run_with(
            &mut app,
            vec![result("h1", needs_password_from(Some("db.example.com")))],
        );
        assert_eq!(app.screen, crate::app::Screen::PasswordPrompt);
        assert_eq!(
            app.password_prompt.as_ref().map(|s| s.alias.as_str()),
            Some("h1")
        );
    }

    #[test]
    fn a_refusal_naming_nobody_still_asks_on_a_host_with_no_jump() {
        // Without a ProxyJump there is only one host that could have
        // refused, so the name ssh printed does not have to be read.
        let mut app = app_with_hosts("Host h1\n  HostName db.example.com\n");
        run_with(&mut app, vec![result("h1", needs_password_from(None))]);
        assert_eq!(app.screen, crate::app::Screen::PasswordPrompt);
    }

    #[test]
    fn an_unknown_jump_host_key_is_not_trusted_under_the_targets_name() {
        // Trusting here would record the bastion's key against the target,
        // so purple names the hop and leaves the choice there.
        let mut app = app_with_hosts(
            "Host bast\n  HostName bastion.example.com\n\
             Host h1\n  HostName db.example.com\n  ProxyJump bast\n",
        );
        run_with(
            &mut app,
            vec![result(
                "h1",
                unknown_host_key_from(Some("bastion.example.com")),
            )],
        );
        assert_eq!(app.screen, crate::app::Screen::HostList);
        let said = app
            .status_center
            .status()
            .map(|s| s.text.clone())
            .or_else(|| app.status_center.toast().map(|t| t.text.clone()))
            .expect("the failure is reported");
        assert!(said.contains("h1"), "got: {said}");
    }

    #[test]
    fn a_bracketed_host_and_port_still_matches_the_target() {
        // ssh writes `[host]:port` when the port is not 22. The bracketed
        // form must still read as the target rather than as a hop.
        let mut app = app_with_hosts(
            "Host bast\n  HostName bastion.example.com\n\
             Host h1\n  HostName db.example.com\n  Port 2222\n  ProxyJump bast\n",
        );
        run_with(
            &mut app,
            vec![result(
                "h1",
                unknown_host_key_from(Some("[db.example.com]:2222")),
            )],
        );
        assert!(
            matches!(app.screen, crate::app::Screen::ConfirmHostKeyTrust { .. }),
            "got: {:?}",
            app.screen
        );
    }

    #[test]
    fn a_dialog_waits_while_the_user_is_in_a_form() {
        // Taking the screen from a form would send the keys they are still
        // typing into the password field, and Enter would submit them.
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\n");
        app.set_screen(crate::app::Screen::AddHost);
        run_with(&mut app, vec![result("h1", needs_password_from(None))]);
        assert_eq!(app.screen, crate::app::Screen::AddHost, "the form stands");
        assert!(app.password_prompt.is_none());
        assert_eq!(
            app.keys.push().pending_prompts.len(),
            1,
            "the host keeps its turn"
        );
        // Back on a page that can carry a dialog, the question opens.
        app.set_screen(crate::app::Screen::HostList);
        assert!(drain_next_key_push_prompt(&mut app));
        assert_eq!(app.screen, crate::app::Screen::PasswordPrompt);
    }

    #[test]
    fn a_configured_source_that_did_not_deliver_gets_no_prompt() {
        // Bitwarden answered and the server still said no, so a typed
        // password would not unlock the vault or fix the stored entry.
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\n  # purple:askpass bw:item\n");
        run_with(&mut app, vec![result("h1", needs_password_from(None))]);
        assert_eq!(app.screen, crate::app::Screen::HostList);
        assert!(app.password_prompt.is_none());
        let status = app.status_center.status().expect("sticky failure");
        assert!(status.is_error(), "got: {}", status.text);
    }

    #[test]
    fn a_keychain_source_still_gets_the_prompt() {
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\n  # purple:askpass keychain\n");
        run_with(&mut app, vec![result("h1", needs_password_from(None))]);
        assert_eq!(app.screen, crate::app::Screen::PasswordPrompt);
        assert!(
            app.password_prompt.as_ref().unwrap().source_is_keychain,
            "a keychain host needs no second config write on submit"
        );
    }

    #[test]
    fn hosts_that_landed_are_reported_before_the_dialog_opens() {
        // `AlreadyPresent` rather than `Appended`: an append rescans the ssh
        // directory, and the sandbox holds no key file, so the seeded key
        // would vanish from the list for reasons that have nothing to do
        // with what this test is about.
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\nHost h2\n  HostName 2.2.2.2\n");
        run_with(
            &mut app,
            vec![
                result("h1", KeyPushOutcome::AlreadyPresent),
                result("h2", needs_password_from(None)),
            ],
        );
        let toast = app.status_center.toast().expect("summary toast");
        assert!(toast.text.contains('1'), "got: {}", toast.text);
        assert_eq!(app.screen, crate::app::Screen::PasswordPrompt);
    }

    #[test]
    fn queued_hosts_are_answered_one_at_a_time_in_run_order() {
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\nHost h2\n  HostName 2.2.2.2\n");
        run_with(
            &mut app,
            vec![
                result("h1", needs_password_from(None)),
                result("h2", needs_password_from(None)),
            ],
        );
        assert_eq!(
            app.password_prompt.as_ref().map(|s| s.alias.as_str()),
            Some("h1"),
            "the first host of the run asks first"
        );
        assert_eq!(app.keys.push().pending_prompts.len(), 1, "h2 still waits");
        // Answering h1 (here: dismissing it) hands the dialog to h2. The
        // screen has to come back too: while a dialog is still on screen the
        // drain deliberately holds the queue.
        app.password_prompt = None;
        app.screen = crate::app::Screen::HostList;
        assert!(drain_next_key_push_prompt(&mut app));
        assert_eq!(
            app.password_prompt.as_ref().map(|s| s.alias.as_str()),
            Some("h2")
        );
        assert!(app.keys.push().pending_prompts.is_empty());
        assert!(!drain_next_key_push_prompt(&mut app), "queue is empty");
    }

    #[test]
    fn no_dialog_opens_over_another_one() {
        // A file-browser prompt is already waiting. Opening the key-push
        // dialog over it would drop the retry it holds.
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\n");
        app.keys.push_mut().key_path = "~/.ssh/id_test".to_string();
        app.open_password_prompt(
            "h1",
            crate::app::PendingRetry::FileBrowserListing {
                alias: "h1".into(),
                path: "/".into(),
            },
            false,
        );
        app.keys
            .push_mut()
            .pending_prompts
            .push_back(crate::app::KeyPushPrompt::Password {
                alias: "h1".into(),
                key_path: "~/.ssh/id_test".into(),
            });
        assert!(!drain_next_key_push_prompt(&mut app));
        assert_eq!(app.keys.push().pending_prompts.len(), 1, "the host waits");
        assert!(matches!(
            app.password_prompt.as_ref().map(|s| s.retry.clone()),
            Some(crate::app::PendingRetry::FileBrowserListing { .. })
        ));
    }

    #[test]
    fn no_dialog_opens_while_a_run_is_in_flight() {
        // A retry for one host must not be interrupted by the next host's
        // dialog; that one opens when the retry finalizes.
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\n");
        app.keys
            .push_mut()
            .pending_prompts
            .push_back(crate::app::KeyPushPrompt::Password {
                alias: "h1".into(),
                key_path: "~/.ssh/id_test".into(),
            });
        app.keys.push_mut().expected_count = 1;
        assert!(!drain_next_key_push_prompt(&mut app));
        assert_eq!(app.keys.push().pending_prompts.len(), 1);
    }

    #[test]
    fn a_vanished_key_drops_only_its_own_question() {
        // Two hosts wait. Each holds a different key and one of those keys
        // is gone. Dropping the whole queue would silently lose the host
        // whose key is still there.
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\nHost h2\n  HostName 2.2.2.2\n");
        app.keys
            .push_mut()
            .pending_prompts
            .push_back(crate::app::KeyPushPrompt::Password {
                alias: "h1".into(),
                key_path: "~/.ssh/id_gone".into(),
            });
        app.keys
            .push_mut()
            .pending_prompts
            .push_back(crate::app::KeyPushPrompt::Password {
                alias: "h2".into(),
                key_path: "~/.ssh/id_test".into(),
            });
        assert!(drain_next_key_push_prompt(&mut app), "h2 keeps its turn");
        assert_eq!(
            app.password_prompt.as_ref().map(|s| s.alias.as_str()),
            Some("h2")
        );
        assert!(app.keys.push().pending_prompts.is_empty());
    }

    #[test]
    fn a_queued_question_is_answered_for_the_key_it_was_queued_for() {
        // A later run overwrites the state's key_path. A question still
        // waiting from an earlier run must not be answered for that new key.
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\n");
        app.keys.list_mut().push(crate::ssh_keys::SshKeyInfo {
            name: "id_other".into(),
            display_path: "~/.ssh/id_other".into(),
            key_type: "ED25519".into(),
            bits: "256".into(),
            fingerprint: String::new(),
            comment: String::new(),
            linked_hosts: vec![],
            bishop_art: String::new(),
            strength_score: 95,
            encrypted: false,
            agent_loaded: false,
            is_certificate: false,
            mtime_ts: None,
        });
        app.keys
            .push_mut()
            .pending_prompts
            .push_back(crate::app::KeyPushPrompt::Password {
                alias: "h1".into(),
                key_path: "~/.ssh/id_test".into(),
            });
        // The state now points at the other key, as a later run would leave it.
        app.keys.push_mut().key_path = "~/.ssh/id_other".to_string();
        assert!(drain_next_key_push_prompt(&mut app));
        let retry = app.password_prompt.as_ref().map(|s| s.retry.clone());
        assert!(
            matches!(
                retry,
                Some(crate::app::PendingRetry::KeyPush { ref key_path, .. })
                    if key_path == "~/.ssh/id_test"
            ),
            "got: {retry:?}"
        );
    }

    #[test]
    fn a_changed_host_key_stays_a_plain_failure() {
        // A changed key is a security event, so it keeps the failure toast
        // and never gets an automatic retry.
        let mut app = app_with_hosts("Host h1\n  HostName 1.1.1.1\n");
        run_with(
            &mut app,
            vec![result(
                "h1",
                KeyPushOutcome::Failed("Host key verification failed.".into()),
            )],
        );
        assert_eq!(app.screen, crate::app::Screen::HostList);
        assert!(app.keys.push().pending_prompts.is_empty());
        assert!(app.status_center.status().expect("sticky").is_error());
    }

    #[test]
    fn finalize_appended_refreshes_keys_against_override_dir_not_real_home() {
        // Regression guard for the host-sensitive finalize branch. The
        // override directory exists but is empty, so the refresh yields
        // zero keys without touching the test runner's actual ~/.ssh.
        let mut app = make_app();
        app.keys.push_mut().expected_count = 1;
        // Pre-seed a stale key entry to prove the refresh ran.
        app.keys.list_mut().push(crate::ssh_keys::SshKeyInfo {
            name: "stale".into(),
            display_path: "~/.ssh/stale".into(),
            key_type: "ED25519".into(),
            bits: "256".into(),
            fingerprint: String::new(),
            comment: String::new(),
            linked_hosts: vec![],
            bishop_art: String::new(),
            strength_score: 90,
            encrypted: false,
            agent_loaded: false,
            is_certificate: false,
            mtime_ts: None,
        });
        handle_key_push_result(&mut app, 1, result("h", KeyPushOutcome::Appended));
        assert!(
            app.keys.list().is_empty(),
            "discover_keys against an empty override dir should return zero keys"
        );
        assert_eq!(app.keys.list_state().selected(), None);
    }
}
