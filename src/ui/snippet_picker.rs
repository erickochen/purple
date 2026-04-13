use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, Screen};

pub fn render(frame: &mut Frame, app: &mut App) {
    let host_count = match &app.screen {
        Screen::SnippetPicker { target_aliases } => target_aliases.len(),
        Screen::SnippetForm { target_aliases, .. } => target_aliases.len(),
        Screen::SnippetParamForm { target_aliases, .. } => target_aliases.len(),
        _ => 1,
    };

    let searching = app.ui.snippet_search.is_some();

    let title = if host_count > 1 {
        Line::from(Span::styled(
            format!(" Snippets ({} hosts) ", host_count),
            theme::brand(),
        ))
    } else {
        Line::from(Span::styled(" Snippets ", theme::brand()))
    };

    let filtered = app.filtered_snippet_indices();
    let item_count = if searching {
        filtered.len().max(1)
    } else {
        app.snippet_store.snippets.len().max(1)
    };
    let has_snippets = if searching {
        !filtered.is_empty()
    } else {
        !app.snippet_store.snippets.is_empty()
    };
    let search_row = if searching { 1u16 } else { 0 };
    let header_row = if has_snippets { 1u16 } else { 0 };
    let height = (item_count as u16 + 6 + search_row + header_row)
        .min(frame.area().height.saturating_sub(4));
    let area = {
        let r = super::centered_rect(70, 80, frame.area());
        super::centered_rect_fixed(r.width, height, frame.area())
    };
    frame.render_widget(Clear, area);

    let border_style = if searching {
        theme::border_search()
    } else {
        theme::accent()
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Layout: optional search bar + optional header + list + footer
    let mut constraints = Vec::new();
    if searching {
        constraints.push(Constraint::Length(1));
    }
    if has_snippets {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(0));
    constraints.push(Constraint::Length(1)); // spacer
    constraints.push(Constraint::Length(1)); // footer
    let chunks = Layout::vertical(constraints).split(inner);

    // Resolve chunk indices based on which optional rows are present
    let search_ci = if searching { Some(0) } else { None };
    let header_ci = if has_snippets {
        Some(searching as usize)
    } else {
        None
    };
    let list_ci = searching as usize + has_snippets as usize;
    let footer_ci = list_ci + 2;

    // Search bar
    if let Some(si) = search_ci {
        let query = app.ui.snippet_search.as_deref().unwrap_or("");
        let search_line = Line::from(vec![
            Span::styled(" / ", theme::brand_badge()),
            Span::styled(query, theme::bold()),
            Span::styled("_", theme::accent()),
        ]);
        frame.render_widget(Paragraph::new(search_line), chunks[si]);

        // Cursor position
        let cursor_x = chunks[si].x + 3 + query.width() as u16;
        if cursor_x < chunks[si].x + chunks[si].width {
            frame.set_cursor_position((cursor_x, chunks[si].y));
        }
    }

    let list_area = chunks[list_ci];
    let footer_area = chunks[footer_ci];

    // Build snippet list (filtered when searching)
    let indices = if searching {
        filtered
    } else {
        (0..app.snippet_store.snippets.len()).collect()
    };

    // Column widths: name gets ~28%, command gets the rest (or split with description)
    // Each column pair separated by a 2-char gap for readability.
    let col_gap = 2;
    let usable = list_area.width.saturating_sub(3) as usize; // 2 highlight + 1 leading space
    let has_desc = indices
        .iter()
        .any(|&i| !app.snippet_store.snippets[i].description.is_empty());
    let (name_w, cmd_w, desc_w) = if has_desc {
        let nw = (usable * 28 / 100).max(10);
        let dw = (usable * 28 / 100).max(10);
        let cw = usable.saturating_sub(nw + col_gap + dw + col_gap);
        (nw, cw, dw)
    } else {
        let nw = (usable * 30 / 100).max(10);
        let cw = usable.saturating_sub(nw + col_gap);
        (nw, cw, 0)
    };

    // Column header (3-space prefix = 2 highlight_symbol + 1 leading space in items)
    let gap_str = " ".repeat(col_gap);
    if let Some(hi) = header_ci {
        let style = theme::bold();
        let mut hdr = vec![
            Span::styled(format!("   {:<name_w$}", "NAME"), style),
            Span::raw(gap_str.clone()),
            Span::styled(format!("{:<cmd_w$}", "COMMAND"), style),
        ];
        if has_desc {
            hdr.push(Span::raw(gap_str.clone()));
            hdr.push(Span::styled(format!("{:<desc_w$}", "DESCRIPTION"), style));
        }
        frame.render_widget(Paragraph::new(Line::from(hdr)), chunks[hi]);
    }

    if indices.is_empty() {
        let msg = if searching {
            "  No matches."
        } else {
            "  No snippets yet. Press 'a' to add one."
        };
        frame.render_widget(Paragraph::new(msg).style(theme::muted()), list_area);
    } else {
        let items: Vec<ListItem> = indices
            .iter()
            .map(|&idx| {
                let snippet = &app.snippet_store.snippets[idx];
                let mut spans = vec![
                    Span::styled(
                        format!(" {:<name_w$}", super::truncate(&snippet.name, name_w)),
                        theme::bold(),
                    ),
                    Span::raw(gap_str.clone()),
                    Span::styled(
                        format!("{:<cmd_w$}", super::truncate(&snippet.command, cmd_w)),
                        theme::muted(),
                    ),
                ];
                if has_desc {
                    spans.push(Span::raw(gap_str.clone()));
                    spans.push(Span::styled(
                        format!("{:<desc_w$}", super::truncate(&snippet.description, desc_w)),
                        theme::muted(),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .highlight_style(theme::selected_row())
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, list_area, &mut app.ui.snippet_picker_state);
    }

    // Footer
    if searching {
        super::render_footer_with_status(
            frame,
            footer_area,
            vec![
                Span::styled(" Enter ", theme::footer_key()),
                Span::styled(" select ", theme::muted()),
                Span::raw("  "),
                Span::styled(" Esc ", theme::footer_key()),
                Span::styled(" cancel", theme::muted()),
            ],
            app,
        );
    } else if app.pending_snippet_delete.is_some() {
        let name = app
            .pending_snippet_delete
            .and_then(|i| app.snippet_store.snippets.get(i))
            .map(|s| s.name.as_str())
            .unwrap_or("");
        super::render_footer_with_status(
            frame,
            footer_area,
            vec![
                Span::styled(
                    format!(" Remove '{}'? ", super::truncate(name, 20)),
                    theme::bold(),
                ),
                Span::styled(" y ", theme::footer_key()),
                Span::styled(" yes ", theme::muted()),
                Span::raw("  "),
                Span::styled(" Esc ", theme::footer_key()),
                Span::styled(" no", theme::muted()),
            ],
            app,
        );
    } else {
        let mut spans: Vec<Span<'_>> = Vec::new();
        if !app.snippet_store.snippets.is_empty() {
            let [k, l] = super::footer_primary("Enter", " run ");
            spans.extend([k, l, Span::raw("  ")]);
            let [k, l] = super::footer_action("!", " terminal ");
            spans.extend([k, l, Span::raw("  ")]);
        }
        let [k, l] = super::footer_action("a", " add ");
        spans.extend([k, l]);
        if !app.snippet_store.snippets.is_empty() {
            spans.push(Span::raw("  "));
            let [k, l] = super::footer_action("e", " edit ");
            spans.extend([k, l, Span::raw("  ")]);
            let [k, l] = super::footer_action("d", " del ");
            spans.extend([k, l, Span::raw("  ")]);
            let [k, l] = super::footer_action("/", " search ");
            spans.extend([k, l]);
        }
        spans.push(Span::raw("  "));
        let [k, l] = super::footer_action("Esc", " back");
        spans.extend([k, l]);
        super::render_footer_with_status(frame, footer_area, spans, app);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::{Constraint, Layout, Rect};

    #[test]
    fn layout_has_spacer_between_content_and_footer() {
        // Simplest case: no search, no header — just list + spacer + footer
        let area = Rect::new(0, 0, 60, 20);
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(chunks[1].height, 1);
        assert_eq!(chunks[2].height, 1);
        assert!(chunks[2].y > chunks[0].y + chunks[0].height);
    }

    #[test]
    fn layout_with_search_and_header_has_spacer() {
        // search bar + header + list + spacer + footer
        let area = Rect::new(0, 0, 60, 20);
        let chunks = Layout::vertical([
            Constraint::Length(1), // search
            Constraint::Length(1), // header
            Constraint::Min(0),
            Constraint::Length(1), // spacer
            Constraint::Length(1), // footer
        ])
        .split(area);
        let list_ci = 2;
        let footer_ci = 4;
        assert!(chunks[footer_ci].y > chunks[list_ci].y + chunks[list_ci].height);
    }
}
