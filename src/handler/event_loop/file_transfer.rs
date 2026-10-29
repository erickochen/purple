//! File browser listing and SCP transfer completion events. Updates the
//! `app.file_browser_session` overlay, forces a terminal redraw (ssh may
//! have written to /dev/tty) and respawns the remote listing thread once
//! a successful transfer lands.

use std::sync::mpsc;

use crate::app::App;
use crate::event::AppEvent;
use crate::file_browser;
use crate::tui;

/// What a failed listing needs from the user before it can run again. Each
/// carries the host ssh named, so a jump host that refused on the way is
/// not mistaken for the target.
enum ListingBlocked {
    /// The host is not in `known_hosts` yet.
    Trust(Option<String>),
    /// The server takes a password and ssh had none to give.
    Password(Option<String>),
    /// A security key wants a PIN. `refused` says why the key turned the
    /// last one down, if it did. The key belongs to whichever hop asked, so
    /// no host name is needed.
    SecurityKeyPin {
        key_path: String,
        refused: Option<crate::security_key::RefusalReason>,
    },
}

/// Handle `AppEvent::FileBrowserListing`.
pub(crate) fn handle_file_browser_listing(
    app: &mut App,
    alias: String,
    path: String,
    entries: Result<Vec<crate::file_browser::FileEntry>, String>,
    terminal: &mut tui::Tui,
) {
    apply_file_browser_listing(app, alias, path, entries);
    // Force full redraw: ssh may have written to /dev/tty
    terminal.force_redraw();
}

/// Everything the listing event does to `App`. Split from the terminal
/// redraw so the state transitions are testable without a live terminal.
fn apply_file_browser_listing(
    app: &mut App,
    alias: String,
    path: String,
    entries: Result<Vec<crate::file_browser::FileEntry>, String>,
) {
    let mut record_connection = false;
    let mut blocked: Option<ListingBlocked> = None;
    let pin_outcome = app.settle_security_key_run(
        &alias,
        crate::security_key::PinFlow::FileBrowser,
        crate::app::RunEnd::from_run(
            entries.is_ok(),
            entries.as_ref().err().map_or("", String::as_str),
        ),
    );
    let pin_label = match &pin_outcome {
        crate::app::PinOutcome::Needed { key_path }
        | crate::app::PinOutcome::Rejected { key_path, .. } => {
            Some(app.security_key_label(key_path))
        }
        crate::app::PinOutcome::NotInvolved => None,
    };
    if let Some(ref mut fb) = app.file_browser_session
        && fb.alias == alias
    {
        fb.remote_loading = false;
        match entries {
            Ok(listing) => {
                if !fb.connection_recorded {
                    fb.connection_recorded = true;
                    record_connection = true;
                }
                if fb.remote_path.is_empty() || fb.remote_path != path {
                    fb.remote_path = path.clone();
                }
                fb.remote_entries = listing;
                fb.remote_error = None;
                fb.remote_list_state = ratatui::widgets::ListState::default();
                fb.remote_list_state.select(Some(0));
            }
            Err(msg) => {
                if fb.remote_path.is_empty() {
                    fb.remote_path = path.clone();
                }
                fb.remote_entries.clear();
                // A background ssh has no tty, so the questions it cannot
                // ask come back as an error. Ask them in the TUI and run the
                // listing again with the answer. The hop that refused is
                // named on the line, and only the target's own refusal is
                // ours to answer. A security key PIN goes first: the key is
                // the user's own whichever hop asked for it.
                let pin = match &pin_outcome {
                    crate::app::PinOutcome::Needed { key_path } => Some((key_path.clone(), None)),
                    crate::app::PinOutcome::Rejected {
                        key_path, reason, ..
                    } => Some((key_path.clone(), Some(*reason))),
                    crate::app::PinOutcome::NotInvolved => None,
                };
                if let Some((key_path, refused)) = pin {
                    let label = pin_label.as_deref().unwrap_or(&key_path);
                    fb.remote_error =
                        Some(crate::messages::askpass::pin_needed_listing(&alias, label));
                    blocked = Some(ListingBlocked::SecurityKeyPin { key_path, refused });
                } else {
                    if crate::connection::is_unknown_host_key(&msg) {
                        blocked = Some(ListingBlocked::Trust(
                            crate::connection::unknown_host_key_host(&msg).map(str::to_string),
                        ));
                    } else if crate::connection::needs_password(&msg) {
                        blocked = Some(ListingBlocked::Password(
                            crate::connection::denied_host(&msg).map(str::to_string),
                        ));
                    }
                    fb.remote_error = Some(msg);
                }
            }
        }
    }
    // The key refused the PIN while the host let the listing in another way:
    // nothing to retry, but the PIN is gone and the user should know.
    if let crate::app::PinOutcome::Rejected {
        connected: true,
        key_path,
        ..
    } = &pin_outcome
    {
        let label = pin_label.as_deref().unwrap_or(key_path);
        app.notify_warning(crate::messages::askpass::pin_refused_but_connected(
            &alias, label,
        ));
    }
    if record_connection {
        app.history.record(&alias);
        app.record_key_use(&alias, crate::key_activity::now_secs());
        app.apply_sort();
    }
    if let Some(kind) = blocked {
        open_listing_dialog(app, &alias, path, kind);
    }
    // The run is over, so the askpass retry markers it left are stale. The
    // helper clears every marker in the state directory.
    crate::askpass::cleanup_marker(app.env.paths(), &alias);
}

