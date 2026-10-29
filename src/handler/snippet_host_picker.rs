//! Key handler for `Screen::SnippetHostPicker`.
//!
//! Multi-select host picker reached from the Snippets tab to run the chosen
//! snippet against one or more hosts. Selection lives on
//! `app.snippets.host_pick().selected`; the chosen snippet on
//! `app.snippets.flow_snippet`. Mirrors `key_push_picker` minus the vault
//! filtering, because any host can run a snippet.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, HostListItem, Screen, SnippetHostPickPurpose};

/// The picker's rows. With no filter, every host grouped exactly the way the
/// host list is (via `App::grouped_all_hosts`): `GroupBy::None` yields a flat
/// host list, a provider/tag grouping yields group-header rows with their hosts
/// beneath. With a filter active, a flat list of matching hosts ranked by the
/// fuzzy matcher (groups are dropped: ranked results read like the jump
/// palette, best first).
pub(crate) fn picker_rows(app: &App) -> Vec<HostListItem> {
    let query = &app.snippets.host_pick().query;
    if query.is_empty() {
        app.grouped_all_hosts()
    } else {
        fuzzy_host_rows(app, query)
    }
}

/// Flat, fuzzy-ranked Host rows matching `query` (alias / hostname / user /
/// provider / tags), best first.
fn fuzzy_host_rows(app: &App, query: &str) -> Vec<HostListItem> {
    let candidates: Vec<usize> = (0..app.hosts_state.list().len()).collect();
    crate::fuzzy::rank_host_indices(app.hosts_state.list(), &candidates, query)
        .into_iter()
        .map(|index| HostListItem::Host { index })
        .collect()
}

/// Alias for a Host/Pattern row; `None` for a group header.
pub(crate) fn row_alias(app: &App, row: &HostListItem) -> Option<String> {
    match row {
        HostListItem::Host { index } | HostListItem::Pattern { index } => {
            app.hosts_state.list().get(*index).map(|h| h.alias.clone())
        }
        HostListItem::GroupHeader(_) => None,
    }
}

/// Member aliases of the group whose header is at `header_idx`: the Host/Pattern
/// rows that follow it until the next group header. Shared with the picker UI so
/// the aggregate group checkbox and the selection toggle compute membership the
/// same way.
pub(crate) fn group_members(app: &App, rows: &[HostListItem], header_idx: usize) -> Vec<String> {
    rows[header_idx + 1..]
        .iter()
        .take_while(|r| !matches!(r, HostListItem::GroupHeader(_)))
        .filter_map(|r| row_alias(app, r))
        .collect()
}

