//! Key handler for the Snippets tab (`TopPage::Snippets`, `Screen::HostList`).
//!
//! Navigation, search, CRUD (delegated to the shared snippet form/delete flow,
//! returning to the tab) and the snippet -> hosts run flow (opens the host
//! picker). Mirrors the other tab handlers' Tab/Shift+Tab and search rhythm.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, Screen};

pub(super) fn handle_key(app: &mut App, key: KeyEvent) {
    // A pending delete confirmation owns input until resolved (shared with the
    // host-list picker so the dialog behaves identically).
    if app.snippets.pending_delete().is_some() {
        super::snippet::confirm_pending_snippet_delete(app, key);
        return;
    }
    if app.search.query().is_some() {
        handle_search_key(app, key);
        return;
    }
    let count = crate::snippet::filtered_indices(app.snippets.store(), app.search.query()).len();
    match key.code {
        KeyCode::Tab => {
            app.cycle_top_page_next();
            app.search.set_query(None);
        }
        KeyCode::BackTab => {
            app.cycle_top_page_prev();
            app.search.set_query(None);
        }
        KeyCode::Char('j') | KeyCode::Down => move_cursor(app, count, 1),
        KeyCode::Char('k') | KeyCode::Up => move_cursor(app, count, -1),
        KeyCode::PageDown => move_cursor(app, count, 10),
        KeyCode::PageUp => move_cursor(app, count, -10),
        KeyCode::Home | KeyCode::Char('g') if count > 0 => {
            app.snippets.list_state_mut().select(Some(0));
        }
        KeyCode::End | KeyCode::Char('G') if count > 0 => {
            app.snippets.list_state_mut().select(Some(count - 1));
        }
        KeyCode::Char('/') => {
            app.search.set_query(Some(String::new()));
            if count > 0 {
                app.snippets.list_state_mut().select(Some(0));
            }
        }
        KeyCode::Char('v') => {
            app.snippets.toggle_view_mode();
            app.ui.set_detail_toggle_pending(true);
        }
        KeyCode::Char('?') => {
            // return_screen = HostList because TopPage::Snippets shares the
            // HostList screen variant; the help dispatcher reads app.top_page
            // to pick the tab-specific column content.
            app.set_screen(Screen::Help {
                return_screen: Box::new(Screen::HostList),
            });
        }
        KeyCode::Char('a') => super::snippet::open_add_form_for_tab(app),
        KeyCode::Char('e') => open_edit(app),
        KeyCode::Char('d') => request_delete(app),
        KeyCode::Enter | KeyCode::Char('r') => open_host_picker(app, false),
        KeyCode::Char('!') => open_host_picker(app, true),
        // Standard cross-tab keys, identical to the other overview tabs.
        KeyCode::Char(':') => {
            log::debug!("[purple] jump: opened from snippets overview");
            // Open in Snippets mode so the empty-state biases snippet actions to
            // the front, matching how the other tabs bias their own domain.
            app.open_jump(crate::app::JumpMode::Snippets);
        }
        KeyCode::Char('n') => {
            super::whats_new::dismiss_whats_new_toast(app);
            app.set_screen(Screen::WhatsNew(crate::app::WhatsNewState::default()));
        }
        KeyCode::Char('q') => {
            app.running = false;
        }
        KeyCode::Esc
            if !app.ui.esc_quit_hint_shown()
                && !app.status_center.toast().is_some_and(|t| t.sticky) =>
        {
            app.notify(crate::i18n::text(crate::messages::ESC_QUIT_HINT));
            app.ui.set_esc_quit_hint_shown(true);
        }
        _ => {}
    }
}

fn handle_search_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            // Esc clears the filter; Enter confirms it and stays on the list.
            // Either way input returns to the main keymap.
            if matches!(key.code, KeyCode::Esc) {
                app.search.set_query(None);
            }
        }
        KeyCode::Backspace => {
            if let Some(q) = app.search.query() {
                let mut q = q.to_string();
                q.pop();
                app.search.set_query(Some(q));
            }
        }
        KeyCode::Char(c) => {
            if let Some(q) = app.search.query() {
                let mut q = q.to_string();
                q.push(c);
                app.search.set_query(Some(q));
            }
        }
        _ => {}
    }
    // Keep the cursor in range as the filtered list shrinks/grows.
    let count = crate::snippet::filtered_indices(app.snippets.store(), app.search.query()).len();
    if count == 0 {
        app.snippets.list_state_mut().select(None);
    } else {
        app.snippets.list_state_mut().select(Some(0));
    }
}

fn move_cursor(app: &mut App, count: usize, delta: i64) {
    if count == 0 {
        return;
    }
    let cur = app.snippets.list_state().selected().unwrap_or(0) as i64;
    let next = (cur + delta).clamp(0, count as i64 - 1) as usize;
    app.snippets.list_state_mut().select(Some(next));
}

