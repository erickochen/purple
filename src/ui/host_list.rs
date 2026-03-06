use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{self, App, HostListItem, PingStatus, SortMode};

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let is_searching = app.search.query.is_some();
    let is_tagging = app.tag_input.is_some();

    // Layout: host list + optional input bar + footer/status
    let chunks = if is_searching || is_tagging {
        Layout::vertical([
            Constraint::Min(5),   // Host list (maximized)
            Constraint::Length(1), // Search/tag bar
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
        super::render_footer_with_status(frame, chunks[2], search_footer_spans(), app);
    } else if is_tagging {
        render_display_list(frame, app, chunks[0]);
        render_tag_bar(frame, app, chunks[1]);
        super::render_footer_with_status(frame, chunks[2], tag_footer_spans(), app);
    } else {
        render_display_list(frame, app, chunks[0]);
        super::render_footer_with_status(frame, chunks[1], footer_spans(), app);
    }
}

fn render_display_list(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Build multi-span title: brand badge + position counter
    let host_count = app.hosts.len();
    let title = if host_count == 0 {
        Line::from(Span::styled(" purple. ", theme::brand_badge()))
    } else {
        let pos = if let Some(sel) = app.ui.list_state.selected() {
            app.display_list.get(..=sel)
                .map(|slice| slice.iter().filter(|item| matches!(item, HostListItem::Host { .. })).count())
                .unwrap_or(0)
        } else {
            0
        };
        let mut spans = vec![
            Span::styled(" purple. ", theme::brand_badge()),
            Span::raw(format!(" {}/{} ", pos, host_count)),
        ];
        if app.sort_mode != SortMode::Original || app.group_by_provider {
            let mut label = String::new();
            if app.sort_mode != SortMode::Original {
                label.push_str(app.sort_mode.label());
            }
            if app.group_by_provider {
                if !label.is_empty() {
                    label.push_str(", ");
                }
                label.push_str("grouped");
            }
            spans.push(Span::raw(format!("({}) ", label)));
        }
        Line::from(spans)
    };

    let update_title = app.update_available.as_ref().map(|ver| {
        Line::from(Span::styled(
            format!(" v{} available — run '{}' ", ver, app.update_hint),
            theme::update_badge(),
        ))
    });

    if app.hosts.is_empty() {
        let mut block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme::border());
        if let Some(update) = update_title {
            block = block.title_top(update.right_aligned());
        }
        let empty_msg =
            Paragraph::new("  It's quiet in here... Press 'a' to add a host or 'S' for cloud sync.")
                .style(theme::muted())
                .block(block);
        frame.render_widget(empty_msg, area);
        return;
    }

    // Column widths for alignment
    let alias_col = app
        .hosts
        .iter()
        .map(|h| h.alias.width())
        .max()
        .unwrap_or(8)
        .clamp(8, 20);
    let content_width = (area.width as usize).saturating_sub(4);

    // Count hosts per group for group headers
    let group_counts: std::collections::HashMap<&str, usize> = {
        let mut counts = std::collections::HashMap::new();
        let mut current_group: Option<&str> = None;
        for item in &app.display_list {
            match item {
                HostListItem::GroupHeader(text) => {
                    current_group = Some(text.as_str());
                }
                HostListItem::Host { .. } => {
                    if let Some(group) = current_group {
                        *counts.entry(group).or_insert(0) += 1;
                    }
                }
            }
        }
        counts
    };

    let items: Vec<ListItem> = app
        .display_list
        .iter()
        .map(|item| match item {
            HostListItem::GroupHeader(text) => {
                let upper = text.to_uppercase();
                let count = group_counts.get(text.as_str()).copied().unwrap_or(0);
                let label = format!("{} ({})", upper, count);
                let label_width = label.width() + 4; // "── " + label + " "
                let fill = content_width.saturating_sub(label_width);
                let line = Line::from(vec![
                    Span::styled("── ", theme::muted()),
                    Span::styled(label, theme::section_header()),
                    Span::styled(format!(" {}", "─".repeat(fill)), theme::muted()),
                ]);
                ListItem::new(line)
            }
            HostListItem::Host { index } => {
                if let Some(host) = app.hosts.get(*index) {
                    let tunnel_active = app.active_tunnels.contains_key(&host.alias);
                    build_host_item(
                        host,
                        &app.ping_status,
                        &app.history,
                        tunnel_active,
                        None,
                        alias_col,
                        content_width,
                    )
                } else {
                    ListItem::new(Line::from(Span::raw("")))
                }
            }
        })
        .collect();

    let mut block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(theme::border());
    if let Some(update) = update_title {
        block = block.title_top(update.right_aligned());
    }

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, area, &mut app.ui.list_state);
}

