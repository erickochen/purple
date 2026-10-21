use super::*;
use crate::app::{PendingRetry, Screen};
use crate::security_key::SignFailure;
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

fn trial(alias: &str, key_path: &str, flow: PinFlow) -> PinTrial {
    PinTrial {
        alias: alias.to_string(),
        key_path: key_path.to_string(),
        pin: "4242".to_string(),
        flow,
    }
}

fn fb_session(alias: &str) -> crate::file_browser::FileBrowserSession {
    crate::file_browser::FileBrowserSession {
        alias: alias.to_string(),
        askpass: None,
        active_pane: crate::file_browser::BrowserPane::Remote,
        local_path: std::path::PathBuf::from("/tmp"),
        local_entries: Vec::new(),
        local_list_state: ratatui::widgets::ListState::default(),
        local_selected: HashSet::new(),
        local_error: None,
        remote_path: String::new(),
        remote_entries: Vec::new(),
        remote_list_state: ratatui::widgets::ListState::default(),
        remote_selected: HashSet::new(),
        remote_error: None,
        remote_loading: true,
        show_hidden: false,
        sort: crate::file_browser::BrowserSort::Name,
        confirm_copy: None,
        transferring: None,
        transfer_error: None,
        connection_recorded: false,
    }
}

fn refuse(app: &App, key_path: &str, reason: RefusalReason) {
    sk::note_refusal(
        app.env.paths(),
        &SignFailure {
            key_path: key_path.to_string(),
            reason,
        },
    );
}

fn hand_out(app: &App, alias: &str, key_path: &str) {
    sk::record_hand_out(app.env.paths(), alias, 1, key_path);
}

/// Confirm `key_path` for `alias` the only way the store allows: a trial
/// that a successful run handed out.
fn confirm(app: &mut App, alias: &str, key_path: &str) {
    app.security_key_pins
        .start_trial(trial(alias, key_path, PinFlow::FileBrowser));
    hand_out(app, alias, key_path);
    assert_eq!(
        app.settle_security_key_run(alias, PinFlow::FileBrowser, RunEnd::Connected),
        PinOutcome::NotInvolved
    );
    assert_eq!(app.security_key_pins.confirmed_pin(key_path), Some("4242"));
}

#[test]
fn a_trial_that_a_successful_run_handed_out_is_confirmed() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    confirm(&mut app, "h", "/k");
    assert!(app.security_key_pins.trial().is_none());
}

#[test]
fn a_trial_the_run_never_handed_out_is_dropped_unproven() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    app.security_key_pins
        .start_trial(trial("h", "/k", PinFlow::FileBrowser));
    assert_eq!(
        app.settle_security_key_run("h", PinFlow::FileBrowser, RunEnd::Connected),
        PinOutcome::NotInvolved
    );
    assert_eq!(app.security_key_pins.confirmed_pin("/k"), None);
    assert!(app.security_key_pins.trial().is_none());
}

#[test]
fn a_trial_the_key_refused_is_rejected() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    app.security_key_pins
        .start_trial(trial("h", "/k", PinFlow::Tunnel));
    hand_out(&app, "h", "/k");
    refuse(&app, "/k", RefusalReason::WrongPin);
    assert_eq!(
        app.settle_security_key_run("h", PinFlow::Tunnel, RunEnd::AuthRefused),
        PinOutcome::Rejected {
            key_path: "/k".into(),
            reason: RefusalReason::WrongPin,
            connected: false,
        }
    );
    assert_eq!(app.security_key_pins.confirmed_pin("/k"), None);
}

#[test]
fn a_refused_trial_on_a_run_that_still_got_in_is_reported_as_such() {
    // The host let the run in with a password after the key refused: the
    // run succeeded, yet the PIN must never be reused.
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    app.security_key_pins
        .start_trial(trial("h", "/k", PinFlow::FileBrowser));
    hand_out(&app, "h", "/k");
    refuse(&app, "/k", RefusalReason::WrongPin);
    assert!(matches!(
        app.settle_security_key_run("h", PinFlow::FileBrowser, RunEnd::Connected),
        PinOutcome::Rejected {
            connected: true,
            ..
        }
    ));
    assert_eq!(app.security_key_pins.confirmed_pin("/k"), None);
}

