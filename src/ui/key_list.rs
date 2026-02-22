use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let chunks = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .split(area);

    let title = if app.keys.is_empty() {
        Line::from(vec![
            Span::styled(" purple. ", theme::brand_badge()),
            Span::raw(" keys "),
        ])
    } else {
        let pos = app.key_list_state.selected().map(|i| i + 1).unwrap_or(0);
        Line::from(vec![
            Span::styled(" purple. ", theme::brand_badge()),
            Span::raw(format!(" keys {}/{} ", pos, app.keys.len())),
        ])
    };

    if app.keys.is_empty() {
        let empty_msg =
            Paragraph::new("  No keys found in ~/.ssh/. Try ssh-keygen to forge one.")
                .style(theme::muted())
                .block(
                    Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(theme::border()),
                );
        frame.render_widget(empty_msg, chunks[0]);
    } else {
        let items: Vec<ListItem> = app
            .keys
            .iter()
            .map(|key| {
                let type_display = key.type_display();

                let host_count = key.linked_hosts.len();
                let host_label = match host_count {
                    0 => "0 hosts".to_string(),
                    1 => "1 host".to_string(),
                    n => format!("{} hosts", n),
                };

                let comment_display = if key.comment.is_empty() {
                    String::new()
                } else {
                    truncate_fingerprint(&key.comment, 20)
                };

                let line = Line::from(vec![
                    Span::styled(format!(" {:<18}", key.name), theme::bold()),
                    Span::styled(format!("{:<12}", type_display), theme::muted()),
                    Span::styled(format!("{:<22}", comment_display), theme::muted()),
                    Span::styled(format!("{:<10}", host_label), theme::muted()),
                ]);
                ListItem::new(line)
            })
            .collect();

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme::border());

        let inner = block.inner(chunks[0]);
        frame.render_widget(block, chunks[0]);

        // Split inner for column header + list
        let inner_chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);

        // Column header
        let header = Line::from(vec![
            Span::styled(format!(" {:<18}", "NAME"), theme::muted()),
            Span::styled(format!("{:<12}", "TYPE"), theme::muted()),
            Span::styled(format!("{:<22}", "COMMENT"), theme::muted()),
            Span::styled(format!("{:<10}", "HOSTS"), theme::muted()),
        ]);
        frame.render_widget(Paragraph::new(header), inner_chunks[0]);

        // Key list (without block — already rendered above)
        let list = List::new(items)
            .highlight_style(theme::selected())
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, inner_chunks[1], &mut app.key_list_state);
    }

    if app.status.is_some() {
        super::render_status_bar(frame, chunks[1], app);
    } else {
        render_footer(frame, chunks[1]);
    }
}

fn render_footer(frame: &mut Frame, area: ratatui::layout::Rect) {
    let footer = Line::from(vec![
        Span::styled(" K", theme::accent_bold()),
        Span::styled(" hosts  ", theme::muted()),
        Span::styled("Enter", theme::primary_action()),
        Span::styled(" details  ", theme::muted()),
        Span::styled("q", theme::accent_bold()),
        Span::styled(" back", theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(footer), area);
}

/// Truncate a fingerprint to `max_len` display characters.
/// Fingerprints are ASCII (SHA256:base64), so byte length == char count.
fn truncate_fingerprint(fp: &str, max_len: usize) -> String {
    if fp.len() <= max_len {
        fp.to_string()
    } else {
        format!("{}...", &fp[..max_len.saturating_sub(3)])
    }
}
