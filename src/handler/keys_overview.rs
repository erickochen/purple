//! Key handler for the global Keys-tab overview (top_page = Keys).
//!
//! The Keys tab is a master-detail view of every SSH public key found in
//! `~/.ssh/` plus an optional Vault SSH cert TTL strip above the list.
//! All actions in this tab are non-destructive in v1: navigation,
//! clipboard copy of the selected public key, key push, and Vault
//! signing. Key discovery is automatic: purple re-runs `discover_keys`
//! after key pushes and host-list changes, so there is no manual
//! `R`-style reload binding.

use crossterm::event::{KeyCode, KeyEvent};
use log::debug;

use super::ctx::{Effectful, Effects, Nav, Notify};
use crate::app::{App, KeysState, Screen, SearchState, StatusCenter, UiSelection};
use crate::runtime::env::Env;

/// A narrow, explicit borrow of the App state the Keys-tab handlers touch. The
/// handlers operate on this slice instead of `&mut App`, so the compiler
/// rejects any reach into unrelated state (vault, containers, providers, ...).
/// Whole-App operations that cannot run on a slice (top-page cycling, the jump
/// overlay, the bulk Vault SSH sign, the push picker open whose unit tests
/// drive it on `&mut App`) are deferred as effects and applied to the full
/// `App` after the handler returns. Key pushes run on background threads that
/// own their inputs, so they need no deferred effect.
struct KeysCtx<'a> {
    keys: &'a mut KeysState,
    search: &'a mut SearchState,
    ui: &'a mut UiSelection,
    status: &'a mut StatusCenter,
    screen: &'a mut Screen,
    running: &'a mut bool,
    env: &'a Env,
    effects: Effects,
}

impl Nav for KeysCtx<'_> {
    fn screen_mut(&mut self) -> &mut Screen {
        self.screen
    }
}

impl Notify for KeysCtx<'_> {
    fn status_mut(&mut self) -> &mut StatusCenter {
        self.status
    }
}

impl Effectful for KeysCtx<'_> {
    fn effects_mut(&mut self) -> &mut Effects {
        &mut self.effects
    }
}

/// Borrow the disjoint App fields the Keys-tab handlers need into one slice.
fn ctx_from_app(app: &mut App) -> KeysCtx<'_> {
    KeysCtx {
        keys: &mut app.keys,
        search: &mut app.search,
        ui: &mut app.ui,
        status: &mut app.status_center,
        screen: &mut app.screen,
        running: &mut app.running,
        env: app.env.as_ref(),
        effects: Effects::default(),
    }
}

/// Dispatch a key event for the Keys overview tab. Routes to a dedicated
/// search sub-handler while a query is active so typing characters edits
/// the query instead of triggering the normal-mode shortcuts.
pub(super) fn handle_key(app: &mut App, key: KeyEvent) {
    // The in-flight push read is resolved here while we still hold `&App`, so
    // the Esc-cancel guard inside the slice handler is a plain bool.
    let in_flight = push_in_flight(app);
    let effects = {
        let mut ctx = ctx_from_app(app);
        keys_key(&mut ctx, key, in_flight);
        ctx.effects
    };
    effects.apply(app);
}

