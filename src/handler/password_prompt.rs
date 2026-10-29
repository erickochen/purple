//! Key handling for `Screen::PasswordPrompt` plus the retry machinery the
//! password prompt and the host-key trust dialog share.
//!
//! A background ssh cannot reach the terminal, so the answer it needs is
//! collected here and the operation is run again with that answer in hand.

use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, PASSWORD_MAX_CHARS, PasswordPromptField, PendingRetry, Screen};
use crate::event::AppEvent;

pub(super) fn handle_key(app: &mut App, key: KeyEvent, events_tx: &mpsc::Sender<AppEvent>) {
    if app.password_prompt.is_none() {
        // State pruned out from under the screen; do not strand the user.
        let back = return_screen(app);
        app.set_screen(back);
        return;
    }
    match key.code {
        KeyCode::Esc => cancel(app),
        KeyCode::Tab | KeyCode::Down | KeyCode::BackTab | KeyCode::Up => {
            // A PIN prompt has one field: nothing to move to.
            if let Some(state) = app.password_prompt.as_mut()
                && state.security_key.is_none()
            {
                state.focus = state.focus.next();
            }
        }
        KeyCode::Enter => submit(app, events_tx),
        KeyCode::Backspace => {
            if let Some(state) = app.password_prompt.as_mut()
                && state.focus == PasswordPromptField::Password
            {
                state.input.pop();
            }
        }
        // SPACE GUARD MUST PRECEDE the generic Char(c) arm.
        // Rust matches arms top-to-bottom; below the generic insert-char arm
        // this would never fire and the toggle would be unreachable. On the
        // password field Space falls through and inserts a literal space,
        // because a password may contain one.
        KeyCode::Char(' ') => {
            if let Some(state) = app.password_prompt.as_mut() {
                if state.focus == PasswordPromptField::Remember {
                    state.remember = !state.remember;
                } else {
                    insert_char(state, ' ');
                }
            }
        }
        KeyCode::Char(c) => {
            if let Some(state) = app.password_prompt.as_mut()
                && state.focus == PasswordPromptField::Password
            {
                insert_char(state, c);
            }
        }
        _ => {}
    }
}

/// Append one character to the password buffer. Control characters are
/// dropped so a paste carrying a newline or an escape cannot smuggle extra
/// input past the single-line field, and the buffer is capped. A PIN stops
/// at the CTAP2 limit.
fn insert_char(state: &mut crate::app::PasswordPromptState, c: char) {
    if c.is_control() || state.input.chars().count() >= PASSWORD_MAX_CHARS {
        return;
    }
    if state.security_key.is_some()
        && state.input.len() + c.len_utf8() > crate::security_key::PIN_MAX_BYTES
    {
        return;
    }
    state.input.push(c);
}

/// The screen this prompt returns to: the tunnel list it opened over, else
/// the page any dialog returns to.
fn prompt_return_screen(app: &App, state: &crate::app::PasswordPromptState) -> Screen {
    match &state.return_to_tunnel_list {
        Some(alias) => Screen::TunnelList {
            alias: alias.clone(),
        },
        None => return_screen(app),
    }
}

/// Esc: drop the prompt and the operation behind it, then offer the next
/// host waiting for an answer.
fn cancel(app: &mut App) {
    let Some(state) = app.password_prompt.take() else {
        return;
    };
    log::debug!("[purple] password prompt: dismissed alias={}", state.alias);
    let back = prompt_return_screen(app, &state);
    app.set_screen(back);
    let toast = if state.security_key.is_some() {
        crate::messages::askpass::pin_prompt_canceled(&state.alias)
    } else {
        crate::messages::askpass::prompt_cancelled(&state.alias)
    };
    app.notify_warning(toast);
    super::event_loop::key_push::drain_next_key_push_prompt(app);
}

