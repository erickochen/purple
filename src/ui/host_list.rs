use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Layout: host list + footer/status (merged into one row)
    let chunks = Layout::vertical([
        Constraint::Min(5),   // Host list (maximized)
        Constraint::Length(1), // Footer or status message
    ])
    .split(area);

    // Build title with position indicator
    let title = if app.hosts.is_empty() {
        " purple ".to_string()
    } else {
        let pos = app.list_state.selected().map(|i| i + 1).unwrap_or(0);
        format!(" purple [{}/{}] ", pos, app.hosts.len())
    };

    // Host list
    if app.hosts.is_empty() {
        let empty_msg =
            Paragraph::new("  It's quiet in here... Press 'a' to add your first host.")
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
            .hosts
            .iter()
            .map(|host| {
                let user_display = if host.user.is_empty() {
                    String::new()
                } else {
                    format!("{}@", host.user)
                };
                let port_display = if host.port == 22 {
                    String::new()
                } else {
                    format!(":{}", host.port)
                };
                let detail = format!("{}{}{}", user_display, host.hostname, port_display);

                let mut spans = vec![
                    Span::styled(format!(" {} ", host.alias), theme::bold()),
                    Span::styled(" -> ", theme::muted()),
                    Span::styled(detail, theme::muted()),
                ];

                // Show key name if IdentityFile is set
                if !host.identity_file.is_empty() {
                    let key_name = std::path::Path::new(&host.identity_file)
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_else(|| host.identity_file.clone());
                    spans.push(Span::styled(format!(" [{}]", key_name), theme::muted()));
                }

                let line = Line::from(spans);
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

        frame.render_stateful_widget(list, chunks[0], &mut app.list_state);
    }

    // Footer or status (merged: status temporarily replaces footer)
    if app.status.is_some() {
        super::render_status_bar(frame, chunks[1], app);
    } else {
        render_footer(frame, chunks[1]);
    }
}

fn render_footer(frame: &mut Frame, area: ratatui::layout::Rect) {
    let footer = Line::from(vec![
        Span::styled(" a", theme::accent_bold()),
        Span::styled(" add  ", theme::muted()),
        Span::styled("e", theme::accent_bold()),
        Span::styled(" edit  ", theme::muted()),
        Span::styled("d", theme::accent_bold()),
        Span::styled(" delete  ", theme::muted()),
        Span::styled("Enter", theme::primary_action()),
        Span::styled(" connect  ", theme::muted()),
        Span::styled("K", theme::accent_bold()),
        Span::styled(" keys  ", theme::muted()),
        Span::styled("?", theme::accent_bold()),
        Span::styled(" help  ", theme::muted()),
        Span::styled("q", theme::accent_bold()),
        Span::styled(" quit", theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(footer), area);
}
