use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App) {
    let title = if app.keys.is_empty() {
        Span::styled(" SSH Keys ", theme::brand())
    } else {
        let pos = app.ui.key_list_state.selected().map(|i| i + 1).unwrap_or(0);
        Span::styled(
            format!(" SSH Keys {}/{} ", pos, app.keys.len()),
            theme::brand(),
        )
    };

    // Overlay: percentage-based width, height fits content
    let item_count = app.keys.len().max(1);
    let height = (item_count as u16 + 7).min(frame.area().height.saturating_sub(4));
    let area = {
        let r = super::centered_rect(70, 80, frame.area());
        super::centered_rect_fixed(r.width, height, frame.area())
    };
    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.keys.is_empty() {
        let msg = Paragraph::new("  No keys found in ~/.ssh/. Try ssh-keygen to forge one.")
            .style(theme::muted());
        frame.render_widget(msg, inner);
        return;
    }

    // Column layout following containers.rs pattern:
    // Left cluster: NAME + gap + TYPE + gap + HOSTS
    // Flex gap (absorbs surplus)
    // Right cluster: COMMENT
    let usable = inner.width.saturating_sub(2) as usize; // 1 highlight + 1 right margin
    let gap: usize = 2;

    // ~110% of content width (same formula as containers.rs)
    let padded = |w: usize| -> usize { if w == 0 { 0 } else { w + w / 10 + 1 } };

    let name_w = padded(
        app.keys
            .iter()
            .map(|k| k.name.len())
            .max()
            .unwrap_or(4)
            .max(4),
    );
    let type_w = padded(
        app.keys
            .iter()
            .map(|k| k.type_display().len())
            .max()
            .unwrap_or(4)
            .max(4),
    );
    let hosts_w = padded(
        app.keys
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

    let gap_str = " ".repeat(gap);
    let flex_str = " ".repeat(flex_gap);

    // Column header
    let mut header_spans = vec![
        Span::styled(format!("   {:<name_w$}", "NAME"), theme::muted()),
        Span::raw(gap_str.clone()),
        Span::styled(format!("{:<type_w$}", "TYPE"), theme::muted()),
        Span::raw(gap_str.clone()),
        Span::styled(format!("{:<hosts_w$}", "HOSTS"), theme::muted()),
    ];
    if comment_w > 0 {
        header_spans.push(Span::raw(flex_str.clone()));
        header_spans.push(Span::styled("COMMENT", theme::muted()));
    }
    let header = Line::from(header_spans);

    let items: Vec<ListItem> = app
        .keys
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
                Span::raw(&gap_str),
                Span::styled(format!("{:<type_w$}", type_display), theme::muted()),
                Span::raw(&gap_str),
                Span::styled(format!("{:<hosts_w$}", host_label), theme::muted()),
                Span::raw(&flex_str),
                Span::styled(comment_display, theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let inner_chunks = Layout::vertical([
        Constraint::Length(1), // Column header
        Constraint::Min(0),    // List
        Constraint::Length(1), // Spacer
        Constraint::Length(1), // Footer
    ])
    .split(inner);

    frame.render_widget(Paragraph::new(header), inner_chunks[0]);

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, inner_chunks[1], &mut app.ui.key_list_state);

    // Footer
    let spans = vec![
        Span::styled(" Enter ", theme::footer_key()),
        Span::styled(" details ", theme::muted()),
        Span::raw("  "),
        Span::styled(" Esc ", theme::footer_key()),
        Span::styled(" back", theme::muted()),
    ];
    super::render_footer_with_status(frame, inner_chunks[3], spans, app);
}

#[cfg(test)]
mod tests {
    use ratatui::layout::{Constraint, Layout, Rect};

    #[test]
    fn layout_has_spacer_between_list_and_footer() {
        let area = Rect::new(0, 0, 60, 20);
        let chunks = Layout::vertical([
            Constraint::Length(1), // header
            Constraint::Min(0),    // list
            Constraint::Length(1), // spacer
            Constraint::Length(1), // footer
        ])
        .split(area);
        assert_eq!(chunks[2].height, 1, "spacer row should be 1 tall");
        assert_eq!(chunks[3].height, 1, "footer row should be 1 tall");
        assert!(
            chunks[3].y > chunks[1].y + chunks[1].height,
            "footer should be below list end"
        );
    }
}