/// Ask the user what the listing needs. A password source that is not the
/// OS keychain gets no prompt: it answered and the server still said no, so
/// the source itself did not deliver and the pane says which one.
fn open_listing_dialog(app: &mut App, alias: &str, path: String, kind: ListingBlocked) {
    // A key-push dialog may already be waiting, or the user may have moved
    // into a form. Opening over either would drop a retry or steal typing,
    // and the pane keeps the reason anyway, so the user can ask again with
    // `R` once they are back.
    if !app.can_open_dialog() {
        log::debug!("[purple] file_browser: cannot ask right now for alias={alias}");
        return;
    }
    let retry_for = |path: String| crate::app::PendingRetry::FileBrowserListing {
        alias: alias.to_string(),
        path,
    };
    // A PIN belongs to the user's own key, whichever hop asked for it.
    if let ListingBlocked::SecurityKeyPin { key_path, refused } = kind {
        app.open_security_key_pin_prompt(alias, &key_path, retry_for(path), refused, None);
        return;
    }
    // Whichever hop ssh named has to be this host. A jump host that refused
    // on the way is a different question, and the pane already carries its
    // reason, so the dialog stays shut.
    let named = match &kind {
        ListingBlocked::Trust(host) | ListingBlocked::Password(host) => host.clone(),
        ListingBlocked::SecurityKeyPin { .. } => None,
    };
    if !app.refusal_is_this_host(alias, named.as_deref()) {
        log::debug!(
            "[external] file_browser: a hop refused alias={alias} hop={:?}",
            named
        );
        return;
    }
    let retry = retry_for(path);
    match kind {
        // Answered above, before the hop check.
        ListingBlocked::SecurityKeyPin { .. } => {}
        ListingBlocked::Trust(_) => app.open_host_key_trust(alias, retry),
        ListingBlocked::Password(_) => {
            if crate::askpass::take_withheld(app.env.paths(), alias) {
                // The server asked without naming itself on a connection
                // that proxies. A typed password would be held back for the
                // same reason, so none is asked for.
                log::debug!("[external] file_browser: prompt named no host alias={alias}");
                if let Some(fb) = app.file_browser_session.as_mut()
                    && fb.alias == alias
                {
                    fb.remote_error = Some(crate::messages::askpass::prompt_names_no_host(alias));
                }
                return;
            }
            let source = app.askpass_source_for(alias);
            if crate::app::source_allows_prompt(source.as_deref()) {
                let supplied = app.password_was_supplied_for(alias);
                app.open_password_prompt(alias, retry, supplied);
                return;
            }
            let label = crate::askpass::describe_source(source.as_deref().unwrap_or(""));
            log::debug!(
                "[external] file_browser: source did not deliver alias={alias} source={label}"
            );
            if let Some(fb) = app.file_browser_session.as_mut()
                && fb.alias == alias
            {
                fb.remote_error = Some(crate::messages::askpass::source_did_not_deliver(
                    alias, label,
                ));
            }
        }
    }
}

