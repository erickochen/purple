//! Multi-select host picker for running a snippet from the Snippets tab.
//!
//! Lists every host with a checkbox column, grouped exactly the way the host
//! list is (via `App::grouped_all_hosts`). A group-header row toggles every
//! member; an individual host row toggles itself. Any host can run a snippet,
//! so there is no vault exclusion here (unlike the key-push picker).

use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem};

use super::design;
use super::theme;
use crate::app::{App, HostListItem};
use crate::ssh_config::model::HostEntry;

/// Maximum visible rows before the picker starts scrolling.
const MAX_VISIBLE_ROWS: u16 = 16;

/// Render the picker overlay. The chosen snippet lives on
/// `app.snippets.flow_snippet`; its name drives the overlay title.
pub fn render(frame: &mut Frame, app: &mut App) {
    let rows = crate::handler::snippet_host_picker::picker_rows(app);
    let host_total = app.hosts_state.list().len();
    let selected_count = app.snippets.host_pick().selected.len();
    let filtering = app.snippets.host_pick().filtering;
    let query = app.snippets.host_pick().query.clone();
    let show_search = filtering || !query.is_empty();
    let edit_default =
        app.snippets.host_pick().purpose == crate::app::SnippetHostPickPurpose::EditDefault;

    let snippet_name = app
        .snippets
        .flow_snippet()
        .map(|s| s.name.clone())
        .unwrap_or_else(|| crate::i18n::text(crate::messages::SNIPPET_FALLBACK_NAME).to_string());
    let title = if edit_default {
        crate::messages::snippet_default_hosts_picker_title(
            &snippet_name,
            selected_count,
            host_total,
        )
    } else {
        crate::messages::snippet_host_picker_title(&snippet_name, selected_count, host_total)
    };

    let list_rows = (rows.len() as u16).clamp(1, MAX_VISIBLE_ROWS);
    let top_row_h: u16 = if show_search { 1 } else { 0 };
    let total_height = 2 + top_row_h + list_rows; // borders + search + list

    let term_w = frame.area().width;
    let term_h = frame.area().height;
    let target_w = design::PICKER_MIN_W.max(term_w * 60 / 100);
    let overlay_width = target_w
        .min(design::PICKER_MAX_W)
        .min(term_w.saturating_sub(4));
    let height = total_height
        .min(design::PICKER_MAX_H)
        .min(term_h.saturating_sub(3));
    let area = super::centered_rect_fixed(overlay_width, height, frame.area());

    frame.render_widget(Clear, area);
    let block = design::overlay_block(&title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let list_area = if show_search {
        let [top_area, list_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
        super::picker_helpers::render_search_input(frame, top_area, &query, filtering);
        list_area
    } else {
        inner
    };

    if rows.is_empty() {
        let msg = if show_search {
            crate::i18n::text(crate::messages::PICKER_NO_MATCHES)
        } else {
            crate::i18n::text(crate::messages::PICKER_NO_HOSTS)
        };
        design::render_empty(frame, list_area, msg);
    } else {
        let content_w = list_area.width as usize;
        let selected = &app.snippets.host_pick().selected;
        // Hosts after the first group header are indented. Computed once (O(n))
        // instead of re-scanning the row prefix for every row (was O(n^2) per
        // frame while the overlay animates).
        let first_header = rows
            .iter()
            .position(|r| matches!(r, HostListItem::GroupHeader(_)));
        let items: Vec<ListItem> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| build_list_item(app, &rows, first_header, i, row, selected, content_w))
            .collect();

        let sel = app.snippets.host_pick().list_state.selected();
        let new_sel = match sel {
            Some(i) if i < rows.len() => Some(i),
            _ => Some(0),
        };
        if new_sel != sel {
            app.snippets.host_pick_mut().list_state.select(new_sel);
        }

        let list = List::new(items).highlight_style(theme::selected_row());
        frame.render_stateful_widget(
            list,
            list_area,
            &mut app.snippets.host_pick_mut().list_state,
        );
    }

    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    let footer = if filtering {
        // Editing the filter: Enter applies it, Esc clears it.
        design::Footer::new()
            .primary("Enter", crate::i18n::text(fl::ENTER_CONFIRM))
            .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
    } else {
        // Four core actions keep the footer within the overlay width. `a`
        // (select all) and `!` (terminal run, run mode only) stay working keys
        // but are not in the footer. Enter confirms a run, or saves the chosen
        // default hosts when the picker was opened from the edit form.
        let primary = if edit_default {
            crate::i18n::text(fl::ENTER_SAVE)
        } else {
            crate::i18n::text(fl::ENTER_CONFIRM)
        };
        design::Footer::new()
            .primary("Enter", primary)
            .action("/", crate::i18n::text(fl::ACTION_SEARCH))
            .action("Space", crate::i18n::text(fl::SPACE_TOGGLE))
            .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
    };
    footer.render_with_status(frame, footer_area, app);
}

fn build_list_item(
    app: &App,
    rows: &[HostListItem],
    first_header: Option<usize>,
    i: usize,
    row: &HostListItem,
    selected: &HashSet<String>,
    content_w: usize,
) -> ListItem<'static> {
    match row {
        HostListItem::GroupHeader(name) => {
            let members = crate::handler::snippet_host_picker::group_members(app, rows, i);
            ListItem::new(Line::from(group_header_spans(name, &members, selected)))
        }
        HostListItem::Host { index } | HostListItem::Pattern { index } => {
            // Indent hosts that sit under a group header (any row after the
            // first header). Hosts before any header (e.g. provider-less hosts)
            // stay flush left.
            let grouped = first_header.is_some_and(|h| i > h);
            let indent = if grouped { "  " } else { "" };
            match app.hosts_state.list().get(*index) {
                Some(host) => ListItem::new(Line::from(build_row_spans(
                    host, selected, content_w, indent,
                ))),
                None => ListItem::new(Line::default()),
            }
        }
    }
}

