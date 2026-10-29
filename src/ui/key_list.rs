use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, Paragraph};

use super::design;
use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App) {
    let title = if app.keys.list().is_empty() {
        crate::i18n::text("SSH Keys").to_string()
    } else {
        let pos = app.keys.list_state().selected().map(|i| i + 1).unwrap_or(0);
        format!("SSH Keys {}/{}", pos, app.keys.list().len())
    };

    // Overlay: percentage-based width, height fits content. Reserve 1 row
    // below the block for the external footer.
    let item_count = app.keys.list().len().max(1);
    let height = (item_count as u16 + 5).min(frame.area().height.saturating_sub(5));
    let area = design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, height);
    frame.render_widget(Clear, area);

    let block = design::overlay_block(&title);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.keys.list().is_empty() {
        design::render_empty(
            frame,
            inner,
            "No keys found in ~/.ssh/. Try ssh-keygen to forge one.",
        );
        return;
    }

    // Column layout following containers.rs pattern:
    // Left cluster: NAME + gap + TYPE + gap + HOSTS
    // Flex gap (absorbs surplus)
    // Right cluster: COMMENT
    let usable = inner.width.saturating_sub(2) as usize; // 1 highlight + 1 right margin
    let gap: usize = design::COL_GAP as usize;

    let name_w = design::padded_usize(
        app.keys
            .list()
            .iter()
            .map(|k| k.name.len())
            .max()
            .unwrap_or(4)
            .max(4),
    );
    let type_w = design::padded_usize(
        app.keys
            .list()
            .iter()
            .map(|k| k.type_display().len())
            .max()
            .unwrap_or(4)
            .max(4),
    );
    let hosts_w = design::padded_usize(
        app.keys
            .list()
            .iter()
            .map(|k| {
                let n = k.linked_hosts.len();
                match n {
                    0 => 7, // "0 hosts"
                    1 => 6, // "1 host"
                    _ => format!("{} hosts", n).len(),
                }
            })
            .max()
            .unwrap_or(7)
            .max(7),
    );

    let left = name_w + gap + type_w + gap + hosts_w;
    // Comment gets remaining space
    let comment_w = usable.saturating_sub(left + gap);
    let flex_gap = if comment_w > 0 { gap } else { 0 };

    let gap_str = design::COL_GAP_STR;
    let flex_str = " ".repeat(flex_gap);

    // Column header
    let mut header_spans = vec![
        Span::styled(
            format!("{}{:<name_w$}", design::COLUMN_HEADER_PREFIX, "NAME"),
            theme::bold(),
        ),
        Span::raw(gap_str),
        Span::styled(format!("{:<type_w$}", "TYPE"), theme::bold()),
        Span::raw(gap_str),
        Span::styled(format!("{:<hosts_w$}", "HOSTS"), theme::bold()),
    ];
    if comment_w > 0 {
        header_spans.push(Span::raw(flex_str.clone()));
        header_spans.push(Span::styled("COMMENT", theme::bold()));
    }
    let header = Line::from(header_spans);

    let items: Vec<ListItem> = app
        .keys
        .list()
        .iter()
        .map(|key| {
            let type_display = key.type_display();

            let host_label = match key.linked_hosts.len() {
                0 => "0 hosts".to_string(),
                1 => "1 host".to_string(),
                n => format!("{} hosts", n),
            };

            let comment_display = if key.comment.is_empty() {
                String::new()
            } else {
                super::truncate(&key.comment, comment_w.saturating_sub(1))
            };

            let line = Line::from(vec![
                Span::styled(format!(" {:<name_w$}", key.name), theme::bold()),
                Span::raw(gap_str),
                Span::styled(format!("{:<type_w$}", type_display), theme::muted()),
                Span::raw(gap_str),
                Span::styled(format!("{:<hosts_w$}", host_label), theme::muted()),
                Span::raw(flex_str.clone()),
                Span::styled(comment_display, theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let inner_chunks = Layout::vertical([
        Constraint::Length(1), // Column header
        Constraint::Min(0),    // List
    ])
    .split(inner);

    frame.render_widget(Paragraph::new(header), inner_chunks[0]);

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol(design::LIST_HIGHLIGHT);

    frame.render_stateful_widget(list, inner_chunks[1], app.keys.list_state_mut());

    // Footer below the block
    let footer_area = design::render_overlay_footer(frame, area);
    use crate::messages::footer as fl;
    design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_SELECT))
        .action("Esc", crate::i18n::text(fl::ESC_BACK))
        .render_with_status(frame, footer_area, app);
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