fn keys_key(ctx: &mut KeysCtx, key: KeyEvent, push_in_flight: bool) {
    if ctx.search.query().is_some() {
        handle_search_keys(ctx, key);
        return;
    }
    match key.code {
        KeyCode::Tab => {
            ctx.defer(App::cycle_top_page_next);
            ctx.search.set_query(None);
        }
        KeyCode::BackTab => {
            ctx.defer(App::cycle_top_page_prev);
            ctx.search.set_query(None);
        }
        KeyCode::Char('j') | KeyCode::Down | KeyCode::Right => {
            let len = ctx.keys.list().len();
            crate::app::cycle_selection(ctx.keys.list_state_mut(), len, true);
        }
        KeyCode::Char('k') | KeyCode::Up | KeyCode::Left => {
            let len = ctx.keys.list().len();
            crate::app::cycle_selection(ctx.keys.list_state_mut(), len, false);
        }
        KeyCode::PageDown => {
            let len = ctx.keys.list().len();
            crate::app::page_down(ctx.keys.list_state_mut(), len, 10);
        }
        KeyCode::PageUp => {
            let len = ctx.keys.list().len();
            crate::app::page_up(ctx.keys.list_state_mut(), len, 10);
        }
        KeyCode::Home | KeyCode::Char('g') if !ctx.keys.list().is_empty() => {
            ctx.keys.list_state_mut().select(Some(0));
        }
        KeyCode::End | KeyCode::Char('G') if !ctx.keys.list().is_empty() => {
            let last = ctx.keys.list().len() - 1;
            ctx.keys.list_state_mut().select(Some(last));
        }
        // Enter and `c` both copy the selected pubkey. Enter is the
        // advertised primary in the footer; `c` is the muscle-memory
        // shortcut from picker overlays and the broader CLI ecosystem.
        KeyCode::Enter | KeyCode::Char('c') => {
            copy_selected_pubkey(ctx);
        }
        KeyCode::Char('p') => {
            // The picker open reads the selected key + host list and sets the
            // screen, all slice-local, but the unit tests drive it directly on
            // `&mut App`, so it keeps the whole-App signature and is deferred.
            // It is the only action in this arm, so running after the slice
            // borrow ends observes the same state as running inline.
            ctx.defer(open_push_picker);
        }
        // Bulk Vault SSH sign: same entry point the host list uses,
        // shared so the action stays consistent between tabs. Becomes a
        // no-op with a friendly notify when no host has a vault-ssh
        // role configured.
        KeyCode::Char('V') => {
            ctx.defer(super::host_list::actions::initiate_bulk_vault_sign);
        }
        KeyCode::Char('/') => {
            // Enter search mode. We deliberately do not reuse
            // `App::start_search()` because that helper drives the
            // hosts-specific `filtered_indices` state machine; the Keys
            // tab filters at render time and only needs the query string.
            ctx.search.set_query(Some(String::new()));
            // Reset selection so we always land on the first match.
            if !ctx.keys.list().is_empty() {
                ctx.keys.list_state_mut().select(Some(0));
            }
            log::debug!("[purple] keys: opened search");
        }
        KeyCode::Char(':') => {
            log::debug!("[purple] jump: opened from keys overview");
            ctx.defer(|app| app.open_jump(crate::app::JumpMode::Keys));
        }
        KeyCode::Char('n') => {
            // Match host-list and tunnels: dismiss the upgrade toast and
            // open the What's New overlay so release notes are reachable
            // from any main tab.
            let fragment = crate::i18n::text(crate::messages::whats_new_toast::INVITE_FRAGMENT);
            ctx.status.drop_toasts_where(|t| t.text.contains(fragment));
            ctx.set_screen(Screen::WhatsNew(crate::app::WhatsNewState::default()));
        }
        KeyCode::Char('?') => {
            ctx.set_screen(Screen::Help {
                return_screen: Box::new(Screen::HostList),
            });
        }
        KeyCode::Char('q') => {
            *ctx.running = false;
        }
        // Esc while a push is in flight cancels the run. Higher priority
        // than the q-hint toast because cancelling is the only Esc-shaped
        // affordance the user has during a long push.
        KeyCode::Esc if push_in_flight => {
            cancel_push_if_running(ctx);
        }
        // Mirrors host-list / tunnels-overview policy: idle Esc never quits.
        // The first idle press surfaces a one-shot toast pointing to `q`;
        // the flag is shared across tabs so the hint shows at most once per
        // session. The sticky-toast guard skips the hint when a sticky toast
        // is active so an informational nudge cannot displace a sticky error.
        KeyCode::Esc
            if !ctx.ui.esc_quit_hint_shown() && !ctx.status.toast().is_some_and(|t| t.sticky) =>
        {
            log::debug!("[purple] esc on idle keys overview, showing quit hint toast");
            ctx.notify(crate::i18n::text(crate::messages::ESC_QUIT_HINT));
            ctx.ui.set_esc_quit_hint_shown(true);
        }
        _ => {}
    }
}

/// True iff a push run is currently in flight (worker spawned, not yet
/// finalised). Used as the Esc-guard so cancel only fires when there is
/// something to cancel.
pub(super) fn push_in_flight(app: &App) -> bool {
    app.keys.push().expected_count > 0 && app.keys.push().cancel.is_some()
}