/// Enter: store the password where the user asked, then run the operation
/// again. An empty buffer is a no-op, like the container exec prompt.
fn submit(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    let Some(state) = app.password_prompt.as_ref() else {
        return;
    };
    if state.input.is_empty() {
        return;
    }
    if state.security_key.is_some() {
        submit_security_key_pin(app, events_tx);
        return;
    }
    let Some(state) = app.password_prompt.take() else {
        return;
    };
    let crate::app::PasswordPromptState {
        alias,
        source_is_keychain,
        input,
        remember,
        retry,
        ..
    } = state;

    // Keep the password for this session unless it is provably reachable
    // through the keychain on the next run.
    let stored = remember && store_in_keychain(app, &alias, &input, source_is_keychain);
    if stored {
        app.session_passwords.remove(&alias);
    } else {
        app.session_passwords.insert(alias.clone(), input);
    }
    log::debug!(
        "[purple] password prompt: submitted alias={} remember={} keychain={}",
        alias,
        remember,
        stored
    );

    let back = return_screen(app);
    app.set_screen(back);
    execute_retry(app, retry, false, events_tx);
    // Hand the next waiting host its dialog. A key-push retry has just
    // started a run, so the drain refuses and that run's finalize takes
    // over. Any other retry leaves the queue free to continue here.
    super::event_loop::key_push::drain_next_key_push_prompt(app);
}

/// Enter on a PIN prompt: put the PIN on trial and run the operation again
/// with it. A PIN too short to ever be right stays in the field, because
/// sending it would still cost the key an attempt.
fn submit_security_key_pin(app: &mut App, events_tx: &mpsc::Sender<AppEvent>) {
    let too_short = app
        .password_prompt
        .as_ref()
        .is_some_and(|s| s.input.chars().count() < crate::security_key::PIN_MIN_CHARS);
    if too_short {
        app.notify_warning(crate::messages::askpass::pin_too_short());
        return;
    }
    let Some(state) = app.password_prompt.take() else {
        return;
    };
    let back = prompt_return_screen(app, &state);
    let crate::app::PasswordPromptState {
        alias,
        input,
        retry,
        security_key,
        ..
    } = state;
    let Some(key_path) = security_key else {
        return;
    };
    // A new PIN starts clean: an old refusal or an old hand-out must not
    // decide what happens to this one.
    let paths = app.env.paths().cloned();
    crate::security_key::clear_refusal(paths.as_ref(), &key_path);
    let _ = crate::security_key::take_handed_out_keys(paths.as_ref(), &alias);
    log::debug!(
        "[purple] security key: PIN submitted alias={} key={} retry={}",
        alias,
        key_path,
        retry.kind_label()
    );
    let flow = retry.pin_flow();
    app.security_key_pins.start_trial(crate::app::PinTrial {
        alias: alias.clone(),
        key_path,
        pin: input,
        flow,
    });
    app.set_screen(back);
    if execute_retry(app, retry, false, events_tx) {
        app.notify(crate::i18n::text(crate::messages::askpass::PIN_TOUCH_KEY));
    } else {
        // Nothing carries the PIN, so the next run asks for it again.
        app.abandon_security_key_trial(flow, Some(&alias));
    }
    super::event_loop::key_push::drain_next_key_push_prompt(app);
}

/// Store the password in the OS keychain and point the host at it. Returns
/// true only when a later run will find the password by itself: the store
/// succeeded and the alias already resolves to `keychain`, or the directive
/// was written. Every other outcome falls back to the session password and
/// says why.
fn store_in_keychain(app: &mut App, alias: &str, password: &str, already_keychain: bool) -> bool {
    if let Err(e) = crate::askpass::store_in_keychain(app.env(), alias, password) {
        log::warn!("[external] password prompt: keychain store failed alias={alias}: {e}");
        app.notify_error(crate::messages::askpass::keychain_store_failed_session(&e));
        return false;
    }
    if already_keychain {
        log::debug!(
            "[purple] password prompt: stored alias={alias} in keychain, source already keychain"
        );
        return true;
    }
    point_host_at_keychain(app, alias)
}

