use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, Paragraph};

use super::design;
use super::theme;
use crate::app::{App, BulkTagAction};

/// Render the bulk tag editor overlay. Shows one row per candidate tag
/// with a tri-state glyph and the current count of selected hosts that
/// have that tag. A "+ new tag" row lets the user add a tag that exists
/// nowhere else in the config yet.
pub fn render(frame: &mut Frame, app: &mut App) {
    let host_count = app.forms.bulk_tag_editor().aliases.len();
    let editable_count =
        host_count.saturating_sub(app.forms.bulk_tag_editor().skipped_included.len());
    let input_active = app.forms.bulk_tag_editor().new_tag_input.is_some();
    let has_skipped = !app.forms.bulk_tag_editor().skipped_included.is_empty();

    // +3 rows reserved for: legend, optional skip-warning, spacer/input.
    // Footer renders below the block.
    let content_rows = app.forms.bulk_tag_editor().rows.len() as u16 + 3;
    let overlay_h = (content_rows + 4).min(frame.area().height.saturating_sub(5));
    let overlay_w = 52u16.min(frame.area().width.saturating_sub(4));
    let area = super::centered_rect_fixed(overlay_w, overlay_h, frame.area());
    frame.render_widget(Clear, area);

    let title_text = if host_count == 1 {
        "Bulk tags \u{00B7} 1 host".to_string()
    } else {
        format!("Bulk tags \u{00B7} {} hosts", host_count)
    };
    let block = design::overlay_block(&title_text);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Layout inside the block. Footer renders BELOW the block.
    let chunks = if has_skipped {
        Layout::vertical([
            Constraint::Length(1), // legend
            Constraint::Length(1), // skip warning
            Constraint::Min(0),    // list
            Constraint::Length(1), // spacer / input bar
        ])
        .split(inner)
    } else {
        Layout::vertical([
            Constraint::Length(1), // legend
            Constraint::Min(0),    // list
            Constraint::Length(1), // spacer / input bar
        ])
        .split(inner)
    };

    // Legend — always visible so first-time users learn the glyphs.
    let legend = if editable_count == 0 {
        Line::from(Span::styled(
            crate::i18n::text("  No editable hosts in selection."),
            theme::muted(),
        ))
    } else {
        Line::from(Span::styled(
            "  [x] add  [ ] remove  [~] leave as-is",
            theme::muted(),
        ))
    };
    frame.render_widget(Paragraph::new(legend), chunks[0]);

    // Skip warning (only present when the layout has 5 rows).
    if has_skipped {
        let skipped = app.forms.bulk_tag_editor().skipped_included.len();
        let warn = Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!(
                    "{} include-file host{} skipped",
                    skipped,
                    if skipped == 1 { "" } else { "s" }
                ),
                theme::warning(),
            ),
        ]);
        frame.render_widget(Paragraph::new(warn), chunks[1]);
    }

    let list_idx = if has_skipped { 2 } else { 1 };
    let spacer_idx = if has_skipped { 3 } else { 2 };

    if app.forms.bulk_tag_editor().rows.is_empty() && !input_active {
        design::render_empty_with_hint(frame, chunks[list_idx], "No tags yet.", "+", "add");
    } else {
        let items: Vec<ListItem> = app
            .forms
            .bulk_tag_editor()
            .rows
            .iter()
            .map(|row| build_row_line(row, editable_count))
            .map(ListItem::new)
            .collect();
        let list = List::new(items)
            .highlight_style(theme::selected_row())
            .highlight_symbol(design::LIST_HIGHLIGHT);
        frame.render_stateful_widget(list, chunks[list_idx], app.ui.bulk_tag_editor_state_mut());
    }

    // Optional input bar rendered in-place of the spacer when typing a new
    // tag name. This keeps the layout stable — the list never jumps.
    if input_active {
        let input = app
            .forms
            .bulk_tag_editor()
            .new_tag_input
            .as_deref()
            .unwrap_or("");
        let spans = new_tag_input_spans(input);
        frame.render_widget(Paragraph::new(Line::from(spans)), chunks[spacer_idx]);
    }

    // Footer below the block. Discard prompt takes precedence — every
    // dirty-checked surface routes through `render_discard_prompt` for
    // uniform confirm behavior.
    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    if app.forms.is_discard_pending() {
        design::render_discard_prompt(frame, footer_area, app);
    } else {
        let f = if input_active {
            design::Footer::new()
                .primary("Enter", crate::i18n::text(fl::ENTER_ADD))
                .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
        } else {
            // Stakes test: this is a list-completion action ("apply my
            // changes"), so primary verb is "apply" not generic "ok".
            // NNGroup: name a button to explain what it does.
            design::Footer::new()
                .primary("Enter", crate::i18n::text(fl::ENTER_APPLY))
                .action("Space", crate::i18n::text(fl::SPACE_CYCLE))
                .action("+", crate::i18n::text(fl::ACTION_NEW))
                .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
        };
        f.render_with_status(frame, footer_area, app);
    }
}