fn render_search_list(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let title = Line::from(vec![
        Span::styled(" purple. ", theme::brand_badge()),
        Span::raw(format!(
            " search: {}/{} ",
            app.search.filtered_indices.len(),
            app.hosts.len()
        )),
    ]);

    if app.search.filtered_indices.is_empty() {
        let empty_msg = Paragraph::new("  No matches. Try a different search.")
            .style(theme::muted())
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(theme::accent()),
            );
        frame.render_widget(empty_msg, area);
        return;
    }

    let alias_col = app
        .search.filtered_indices
        .iter()
        .filter_map(|&i| app.hosts.get(i))
        .map(|h| h.alias.width())
        .max()
        .unwrap_or(8)
        .clamp(8, 20);
    let content_width = (area.width as usize).saturating_sub(4);

    let query = app.search.query.as_deref();
    let items: Vec<ListItem> = app
        .search.filtered_indices
        .iter()
        .filter_map(|&idx| {
            let host = app.hosts.get(idx)?;
            let tunnel_active = app.active_tunnels.contains_key(&host.alias);
            Some(build_host_item(
                host,
                &app.ping_status,
                &app.history,
                tunnel_active,
                query,
                alias_col,
                content_width,
            ))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(theme::accent()),
        )
        .highlight_style(theme::selected())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, area, &mut app.ui.list_state);
}

