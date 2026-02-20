use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::theme;
use crate::app::{App, HostListItem, PingStatus};

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let is_searching = app.search_query.is_some();

    // Layout: host list + optional search bar + footer/status
    let chunks = if is_searching {
        Layout::vertical([
            Constraint::Min(5),   // Host list (maximized)
            Constraint::Length(1), // Search bar
            Constraint::Length(1), // Footer or status message
        ])
        .split(area)
    } else {
        Layout::vertical([
            Constraint::Min(5),   // Host list (maximized)
            Constraint::Length(1), // Footer or status message
        ])
        .split(area)
    };

    if is_searching {
        render_search_list(frame, app, chunks[0]);
        render_search_bar(frame, app, chunks[1]);
        // Footer or status
        if app.status.is_some() {
            super::render_status_bar(frame, chunks[2], app);
        } else {
            render_search_footer(frame, chunks[2]);
        }
    } else {
        render_display_list(frame, app, chunks[0]);
        // Footer or status
        let footer_area = chunks[1];
        if app.status.is_some() {
            super::render_status_bar(frame, footer_area, app);
        } else {
            render_footer(frame, footer_area);
        }
    }
}

fn render_display_list(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Build title with position indicator
    let host_count = app.hosts.len();
    let title = if host_count == 0 {
        " purple ".to_string()
    } else {
        let pos = app
            .selected_host_index()
            .map(|i| i + 1)
            .unwrap_or(0);
        format!(" purple [{}/{}] ", pos, host_count)
    };

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
        frame.render_widget(empty_msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .display_list
        .iter()
        .map(|item| match item {
            HostListItem::GroupHeader(text) => {
                let line = Line::from(vec![
                    Span::styled(
                        format!("  {} ", text),
                        theme::section_header(),
                    ),
                    Span::styled(" ────", theme::muted()),
                ]);
                ListItem::new(line)
            }
            HostListItem::Host { index } => {
                let host = &app.hosts[*index];
                build_host_item(host, &app.ping_status)
            }
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

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn render_search_list(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let title = format!(
        " purple [search: {}/{}] ",
        app.filtered_indices.len(),
        app.hosts.len()
    );

    if app.filtered_indices.is_empty() {
        let empty_msg = Paragraph::new("  No matches. Try a different search.")
            .style(theme::muted())
            .block(
                Block::default()
                    .title(Span::styled(title, theme::brand()))
                    .borders(Borders::ALL)
                    .border_style(theme::accent()),
            );
        frame.render_widget(empty_msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .map(|&idx| {
            let host = &app.hosts[idx];
            build_host_item(host, &app.ping_status)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(Span::styled(title, theme::brand()))
                .borders(Borders::ALL)
                .border_style(theme::accent()),
        )
        .highlight_style(theme::selected())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn build_host_item<'a>(
    host: &'a crate::ssh_config::model::HostEntry,
    ping_status: &'a std::collections::HashMap<String, PingStatus>,
) -> ListItem<'a> {
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

    // Show source file for included hosts
    if let Some(ref source) = host.source_file {
        let file_name = source
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        if !file_name.is_empty() {
            spans.push(Span::styled(format!(" ({})", file_name), theme::muted()));
        }
    }

    // Ping indicator
    if let Some(status) = ping_status.get(&host.alias) {
        let (indicator, style) = match status {
            PingStatus::Checking => (" [..]", theme::muted()),
            PingStatus::Reachable => (" [ok]", theme::success()),
            PingStatus::Unreachable => (" [--]", theme::error()),
            PingStatus::Skipped => (" [??]", theme::muted()),
        };
        spans.push(Span::styled(indicator, style));
    }

    let line = Line::from(spans);
    ListItem::new(line)
}

fn render_search_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let query = app.search_query.as_deref().unwrap_or("");
    let search_line = Line::from(vec![
        Span::styled(" / ", theme::accent_bold()),
        Span::raw(query),
        Span::styled("_", theme::accent()),
    ]);
    frame.render_widget(Paragraph::new(search_line), area);
}

fn render_footer(frame: &mut Frame, area: ratatui::layout::Rect) {
    let footer = Line::from(vec![
        Span::styled(" a", theme::accent_bold()),
        Span::styled(" add  ", theme::muted()),
        Span::styled("e", theme::accent_bold()),
        Span::styled(" edit  ", theme::muted()),
        Span::styled("d", theme::accent_bold()),
        Span::styled(" delete  ", theme::muted()),
        Span::styled("y", theme::accent_bold()),
        Span::styled(" yank  ", theme::muted()),
        Span::styled("Enter", theme::primary_action()),
        Span::styled(" connect  ", theme::muted()),
        Span::styled("/", theme::accent_bold()),
        Span::styled(" search  ", theme::muted()),
        Span::styled("?", theme::accent_bold()),
        Span::styled(" help", theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(footer), area);
}

fn render_search_footer(frame: &mut Frame, area: ratatui::layout::Rect) {
    let footer = Line::from(vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" connect  ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" cancel", theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(footer), area);
}