/// Build the rendered line for a single bulk-tag row.
///
/// The count column reads e.g. `2/5` to make the mixed-state math visible.
/// A pending action that will change the set is annotated with a small
/// arrow so users see the end state at a glance without reading the glyph.
pub(crate) fn build_row_line(row: &crate::app::BulkTagRow, editable_count: usize) -> Line<'static> {
    let glyph = row.action.glyph();
    let glyph_style = match row.action {
        BulkTagAction::Leave => theme::muted(),
        BulkTagAction::AddToAll => theme::success(),
        BulkTagAction::RemoveFromAll => theme::error(),
    };
    let count = format!("{}/{}", row.initial_count, editable_count);
    let is_mixed = row.initial_count > 0 && row.initial_count < editable_count;
    let count_style = if row.initial_count == editable_count && editable_count > 0 {
        theme::success()
    } else if row.initial_count == 0 {
        theme::muted()
    } else {
        theme::warning()
    };

    // NO_COLOR disambiguation: when the tag is on some-but-not-all hosts
    // and no action has been taken yet, append "mixed" so the glyph `[~]`
    // isn't the only signal. Once the user cycles away from Leave, the
    // glyph/arrow carry meaning and the suffix is not needed.
    let count_label = if is_mixed && row.action == BulkTagAction::Leave {
        format!("({} mixed)", count)
    } else {
        format!("({})", count)
    };

    let mut spans = vec![
        Span::raw(" "),
        Span::styled(glyph.to_string(), glyph_style),
        Span::raw(" "),
        Span::styled(row.tag.clone(), theme::bold()),
        Span::raw(" "),
        Span::styled(count_label, count_style),
    ];

    let end_state = preview_end_state(row, editable_count);
    if let Some((arrow, target, target_style)) = end_state {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(arrow, theme::muted()));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(target, target_style));
    }
    Line::from(spans)
}

/// Arrow + end-state label + style for a row, or `None` when no change is pending.
fn preview_end_state(
    row: &crate::app::BulkTagRow,
    editable_count: usize,
) -> Option<(&'static str, String, ratatui::style::Style)> {
    match row.action {
        BulkTagAction::Leave => None,
        BulkTagAction::AddToAll => {
            if row.initial_count == editable_count {
                None
            } else {
                let delta = editable_count.saturating_sub(row.initial_count);
                Some(("\u{2192}", format!("+{}", delta), theme::success()))
            }
        }
        BulkTagAction::RemoveFromAll => {
            if row.initial_count == 0 {
                None
            } else {
                Some((
                    "\u{2192}",
                    format!("-{}", row.initial_count),
                    theme::error(),
                ))
            }
        }
    }
}

fn new_tag_input_spans(input: &str) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(" + ", theme::accent_bold())];
    if input.is_empty() {
        spans.push(Span::styled("_", theme::accent()));
        spans.push(Span::styled("  e.g. prod", theme::muted()));
    } else {
        spans.push(Span::raw(input.to_string()));
        spans.push(Span::styled("_", theme::accent()));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::BulkTagRow;

    #[test]
    fn leave_action_has_no_preview() {
        let row = BulkTagRow {
            tag: "prod".into(),
            initial_count: 2,
            action: BulkTagAction::Leave,
        };
        assert!(preview_end_state(&row, 5).is_none());
    }

    #[test]
    fn add_to_all_shows_delta_when_partial() {
        let row = BulkTagRow {
            tag: "prod".into(),
            initial_count: 2,
            action: BulkTagAction::AddToAll,
        };
        let (arrow, target, _) = preview_end_state(&row, 5).unwrap();
        assert_eq!(arrow, "\u{2192}");
        assert_eq!(target, "+3");
    }

    #[test]
    fn add_to_all_hidden_when_already_all() {
        let row = BulkTagRow {
            tag: "prod".into(),
            initial_count: 5,
            action: BulkTagAction::AddToAll,
        };
        assert!(preview_end_state(&row, 5).is_none());
    }

    #[test]
    fn remove_from_all_shows_delta_when_present() {
        let row = BulkTagRow {
            tag: "stage".into(),
            initial_count: 3,
            action: BulkTagAction::RemoveFromAll,
        };
        let (_, target, _) = preview_end_state(&row, 5).unwrap();
        assert_eq!(target, "-3");
    }

    #[test]
    fn remove_from_all_hidden_when_none_have_it() {
        let row = BulkTagRow {
            tag: "stage".into(),
            initial_count: 0,
            action: BulkTagAction::RemoveFromAll,
        };
        assert!(preview_end_state(&row, 5).is_none());
    }

    #[test]
    fn row_line_has_expected_spans() {
        let row = BulkTagRow {
            tag: "prod".into(),
            initial_count: 2,
            action: BulkTagAction::AddToAll,
        };
        let line = build_row_line(&row, 5);
        let content: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(content.contains("[x]"));
        assert!(content.contains("prod"));
        assert!(content.contains("(2/5)"));
        assert!(content.contains("+3"));
    }

    #[test]
    fn mixed_state_leave_shows_mixed_suffix() {
        let row = BulkTagRow {
            tag: "db".into(),
            initial_count: 2,
            action: BulkTagAction::Leave,
        };
        let line = build_row_line(&row, 5);
        let content: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            content.contains("mixed"),
            "expected 'mixed' suffix, got: {content}"
        );
    }

    #[test]
    fn all_count_no_mixed_suffix() {
        let row = BulkTagRow {
            tag: "prod".into(),
            initial_count: 5,
            action: BulkTagAction::Leave,
        };
        let line = build_row_line(&row, 5);
        let content: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            !content.contains("mixed"),
            "should not show mixed when all have it"
        );
    }
}