/// Cancel an in-flight push run. Sets the cancel flag (so the worker
/// short-circuits at its next iteration), bumps `run_id` so any tail
/// events from the cancelled worker are dropped on arrival, and emits a
/// toast naming the per-host progress at the moment of cancel.
///
/// The worker handle is intentionally NOT cleared here: the thread may
/// still be inside a `wait_with_output` for the in-flight ssh (bounded
/// by `ServerAliveInterval × CountMax = 30s`). `start_key_push` refuses
/// to launch a second worker until `worker.is_finished()`; `App::drop`
/// joins on exit.
fn cancel_push_if_running(ctx: &mut KeysCtx) {
    let done = ctx.keys.push().results.len();
    let total = ctx.keys.push().expected_count;
    log::debug!(
        "[purple] key_push: cancel requested, done={}/{}",
        done,
        total
    );
    // Raise the flag, kill the ssh child that is running right now, clear
    // the accumulators and bump run_id so any KeyPushResult event still in
    // flight from the aborted worker is dropped on arrival.
    ctx.keys.push_mut().cancel_run();
    ctx.defer(|app| {
        app.abandon_security_key_trial(crate::security_key::PinFlow::KeyPush, None);
    });
    // Drop the progress toast through the status-center invariant so the
    // cancel message is unambiguously the latest status.
    ctx.status.clear_sticky_status();
    ctx.notify(crate::messages::key_push_cancelled(done, total));
}

/// Search-mode sub-handler. Typing edits the query, navigation keys move
/// through the filtered list, Esc cancels (clears query), Enter commits
/// (copies the highlighted match and clears the query). Tab/BackTab also
/// exit search-mode before cycling tabs.
fn handle_search_keys(ctx: &mut KeysCtx, key: KeyEvent) {
    let filtered = crate::ssh_keys::filtered_key_indices(ctx.keys.list(), ctx.search.query());
    let count = filtered.len();
    match key.code {
        KeyCode::Esc => {
            ctx.search.set_query(None);
            // Restore selection to first key so navigation feels stable
            // when the user re-opens the same view.
            if !ctx.keys.list().is_empty() {
                ctx.keys.list_state_mut().select(Some(0));
            } else {
                ctx.keys.list_state_mut().select(None);
            }
        }
        KeyCode::Enter => {
            // Copy the currently highlighted match, then exit search.
            copy_selected_pubkey(ctx);
            ctx.search.set_query(None);
        }
        KeyCode::Tab => {
            ctx.search.set_query(None);
            ctx.defer(App::cycle_top_page_next);
        }
        KeyCode::BackTab => {
            ctx.search.set_query(None);
            ctx.defer(App::cycle_top_page_prev);
        }
        KeyCode::Down | KeyCode::Right if count > 0 => {
            let cur = ctx.keys.list_state().selected().unwrap_or(0);
            ctx.keys
                .list_state_mut()
                .select(Some((cur + 1).min(count - 1)));
        }
        KeyCode::Up | KeyCode::Left if count > 0 => {
            let cur = ctx.keys.list_state().selected().unwrap_or(0);
            ctx.keys
                .list_state_mut()
                .select(Some(cur.saturating_sub(1)));
        }
        KeyCode::PageDown => {
            crate::app::page_down(ctx.keys.list_state_mut(), count, 10);
        }
        KeyCode::PageUp => {
            crate::app::page_up(ctx.keys.list_state_mut(), count, 10);
        }
        KeyCode::Backspace => {
            ctx.search.pop_query_char();
            // Re-anchor selection to the first match after the query shrinks.
            let new_count =
                crate::ssh_keys::filtered_key_indices(ctx.keys.list(), ctx.search.query()).len();
            if new_count == 0 {
                ctx.keys.list_state_mut().select(None);
            } else {
                ctx.keys.list_state_mut().select(Some(0));
            }
        }
        KeyCode::Char(c) => {
            ctx.search.push_query_char(c);
            let new_count =
                crate::ssh_keys::filtered_key_indices(ctx.keys.list(), ctx.search.query()).len();
            if new_count == 0 {
                ctx.keys.list_state_mut().select(None);
            } else {
                ctx.keys.list_state_mut().select(Some(0));
            }
        }
        _ => {}
    }
}