#[test]
fn a_trial_settles_only_in_its_own_flow() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    app.security_key_pins
        .start_trial(trial("h", "/k", PinFlow::FileBrowser));
    hand_out(&app, "h", "/k");
    app.settle_security_key_run("h", PinFlow::Tunnel, RunEnd::Connected);
    assert!(app.security_key_pins.trial().is_some(), "still on trial");
    assert_eq!(app.security_key_pins.confirmed_pin("/k"), None);
}

#[test]
fn a_confirmed_pin_the_key_refuses_later_is_forgotten() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\nHost g\n  HostName 2.2.2.2\n");
    confirm(&mut app, "h", "/k");
    hand_out(&app, "g", "/k");
    refuse(&app, "/k", RefusalReason::NoAnswer);
    assert_eq!(
        app.settle_security_key_run("g", PinFlow::KeyPush, RunEnd::AuthRefused),
        PinOutcome::Rejected {
            key_path: "/k".into(),
            reason: RefusalReason::NoAnswer,
            connected: false,
        }
    );
    assert_eq!(app.security_key_pins.confirmed_pin("/k"), None);
}

#[test]
fn a_pin_refused_by_another_run_is_forgotten_without_blaming_this_one() {
    // A container refresh on h hit the refusal. A listing on g that never
    // handed that PIN out reports its own failure, not the key's.
    let mut app = app_with("Host h\n  HostName 1.1.1.1\nHost g\n  HostName 2.2.2.2\n");
    confirm(&mut app, "h", "/k");
    refuse(&app, "/k", RefusalReason::WrongPin);
    assert_eq!(
        app.settle_security_key_run("g", PinFlow::FileBrowser, RunEnd::OtherFailure),
        PinOutcome::NotInvolved
    );
    assert_eq!(app.security_key_pins.confirmed_pin("/k"), None);
}

#[test]
fn a_pin_refused_elsewhere_still_lets_this_run_ask_for_it() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\nHost g\n  HostName 2.2.2.2\n");
    confirm(&mut app, "h", "/k");
    refuse(&app, "/k", RefusalReason::WrongPin);
    sk::arm_pin_request(app.env.paths(), PinFlow::FileBrowser, "g", "/k");
    assert_eq!(
        app.settle_security_key_run("g", PinFlow::FileBrowser, RunEnd::AuthRefused),
        PinOutcome::Needed {
            key_path: "/k".into()
        }
    );
}

#[test]
fn a_pin_the_run_asked_for_is_reported_when_the_login_was_refused() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    sk::arm_pin_request(app.env.paths(), PinFlow::Tunnel, "h", "/k");
    assert_eq!(
        app.settle_security_key_run("h", PinFlow::Tunnel, RunEnd::AuthRefused),
        PinOutcome::Needed {
            key_path: "/k".into()
        }
    );
}

#[test]
fn a_request_from_a_run_that_got_in_is_gone_for_a_later_failure() {
    // The host let the run in another way. A later refusal must not open a
    // PIN prompt on that leftover request.
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    sk::arm_pin_request(app.env.paths(), PinFlow::Tunnel, "h", "/k");
    assert_eq!(
        app.settle_security_key_run("h", PinFlow::Tunnel, RunEnd::Connected),
        PinOutcome::NotInvolved
    );
    assert_eq!(
        app.settle_security_key_run("h", PinFlow::Tunnel, RunEnd::AuthRefused),
        PinOutcome::NotInvolved
    );
}

#[test]
fn a_failure_that_is_not_a_refused_login_asks_for_no_pin() {
    // The run connected another way and the remote command failed.
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    sk::arm_pin_request(app.env.paths(), PinFlow::FileBrowser, "h", "/k");
    assert_eq!(
        app.settle_security_key_run("h", PinFlow::FileBrowser, RunEnd::OtherFailure),
        PinOutcome::NotInvolved
    );
    assert_eq!(
        sk::take_pin_request(app.env.paths(), PinFlow::FileBrowser, "h"),
        None,
        "the request went with the run that left it"
    );
}

#[test]
fn a_run_end_is_read_from_what_ssh_printed() {
    assert_eq!(RunEnd::from_run(true, ""), RunEnd::Connected);
    assert_eq!(
        RunEnd::from_run(false, "ops@h: Permission denied (publickey)."),
        RunEnd::AuthRefused
    );
    assert_eq!(
        RunEnd::from_run(false, "ls: /x: No such file or directory"),
        RunEnd::OtherFailure
    );
}

