use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::design;
use super::theme;
use crate::app::{App, Screen};

/// Render the snippet delete confirmation popup. Shared by the host-list
/// snippet picker and the Snippets tab so the dialog reads identically. Drawn
/// as a centred destructive popup to match host delete and the other danger
/// dialogs rather than a footer prompt.
pub(crate) fn render_delete_popup(frame: &mut Frame, app: &App) {
    let name = app
        .snippets
        .pending_delete()
        .and_then(|i| app.snippets.store().snippets.get(i))
        .map(|s| s.name.as_str())
        .unwrap_or("");
    design::render_destructive_popup(
        frame,
        crate::i18n::text(crate::messages::CONFIRM_SNIPPET_DELETE_TITLE),
        &crate::messages::confirm_snippet_delete_question(&super::truncate(name, 32)),
        crate::i18n::text(crate::messages::CONFIRM_SNIPPET_DELETE_DETAIL),
        "delete",
        "keep",
        app,
    );
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let host_count = match &app.screen {
        Screen::SnippetPicker | Screen::SnippetForm | Screen::SnippetParamForm => {
            app.snippets.flow_targets().len()
        }
        _ => 1,
    };

    let searching = app.ui.snippet_search().is_some();

    let title = if host_count > 1 {
        format!("Snippets ({} hosts)", host_count)
    } else {
        crate::i18n::text("Snippets").to_string()
    };

    let filtered = app.filtered_snippet_indices();
    let item_count = if searching {
        filtered.len().max(1)
    } else {
        app.snippets.store().snippets.len().max(1)
    };
    let has_snippets = if searching {
        !filtered.is_empty()
    } else {
        !app.snippets.store().snippets.is_empty()
    };
    let search_row = if searching { 1u16 } else { 0 };
    let header_row = if has_snippets { 1u16 } else { 0 };
    // Reserve 1 row below the block for the external footer.
    let height = (item_count as u16 + 4 + search_row + header_row)
        .min(frame.area().height.saturating_sub(5));
    let area = design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, height);
    frame.render_widget(Clear, area);

    let block = if searching {
        design::overlay_block(&title).border_style(theme::border_search())
    } else {
        design::overlay_block(&title)
    };

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Layout inside the block: optional search bar + optional header + list.
    // Footer renders BELOW the block via design::form_footer.
    let mut constraints = Vec::new();
    if searching {
        constraints.push(Constraint::Length(1));
    }
    if has_snippets {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(0));
    let chunks = Layout::vertical(constraints).split(inner);

    // Resolve chunk indices based on which optional rows are present
    let search_ci = if searching { Some(0) } else { None };
    let header_ci = if has_snippets {
        Some(searching as usize)
    } else {
        None
    };
    let list_ci = searching as usize + has_snippets as usize;

    // Search bar
    if let Some(si) = search_ci {
        let query = app.ui.snippet_search().map(|s| s.as_str()).unwrap_or("");
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
    let footer_area = design::render_overlay_footer(frame, area);

    // Build snippet list (filtered when searching)
    let indices = if searching {
        filtered
    } else {
        (0..app.snippets.store().snippets.len()).collect()
    };

    // Column widths: name gets ~28%, command gets the rest (or split with description)
    // Each column pair separated by a 2-char gap for readability.
    let col_gap = 2;
    let usable = list_area.width.saturating_sub(3) as usize; // 2 highlight + 1 leading space
    let has_desc = indices
        .iter()
        .any(|&i| !app.snippets.store().snippets[i].description.is_empty());
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
            Span::styled(
                format!("{}{:<name_w$}", design::COLUMN_HEADER_PREFIX, "NAME"),
                style,
            ),
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
        if searching {
            design::render_empty(frame, list_area, "No matches.");
        } else {
            design::render_empty_with_hint(frame, list_area, "No snippets yet.", "a", "add one");
        }
    } else {
        let items: Vec<ListItem> = indices
            .iter()
            .map(|&idx| {
                let snippet = &app.snippets.store().snippets[idx];
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
            .highlight_symbol(design::LIST_HIGHLIGHT);

        frame.render_stateful_widget(list, list_area, app.ui.snippet_picker_state_mut());
    }

    // Footer
    use crate::messages::footer as fl;
    if searching {
        design::Footer::new()
            .primary("Enter", crate::i18n::text(fl::ENTER_SELECT))
            .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
            .render_with_status(frame, footer_area, app);
    } else if app.snippets.pending_delete().is_some() {
        render_delete_popup(frame, app);
        super::render_footer_with_status(frame, footer_area, Vec::new(), app);
    } else {
        let mut f = design::Footer::new();
        if !app.snippets.store().snippets.is_empty() {
            f = f
                .primary("Enter", crate::i18n::text(fl::ENTER_RUN))
                .action("!", crate::i18n::text(fl::ACTION_TERMINAL));
        }
        f = f.action("a", crate::i18n::text(fl::ACTION_ADD));
        if !app.snippets.store().snippets.is_empty() {
            f = f
                .action("e", crate::i18n::text(fl::ACTION_EDIT))
                .action("d", crate::i18n::text(fl::ACTION_DEL))
                .action("/", crate::i18n::text(fl::ACTION_SEARCH));
        }
        f = f.action("Esc", crate::i18n::text(fl::ESC_BACK));
        f.render_with_status(frame, footer_area, app);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::design;

    #[test]
    fn footer_sits_directly_below_block() {
        let area = Rect::new(0, 0, 60, 20);
        let footer = design::form_footer(area, area.height);
        assert_eq!(footer.height, 1);
        assert_eq!(footer.y, area.y + area.height);
    }
}