/// A group-header row: aggregate checkbox (`[x]` all / `[~]` some / `[ ]` none)
/// + group name + member count.
fn group_header_spans(
    name: &str,
    members: &[String],
    selected: &HashSet<String>,
) -> Vec<Span<'static>> {
    let sel_count = members.iter().filter(|a| selected.contains(*a)).count();
    let (mark, mark_style) = if !members.is_empty() && sel_count == members.len() {
        ("[x]", theme::accent_bold())
    } else if sel_count > 0 {
        ("[~]", theme::accent())
    } else {
        ("[ ]", theme::muted())
    };
    vec![
        Span::raw(" "),
        Span::styled(mark.to_string(), mark_style),
        Span::raw(" "),
        Span::styled(name.to_uppercase(), theme::bold()),
        Span::styled(format!(" ({})", members.len()), theme::muted()),
    ]
}

/// One host row's spans: `<indent>[x| ] alias    hostname`.
fn build_row_spans(
    host: &HostEntry,
    selected: &HashSet<String>,
    content_w: usize,
    indent: &str,
) -> Vec<Span<'static>> {
    use unicode_width::UnicodeWidthStr;

    let is_selected = selected.contains(&host.alias);
    let checkbox = if is_selected { "[x]" } else { "[ ]" };
    // `[x]` is a user-action state (chosen for the next run), not a live-state
    // signal, so use `accent_bold` (brand purple) when selected.
    let checkbox_style = if is_selected {
        theme::accent_bold()
    } else {
        theme::muted()
    };
    let alias_style = theme::bold();
    let hostname_style = theme::muted();

    let alias_w = host.alias.width();
    let leading = 1 + indent.width();
    let checkbox_w = 3;
    let gap = 2;
    let used = leading + checkbox_w + gap + alias_w + gap;
    let hostname_budget = content_w.saturating_sub(used);
    let hostname_truncated = super::truncate(&host.hostname, hostname_budget);

    vec![
        Span::raw(format!(" {indent}")),
        Span::styled(checkbox.to_string(), checkbox_style),
        Span::raw(" "),
        Span::styled(host.alias.clone(), alias_style),
        Span::raw(design::COL_GAP_STR),
        Span::styled(hostname_truncated, hostname_style),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(alias: &str, hostname: &str) -> HostEntry {
        HostEntry {
            alias: alias.to_string(),
            hostname: hostname.to_string(),
            ..Default::default()
        }
    }

    fn flatten(spans: &[Span<'_>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn build_row_marks_selected_with_x() {
        let host = h("prod", "10.0.0.2");
        let mut selected = HashSet::new();
        selected.insert("prod".to_string());
        let text = flatten(&build_row_spans(&host, &selected, 80, ""));
        assert!(text.contains("[x]"));
    }

    #[test]
    fn build_row_unselected_shows_empty_box() {
        let host = h("staging", "1.2.3.4");
        let selected = HashSet::new();
        let text = flatten(&build_row_spans(&host, &selected, 80, ""));
        assert!(text.contains("[ ]"));
        assert!(!text.contains("[x]"));
    }

    #[test]
    fn group_header_aggregate_checkbox_reflects_member_selection() {
        let members = vec!["a".to_string(), "b".to_string()];
        let mut selected = HashSet::new();
        // none selected
        assert!(flatten(&group_header_spans("prod", &members, &selected)).contains("[ ]"));
        // some selected
        selected.insert("a".to_string());
        assert!(flatten(&group_header_spans("prod", &members, &selected)).contains("[~]"));
        // all selected
        selected.insert("b".to_string());
        assert!(flatten(&group_header_spans("prod", &members, &selected)).contains("[x]"));
    }
}