/// Read the selected key's public key file and push it to the clipboard.
/// Toasts on success and on every error path so the user always gets feedback.
///
/// When search is active, `key_list_state.selected()` is an index into the
/// filtered list, so we translate back through `filtered_key_indices`
/// before looking up the underlying `SshKeyInfo`.
fn copy_selected_pubkey(ctx: &mut KeysCtx) {
    let Some(sel) = ctx.keys.list_state().selected() else {
        return;
    };
    let Some(idx) = crate::ssh_keys::resolve_selection(ctx.keys.list(), ctx.search.query(), sel)
    else {
        return;
    };
    let Some(key_info) = ctx.keys.list().get(idx) else {
        return;
    };
    let pub_path = format!("{}.pub", key_info.display_path);
    let expanded = expand_tilde(ctx.env.paths(), &pub_path);
    let body = match std::fs::read_to_string(&expanded) {
        Ok(s) => s,
        Err(e) => {
            debug!(
                "[purple] keys: read pubkey failed path={} err={}",
                expanded, e
            );
            ctx.notify_error(crate::messages::keys_copy_read_failed(&key_info.name));
            return;
        }
    };
    let name = key_info.name.clone();
    match crate::clipboard::copy_to_clipboard(body.trim_end()) {
        Ok(()) => {
            debug!("[purple] keys: copied pubkey for {}", name);
            ctx.notify(crate::messages::keys_copy_success(&name));
        }
        Err(e) => {
            debug!("[purple] keys: clipboard copy failed: {}", e);
            ctx.notify_error(e);
        }
    }
}

/// Open the multi-host picker for the currently highlighted key. When
/// search is active, translate the filtered index to the underlying
/// `app.keys.list` index so the picker title and the eventual confirm dialog
/// name the right key.
fn open_push_picker(app: &mut App) {
    let Some(sel) = app.keys.list_state().selected() else {
        return;
    };
    let Some(key_index) =
        crate::ssh_keys::resolve_selection(app.keys.list(), app.search.query(), sel)
    else {
        return;
    };
    if app.keys.list().get(key_index).is_none() {
        return;
    }
    // Guard: pushing to zero hosts surfaces an empty picker, which
    // reads as a bug. Notify and short-circuit so the picker only
    // opens when it has something to pick from. Matches the
    // tunnels-tab / containers-tab guard pattern.
    if app.hosts_state.list().is_empty() {
        app.notify_warning(crate::i18n::text(crate::messages::PICKER_NO_HOSTS));
        return;
    }
    // Fresh picker: drop any leftover selection from a prior run.
    app.keys.push_mut().reset_picker();
    app.set_screen(Screen::KeyPushPicker { key_index });
    log::debug!("[purple] keys: opened push picker for index={}", key_index);
}