pub(super) fn handle_key(app: &mut App, key: KeyEvent) {
    // Filter mode: keystrokes edit the query (live fuzzy filter) instead of
    // acting as selection commands. Entered with `/`, left with Enter/Esc.
    if app.snippets.host_pick().filtering {
        handle_filter_key(app, key);
        return;
    }
    let rows = picker_rows(app);
    let row_count = rows.len();
    match key.code {
        KeyCode::Char('/') => {
            app.snippets.host_pick_mut().query.clear();
            app.snippets.host_pick_mut().filtering = true;
            app.snippets.host_pick_mut().list_state.select(Some(0));
        }
        KeyCode::Esc => {
            // Esc peels back the filter first, then leaves the picker. Where it
            // returns depends on why the picker was opened.
            if !app.snippets.host_pick().query.is_empty() {
                app.snippets.host_pick_mut().query.clear();
                app.snippets.host_pick_mut().list_state.select(Some(0));
            } else {
                match app.snippets.host_pick().purpose {
                    SnippetHostPickPurpose::EditDefault => {
                        // Back to the edit form, leaving its default hosts unchanged.
                        app.snippets.set_flow_snippet(None);
                        app.set_screen(Screen::SnippetForm);
                    }
                    SnippetHostPickPurpose::Run => {
                        app.snippets.host_pick_mut().selected.clear();
                        app.snippets.set_flow_snippet(None);
                        app.set_screen(Screen::HostList);
                    }
                }
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if row_count == 0 {
                return;
            }
            let cur = app.snippets.host_pick().list_state.selected().unwrap_or(0);
            app.snippets
                .host_pick_mut()
                .list_state
                .select(Some((cur + 1).min(row_count - 1)));
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if row_count == 0 {
                return;
            }
            let cur = app.snippets.host_pick().list_state.selected().unwrap_or(0);
            app.snippets
                .host_pick_mut()
                .list_state
                .select(Some(cur.saturating_sub(1)));
        }
        KeyCode::PageDown => {
            crate::app::page_down(&mut app.snippets.host_pick_mut().list_state, row_count, 10);
        }
        KeyCode::PageUp => {
            crate::app::page_up(&mut app.snippets.host_pick_mut().list_state, row_count, 10);
        }
        KeyCode::Home | KeyCode::Char('g') if row_count > 0 => {
            app.snippets.host_pick_mut().list_state.select(Some(0));
        }
        KeyCode::End | KeyCode::Char('G') if row_count > 0 => {
            app.snippets
                .host_pick_mut()
                .list_state
                .select(Some(row_count - 1));
        }
        // SPACE GUARD MUST PRECEDE any generic Char(c) arm: Space toggles the
        // focused host. The literal-text Char(c) arm lives in handle_filter_key
        // (only reached while filtering), so there is no live conflict today,
        // but keep Space ahead of any future catch-all merged into this match.
        KeyCode::Char(' ') => toggle_at_cursor(app, &rows),
        KeyCode::Char('a') | KeyCode::Char('A') => toggle_select_all(app, &rows),
        KeyCode::Enter => commit(app, false),
        KeyCode::Char('!') => commit(app, true),
        _ => {}
    }
}

/// Keystrokes while the type-to-filter input is active. The query filters the
/// host rows live (fuzzy). Enter keeps the filter and leaves edit mode (so
/// Space/`a` act on the visible matches); Esc clears the filter and leaves.
fn handle_filter_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.snippets.host_pick_mut().query.clear();
            app.snippets.host_pick_mut().filtering = false;
            app.snippets.host_pick_mut().list_state.select(Some(0));
        }
        KeyCode::Enter => {
            app.snippets.host_pick_mut().filtering = false;
            app.snippets.host_pick_mut().list_state.select(Some(0));
        }
        KeyCode::Backspace => {
            app.snippets.host_pick_mut().query.pop();
            app.snippets.host_pick_mut().list_state.select(Some(0));
        }
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && app.snippets.host_pick().query.chars().count() < 64 =>
        {
            app.snippets.host_pick_mut().query.push(c);
            app.snippets.host_pick_mut().list_state.select(Some(0));
        }
        _ => {}
    }
}

/// Space on a host row toggles that host; Space on a group header toggles every
/// member of that group (select-all-or-clear within the group).
fn toggle_at_cursor(app: &mut App, rows: &[HostListItem]) {
    let Some(i) = app.snippets.host_pick().list_state.selected() else {
        return;
    };
    match rows.get(i) {
        Some(HostListItem::GroupHeader(name)) => {
            let members = group_members(app, rows, i);
            toggle_group(app, &members);
            log::debug!(
                "[purple] snippet host picker: toggled group {} ({} members, now {} selected)",
                name,
                members.len(),
                app.snippets.host_pick().selected.len()
            );
        }
        Some(row) => {
            let Some(alias) = row_alias(app, row) else {
                return;
            };
            if app.snippets.host_pick().selected.contains(&alias) {
                app.snippets.host_pick_mut().selected.remove(&alias);
            } else {
                app.snippets.host_pick_mut().selected.insert(alias.clone());
            }
            log::debug!(
                "[purple] snippet host picker: toggled {} (now {} selected)",
                alias,
                app.snippets.host_pick().selected.len()
            );
        }
        None => {}
    }
}

/// Select every member of a group, or clear them all when they are already
/// fully selected.
fn toggle_group(app: &mut App, members: &[String]) {
    if members.is_empty() {
        return;
    }
    let all_selected = members
        .iter()
        .all(|a| app.snippets.host_pick().selected.contains(a));
    if all_selected {
        for alias in members {
            app.snippets.host_pick_mut().selected.remove(alias);
        }
    } else {
        for alias in members {
            app.snippets.host_pick_mut().selected.insert(alias.clone());
        }
    }
}