/// Write `# purple:askpass keychain` on the host so a later run finds the
/// password by itself. Returns false when the directive did not land, which
/// keeps the password for this session and says why. Split from the keychain
/// call so the config outcomes are testable on their own.
fn point_host_at_keychain(app: &mut App, alias: &str) -> bool {
    // The config watcher is suppressed while a dialog is open, so the
    // in-memory model can be older than the file. Writing it back would
    // silently undo whatever was edited meanwhile, and this write only adds
    // a convenience directive. Leave the file alone and say so.
    if app.external_config_changed() {
        log::warn!(
            "[config] password prompt: askpass write skipped for alias={alias}, config changed on disk"
        );
        app.notify_warning(
            crate::messages::askpass::keychain_source_skipped_external_change(alias),
        );
        return false;
    }
    let backup = app.hosts_state.ssh_config().clone();
    if !app
        .hosts_state
        .ssh_config_mut()
        .set_host_askpass(alias, "keychain")
    {
        log::warn!("[config] password prompt: askpass directive not written for alias={alias}");
        app.notify_warning(crate::messages::askpass::keychain_source_not_written(alias));
        return false;
    }
    if let Err(e) = app.hosts_state.ssh_config().write() {
        app.hosts_state.set_ssh_config(backup);
        log::warn!("[config] password prompt: config write failed alias={alias}: {e}");
        // The keychain entry stays. `security add-generic-password -U` and
        // `secret-tool store` both overwrite, so purple cannot tell whether
        // it created the entry or replaced one the user put there, and
        // deleting it could throw away a working credential.
        app.notify_error(crate::messages::askpass::keychain_stored_without_source(&e));
        return false;
    }
    log::debug!("[purple] password prompt: wrote askpass=keychain for alias={alias}");
    app.update_last_modified();
    app.reload_hosts();
    true
}

/// The screen a dialog returns to: the file browser while its overlay is
/// still open, else the host list.
pub(crate) fn return_screen(app: &App) -> Screen {
    match app.file_browser_session.as_ref() {
        Some(fb) => Screen::FileBrowser {
            alias: fb.alias.clone(),
        },
        None => Screen::HostList,
    }
}