/// Expand a leading `~/` to the injected home directory. Unchanged otherwise.
fn expand_tilde(paths: Option<&crate::runtime::env::Paths>, p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(paths) = paths
    {
        return paths.home().join(rest).display().to_string();
    }
    p.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::ssh_config::model::SshConfigFile;
    use crate::ssh_keys::SshKeyInfo;
    use crossterm::event::KeyModifiers;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn expand_tilde_replaces_prefix() {
        let paths = crate::runtime::env::Paths::new("/home/u");
        let result = expand_tilde(Some(&paths), "~/.ssh/id_ed25519.pub");
        assert_eq!(result, "/home/u/.ssh/id_ed25519.pub");
        assert!(!result.starts_with('~'));
    }

    #[test]
    fn expand_tilde_passthrough_for_absolute() {
        assert_eq!(
            expand_tilde(None, "/tmp/id_ed25519.pub"),
            "/tmp/id_ed25519.pub"
        );
    }

    #[test]
    fn expand_tilde_passthrough_for_relative() {
        assert_eq!(
            expand_tilde(None, "keys/id_ed25519.pub"),
            "keys/id_ed25519.pub"
        );
    }

    fn key(name: &str) -> SshKeyInfo {
        SshKeyInfo {
            name: name.to_string(),
            display_path: format!("~/.ssh/{}", name),
            key_type: "ED25519".into(),
            bits: "256".into(),
            fingerprint: String::new(),
            comment: String::new(),
            linked_hosts: vec![],
            bishop_art: String::new(),
            strength_score: 90,
            encrypted: true,
            agent_loaded: false,
            is_certificate: false,
            mtime_ts: None,
        }
    }

    fn make_app() -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(""),
            path: scratch.join("test_config"),
            crlf: false,
            bom: false,
        };
        App::new(config)
    }

    fn k(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    /// Seed a single host so `open_push_picker` clears its empty-hosts
    /// guard. Tests that exercise filtered-index translation /
    /// picker-state reset do not care about the host count itself.
    fn seed_one_host(app: &mut App) {
        app.hosts_state
            .list_mut()
            .push(crate::ssh_config::model::HostEntry {
                alias: "h1".into(),
                ..Default::default()
            });
    }

    #[test]
    fn open_push_picker_under_search_translates_filtered_index() {
        let mut app = make_app();
        seed_one_host(&mut app);
        app.keys.set_list(vec![
            key("id_ed25519"),
            key("yubikey_work"),
            key("customer-x"),
        ]);
        app.search.set_query(Some("yubi".to_string()));
        // After applying the filter, position 0 in the visible list is
        // "yubikey_work" which is index 1 in app.keys.list.
        app.keys.list_state_mut().select(Some(0));
        open_push_picker(&mut app);
        match app.screen {
            Screen::KeyPushPicker { key_index } => {
                assert_eq!(
                    key_index, 1,
                    "filtered idx 0 must map to app.keys.list() idx 1"
                );
            }
            ref other => panic!("expected KeyPushPicker, got {:?}", other),
        }
    }

    #[test]
    fn open_push_picker_resets_picker_state() {
        let mut app = make_app();
        seed_one_host(&mut app);
        app.keys.set_list(vec![key("id_ed25519")]);
        app.keys.list_state_mut().select(Some(0));
        // Pre-existing stale selection from a previous picker run.
        app.keys.push_mut().selected.insert("old-host".to_string());
        open_push_picker(&mut app);
        assert!(
            app.keys.push().selected.is_empty(),
            "selection must be reset on new picker open"
        );
    }

    #[test]
    fn push_in_flight_true_only_when_cancel_and_expected_set() {
        let mut app = make_app();
        assert!(!push_in_flight(&app));
        app.keys.push_mut().expected_count = 3;
        assert!(
            !push_in_flight(&app),
            "expected_count alone is not in-flight"
        );
        app.keys.push_mut().cancel = Some(Arc::new(AtomicBool::new(false)));
        assert!(push_in_flight(&app), "both fields set: in flight");
        app.keys.push_mut().cancel = None;
        assert!(!push_in_flight(&app));
    }

    #[test]
    fn esc_cancels_in_flight_push_clears_state() {
        let mut app = make_app();
        // Seed an in-flight push.
        let flag = Arc::new(AtomicBool::new(false));
        app.keys.push_mut().cancel = Some(flag.clone());
        app.keys.push_mut().expected_count = 5;
        app.keys
            .push_mut()
            .results
            .push(crate::key_push::KeyPushResult {
                alias: "h1".into(),
                outcome: crate::key_push::KeyPushOutcome::Appended,
            });
        app.keys.push_mut().selected.insert("h1".to_string());
        // Esc should observe push_in_flight and cancel.
        handle_key(&mut app, k(KeyCode::Esc));
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(app.keys.push().expected_count, 0);
        assert!(app.keys.push().results.is_empty());
        assert!(app.keys.push().cancel.is_none());
        assert!(app.keys.push().selected.is_empty());
        // Cancel toast surfaced.
        assert!(app.status_center.toast().is_some());
    }

    #[test]
    fn esc_cancel_drops_the_pin_on_trial_for_the_push() {
        let mut app = make_app();
        app.keys.push_mut().cancel = Some(Arc::new(AtomicBool::new(false)));
        app.keys.push_mut().expected_count = 1;
        app.security_key_pins.start_trial(crate::app::PinTrial {
            alias: "h1".into(),
            key_path: "/k".into(),
            pin: "4242".into(),
            flow: crate::security_key::PinFlow::KeyPush,
        });
        handle_key(&mut app, k(KeyCode::Esc));
        assert!(
            app.security_key_pins.trial().is_none(),
            "a new push asks for the PIN again"
        );
    }

    // --- Arrow-key navigation (j/k aliases via Left/Right) ---

    #[test]
    fn right_arrow_advances_key_selection() {
        let mut app = make_app();
        app.keys.set_list(vec![key("a"), key("b"), key("c")]);
        app.keys.list_state_mut().select(Some(0));
        handle_key(&mut app, k(KeyCode::Right));
        assert_eq!(app.keys.list_state().selected(), Some(1));
    }

    #[test]
    fn left_arrow_retreats_key_selection() {
        let mut app = make_app();
        app.keys.set_list(vec![key("a"), key("b"), key("c")]);
        app.keys.list_state_mut().select(Some(2));
        handle_key(&mut app, k(KeyCode::Left));
        assert_eq!(app.keys.list_state().selected(), Some(1));
    }

    #[test]
    fn right_arrow_at_end_wraps_to_first() {
        // select_next_key wraps modulo, matching the j/k behaviour we
        // preserve via the alias.
        let mut app = make_app();
        app.keys.set_list(vec![key("a"), key("b")]);
        app.keys.list_state_mut().select(Some(1));
        handle_key(&mut app, k(KeyCode::Right));
        assert_eq!(app.keys.list_state().selected(), Some(0));
    }

    #[test]
    fn left_arrow_at_start_wraps_to_last() {
        let mut app = make_app();
        app.keys.set_list(vec![key("a"), key("b")]);
        app.keys.list_state_mut().select(Some(0));
        handle_key(&mut app, k(KeyCode::Left));
        assert_eq!(app.keys.list_state().selected(), Some(1));
    }

    // --- Dispatcher coverage: navigation and search (H12) ---

    #[test]
    fn slash_opens_search_and_resets_selection() {
        let mut app = make_app();
        app.keys.set_list(vec![key("a"), key("b"), key("c")]);
        app.keys.list_state_mut().select(Some(2));
        handle_key(&mut app, k(KeyCode::Char('/')));
        assert_eq!(app.search.query(), Some(""));
        assert_eq!(
            app.keys.list_state().selected(),
            Some(0),
            "search must land cursor on the first match"
        );
    }

    #[test]
    fn search_typing_appends_to_query() {
        let mut app = make_app();
        app.keys.set_list(vec![key("alpha"), key("bravo")]);
        handle_key(&mut app, k(KeyCode::Char('/')));
        handle_key(&mut app, k(KeyCode::Char('a')));
        handle_key(&mut app, k(KeyCode::Char('l')));
        assert_eq!(app.search.query(), Some("al"));
    }

    #[test]
    fn search_esc_clears_query() {
        let mut app = make_app();
        app.keys.set_list(vec![key("alpha")]);
        handle_key(&mut app, k(KeyCode::Char('/')));
        handle_key(&mut app, k(KeyCode::Char('a')));
        handle_key(&mut app, k(KeyCode::Esc));
        assert!(app.search.query().is_none(), "Esc must close search");
    }

    #[test]
    fn search_backspace_on_empty_query_is_noop_and_keeps_search_open() {
        // Backspace on an empty query pop()s a no-op string but does NOT
        // close search mode. The user can keep typing to refine the query.
        // Esc is the explicit "close search" affordance.
        let mut app = make_app();
        app.keys.set_list(vec![key("alpha")]);
        handle_key(&mut app, k(KeyCode::Char('/')));
        handle_key(&mut app, k(KeyCode::Backspace));
        assert_eq!(app.search.query(), Some(""));
        // Cursor must remain on a valid match index when filtered list is non-empty.
        assert_eq!(app.keys.list_state().selected(), Some(0));
    }

    #[test]
    fn tab_cycles_to_next_top_page_and_closes_search() {
        let mut app = make_app();
        app.top_page = crate::app::TopPage::Keys;
        app.search.set_query(None);
        handle_key(&mut app, k(KeyCode::Tab));
        assert!(!matches!(app.top_page, crate::app::TopPage::Keys));
    }

    #[test]
    fn tab_in_search_mode_exits_search_before_cycling() {
        let mut app = make_app();
        app.top_page = crate::app::TopPage::Keys;
        app.keys.set_list(vec![key("alpha")]);
        handle_key(&mut app, k(KeyCode::Char('/')));
        handle_key(&mut app, k(KeyCode::Tab));
        assert!(app.search.query().is_none());
        assert!(!matches!(app.top_page, crate::app::TopPage::Keys));
    }

    #[test]
    fn q_quits_the_app() {
        let mut app = make_app();
        assert!(app.running);
        handle_key(&mut app, k(KeyCode::Char('q')));
        assert!(!app.running);
    }

    #[test]
    fn copy_pubkey_on_empty_list_is_noop() {
        // Pressing Enter on an empty Keys tab must not panic. Earlier
        // versions indexed app.keys.list[0] in the copy path.
        let mut app = make_app();
        app.keys.list_mut().clear();
        app.keys.list_state_mut().select(None);
        handle_key(&mut app, k(KeyCode::Enter));
        // The presence of any toast is fine; the invariant is "no panic".
    }

    #[test]
    fn n_opens_whats_new_overlay() {
        let mut app = make_app();
        app.keys.set_list(vec![key("a")]);
        handle_key(&mut app, k(KeyCode::Char('n')));
        assert!(matches!(app.screen, Screen::WhatsNew(_)));
    }
}
