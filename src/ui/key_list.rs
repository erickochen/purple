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
        " purple keys ".to_string()
    } else {
        let pos = app.key_list_state.selected().map(|i| i + 1).unwrap_or(0);
        format!(" purple keys [{}/{}] ", pos, app.keys.len())
    };

    if app.keys.is_empty() {
        let empty_msg =
            Paragraph::new("  No keys found in ~/.ssh/. Try ssh-keygen to forge one.")
                .style(theme::muted())
                .block(
                    Block::default()
                        .title(Span::styled(title, theme::brand()))
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

                let line = Line::from(vec![
                    Span::styled(format!(" {:<18}", key.name), theme::bold()),
                    Span::styled(format!("{:<12}", type_display), theme::muted()),
                    Span::styled(format!("{:<24}", truncate_fingerprint(&key.fingerprint, 22)), theme::muted()),
                    Span::styled(host_label, theme::muted()),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(Span::styled(title, theme::brand()))
                    .borders(Borders::ALL)
                    .border_style(theme::border()),
            )
            .highlight_style(theme::selected())
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, chunks[0], &mut app.key_list_state);
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

fn truncate_fingerprint(fp: &str, max_len: usize) -> String {
    if fp.len() <= max_len {
        fp.to_string()
    } else {
        let truncated: String = fp.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}