/// Run the operation a dialog was holding. `trust_new_host_key` is true
/// only right after the user accepted the trust dialog, so ssh records the
/// host key on that one run. Returns true when the operation started.
pub(crate) fn execute_retry(
    app: &mut App,
    retry: PendingRetry,
    trust_new_host_key: bool,
    events_tx: &mpsc::Sender<AppEvent>,
) -> bool {
    match retry {
        PendingRetry::KeyPush { key_path, alias } => {
            // Look the key up by path: a successful push rebuilds the list,
            // so the position it had when the dialog opened may now be
            // another key.
            let Some(key_index) = app
                .keys
                .list()
                .iter()
                .position(|k| k.display_path == key_path)
            else {
                log::debug!("[purple] retry: key {key_path} is gone, dropping alias={alias}");
                return false;
            };
            log::debug!("[purple] retry: key push alias={alias} trust={trust_new_host_key}");
            super::confirm::start_key_push(
                app,
                key_index,
                vec![alias],
                trust_new_host_key,
                events_tx,
            )
        }
        PendingRetry::FileBrowserListing { alias, path } => {
            // The overlay may have been closed while the dialog was open.
            let Some(fb) = app.file_browser_session.as_mut() else {
                return false;
            };
            if fb.alias != alias {
                return false;
            }
            fb.remote_loading = true;
            fb.remote_error = None;
            fb.remote_entries.clear();
            fb.remote_selected.clear();
            fb.remote_list_state = ratatui::widgets::ListState::default();
            let show_hidden = fb.show_hidden;
            let sort = fb.sort;
            // Resolve the source again rather than reusing the snapshot the
            // session took when it opened. Answering the prompt with
            // "remember" on writes `# purple:askpass keychain` on the host,
            // and both this retry and every later listing must see it.
            let askpass = app.askpass_source_for(&alias);
            if let Some(fb) = app.file_browser_session.as_mut() {
                fb.askpass = askpass.clone();
            }
            log::debug!("[purple] retry: remote listing alias={alias} trust={trust_new_host_key}");
            let mut ctx = app.file_browser_ssh_context(alias, askpass);
            ctx.trust_new_host_key = trust_new_host_key;
            crate::file_browser::spawn_remote_open(
                ctx,
                path,
                show_hidden,
                sort,
                super::file_browser::fb_send(events_tx.clone()),
            );
            true
        }
        PendingRetry::Tunnel { alias } => {
            // Started again meanwhile, or demo mode where tunnels never run.
            if app.demo_mode || app.tunnels.active_contains(&alias) {
                log::debug!("[purple] retry: tunnel alias={alias} already running or demo");
                return false;
            }
            log::debug!("[purple] retry: tunnel alias={alias}");
            let (started, effects) = {
                let mut ctx = super::tunnel::ctx_from_app(app);
                let started = ctx.spawn_active_tunnel(&alias);
                (started, ctx.effects)
            };
            effects.apply(app);
            started
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{PasswordPromptState, Screen};
    use crate::ssh_config::model::SshConfigFile;
    use crossterm::event::KeyModifiers;

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

    fn open_prompt(app: &mut App, alias: &str) {
        app.password_prompt = Some(PasswordPromptState::new(
            alias.to_string(),
            false,
            PendingRetry::KeyPush {
                key_path: "~/.ssh/id_test".into(),
                alias: alias.to_string(),
            },
        ));
        app.screen = Screen::PasswordPrompt;
    }

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn press(app: &mut App, code: KeyCode) {
        let (tx, _rx) = mpsc::channel();
        handle_key(app, k(code), &tx);
    }

    #[test]
    fn typing_fills_the_password_field() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_prompt(&mut app, "h");
        for c in "pw".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        assert_eq!(app.password_prompt.as_ref().unwrap().input, "pw");
    }

    #[test]
    fn space_inserts_a_literal_space_in_the_password() {
        // A password may contain a space; only the toggle field consumes it.
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_prompt(&mut app, "h");
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('b'));
        assert_eq!(app.password_prompt.as_ref().unwrap().input, "a b");
    }

    #[test]
    fn space_on_the_toggle_field_flips_remember() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_prompt(&mut app, "h");
        press(&mut app, KeyCode::Tab);
        assert_eq!(
            app.password_prompt.as_ref().unwrap().focus,
            PasswordPromptField::Remember
        );
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.password_prompt.as_ref().unwrap().remember);
        press(&mut app, KeyCode::Char(' '));
        assert!(app.password_prompt.as_ref().unwrap().remember);
        // The toggle field never collects characters.
        assert!(app.password_prompt.as_ref().unwrap().input.is_empty());
    }

    #[test]
    fn focus_cycles_with_tab_and_arrows() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_prompt(&mut app, "h");
        for code in [KeyCode::Tab, KeyCode::Down, KeyCode::BackTab, KeyCode::Up] {
            let before = app.password_prompt.as_ref().unwrap().focus;
            press(&mut app, code);
            assert_ne!(
                app.password_prompt.as_ref().unwrap().focus,
                before,
                "{code:?}"
            );
        }
    }

    #[test]
    fn backspace_deletes_from_the_password_only() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_prompt(&mut app, "h");
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char('b'));
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.password_prompt.as_ref().unwrap().input, "a");
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.password_prompt.as_ref().unwrap().input, "a");
    }

    #[test]
    fn control_characters_are_dropped() {
        // A paste carrying a newline must not smuggle extra input through.
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_prompt(&mut app, "h");
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char('\n'));
        press(&mut app, KeyCode::Char('\t'));
        press(&mut app, KeyCode::Char('b'));
        assert_eq!(app.password_prompt.as_ref().unwrap().input, "ab");
    }

    #[test]
    fn the_buffer_is_capped() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_prompt(&mut app, "h");
        for _ in 0..(PASSWORD_MAX_CHARS + 50) {
            press(&mut app, KeyCode::Char('x'));
        }
        assert_eq!(
            app.password_prompt.as_ref().unwrap().input.chars().count(),
            PASSWORD_MAX_CHARS
        );
    }

    #[test]
    fn an_empty_submit_is_a_no_op() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_prompt(&mut app, "h");
        press(&mut app, KeyCode::Enter);
        assert!(app.password_prompt.is_some(), "prompt stays open");
        assert_eq!(app.screen, Screen::PasswordPrompt);
    }

    #[test]
    fn esc_drops_the_prompt_and_warns() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_prompt(&mut app, "h");
        press(&mut app, KeyCode::Char('x'));
        press(&mut app, KeyCode::Esc);
        assert!(app.password_prompt.is_none());
        assert_eq!(app.screen, Screen::HostList);
        let toast = app.status_center.toast().expect("toast");
        assert!(toast.text.contains('h'), "got: {}", toast.text);
    }

    /// An App whose config file exists on disk, so the mtime guard and the
    /// writer both have something real to work with.
    fn app_with_file(config: &str) -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let path = scratch.join("config");
        std::fs::write(&path, config).expect("write config");
        let cfg = SshConfigFile {
            elements: SshConfigFile::parse_content(config),
            path,
            crlf: false,
            bom: false,
        };
        let mut app = App::new(cfg);
        app.update_last_modified();
        app.reload_hosts();
        app
    }

    #[test]
    fn pointing_a_host_at_the_keychain_writes_the_directive() {
        let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut app = app_with_file("Host h\n  HostName 1.1.1.1\n");
        let path = app.hosts_state.ssh_config().path.clone();
        assert!(point_host_at_keychain(&mut app, "h"));
        let on_disk = std::fs::read_to_string(&path).expect("read back");
        assert!(
            on_disk.contains("# purple:askpass keychain"),
            "got: {on_disk}"
        );
        assert_eq!(
            app.askpass_source_for("h").as_deref(),
            Some("keychain"),
            "the reload picks the directive up"
        );
    }

    #[test]
    fn a_config_edited_while_the_prompt_was_open_is_left_alone() {
        let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // The watcher is suppressed behind a dialog, so the in-memory model
        // can be older than the file. Writing it back would undo the edit.
        let mut app = app_with_file("Host h\n  HostName 1.1.1.1\n");
        let path = app.hosts_state.ssh_config().path.clone();
        // Someone else edits the file. Move the mtime past the snapshot so
        // the guard sees the change on filesystems with coarse timestamps.
        std::fs::write(&path, "Host h\n  HostName 2.2.2.2\n  Port 2222\n").expect("edit");
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|f| f.set_modified(later))
            .expect("move the mtime past the snapshot");
        assert!(
            !point_host_at_keychain(&mut app, "h"),
            "the write is skipped"
        );
        let on_disk = std::fs::read_to_string(&path).expect("read back");
        assert!(
            on_disk.contains("Port 2222"),
            "the edit survives: {on_disk}"
        );
        assert!(!on_disk.contains("purple:askpass"), "got: {on_disk}");
    }

    #[test]
    fn a_host_purple_does_not_own_keeps_the_password_for_the_session() {
        let _guard = crate::demo_flag::GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // No block for this alias in the file purple writes, so there is
        // nowhere to put the directive and the session copy carries on.
        let mut app = app_with_file("Host other\n  HostName 1.1.1.1\n");
        let path = app.hosts_state.ssh_config().path.clone();
        assert!(!point_host_at_keychain(&mut app, "h"));
        let on_disk = std::fs::read_to_string(&path).expect("read back");
        assert!(!on_disk.contains("purple:askpass"), "got: {on_disk}");
    }

    #[test]
    fn submit_with_remember_off_keeps_the_password_for_the_session() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_prompt(&mut app, "h");
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char(' ')); // remember off
        press(&mut app, KeyCode::BackTab);
        for c in "hunter2".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.password_prompt.is_none());
        assert_eq!(
            app.session_passwords.get("h").map(String::as_str),
            Some("hunter2")
        );
        // Nothing was written to the config.
        assert!(
            !app.hosts_state
                .ssh_config()
                .host_entries()
                .iter()
                .any(|e| e.askpass.is_some())
        );
    }

    /// A PIN prompt for a tunnel retry. Demo mode keeps the retry from
    /// starting a real ssh.
    fn open_pin_prompt(app: &mut App, alias: &str) {
        app.demo_mode = true;
        app.open_security_key_pin_prompt(
            alias,
            "/home/u/.ssh/id_sk",
            PendingRetry::Tunnel {
                alias: alias.to_string(),
            },
            None,
            None,
        );
    }

    #[test]
    fn a_pin_prompt_has_one_field_and_takes_spaces_literally() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_pin_prompt(&mut app, "h");
        press(&mut app, KeyCode::Tab);
        assert_eq!(
            app.password_prompt.as_ref().unwrap().focus,
            PasswordPromptField::Password
        );
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('b'));
        assert_eq!(app.password_prompt.as_ref().unwrap().input, "a b");
    }

    #[test]
    fn a_pin_stops_at_the_ctap2_limit() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_pin_prompt(&mut app, "h");
        for _ in 0..crate::security_key::PIN_MAX_BYTES + 10 {
            press(&mut app, KeyCode::Char('7'));
        }
        assert_eq!(
            app.password_prompt.as_ref().unwrap().input.len(),
            crate::security_key::PIN_MAX_BYTES
        );
    }

    #[test]
    fn a_pin_too_short_to_be_right_stays_in_the_field() {
        // Sending it would still cost the key an attempt.
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_pin_prompt(&mut app, "h");
        for c in "123".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.screen, Screen::PasswordPrompt);
        assert!(app.security_key_pins.trial().is_none());
        let toast = app.status_center.toast().expect("toast");
        assert!(toast.text.contains('4'), "got: {}", toast.text);
    }

    /// A PIN prompt for a listing retry in an open file browser. The config
    /// file is never written, so the retry's ssh exits at once.
    fn open_listing_pin_prompt(app: &mut App, alias: &str) {
        app.file_browser_session = Some(crate::file_browser::FileBrowserSession {
            alias: alias.into(),
            askpass: None,
            active_pane: crate::file_browser::BrowserPane::Remote,
            local_path: std::path::PathBuf::from("/tmp"),
            local_entries: Vec::new(),
            local_list_state: ratatui::widgets::ListState::default(),
            local_selected: std::collections::HashSet::new(),
            local_error: None,
            remote_path: String::new(),
            remote_entries: Vec::new(),
            remote_list_state: ratatui::widgets::ListState::default(),
            remote_selected: std::collections::HashSet::new(),
            remote_error: None,
            remote_loading: false,
            show_hidden: false,
            sort: crate::file_browser::BrowserSort::Name,
            confirm_copy: None,
            transferring: None,
            transfer_error: None,
            connection_recorded: false,
        });
        app.screen = Screen::FileBrowser {
            alias: alias.into(),
        };
        app.open_security_key_pin_prompt(
            alias,
            "/home/u/.ssh/id_sk",
            PendingRetry::FileBrowserListing {
                alias: alias.to_string(),
                path: String::new(),
            },
            None,
            None,
        );
    }

    #[test]
    fn submitting_a_pin_puts_it_on_trial_with_a_clean_slate() {
        let mut app = app_with("Host h\n  HostName 127.0.0.1\n  Port 1\n");
        let paths = app.env().paths().cloned();
        crate::security_key::note_refusal(
            paths.as_ref(),
            &crate::security_key::SignFailure {
                key_path: "/home/u/.ssh/id_sk".into(),
                reason: crate::security_key::RefusalReason::WrongPin,
            },
        );
        crate::security_key::record_hand_out(paths.as_ref(), "h", 1, "/home/u/.ssh/id_sk");
        open_listing_pin_prompt(&mut app, "h");
        for c in "4242".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);

        assert!(app.password_prompt.is_none());
        assert_eq!(app.screen, Screen::FileBrowser { alias: "h".into() });
        assert!(app.file_browser_session.as_ref().unwrap().remote_loading);
        let trial = app.security_key_pins.trial().expect("trial");
        assert_eq!(trial.alias, "h");
        assert_eq!(trial.key_path, "/home/u/.ssh/id_sk");
        assert_eq!(trial.pin, "4242");
        assert_eq!(trial.flow, crate::security_key::PinFlow::FileBrowser);
        assert_eq!(
            crate::security_key::refusal_for(paths.as_ref(), "/home/u/.ssh/id_sk"),
            None,
            "the old refusal must not decide the new PIN"
        );
        assert!(
            crate::security_key::take_handed_out_keys(paths.as_ref(), "h").is_empty(),
            "an old hand-out must not prove the new PIN"
        );
        let toast = app.status_center.toast().expect("toast");
        assert_eq!(
            toast.text,
            crate::i18n::text(crate::messages::askpass::PIN_TOUCH_KEY)
        );
    }

    #[test]
    fn a_pin_whose_retry_cannot_start_is_not_kept_for_a_later_run() {
        // Demo mode keeps the tunnel retry from starting. Nothing carried the
        // PIN, so the next tunnel start asks for it again.
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_pin_prompt(&mut app, "h");
        for c in "4242".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.password_prompt.is_none());
        assert!(app.security_key_pins.trial().is_none());
        assert_ne!(
            app.status_center.toast().map(|t| t.text.as_str()),
            Some(crate::i18n::text(crate::messages::askpass::PIN_TOUCH_KEY)),
            "no key will blink"
        );
    }

    #[test]
    fn a_pin_for_a_key_that_left_the_list_is_not_kept() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.open_security_key_pin_prompt(
            "h",
            "/home/u/.ssh/id_sk",
            PendingRetry::KeyPush {
                key_path: "~/.ssh/gone.pub".into(),
                alias: "h".into(),
            },
            None,
            None,
        );
        for c in "4242".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.security_key_pins.trial().is_none());
    }

    #[test]
    fn a_pin_for_a_closed_file_browser_is_not_kept() {
        let mut app = app_with("Host h\n  HostName 127.0.0.1\n  Port 1\n");
        open_listing_pin_prompt(&mut app, "h");
        app.file_browser_session = None;
        for c in "4242".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.security_key_pins.trial().is_none());
    }

    #[test]
    fn esc_on_a_pin_prompt_says_no_pin_was_given() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        open_pin_prompt(&mut app, "h");
        press(&mut app, KeyCode::Esc);
        assert!(app.password_prompt.is_none());
        let toast = app.status_center.toast().expect("toast");
        assert_eq!(
            toast.text,
            crate::messages::askpass::pin_prompt_canceled("h")
        );
    }

    #[test]
    fn a_pin_prompt_over_a_tunnel_list_returns_to_it() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.screen = Screen::TunnelList { alias: "h".into() };
        open_pin_prompt(&mut app, "h");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::TunnelList { alias: "h".into() });
    }

    #[test]
    fn a_tunnel_retry_in_demo_mode_starts_nothing() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.demo_mode = true;
        let (tx, _rx) = mpsc::channel();
        assert!(!execute_retry(
            &mut app,
            PendingRetry::Tunnel { alias: "h".into() },
            false,
            &tx,
        ));
        assert!(!app.tunnels.active_contains("h"));
    }

    #[test]
    fn a_missing_state_returns_to_the_host_list_instead_of_stranding() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        app.screen = Screen::PasswordPrompt;
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.screen, Screen::HostList);
    }

    #[test]
    fn return_screen_prefers_an_open_file_browser() {
        let mut app = app_with("Host h\n  HostName 1.1.1.1\n");
        assert_eq!(return_screen(&app), Screen::HostList);
        app.file_browser_session = Some(crate::file_browser::FileBrowserSession {
            alias: "h".into(),
            askpass: None,
            active_pane: crate::file_browser::BrowserPane::Local,
            local_path: std::path::PathBuf::from("/tmp"),
            local_entries: Vec::new(),
            local_list_state: ratatui::widgets::ListState::default(),
            local_selected: std::collections::HashSet::new(),
            local_error: None,
            remote_path: "/home".into(),
            remote_entries: Vec::new(),
            remote_list_state: ratatui::widgets::ListState::default(),
            remote_selected: std::collections::HashSet::new(),
            remote_error: None,
            remote_loading: false,
            show_hidden: false,
            sort: crate::file_browser::BrowserSort::Name,
            confirm_copy: None,
            transferring: None,
            transfer_error: None,
            connection_recorded: false,
        });
        assert_eq!(
            return_screen(&app),
            Screen::FileBrowser { alias: "h".into() }
        );
    }
}