/// `a` toggle: if every host is already selected, clear; else select all.
fn toggle_select_all(app: &mut App, rows: &[HostListItem]) {
    let aliases: Vec<String> = rows.iter().filter_map(|r| row_alias(app, r)).collect();
    if aliases.is_empty() {
        return;
    }
    let all_selected = aliases
        .iter()
        .all(|a| app.snippets.host_pick().selected.contains(a));
    if all_selected {
        app.snippets.host_pick_mut().selected.clear();
        log::debug!("[purple] snippet host picker: cleared all selections");
    } else {
        for alias in aliases {
            app.snippets.host_pick_mut().selected.insert(alias);
        }
        log::debug!(
            "[purple] snippet host picker: selected all {} hosts",
            app.snippets.host_pick().selected.len()
        );
    }
}

/// Enter / `!`: commit the selection. In `Run` mode it freezes into
/// `flow_targets` and opens the confirm dialog (empty selection notifies and
/// stays). In `EditDefault` mode it writes the selection to the edit form's
/// `default_hosts` and returns to the form (an empty selection is valid and
/// clears the defaults).
///
/// The selection is collected in host-list order, not filtered-row order, so it
/// is independent of any active filter: a host selected under one filter is
/// never silently dropped when committing under another.
fn commit(app: &mut App, terminal: bool) {
    let aliases: Vec<String> = app
        .hosts_state
        .list()
        .iter()
        .map(|h| h.alias.clone())
        .filter(|a| app.snippets.host_pick().selected.contains(a))
        .collect();

    match app.snippets.host_pick().purpose {
        SnippetHostPickPurpose::EditDefault => {
            log::debug!(
                "[purple] snippet edit form: default hosts set to {} host(s)",
                aliases.len()
            );
            app.snippets.form_mut().default_hosts = aliases;
            app.snippets.set_flow_snippet(None);
            app.set_screen(Screen::SnippetForm);
        }
        SnippetHostPickPurpose::Run => {
            if aliases.is_empty() {
                app.notify_warning(crate::i18n::text(crate::messages::PICKER_NONE_SELECTED));
                return;
            }
            log::debug!(
                "[purple] snippet host picker: committed {} hosts (terminal={})",
                aliases.len(),
                terminal
            );
            app.snippets.set_flow_targets(aliases);
            app.snippets.set_flow_terminal(terminal);
            app.set_screen(Screen::ConfirmRunSnippet);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::ssh_config::model::SshConfigFile;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_app(content: &str) -> App {
        let scratch = tempfile::tempdir().expect("tempdir").keep();
        let config = SshConfigFile {
            elements: SshConfigFile::parse_content(content),
            path: scratch.join("test_config"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        app.snippets.set_flow_snippet(Some(crate::snippet::Snippet {
            name: "deploy".into(),
            command: "make deploy".into(),
            description: String::new(),
            interactive: false,
        }));
        app.set_screen(Screen::SnippetHostPicker);
        app.snippets.host_pick_mut().list_state.select(Some(0));
        app
    }

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn space_toggles_selection() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\nHost h2\n  HostName 2.2.2.2\n");
        handle_key(&mut app, k(KeyCode::Char(' ')));
        assert!(app.snippets.host_pick().selected.contains("h1"));
        handle_key(&mut app, k(KeyCode::Char(' ')));
        assert!(!app.snippets.host_pick().selected.contains("h1"));
    }

    #[test]
    fn a_selects_all_then_clears() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\nHost h2\n  HostName 2.2.2.2\n");
        handle_key(&mut app, k(KeyCode::Char('a')));
        assert_eq!(app.snippets.host_pick().selected.len(), 2);
        handle_key(&mut app, k(KeyCode::Char('a')));
        assert!(app.snippets.host_pick().selected.is_empty());
    }

    #[test]
    fn enter_with_empty_selection_notifies_and_stays() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Enter));
        assert!(matches!(app.screen, Screen::SnippetHostPicker));
        assert!(app.status_center.toast().is_some());
    }

    #[test]
    fn enter_commits_in_picker_order_and_opens_confirm() {
        let mut app = make_app(
            "Host alpha\n  HostName 1.1.1.1\nHost beta\n  HostName 2.2.2.2\nHost gamma\n  HostName 3.3.3.3\n",
        );
        app.snippets.host_pick_mut().selected.insert("gamma".into());
        app.snippets.host_pick_mut().selected.insert("alpha".into());
        handle_key(&mut app, k(KeyCode::Enter));
        assert!(matches!(app.screen, Screen::ConfirmRunSnippet));
        assert_eq!(app.snippets.flow_targets(), &["alpha", "gamma"]);
        assert!(!app.snippets.flow_terminal());
    }

    #[test]
    fn bang_commits_with_terminal_flag() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        app.snippets.host_pick_mut().selected.insert("h1".into());
        handle_key(&mut app, k(KeyCode::Char('!')));
        assert!(matches!(app.screen, Screen::ConfirmRunSnippet));
        assert!(app.snippets.flow_terminal());
    }

    #[test]
    fn edit_default_enter_writes_form_default_hosts_and_returns_to_form() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\nHost h2\n  HostName 2.2.2.2\n");
        app.snippets.host_pick_mut().purpose = SnippetHostPickPurpose::EditDefault;
        app.snippets.host_pick_mut().selected.insert("h2".into());
        app.snippets.host_pick_mut().selected.insert("h1".into());
        handle_key(&mut app, k(KeyCode::Enter));
        // Commits to the form (not a run) and returns there, in host-list order.
        assert!(matches!(app.screen, Screen::SnippetForm));
        assert_eq!(
            app.snippets.form().default_hosts,
            vec!["h1".to_string(), "h2".to_string()]
        );
    }

    #[test]
    fn edit_default_empty_selection_clears_form_default_hosts() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        app.snippets.form_mut().default_hosts = vec!["h1".into()];
        app.snippets.host_pick_mut().purpose = SnippetHostPickPurpose::EditDefault;
        // Nothing selected: an empty commit is valid and clears the defaults.
        handle_key(&mut app, k(KeyCode::Enter));
        assert!(matches!(app.screen, Screen::SnippetForm));
        assert!(app.snippets.form().default_hosts.is_empty());
    }

    #[test]
    fn edit_default_esc_returns_to_form_without_changing_hosts() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        app.snippets.form_mut().default_hosts = vec!["h1".into()];
        app.snippets.host_pick_mut().purpose = SnippetHostPickPurpose::EditDefault;
        app.snippets.host_pick_mut().selected.insert("h1".into());
        handle_key(&mut app, k(KeyCode::Esc));
        // Esc does not commit, so the form's default hosts are untouched.
        assert!(matches!(app.screen, Screen::SnippetForm));
        assert_eq!(app.snippets.form().default_hosts, vec!["h1".to_string()]);
    }

    #[test]
    fn esc_clears_and_returns() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        app.snippets.host_pick_mut().selected.insert("h1".into());
        handle_key(&mut app, k(KeyCode::Esc));
        assert!(app.snippets.host_pick().selected.is_empty());
        assert!(app.snippets.flow_snippet().is_none());
        assert!(matches!(app.screen, Screen::HostList));
    }

    #[test]
    fn space_on_group_header_toggles_all_members() {
        use crate::app::{GroupBy, HostListItem};
        let mut app = make_app(
            "Host web1\n  HostName 1.1.1.1\n  # purple:provider aws:i-1\n\
             Host web2\n  HostName 2.2.2.2\n  # purple:provider aws:i-2\n\
             Host db1\n  HostName 3.3.3.3\n  # purple:provider digitalocean:d-1\n",
        );
        app.hosts_state.set_group_by(GroupBy::Provider);
        let rows = picker_rows(&app);
        assert!(
            matches!(rows.first(), Some(HostListItem::GroupHeader(_))),
            "all hosts have providers, so the first row is a group header: {rows:?}"
        );
        // Cursor on the first group header, Space toggles its whole group.
        app.snippets.host_pick_mut().list_state.select(Some(0));
        handle_key(&mut app, k(KeyCode::Char(' ')));
        let sel = &app.snippets.host_pick().selected;
        assert!(sel.contains("web1"));
        assert!(sel.contains("web2"));
        assert!(
            !sel.contains("db1"),
            "only the first group's members toggle"
        );
        // Space again clears the group.
        handle_key(&mut app, k(KeyCode::Char(' ')));
        assert!(app.snippets.host_pick().selected.is_empty());
    }

    #[test]
    fn slash_enters_filter_mode_and_typing_filters() {
        let mut app = make_app(
            "Host aws1\n  HostName 1.1.1.1\n  # purple:provider aws:i-1\nHost db1\n  HostName 2.2.2.2\n",
        );
        handle_key(&mut app, k(KeyCode::Char('/')));
        assert!(app.snippets.host_pick().filtering);
        for c in "aws".chars() {
            handle_key(&mut app, k(KeyCode::Char(c)));
        }
        assert_eq!(app.snippets.host_pick().query, "aws");
        let rows = picker_rows(&app);
        // Filtered view is flat host rows (no group headers), only matches.
        assert!(rows.iter().all(|r| matches!(r, HostListItem::Host { .. })));
        let aliases: Vec<String> = rows
            .iter()
            .filter_map(|r| match r {
                HostListItem::Host { index } => Some(app.hosts_state.list()[*index].alias.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(aliases, vec!["aws1"]);
    }

    #[test]
    fn enter_in_filter_keeps_query_and_leaves_edit_mode() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char('/')));
        handle_key(&mut app, k(KeyCode::Char('h')));
        handle_key(&mut app, k(KeyCode::Enter));
        assert!(!app.snippets.host_pick().filtering);
        assert_eq!(app.snippets.host_pick().query, "h");
        assert!(matches!(app.screen, Screen::SnippetHostPicker));
    }

    #[test]
    fn esc_in_filter_clears_query_and_stays() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char('/')));
        handle_key(&mut app, k(KeyCode::Char('x')));
        handle_key(&mut app, k(KeyCode::Esc));
        assert!(!app.snippets.host_pick().filtering);
        assert!(app.snippets.host_pick().query.is_empty());
        assert!(matches!(app.screen, Screen::SnippetHostPicker));
    }

    #[test]
    fn esc_peels_applied_filter_before_cancelling() {
        let mut app = make_app("Host h1\n  HostName 1.1.1.1\n");
        handle_key(&mut app, k(KeyCode::Char('/')));
        handle_key(&mut app, k(KeyCode::Char('h')));
        handle_key(&mut app, k(KeyCode::Enter)); // query applied, edit mode off
        handle_key(&mut app, k(KeyCode::Esc)); // first Esc clears the filter
        assert!(app.snippets.host_pick().query.is_empty());
        assert!(matches!(app.screen, Screen::SnippetHostPicker));
        handle_key(&mut app, k(KeyCode::Esc)); // second Esc cancels the picker
        assert!(matches!(app.screen, Screen::HostList));
    }

    #[test]
    fn a_selects_all_visible_matches_when_filtered() {
        let mut app = make_app(
            "Host aws1\n  HostName 1.1.1.1\n  # purple:provider aws:i-1\n\
             Host aws2\n  HostName 2.2.2.2\n  # purple:provider aws:i-2\n\
             Host db1\n  HostName 3.3.3.3\n",
        );
        app.snippets.host_pick_mut().query = "aws".to_string();
        handle_key(&mut app, k(KeyCode::Char('a')));
        let sel = &app.snippets.host_pick().selected;
        assert!(sel.contains("aws1"));
        assert!(sel.contains("aws2"));
        assert!(
            !sel.contains("db1"),
            "filtered-out host is not selected by 'a'"
        );
    }

    #[test]
    fn select_all_spans_every_group_then_commits_in_row_order() {
        use crate::app::GroupBy;
        let mut app = make_app(
            "Host web1\n  HostName 1.1.1.1\n  # purple:provider aws:i-1\n\
             Host db1\n  HostName 3.3.3.3\n  # purple:provider digitalocean:d-1\n",
        );
        app.hosts_state.set_group_by(GroupBy::Provider);
        handle_key(&mut app, k(KeyCode::Char('a')));
        assert_eq!(app.snippets.host_pick().selected.len(), 2);
        handle_key(&mut app, k(KeyCode::Enter));
        assert!(matches!(app.screen, Screen::ConfirmRunSnippet));
        // Committed in row (group) order: aws group, then digitalocean.
        assert_eq!(app.snippets.flow_targets(), &["web1", "db1"]);
    }
}