#[test]
fn a_request_from_another_flow_is_not_this_runs_business() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    sk::arm_pin_request(app.env.paths(), PinFlow::Tunnel, "h", "/k");
    assert_eq!(
        app.settle_security_key_run("h", PinFlow::FileBrowser, RunEnd::AuthRefused),
        PinOutcome::NotInvolved
    );
}

#[test]
fn a_run_carries_proven_pins_and_only_its_own_trial() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\nHost g\n  HostName 2.2.2.2\n");
    confirm(&mut app, "h", "/proven");
    app.security_key_pins
        .start_trial(trial("g", "/new", PinFlow::Tunnel));

    let tunnel_g = app.security_key_run_for("g", PinFlow::Tunnel);
    assert!(tunnel_g.carries("/proven"));
    assert!(tunnel_g.carries("/new"));
    assert_eq!(tunnel_g.flow, Some(PinFlow::Tunnel));

    let listing_g = app.security_key_run_for("g", PinFlow::FileBrowser);
    assert!(
        !listing_g.carries("/new"),
        "the trial belongs to the tunnel"
    );
    assert!(!app.security_key_confirmed_run().carries("/new"));
    assert!(app.security_key_confirmed_run().carries("/proven"));
    assert_eq!(app.security_key_confirmed_run().flow, None);
}

#[test]
fn a_refused_proven_pin_is_not_carried_even_before_it_is_forgotten() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    confirm(&mut app, "h", "/k");
    refuse(&app, "/k", RefusalReason::WrongPin);
    assert!(!app.security_key_confirmed_run().carries("/k"));
    assert!(!app.security_key_run_for("h", PinFlow::Tunnel).carries("/k"));
}

#[test]
fn a_session_without_pins_hands_out_nothing() {
    let app = app_with("Host h\n  HostName 1.1.1.1\n");
    assert_eq!(app.security_key_confirmed_run(), SecurityKeyRun::default());
}

#[test]
fn a_closed_gate_hands_out_nothing_and_asks_for_nothing() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    confirm(&mut app, "h", "/k");
    app.ssh_allows_security_key_pins = std::cell::OnceCell::new();
    let _ = app.ssh_allows_security_key_pins.set(false);
    assert_eq!(app.security_key_confirmed_run(), SecurityKeyRun::default());
    assert_eq!(
        app.security_key_run_for("h", PinFlow::Tunnel),
        SecurityKeyRun::default()
    );
}

#[test]
fn a_trial_for_a_host_that_left_the_config_is_dropped() {
    let mut store = SecurityKeyPins::default();
    store.start_trial(trial("gone", "/k", PinFlow::Tunnel));
    let valid: HashSet<&str> = ["h"].into_iter().collect();
    assert!(store.prune_aliases(&valid));
    assert!(store.trial().is_none());
    assert!(!store.prune_aliases(&valid));
}

#[test]
fn a_trial_is_dropped_only_for_its_own_flow_and_host() {
    let mut store = SecurityKeyPins::default();
    store.start_trial(trial("h", "/k", PinFlow::FileBrowser));
    assert!(!store.drop_trial(PinFlow::Tunnel, Some("h")));
    assert!(!store.drop_trial(PinFlow::FileBrowser, Some("other")));
    assert!(store.trial().is_some());
    assert!(store.drop_trial(PinFlow::FileBrowser, Some("h")));
    assert!(store.trial().is_none());

    store.start_trial(trial("h", "/k", PinFlow::KeyPush));
    assert!(store.drop_trial(PinFlow::KeyPush, None), "any host");
    assert!(store.trial().is_none());
}

#[test]
fn closing_the_file_browser_takes_its_trial_out_of_the_next_run() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    app.open_file_browser(fb_session("h"));
    app.security_key_pins
        .start_trial(trial("h", "/k", PinFlow::FileBrowser));
    app.close_file_browser();
    assert!(app.security_key_pins.trial().is_none());
    assert!(
        app.security_key_run_for("h", PinFlow::FileBrowser)
            .pins
            .is_empty(),
        "a reopened browser asks for the PIN again"
    );
}

#[test]
fn closing_the_file_browser_keeps_a_trial_of_another_flow() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    app.open_file_browser(fb_session("h"));
    app.security_key_pins
        .start_trial(trial("h", "/k", PinFlow::Tunnel));
    app.close_file_browser();
    assert!(app.security_key_pins.trial().is_some());
}