fn build_host_item<'a>(
    host: &'a crate::ssh_config::model::HostEntry,
    ping_status: &'a std::collections::HashMap<String, PingStatus>,
    history: &'a crate::history::ConnectionHistory,
    tunnel_active: bool,
    query: Option<&str>,
    alias_col: usize,
    content_width: usize,
) -> ListItem<'a> {
    let q = query.unwrap_or("");

    // Determine which field matches for search highlighting
    let alias_matches = !q.is_empty() && app::contains_ci(&host.alias, q);
    let host_matches = !alias_matches && !q.is_empty() && app::contains_ci(&host.hostname, q);
    let user_matches =
        !alias_matches && !host_matches && !q.is_empty() && app::contains_ci(&host.user, q);

    // === LEFT: alias (fixed column) + user@hostname:port ===
    let alias_style = if alias_matches {
        theme::highlight_bold()
    } else {
        theme::bold()
    };
    let alias_display = format!(" {:<width$} ", host.alias, width = alias_col);
    let mut left_len = alias_display.width();
    let mut left_spans = vec![Span::styled(alias_display, alias_style)];

    if !host.user.is_empty() {
        let user_style = if user_matches {
            theme::highlight_bold()
        } else {
            theme::muted()
        };
        let s = format!("{}@", host.user);
        left_len += s.width();
        left_spans.push(Span::styled(s, user_style));
    }

    let hostname_style = if host_matches {
        theme::highlight_bold()
    } else {
        Style::default()
    };
    left_len += host.hostname.width();
    left_spans.push(Span::styled(host.hostname.as_str(), hostname_style));

    if host.port != 22 {
        let s = format!(":{}", host.port);
        left_len += s.width();
        left_spans.push(Span::styled(s, theme::muted()));
    }

    // === RIGHT: tags, provider, source, tunnels, ping, history ===
    let mut right_spans: Vec<Span> = Vec::new();
    let mut right_len: usize = 0;

    let tag_matches = !q.is_empty() && !alias_matches && !host_matches && !user_matches;
    for tag in &host.tags {
        let style = if tag_matches && app::contains_ci(tag, q) {
            theme::highlight_bold()
        } else {
            theme::accent()
        };
        let s = format!(" #{}", tag);
        right_len += s.width();
        right_spans.push(Span::styled(s, style));
    }

    if let Some(ref label) = host.provider {
        let style = if tag_matches && app::contains_ci(label, q) {
            theme::highlight_bold()
        } else {
            theme::accent()
        };
        let s = format!(" #{}", label);
        right_len += s.width();
        right_spans.push(Span::styled(s, style));
    }

    if let Some(ref source) = host.source_file {
        let file_name = source
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        if !file_name.is_empty() {
            let s = format!(" ({})", file_name);
            right_len += s.width();
            right_spans.push(Span::styled(s, theme::muted()));
        }
    }

    // Password source indicator
    if host.askpass.is_some() {
        let indicator = " [P]";
        right_len += indicator.len();
        right_spans.push(Span::styled(indicator, theme::muted()));
    }

    // Tunnel indicator
    if host.tunnel_count > 0 {
        let (indicator, style) = if tunnel_active {
            (" [T]", theme::bold())
        } else {
            (" [T]", theme::muted())
        };
        right_len += indicator.width();
        right_spans.push(Span::styled(indicator, style));
    }

    if let Some(status) = ping_status.get(&host.alias) {
        let (indicator, style) = match status {
            PingStatus::Checking => ("[..]", theme::muted()),
            PingStatus::Reachable => ("[ok]", theme::success()),
            PingStatus::Unreachable => ("[--]", theme::error()),
            PingStatus::Skipped => ("[??]", theme::muted()),
        };
        let sep = " ";
        right_len += sep.width() + indicator.width();
        right_spans.push(Span::raw(sep));
        right_spans.push(Span::styled(indicator, style));
    }

    if let Some(entry) = history.entries.get(&host.alias) {
        let ago = crate::history::ConnectionHistory::format_time_ago(entry.last_connected);
        if !ago.is_empty() {
            let s = format!(" {}", ago);
            right_len += s.width();
            right_spans.push(Span::styled(s, theme::muted()));
        }
    }

    // === COMBINE: left + padding + right ===
    let padding = content_width.saturating_sub(left_len + right_len);
    let mut spans = left_spans;
    if padding > 0 {
        spans.push(Span::raw(" ".repeat(padding)));
    }
    spans.extend(right_spans);

    ListItem::new(Line::from(spans))
}

fn render_search_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let query = app.search.query.as_deref().unwrap_or("");
    let match_info = if query.is_empty() {
        String::new()
    } else {
        let count = app.search.filtered_indices.len();
        match count {
            0 => " (no matches)".to_string(),
            1 => " (1 match)".to_string(),
            n => format!(" ({} matches)", n),
        }
    };
    let search_line = Line::from(vec![
        Span::styled(" / ", theme::accent_bold()),
        Span::raw(query),
        Span::styled("_", theme::accent()),
        Span::styled(match_info, theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(search_line), area);
}

fn footer_spans<'a>() -> Vec<Span<'a>> {
    vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" connect ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("/", theme::accent_bold()),
        Span::styled(" search ", theme::muted()),
        Span::styled("#", theme::accent_bold()),
        Span::styled(" tag ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("a", theme::accent_bold()),
        Span::styled(" add ", theme::muted()),
        Span::styled("e", theme::accent_bold()),
        Span::styled(" edit ", theme::muted()),
        Span::styled("d", theme::accent_bold()),
        Span::styled(" del ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("?", theme::accent_bold()),
        Span::styled(" help", theme::muted()),
    ]
}

fn search_footer_spans<'a>() -> Vec<Span<'a>> {
    vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" connect ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" cancel", theme::muted()),
    ]
}

fn render_tag_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let input = app.tag_input.as_deref().unwrap_or("");
    let tag_line = Line::from(vec![
        Span::styled(" tags: ", theme::accent_bold()),
        Span::raw(input),
        Span::styled("_", theme::accent()),
    ]);
    frame.render_widget(Paragraph::new(tag_line), area);
}

fn tag_footer_spans<'a>() -> Vec<Span<'a>> {
    vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" save  ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" cancel  ", theme::muted()),
        Span::styled("comma-separated", theme::muted()),
    ]
}