/// Handle `AppEvent::ScpComplete`.
pub(crate) fn handle_scp_complete(
    app: &mut App,
    alias: String,
    success: bool,
    message: String,
    events_tx: &mpsc::Sender<AppEvent>,
    terminal: &mut tui::Tui,
) {
    apply_scp_complete(app, alias, success, message, events_tx);
    // Force full redraw: ssh may have written to /dev/tty
    terminal.force_redraw();
}

/// Everything the transfer event does to `App`. Split from the terminal
/// redraw so the state transitions are testable without a live terminal.
fn apply_scp_complete(
    app: &mut App,
    alias: String,
    success: bool,
    message: String,
    events_tx: &mpsc::Sender<AppEvent>,
) {
    // A transfer has no retry of its own, so a PIN it wanted is explained in
    // the transfer error and entered through the listing.
    let pin_outcome = app.settle_security_key_run(
        &alias,
        crate::security_key::PinFlow::FileBrowser,
        crate::app::RunEnd::from_run(success, &message),
    );
    let pin_message = match &pin_outcome {
        crate::app::PinOutcome::Needed { key_path } => {
            Some(crate::messages::askpass::pin_needed_transfer(
                &alias,
                &app.security_key_label(key_path),
            ))
        }
        crate::app::PinOutcome::Rejected {
            key_path,
            reason,
            connected: false,
        } => Some(crate::messages::askpass::pin_refused(
            &app.security_key_label(key_path),
            *reason,
        )),
        _ => None,
    };
    if let crate::app::PinOutcome::Rejected {
        key_path,
        connected: true,
        ..
    } = &pin_outcome
    {
        let label = app.security_key_label(key_path);
        app.notify_warning(crate::messages::askpass::pin_refused_but_connected(
            &alias, &label,
        ));
    }
    // Track whether we need to spawn a remote refresh (can't do it inside the fb borrow
    // because spawn_remote_listing needs values from app too)
    let mut refresh_remote: Option<(
        String,
        Option<String>,
        String,
        bool,
        file_browser::BrowserSort,
    )> = None;
    let matched = if let Some(ref mut fb) = app.file_browser_session {
        if fb.alias == alias {
            fb.transferring = None;
            if success {
                app.history.record(&alias);
                // Field-disjoint helper: fb already holds &mut app.file_browser_session,
                // so the `App::record_key_use` method would not borrow-check.
                crate::key_activity::record_and_flush(
                    app.keys.activity_mut(),
                    &alias,
                    crate::key_activity::now_secs(),
                    app.env.paths().cloned().as_ref(),
                );
                // history_width depends on formatted timestamps; rebuild next render
                app.hosts_state.invalidate_render_cache();
                fb.local_selected.clear();
                fb.remote_selected.clear();
                match file_browser::list_local(&fb.local_path, fb.show_hidden, fb.sort) {
                    Ok(entries) => {
                        fb.local_entries = entries;
                        fb.local_error = None;
                    }
                    Err(e) => {
                        fb.local_entries = Vec::new();
                        fb.local_error = Some(e.to_string());
                    }
                }
                fb.local_list_state.select(Some(0));
                if !fb.remote_path.is_empty() {
                    fb.remote_loading = true;
                    fb.remote_entries.clear();
                    fb.remote_error = None;
                    fb.remote_list_state = ratatui::widgets::ListState::default();
                    refresh_remote = Some((
                        fb.alias.clone(),
                        fb.askpass.clone(),
                        fb.remote_path.clone(),
                        fb.show_hidden,
                        fb.sort,
                    ));
                }
            } else {
                fb.transfer_error = Some(pin_message.unwrap_or_else(|| message.clone()));
            }
            true
        } else {
            false
        }
    } else {
        false
    };
    if matched && success {
        app.notify_background(crate::i18n::text(crate::messages::TRANSFER_COMPLETE));
        // Rebuild display list so frecency sort and LAST column reflect the transfer
        app.apply_sort();
    }
    if let Some((fb_alias, askpass_fb, path, show_hidden, sort)) = refresh_remote {
        let ctx = app.file_browser_ssh_context(fb_alias, askpass_fb);
        let tx = events_tx.clone();
        file_browser::spawn_remote_listing(ctx, path, show_hidden, sort, move |a, p, e| {
            let _ = tx.send(AppEvent::FileBrowserListing {
                alias: a,
                path: p,
                entries: e,
            });
        });
    }
    crate::askpass::cleanup_marker(app.env.paths(), &alias);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{PendingRetry, Screen};
    use crate::ssh_config::model::SshConfigFile;

    fn app_with(config: &str) -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let cfg = SshConfigFile {
            elements: SshConfigFile::parse_content(config),
            path: scratch.join("config"),
            crlf: false,
            bom: false,
        };
        App::new(cfg)
    }

    fn open_browser(app: &mut App, alias: &str, remote_path: &str) {
        app.file_browser_session = Some(crate::file_browser::FileBrowserSession {
            alias: alias.to_string(),
            askpass: None,
            active_pane: crate::file_browser::BrowserPane::Local,
            local_path: std::path::PathBuf::from("/tmp"),
            local_entries: Vec::new(),
            local_list_state: ratatui::widgets::ListState::default(),
            local_selected: std::collections::HashSet::new(),
            local_error: None,
            remote_path: remote_path.to_string(),
            remote_entries: Vec::new(),
            remote_list_state: ratatui::widgets::ListState::default(),
            remote_selected: std::collections::HashSet::new(),
            remote_error: None,
            remote_loading: true,
            show_hidden: false,
            sort: crate::file_browser::BrowserSort::Name,
            confirm_copy: None,
            transferring: None,
            transfer_error: None,
            connection_recorded: false,
        });
        app.screen = Screen::FileBrowser {
            alias: alias.to_string(),
        };
    }

    /// Exactly what OpenSSH 10.3 prints for these two refusals.
    const DENIED: &str = "test@localhost: Permission denied (publickey,password).";
    const UNKNOWN_KEY: &str = "No ED25519 host key is known for [localhost]:12222 and you have requested strict checking.\nHost key verification failed.";

    #[test]
    fn a_password_failure_opens_the_prompt_with_the_path_to_retry() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_browser(&mut app, "h", "/var/log");
        apply_file_browser_listing(
            &mut app,
            "h".into(),
            "/var/log".into(),
            Err(DENIED.to_string()),
        );
        assert_eq!(app.screen, Screen::PasswordPrompt);
        let state = app.password_prompt.as_ref().expect("prompt");
        assert_eq!(
            state.retry,
            PendingRetry::FileBrowserListing {
                alias: "h".into(),
                path: "/var/log".into(),
            }
        );
        // The pane still says why, so Esc leaves the user informed.
        let fb = app.file_browser_session.as_ref().unwrap();
        assert!(fb.remote_error.is_some());
        assert!(!fb.remote_loading);
    }

    #[test]
    fn a_failed_home_lookup_retries_with_an_empty_path() {
        // An empty path means the remote home was never resolved, so the
        // retry runs `pwd` again before listing.
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_browser(&mut app, "h", "");
        apply_file_browser_listing(&mut app, "h".into(), String::new(), Err(DENIED.to_string()));
        assert_eq!(
            app.password_prompt.as_ref().unwrap().retry,
            PendingRetry::FileBrowserListing {
                alias: "h".into(),
                path: String::new(),
            }
        );
    }

    #[test]
    fn an_unknown_host_key_opens_the_trust_dialog() {
        let mut app = app_with("Host h\n  HostName db.example.com\n");
        open_browser(&mut app, "h", "/home");
        apply_file_browser_listing(
            &mut app,
            "h".into(),
            "/home".into(),
            Err(UNKNOWN_KEY.to_string()),
        );
        match &app.screen {
            Screen::ConfirmHostKeyTrust {
                alias, hostname, ..
            } => {
                assert_eq!(alias, "h");
                assert_eq!(hostname, "db.example.com");
            }
            other => panic!("expected the trust dialog, got {other:?}"),
        }
    }

    #[test]
    fn a_configured_source_that_did_not_deliver_gets_no_prompt() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n  # purple:askpass op://v/i/f\n");
        open_browser(&mut app, "h", "/home");
        apply_file_browser_listing(
            &mut app,
            "h".into(),
            "/home".into(),
            Err(DENIED.to_string()),
        );
        assert_eq!(app.screen, Screen::FileBrowser { alias: "h".into() });
        assert!(app.password_prompt.is_none());
        let err = app
            .file_browser_session
            .as_ref()
            .unwrap()
            .remote_error
            .clone()
            .expect("pane error");
        assert!(err.contains("1Password"), "got: {err}");
    }

    #[test]
    fn an_ordinary_failure_only_fills_the_pane() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_browser(&mut app, "h", "/home");
        apply_file_browser_listing(
            &mut app,
            "h".into(),
            "/home".into(),
            Err("ls: /home/x: No such file or directory".into()),
        );
        assert_eq!(app.screen, Screen::FileBrowser { alias: "h".into() });
        assert!(app.password_prompt.is_none());
        assert!(
            app.file_browser_session
                .as_ref()
                .unwrap()
                .remote_error
                .is_some()
        );
    }

    #[test]
    fn a_listing_for_another_host_is_ignored() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_browser(&mut app, "h", "/home");
        apply_file_browser_listing(
            &mut app,
            "other".into(),
            "/home".into(),
            Err(DENIED.to_string()),
        );
        assert_eq!(app.screen, Screen::FileBrowser { alias: "h".into() });
        assert!(app.password_prompt.is_none());
    }

    const KEY: &str = "/home/u/.ssh/id_sk";

    #[test]
    fn a_pin_request_opens_the_pin_prompt_ahead_of_the_password_check() {
        // The server also takes a password, yet the key signs the login
        // first, so the PIN is the question to ask.
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_browser(&mut app, "h", "/var/log");
        crate::security_key::arm_pin_request(
            app.env.paths(),
            crate::security_key::PinFlow::FileBrowser,
            "h",
            KEY,
        );
        apply_file_browser_listing(
            &mut app,
            "h".into(),
            "/var/log".into(),
            Err(DENIED.to_string()),
        );
        assert_eq!(app.screen, Screen::PasswordPrompt);
        let state = app.password_prompt.as_ref().expect("prompt");
        assert_eq!(state.security_key.as_deref(), Some(KEY));
        assert_eq!(
            state.retry,
            PendingRetry::FileBrowserListing {
                alias: "h".into(),
                path: "/var/log".into(),
            }
        );
    }

    #[test]
    fn a_pin_request_waits_in_the_pane_while_no_dialog_may_open() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_browser(&mut app, "h", "/var/log");
        app.search.set_query(Some("x".to_string()));
        crate::security_key::arm_pin_request(
            app.env.paths(),
            crate::security_key::PinFlow::FileBrowser,
            "h",
            KEY,
        );
        apply_file_browser_listing(
            &mut app,
            "h".into(),
            "/var/log".into(),
            Err(DENIED.to_string()),
        );
        assert!(app.password_prompt.is_none());
        assert_eq!(
            app.file_browser_session.as_ref().unwrap().remote_error,
            Some(crate::messages::askpass::pin_needed_listing("h", KEY))
        );
    }

    #[test]
    fn a_refused_trial_pin_reopens_the_prompt_and_says_why() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_browser(&mut app, "h", "/var/log");
        app.security_key_pins.start_trial(crate::app::PinTrial {
            alias: "h".into(),
            key_path: KEY.into(),
            pin: "0000".into(),
            flow: crate::security_key::PinFlow::FileBrowser,
        });
        crate::security_key::note_refusal(
            app.env.paths(),
            &crate::security_key::SignFailure {
                key_path: KEY.into(),
                reason: crate::security_key::RefusalReason::WrongPin,
            },
        );
        apply_file_browser_listing(
            &mut app,
            "h".into(),
            "/var/log".into(),
            Err("u@h: Permission denied (publickey).".to_string()),
        );
        assert_eq!(app.screen, Screen::PasswordPrompt);
        assert!(app.security_key_pins.trial().is_none(), "the PIN is gone");
        let toast = app.status_center.toast().expect("toast");
        assert!(toast.text.contains(KEY), "got: {}", toast.text);
    }

    #[test]
    fn a_listing_that_went_out_with_a_trial_pin_confirms_it() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_browser(&mut app, "h", "");
        app.security_key_pins.start_trial(crate::app::PinTrial {
            alias: "h".into(),
            key_path: KEY.into(),
            pin: "4242".into(),
            flow: crate::security_key::PinFlow::FileBrowser,
        });
        crate::security_key::record_hand_out(app.env.paths(), "h", 1, KEY);
        apply_file_browser_listing(&mut app, "h".into(), "/root".into(), Ok(Vec::new()));
        assert_eq!(app.security_key_pins.confirmed_pin(KEY), Some("4242"));
    }

    #[test]
    fn a_transfer_that_wanted_a_pin_says_so_in_its_error() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_browser(&mut app, "h", "/var/log");
        crate::security_key::arm_pin_request(
            app.env.paths(),
            crate::security_key::PinFlow::FileBrowser,
            "h",
            KEY,
        );
        let (tx, _rx) = mpsc::channel();
        apply_scp_complete(
            &mut app,
            "h".into(),
            false,
            "u@h: Permission denied (publickey).".into(),
            &tx,
        );
        assert_eq!(
            app.file_browser_session.as_ref().unwrap().transfer_error,
            Some(crate::messages::askpass::pin_needed_transfer("h", KEY))
        );
    }

    #[test]
    fn a_listing_that_got_in_but_failed_opens_no_pin_prompt() {
        // ssh met the PIN prompt, logged in another way and `ls` failed.
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_browser(&mut app, "h", "/var/log");
        crate::security_key::arm_pin_request(
            app.env.paths(),
            crate::security_key::PinFlow::FileBrowser,
            "h",
            KEY,
        );
        apply_file_browser_listing(
            &mut app,
            "h".into(),
            "/var/log".into(),
            Err("ls: /var/log: Permission denied".to_string()),
        );
        assert!(app.password_prompt.is_none());
        assert_eq!(
            app.file_browser_session.as_ref().unwrap().remote_error,
            Some("ls: /var/log: Permission denied".to_string())
        );
    }

    #[test]
    fn a_plain_transfer_failure_keeps_its_own_message() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_browser(&mut app, "h", "/var/log");
        let (tx, _rx) = mpsc::channel();
        apply_scp_complete(&mut app, "h".into(), false, "No such file".into(), &tx);
        assert_eq!(
            app.file_browser_session.as_ref().unwrap().transfer_error,
            Some("No such file".to_string())
        );
    }

    #[test]
    fn a_successful_listing_clears_the_error_and_fills_the_pane() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_browser(&mut app, "h", "");
        let entries = vec![crate::file_browser::FileEntry {
            name: "etc".into(),
            is_dir: true,
            size: None,
            modified: None,
        }];
        apply_file_browser_listing(&mut app, "h".into(), "/root".into(), Ok(entries));
        let fb = app.file_browser_session.as_ref().unwrap();
        assert_eq!(fb.remote_path, "/root");
        assert_eq!(fb.remote_entries.len(), 1);
        assert!(fb.remote_error.is_none());
        assert!(!fb.remote_loading);
        assert_eq!(app.screen, Screen::FileBrowser { alias: "h".into() });
    }
}