/// Resolve the snippet under the cursor, translating the filtered index back to
/// a store index, into `(store_idx, snippet)`.
fn selected_snippet(app: &App) -> Option<(usize, crate::snippet::Snippet)> {
    let indices = crate::snippet::filtered_indices(app.snippets.store(), app.search.query());
    let cursor = app.snippets.list_state().selected()?;
    let store_idx = *indices.get(cursor)?;
    let snippet = app.snippets.store().snippets.get(store_idx)?.clone();
    Some((store_idx, snippet))
}

fn open_edit(app: &mut App) {
    if let Some((store_idx, _)) = selected_snippet(app) {
        super::snippet::open_edit_form_for_tab(app, store_idx);
    }
}

fn request_delete(app: &mut App) {
    if let Some((store_idx, _)) = selected_snippet(app) {
        app.snippets.request_delete(store_idx);
    }
}

fn open_host_picker(app: &mut App, terminal: bool) {
    if app.hosts_state.list().is_empty() {
        app.notify_warning(crate::i18n::text(crate::messages::PICKER_NO_HOSTS));
        return;
    }
    let Some((_, snippet)) = selected_snippet(app) else {
        return;
    };
    app.snippets.set_flow_terminal(terminal);
    app.snippets.reset_host_pick();
    // Pre-select the snippet's saved default targets that still exist, so a
    // repeat run is just Enter -> confirm.
    let existing: std::collections::HashSet<String> = app
        .hosts_state
        .list()
        .iter()
        .map(|h| h.alias.clone())
        .collect();
    let saved: Vec<String> = app
        .snippets
        .store()
        .targets_for(&snippet.name)
        .iter()
        .filter(|a| existing.contains(*a))
        .cloned()
        .collect();
    for alias in saved {
        app.snippets.host_pick_mut().selected.insert(alias);
    }
    app.snippets.set_flow_snippet(Some(snippet));
    app.set_screen(Screen::SnippetHostPicker);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, TopPage};
    use crate::ssh_config::model::SshConfigFile;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_app(content: &str) -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: scratch.join("cfg"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        app.top_page = TopPage::Snippets;
        app.snippets.store_mut().snippets = vec![
            crate::snippet::Snippet {
                name: "deploy".into(),
                command: "make".into(),
                description: String::new(),
                interactive: false,
            },
            crate::snippet::Snippet {
                name: "uptime".into(),
                command: "uptime".into(),
                description: String::new(),
                interactive: false,
            },
        ];
        app.snippets.list_state_mut().select(Some(0));
        app
    }

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn tab_cycles_to_keys() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Tab));
        assert_eq!(app.top_page, TopPage::Keys);
    }

    #[test]
    fn backtab_cycles_to_containers() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::BackTab));
        assert_eq!(app.top_page, TopPage::Containers);
    }

    #[test]
    fn j_k_move_within_bounds() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char('j')));
        assert_eq!(app.snippets.list_state().selected(), Some(1));
        handle_key(&mut app, k(KeyCode::Char('j')));
        assert_eq!(app.snippets.list_state().selected(), Some(1));
        handle_key(&mut app, k(KeyCode::Char('k')));
        assert_eq!(app.snippets.list_state().selected(), Some(0));
    }

    #[test]
    fn v_toggles_view_mode_and_marks_anim_pending() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        let before = app.snippets.view_mode();
        handle_key(&mut app, k(KeyCode::Char('v')));
        assert_ne!(app.snippets.view_mode(), before);
        assert!(app.ui.detail_toggle_pending());
    }

    #[test]
    fn enter_opens_host_picker_with_chosen_snippet() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Enter));
        assert!(matches!(app.screen, Screen::SnippetHostPicker));
        assert_eq!(
            app.snippets.flow_snippet().map(|s| s.name.as_str()),
            Some("deploy")
        );
        assert!(!app.snippets.flow_terminal());
    }

    #[test]
    fn enter_preselects_saved_default_targets() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\nHost h2\n  HostName 2.2.2.2\n");
        // deploy is store index 0 in make_app; give it a saved default host.
        app.snippets
            .store_mut()
            .set_targets("deploy", vec!["h1".into()]);
        app.snippets.list_state_mut().select(Some(0));
        handle_key(&mut app, k(KeyCode::Enter));
        assert!(matches!(app.screen, Screen::SnippetHostPicker));
        assert!(app.snippets.host_pick().selected.contains("h1"));
        assert!(!app.snippets.host_pick().selected.contains("h2"));
    }

    #[test]
    fn enter_with_no_hosts_notifies_and_stays() {
        let mut app = make_app("");
        handle_key(&mut app, k(KeyCode::Enter));
        assert!(matches!(app.screen, Screen::HostList));
        assert!(app.status_center.toast().is_some());
    }

    #[test]
    fn e_opens_edit_form_for_selected_snippet_returning_to_tab() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        app.snippets.list_state_mut().select(Some(1)); // uptime (store idx 1)
        handle_key(&mut app, k(KeyCode::Char('e')));
        assert!(matches!(app.screen, Screen::SnippetForm));
        assert!(app.snippets.form_return_to_tab());
        assert_eq!(app.snippets.form_editing(), Some(1));
        assert_eq!(app.snippets.form().name, "uptime");
    }

    #[test]
    fn bang_opens_host_picker_in_terminal_mode() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char('!')));
        assert!(matches!(app.screen, Screen::SnippetHostPicker));
        assert!(app.snippets.flow_terminal());
    }

    #[test]
    fn r_opens_host_picker_like_enter() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char('r')));
        assert!(matches!(app.screen, Screen::SnippetHostPicker));
        assert!(!app.snippets.flow_terminal());
    }

    #[test]
    fn deleting_last_snippet_from_tab_clamps_cursor() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        // Cursor on the last of two snippets; delete it.
        app.snippets.list_state_mut().select(Some(1));
        handle_key(&mut app, k(KeyCode::Char('d')));
        assert_eq!(app.snippets.pending_delete(), Some(1));
        handle_key(&mut app, k(KeyCode::Char('y')));
        assert_eq!(app.snippets.store().snippets.len(), 1);
        assert_eq!(app.snippets.store().snippets[0].name, "deploy");
        // Tab cursor clamped back into range, not dangling at the old index 1.
        assert_eq!(app.snippets.list_state().selected(), Some(0));
    }

    #[test]
    fn slash_enters_search_mode() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char('/')));
        assert!(app.search.query().is_some());
    }

    #[test]
    fn search_filters_and_selected_snippet_resolves_store_index() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char('/')));
        for c in "uptime".chars() {
            handle_key(&mut app, k(KeyCode::Char(c)));
        }
        // Filtered list now holds only "uptime" (store index 1) at cursor 0.
        let (idx, snip) = selected_snippet(&app).expect("a snippet");
        assert_eq!(idx, 1);
        assert_eq!(snip.name, "uptime");
    }

    #[test]
    fn d_requests_delete_and_confirm_y_removes() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char('d')));
        assert_eq!(app.snippets.pending_delete(), Some(0));
        handle_key(&mut app, k(KeyCode::Char('y')));
        assert!(app.snippets.pending_delete().is_none());
        assert_eq!(app.snippets.store().snippets.len(), 1);
        assert_eq!(app.snippets.store().snippets[0].name, "uptime");
    }

    #[test]
    fn deleting_snippet_drops_its_run_ledger_entry() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        // Give "deploy" (store idx 0) a recorded run.
        app.snippets.runs_mut().record(
            "deploy",
            crate::snippet_runs::RunRecord {
                ts: 1,
                hosts: 2,
                ok: 2,
                failed: 0,
            },
        );
        assert_eq!(app.snippets.runs().run_count("deploy"), 1);
        app.snippets.list_state_mut().select(Some(0));
        handle_key(&mut app, k(KeyCode::Char('d')));
        handle_key(&mut app, k(KeyCode::Char('y')));
        // Snippet gone AND its ledger entry gone, so a recreated "deploy" starts
        // with a clean TRACK RECORD.
        assert!(app.snippets.store().get("deploy").is_none());
        assert_eq!(app.snippets.runs().run_count("deploy"), 0);
    }

    #[test]
    fn a_opens_add_form_returning_to_tab() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char('a')));
        assert!(matches!(app.screen, Screen::SnippetForm));
        assert!(app.snippets.form_return_to_tab());
    }

    #[test]
    fn question_opens_help_returning_to_host_list() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char('?')));
        match &app.screen {
            Screen::Help { return_screen } => {
                assert!(matches!(**return_screen, Screen::HostList));
            }
            other => panic!("expected Help, got {other:?}"),
        }
    }

    #[test]
    fn q_quits() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        assert!(app.running);
        handle_key(&mut app, k(KeyCode::Char('q')));
        assert!(!app.running);
    }

    #[test]
    fn colon_opens_jump_palette_in_snippets_mode() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char(':')));
        let jump = app.jump.as_ref().expect("jump open");
        assert_eq!(jump.mode(), crate::app::JumpMode::Snippets);
    }

    #[test]
    fn n_opens_whats_new() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char('n')));
        assert!(matches!(app.screen, Screen::WhatsNew(_)));
    }

    #[test]
    fn esc_on_idle_shows_quit_hint() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Esc));
        assert!(app.ui.esc_quit_hint_shown());
        assert!(app.status_center.toast().is_some());
    }
}