#[test]
fn the_startup_version_decides_the_gate_without_asking_ssh_again() {
    let mut old = app_with("");
    old.ssh_allows_security_key_pins = std::cell::OnceCell::new();
    old.seed_ssh_version("OpenSSH_8.4p1, LibreSSL 3.3.6");
    assert!(!old.security_key_pins_allowed());

    let mut new = app_with("");
    new.ssh_allows_security_key_pins = std::cell::OnceCell::new();
    new.seed_ssh_version("OpenSSH_10.0p2, OpenSSL 3.5.1");
    assert!(new.security_key_pins_allowed());
    new.seed_ssh_version("OpenSSH_8.4p1");
    assert!(new.security_key_pins_allowed(), "the first answer stands");
}

#[test]
fn the_debug_output_never_shows_a_pin() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    confirm(&mut app, "h", "/k");
    app.security_key_pins
        .start_trial(trial("h", "/t", PinFlow::Tunnel));
    let text = format!("{:?}", app.security_key_pins);
    assert!(text.contains("/k") && text.contains("/t"), "{text}");
    assert!(!text.contains("4242"), "{text}");
}

#[test]
fn the_pin_prompt_opens_with_its_key_and_retry() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    let retry = PendingRetry::Tunnel { alias: "h".into() };
    app.open_security_key_pin_prompt("h", "/k", retry.clone(), None, None);
    assert_eq!(app.screen, Screen::PasswordPrompt);
    let state = app.password_prompt.as_ref().expect("prompt");
    assert_eq!(state.security_key.as_deref(), Some("/k"));
    assert_eq!(state.retry, retry);
    assert!(!state.remember, "a PIN never goes to the keychain");
    assert!(state.input.is_empty());
    assert!(state.return_to_tunnel_list.is_none());
}

#[test]
fn the_pin_prompt_over_a_tunnel_list_returns_there_and_can_be_prefilled() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    app.screen = Screen::TunnelList { alias: "h".into() };
    assert!(app.can_open_tunnel_dialog());
    app.open_security_key_pin_prompt(
        "h",
        "/k",
        PendingRetry::Tunnel { alias: "h".into() },
        Some(RefusalReason::WrongPin),
        Some("4242".into()),
    );
    let state = app.password_prompt.as_ref().expect("prompt");
    assert_eq!(state.return_to_tunnel_list.as_deref(), Some("h"));
    assert_eq!(state.input, "4242");
    let toast = app.status_center.toast().expect("refusal toast");
    assert!(toast.text.contains("/k"), "{}", toast.text);
}

#[test]
fn no_tunnel_dialog_opens_over_a_pending_delete_or_another_dialog() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    app.screen = Screen::TunnelList { alias: "h".into() };
    app.tunnels.request_delete(0);
    assert!(!app.can_open_tunnel_dialog());
    app.tunnels.cancel_delete();
    app.open_security_key_pin_prompt(
        "h",
        "/k",
        PendingRetry::Tunnel { alias: "h".into() },
        None,
        None,
    );
    assert!(!app.can_open_tunnel_dialog(), "a dialog is already open");
}

#[cfg(unix)]
#[test]
fn a_tunnel_trial_settles_once_the_target_let_it_in() {
    let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
    app.security_key_pins
        .start_trial(trial("h", "/k", PinFlow::Tunnel));
    let child = std::process::Command::new("true")
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn true");
    let (tx, _rx) = std::sync::mpsc::channel();
    let tunnel = crate::tunnel::ActiveTunnel::spawn(
        child,
        "h",
        tx,
        crate::tunnel_live::AuthWatch::default(),
    );
    app.tunnels.active_insert("h".into(), tunnel);
    assert_eq!(app.settle_authenticated_tunnel_trial(), None, "not in yet");

    hand_out(&app, "h", "/k");
    app.tunnels
        .active_get("h")
        .expect("tunnel")
        .live
        .authenticated
        .store(true, Ordering::Relaxed);
    assert_eq!(
        app.settle_authenticated_tunnel_trial(),
        Some(("h".to_string(), PinOutcome::NotInvolved))
    );
    assert_eq!(app.security_key_pins.confirmed_pin("/k"), Some("4242"));
    assert_eq!(
        app.settle_authenticated_tunnel_trial(),
        None,
        "settled once"
    );
}
