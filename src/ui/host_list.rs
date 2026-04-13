use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, List, ListItem, Paragraph, Tabs};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{self, App, GroupBy, HostListItem, PingStatus, ViewMode};

/// Minimum terminal width to show the detail panel in detailed view mode.
const DETAIL_MIN_WIDTH: u16 = 95;

/// Format an RTT value in milliseconds for the PING column.
pub(crate) fn format_rtt(ms: u32) -> String {
    if ms >= 9_950 {
        "10s+".to_string()
    } else if ms >= 1_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}ms", ms)
    }
}

/// Build the update badge label, truncating the headline with ellipsis if needed.
/// `max_width` is the border area width (including border chars).
fn build_update_label(ver: &str, headline: Option<&str>, hint: &str, max_width: u16) -> String {
    // Budget: area width minus 2 border chars and 1 char padding on each side
    let budget = (max_width as usize).saturating_sub(4);
    match headline {
        Some(hl) => {
            let full = format!(" v{}: {} (run {}) ", ver, hl, hint);
            if full.width() <= budget {
                return full;
            }
            // Truncate headline to fit
            let prefix = format!(" v{}: ", ver);
            let suffix = format!(" (run {}) ", hint);
            let hl_budget = budget
                .saturating_sub(prefix.width())
                .saturating_sub(suffix.width());
            if hl_budget >= 4 {
                let hl_trunc = super::truncate(hl, hl_budget);
                format!("{}{}{}", prefix, hl_trunc, suffix)
            } else {
                // Not enough room for headline: fall back to version-only
                format!(" v{} available, run {} ", ver, hint)
            }
        }
        None => format!(" v{} available, run {} ", ver, hint),
    }
}

const HOST_MIN: usize = 12;
/// Width of the row marker (indent + selection checkmark space).
const MARKER_WIDTH: usize = 2;

/// Column layout computed from the visible host list.
pub(crate) struct Columns {
    alias: usize,
    host: usize,
    tags: usize,
    history: usize,
    gap: usize,
    /// Flexible gap between left cluster (NAME+ADDRESS) and right cluster (TAGS+LAST).
    flex_gap: usize,
    /// True when detail panel is showing (ADDRESS column hidden).
    detail_mode: bool,
}

impl Columns {
    /// Add ~10% breathing room to a content-measured column width.
    /// Returns 0 for 0-width columns (no content = no column).
    fn padded(w: usize) -> usize {
        if w == 0 { 0 } else { w + w / 10 + 1 }
    }

    fn compute(
        alias_w: usize,
        host_w: usize,
        tags_w: usize,
        history_w: usize,
        content: usize,
        detail_mode: bool,
    ) -> Self {
        // All columns get ~110% of their content width for breathing room.
        // Columns are capped — they never grow beyond content needs.
        let alias = Self::padded(alias_w).clamp(8, 32);
        let mut host = if detail_mode {
            0
        } else {
            Self::padded(host_w).max(HOST_MIN)
        };
        let mut tags = if tags_w > 0 {
            Self::padded(tags_w).max(4)
        } else {
            0
        };
        let mut history = if history_w > 0 {
            Self::padded(history_w).max(4)
        } else {
            0
        };

        // Fixed gap between columns within a cluster
        let gap: usize = if content >= 120 { 3 } else { 2 };

        // Total width of the right cluster (TAGS, LAST + gaps)
        let right_cluster = |tags: usize, history: usize| -> usize {
            let mut w = 0usize;
            let mut n = 0usize;
            if tags > 0 {
                w += tags;
                n += 1;
            }
            if history > 0 {
                w += history;
                n += 1;
            }
            let gaps = if n > 1 { (n - 1) * gap } else { 0 };
            w + gaps
        };

        // Left cluster: highlight_symbol(1) + marker + status(2) + NAME [+ gap + ADDRESS]
        let left = if detail_mode {
            MARKER_WIDTH + 1 + 2 + alias
        } else {
            MARKER_WIDTH + 1 + 2 + alias + gap + host
        };

        // Total with minimum flex_gap = gap
        let mut rw = right_cluster(tags, history);

        // Hide right-cluster columns by priority: LAST → TAGS → ADDRESS
        if left + gap + rw > content && history > 0 {
            history = 0;
            rw = right_cluster(tags, history);
        }
        if left + gap + rw > content && tags > 0 {
            tags = 0;
            rw = right_cluster(tags, history);
        }
        // Shrink or hide ADDRESS (only when not in detail_mode, where it's already 0)
        if !detail_mode && host > 0 {
            let needed = MARKER_WIDTH + 1 + 2 + alias + gap + host + gap + rw;
            if needed > content {
                let excess = needed - content;
                if host.saturating_sub(excess) >= HOST_MIN {
                    // Shrink host but keep it above minimum
                    host = host.saturating_sub(excess);
                } else {
                    // Can't fit even HOST_MIN: hide address entirely
                    host = 0;
                }
            }
        }

        // Flex gap: remaining space between left and right clusters
        let left_final = if detail_mode {
            MARKER_WIDTH + 1 + 2 + alias
        } else if host > 0 {
            MARKER_WIDTH + 1 + 2 + alias + gap + host
        } else {
            MARKER_WIDTH + 1 + 2 + alias
        };
        let flex_gap = if rw > 0 {
            content.saturating_sub(left_final + rw)
        } else {
            0
        };

        Columns {
            alias,
            host,
            tags,
            history,
            gap,
            flex_gap,
            detail_mode,
        }
    }
}

/// Compute the display width of the composite host label (hostname:port)
/// without allocating a String. Uses the hostname's Unicode width plus the
/// port suffix length when the port is non-default.
fn composite_host_width(host: &crate::ssh_config::model::HostEntry) -> usize {
    let w = host.hostname.width();
    if host.port == 22 {
        w
    } else {
        // ":NNNNN" — colon (1) + digit count
        w + 1 + digit_count(host.port)
    }
}

fn digit_count(mut n: u16) -> usize {
    if n == 0 {
        return 1;
    }
    let mut count = 0;
    while n > 0 {
        count += 1;
        n /= 10;
    }
    count
}

/// Build composite host label: hostname:port (only showing non-default parts).
#[cfg(test)]
fn composite_host_label(host: &crate::ssh_config::model::HostEntry) -> String {
    let mut s = String::new();
    s.push_str(&host.hostname);
    if host.port != 22 {
        s.push(':');
        s.push_str(&host.port.to_string());
    }
    s
}

pub fn render(frame: &mut Frame, app: &mut App, spinner_tick: u64, detail_progress: Option<f32>) {
    let area = frame.area();

    let is_searching = app.search.query.is_some();
    let is_tagging = app.tags.input.is_some();
    // Group bar: bordered block with tabs (top + content + bottom = 3 rows).
    // Only shown when grouping is active and there are groups to display.
    let show_group_bar = !matches!(app.group_by, GroupBy::None);
    let group_bar_height: u16 = if show_group_bar { 3 } else { 0 };

    // Layout: optional group bar + host list + optional input bar + footer/status
    let chunks = if is_searching || is_tagging {
        Layout::vertical([
            Constraint::Length(group_bar_height), // Group bar (0 when hidden)
            Constraint::Min(5),                   // Host list (maximized)
            Constraint::Length(1),                // Search/tag bar
            Constraint::Length(1),                // Footer or status message
        ])
        .split(area)
    } else {
        Layout::vertical([
            Constraint::Length(group_bar_height), // Group bar (0 when hidden)
            Constraint::Min(5),                   // Host list (maximized)
            Constraint::Length(1),                // Footer or status message
        ])
        .split(area)
    };

    if show_group_bar {
        render_group_bar(frame, app, chunks[0]);
    }

    let content_area = chunks[1];
    let target_detail =
        app.view_mode == ViewMode::Detailed && content_area.width >= DETAIL_MIN_WIDTH;
    let full_detail_width = if content_area.width >= 140 {
        46u16
    } else {
        40u16
    };

    // Calculate detail width: interpolated during animation, instant otherwise.
    let detail_width = if content_area.width >= DETAIL_MIN_WIDTH {
        if let Some(progress) = detail_progress {
            (progress * full_detail_width as f32).round() as u16
        } else if target_detail {
            full_detail_width
        } else {
            0
        }
    } else {
        0
    };
    let use_detail = detail_width > 0;

    // Minimum width before we render detail content (border + 1 char padding)
    const DETAIL_RENDER_MIN: u16 = 8;

    let (list_area, detail_area) = if use_detail {
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(detail_width)])
                .areas(content_area);
        (left, Some(right))
    } else {
        (content_area, None)
    };

    if is_searching {
        render_search_list(frame, app, list_area, spinner_tick);
        render_search_bar(frame, app, chunks[2]);
        super::render_footer_with_status(frame, chunks[3], search_footer_spans(), app);
    } else if is_tagging {
        render_display_list(frame, app, list_area, spinner_tick);
        render_tag_bar(frame, app, chunks[2]);
        super::render_footer_with_status(frame, chunks[3], tag_footer_spans(), app);
    } else {
        render_display_list(frame, app, list_area, spinner_tick);
        let spans = if app.is_pattern_selected() {
            pattern_footer_spans(target_detail)
        } else {
            footer_spans(target_detail, app.ping.filter_down_only)
        };
        super::render_footer_with_help(frame, chunks[2], spans, app);
    }

    if let Some(detail) = detail_area {
        if detail.width >= DETAIL_RENDER_MIN {
            super::detail_panel::render(frame, app, detail, spinner_tick);
        } else {
            // During animation: render empty bordered area
            let block = ratatui::widgets::Block::bordered()
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(theme::border());
            frame.render_widget(block, detail);
        }
    }
}

fn render_group_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let total = app.hosts.len() + app.patterns.len();

    let titles: Vec<Line> = match &app.group_by {
        GroupBy::Tag(_) => std::iter::once(Line::from(vec![
            Span::styled(" All ", theme::bold()),
            Span::styled(format!("({})", total), theme::muted()),
        ]))
        .chain(app.group_tab_order.iter().map(|tag| {
            let count = app
                .group_host_counts
                .get(tag.as_str())
                .copied()
                .unwrap_or(0);
            Line::from(vec![
                Span::styled(format!(" {} ", tag), theme::bold()),
                Span::styled(format!("({})", count), theme::muted()),
            ])
        }))
        .collect(),
        _ => std::iter::once(("All".to_string(), total))
            .chain(app.group_tab_order.iter().map(|name| {
                let count = app
                    .group_host_counts
                    .get(name.as_str())
                    .copied()
                    .unwrap_or(0);
                (name.to_uppercase(), count)
            }))
            .map(|(name, count)| {
                Line::from(vec![
                    Span::styled(format!(" {} ", name), theme::bold()),
                    Span::styled(format!("({})", count), theme::muted()),
                ])
            })
            .collect(),
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" purple ", theme::brand()))
        .border_style(theme::border());

    let tabs = Tabs::new(titles)
        .select(app.group_tab_index)
        .highlight_style(theme::brand_badge())
        .divider(Span::raw("  "))
        .block(block);

    frame.render_widget(tabs, area);
}

/// Returns "purple" branding when group bar is hidden, "hosts" when grouped.
fn brand_label_for_group(group_by: &GroupBy) -> &'static str {
    if matches!(group_by, GroupBy::None) {
        " purple "
    } else {
        " HOSTS "
    }
}

fn render_display_list(
    frame: &mut Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
    spinner_tick: u64,
) {
    // Build multi-span title: hosts count + optional state badges.
    // Show "purple" branding when group bar is hidden, "hosts" otherwise.
    let visible_count = app
        .display_list
        .iter()
        .filter(|i| matches!(i, HostListItem::Host { .. } | HostListItem::Pattern { .. }))
        .count();
    let brand_label = brand_label_for_group(&app.group_by);
    let brand_style = if matches!(app.group_by, GroupBy::None) {
        theme::brand_badge()
    } else {
        theme::brand()
    };
    let mut title_spans = vec![
        Span::styled(brand_label, brand_style),
        Span::styled("── ", theme::muted()),
        Span::styled(format!("{} ", visible_count), theme::bold()),
    ];
    if app.tags.input.is_some() {
        title_spans.push(Span::styled("── ", theme::muted()));
        title_spans.push(Span::styled(" TAGGING ", theme::brand_badge()));
    } else if !app.multi_select.is_empty() {
        title_spans.push(Span::styled("── ", theme::muted()));
        title_spans.push(Span::styled(
            format!(" {} SELECTED ", app.multi_select.len()),
            theme::brand_badge(),
        ));
    } else {
        // Health summary after count (scoped to visible hosts when group filter active)
        let health = if app.group_filter.is_some() {
            let visible_aliases = app.display_list.iter().filter_map(|item| match item {
                HostListItem::Host { index } => app.hosts.get(*index).map(|h| h.alias.as_str()),
                _ => None,
            });
            app::health_summary_spans_for(&app.ping.status, visible_aliases)
        } else {
            app::health_summary_spans(&app.ping.status, &app.hosts)
        };
        if !health.is_empty() {
            title_spans.push(Span::styled("── ", theme::muted()));
            title_spans.extend(health);
            title_spans.push(Span::raw(" "));
        }
        // Group filter label
        if let Some(ref filter) = app.group_filter {
            title_spans.push(Span::styled("── ", theme::muted()));
            title_spans.push(Span::styled(format!("{} ", filter), theme::muted()));
        }
    }
    let title = Line::from(title_spans);

    let update_title = app.update.available.as_ref().map(|ver| {
        let label = build_update_label(
            ver,
            app.update.headline.as_deref(),
            app.update.hint,
            area.width,
        );
        Line::from(Span::styled(label, theme::update_badge()))
    });

    let url_label = Line::from(Span::styled(" getpurple.sh ", theme::muted()));

    if app.hosts.is_empty() {
        let mut block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(title)
            .title_bottom(url_label.clone().right_aligned())
            .border_style(theme::border());
        if let Some(update) = update_title {
            block = block.title_top(update.right_aligned());
        }
        let msg = if matches!(app.screen, app::Screen::Welcome { .. }) {
            ""
        } else {
            "  It's quiet in here... Press 'a' to add a host or 'S' for cloud sync."
        };
        let empty_msg = Paragraph::new(msg).style(theme::muted()).block(block);
        frame.render_widget(empty_msg, area);
        return;
    }

    // Build block and render border separately for column header
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .title_bottom(url_label.right_aligned())
        .border_style(theme::border());
    if let Some(update) = update_title {
        block = block.title_top(update.right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Compute column layout
    let content_width = (inner.width as usize).saturating_sub(2); // -1 right margin, -1 left margin
    // Detail mode: detail panel is visible when ViewMode::Detailed and terminal is wide enough.
    let detail_mode = app.view_mode == ViewMode::Detailed && frame.area().width >= DETAIL_MIN_WIDTH;
    let alias_w = app.hosts.iter().map(|h| h.alias.width()).max().unwrap_or(8);
    let host_w = app
        .hosts
        .iter()
        .map(composite_host_width)
        .max()
        .unwrap_or(12);
    let tags_w = app
        .hosts
        .iter()
        .map(|h| host_tags_width(h, &app.group_by, detail_mode))
        .max()
        .unwrap_or(0);
    let history_w = app
        .hosts
        .iter()
        .filter_map(|h| app.history.entries.get(&h.alias))
        .map(|e| crate::history::ConnectionHistory::format_time_ago(e.last_connected))
        .filter(|s| !s.is_empty())
        .map(|s| s.width())
        .max()
        .unwrap_or(0);
    let cols = Columns::compute(
        alias_w,
        host_w,
        tags_w,
        history_w,
        content_width,
        detail_mode,
    );

    // Column header + underline + list body
    let [header_area, underline_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    render_header(frame, header_area, &cols, app.sort_mode);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(underline_area.width as usize),
            theme::muted(),
        )),
        underline_area,
    );

    // Pre-build group alias map for health summaries (avoids O(N²) scan)
    let group_alias_map: std::collections::HashMap<&str, Vec<&str>> = {
        let mut map: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
        let mut current_group: Option<&str> = None;
        for item in &app.display_list {
            match item {
                HostListItem::GroupHeader(text) => {
                    current_group = Some(text.as_str());
                }
                HostListItem::Host { index } => {
                    if let (Some(group), Some(host)) = (current_group, app.hosts.get(*index)) {
                        map.entry(group).or_default().push(host.alias.as_str());
                    }
                }
                _ => {}
            }
        }
        map
    };

    let mut items: Vec<ListItem> = Vec::new();
    for item in &app.display_list {
        match item {
            HostListItem::GroupHeader(text) => {
                let upper = text.to_uppercase();
                let count = app
                    .group_host_counts
                    .get(text.as_str())
                    .copied()
                    .unwrap_or(0);
                let prefix = format!("── {} ({}) ", upper, count);
                // Subtract 1 for the highlight symbol gutter that ratatui
                // prepends to every ListItem.
                let available = content_width.saturating_sub(1);

                // Build health summary for this group's hosts (uses pre-built map)
                let aliases = group_alias_map
                    .get(text.as_str())
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                let health_spans =
                    app::health_summary_spans_for(&app.ping.status, aliases.iter().copied());

                if health_spans.is_empty() {
                    // No pings: just name + count + fill dashes
                    let fill_width = available.saturating_sub(prefix.width());
                    let line = Line::from(vec![
                        Span::styled(prefix, theme::bold()),
                        Span::styled("─".repeat(fill_width), theme::muted()),
                    ]);
                    items.push(ListItem::new(line));
                } else {
                    // With health: name (count) ── health_summary ─────
                    let separator = "── ";
                    let health_text_width: usize =
                        health_spans.iter().map(|s| s.content.width()).sum();
                    let fill_width = available
                        .saturating_sub(prefix.width())
                        .saturating_sub(separator.width())
                        .saturating_sub(health_text_width);
                    let mut spans = vec![
                        Span::styled(prefix, theme::bold()),
                        Span::styled("── ", theme::muted()),
                    ];
                    spans.extend(health_spans);
                    if fill_width > 0 {
                        spans.push(Span::styled("─".repeat(fill_width), theme::muted()));
                    }
                    items.push(ListItem::new(Line::from(spans)));
                }
            }
            HostListItem::Host { index } => {
                if let Some(host) = app.hosts.get(*index) {
                    let tunnel_active = app.active_tunnels.contains_key(&host.alias);
                    let item_ctx = HostItemContext {
                        ping_status: &app.ping.status,
                        history: &app.history,
                        tunnel_active,
                        query: None,
                        cols: &cols,
                        multi_selected: app.multi_select.contains(index),
                        group_by: &app.group_by,
                        detail_mode,
                        spinner_tick,
                    };
                    let list_item = build_host_item(host, &item_ctx);
                    items.push(list_item);
                } else {
                    items.push(ListItem::new(Line::from(Span::raw(""))));
                }
            }
            HostListItem::Pattern { index } => {
                if let Some(pattern) = app.patterns.get(*index) {
                    items.push(build_pattern_item(pattern, &cols));
                } else {
                    items.push(ListItem::new(Line::from(Span::raw(""))));
                }
            }
        }
    }

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol("\u{258C}");

    frame.render_stateful_widget(list, list_area, &mut app.ui.list_state);
}

fn render_search_list(
    frame: &mut Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
    spinner_tick: u64,
) {
    let total_results =
        app.search.filtered_indices.len() + app.search.filtered_pattern_indices.len();
    let total = app.hosts.len() + app.patterns.len();
    let title = Line::from(vec![
        Span::styled(" HOSTS ", theme::brand()),
        Span::styled("── ", theme::muted()),
        Span::styled(
            format!("search: {}/{} ", total_results, total),
            theme::bold(),
        ),
    ]);

    let update_title = app.update.available.as_ref().map(|ver| {
        let label = build_update_label(
            ver,
            app.update.headline.as_deref(),
            app.update.hint,
            area.width,
        );
        Line::from(Span::styled(label, theme::update_badge()))
    });

    let url_label = Line::from(Span::styled(" getpurple.sh ", theme::muted()));

    if app.search.filtered_indices.is_empty() && app.search.filtered_pattern_indices.is_empty() {
        let mut block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(title)
            .title_bottom(url_label.clone().right_aligned())
            .border_style(theme::border_search());
        if let Some(update) = update_title {
            block = block.title_top(update.right_aligned());
        }
        let empty_msg = Paragraph::new("  No matches. Try a different search.")
            .style(theme::muted())
            .block(block);
        frame.render_widget(empty_msg, area);
        return;
    }

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .title_bottom(url_label.right_aligned())
        .border_style(theme::border_search());
    if let Some(update) = update_title {
        block = block.title_top(update.right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let content_width = (inner.width as usize).saturating_sub(2); // -1 right margin, -1 left margin
    let filtered_hosts = || {
        app.search
            .filtered_indices
            .iter()
            .filter_map(|&i| app.hosts.get(i))
    };
    let alias_w = filtered_hosts().map(|h| h.alias.width()).max().unwrap_or(8);
    let host_w = filtered_hosts()
        .map(composite_host_width)
        .max()
        .unwrap_or(12);
    let tags_w = filtered_hosts()
        .map(|h| host_tags_width(h, &app.group_by, false))
        .max()
        .unwrap_or(0);
    let history_w = filtered_hosts()
        .filter_map(|h| app.history.entries.get(&h.alias))
        .map(|e| crate::history::ConnectionHistory::format_time_ago(e.last_connected))
        .filter(|s| !s.is_empty())
        .map(|s| s.width())
        .max()
        .unwrap_or(0);
    let cols = Columns::compute(alias_w, host_w, tags_w, history_w, content_width, false);

    let [header_area, underline_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    render_header(frame, header_area, &cols, app.sort_mode);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(underline_area.width as usize),
            theme::muted(),
        )),
        underline_area,
    );

    let query = app.search.query.as_deref();
    let mut items: Vec<ListItem> = Vec::new();
    for &idx in app.search.filtered_indices.iter() {
        if let Some(host) = app.hosts.get(idx) {
            let tunnel_active = app.active_tunnels.contains_key(&host.alias);
            let item_ctx = HostItemContext {
                ping_status: &app.ping.status,
                history: &app.history,
                tunnel_active,
                query,
                cols: &cols,
                multi_selected: app.multi_select.contains(&idx),
                group_by: &app.group_by,
                detail_mode: false,
                spinner_tick,
            };
            let list_item = build_host_item(host, &item_ctx);
            items.push(list_item);
        }
    }
    for &idx in app.search.filtered_pattern_indices.iter() {
        if let Some(pattern) = app.patterns.get(idx) {
            items.push(build_pattern_item(pattern, &cols));
        }
    }

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol("\u{258C}");

    frame.render_stateful_widget(list, list_area, &mut app.ui.list_state);
}

fn render_header(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    cols: &Columns,
    sort_mode: crate::app::SortMode,
) {
    use crate::app::SortMode;
    let style = theme::bold();
    let gap = " ".repeat(cols.gap);
    let flex = " ".repeat(cols.flex_gap);

    // Sort indicator: ▾ next to the active sort column
    let name_sort = matches!(sort_mode, SortMode::AlphaAlias);
    let host_sort = matches!(sort_mode, SortMode::AlphaHostname);
    let last_sort = matches!(sort_mode, SortMode::MostRecent | SortMode::Frecency);

    let mut spans = vec![Span::styled(
        format!(
            "{}{:<width$}",
            " ".repeat(MARKER_WIDTH + 1 + 2),
            if name_sort { "NAME \u{25BE}" } else { "NAME" },
            width = cols.alias
        ),
        style,
    )];
    // ADDRESS column (hidden in detail_mode)
    if !cols.detail_mode && cols.host > 0 {
        spans.push(Span::raw(gap.clone()));
        spans.push(Span::styled(
            format!(
                "{:<width$}",
                if host_sort {
                    "ADDRESS \u{25BE}"
                } else {
                    "ADDRESS"
                },
                width = cols.host
            ),
            style,
        ));
    }
    // Flex gap between left and right cluster
    if cols.flex_gap > 0 {
        spans.push(Span::raw(flex));
    }
    if cols.tags > 0 {
        spans.push(Span::styled(
            format!("{:<width$}", "TAGS", width = cols.tags),
            style,
        ));
        spans.push(Span::raw(gap.clone()));
    }
    if cols.history > 0 {
        spans.push(Span::styled(
            format!("{:>width$}", "LAST", width = cols.history),
            style,
        ));
        if last_sort {
            spans.push(Span::styled("\u{25BE}", style));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Compute the display width of a host's tags (up to 3 tags, no # prefix).
fn host_tags_width(
    host: &crate::ssh_config::model::HostEntry,
    group_by: &crate::app::GroupBy,
    detail_mode: bool,
) -> usize {
    let tags = crate::app::select_display_tags(host, group_by, detail_mode);
    let mut w = 0usize;
    for tag in &tags {
        if w > 0 {
            w += 1; // space separator
        }
        w += tag.name.width();
    }
    w
}

pub(crate) struct HostItemContext<'a> {
    pub ping_status: &'a std::collections::HashMap<String, PingStatus>,
    pub history: &'a crate::history::ConnectionHistory,
    pub tunnel_active: bool,
    pub query: Option<&'a str>,
    pub cols: &'a Columns,
    pub multi_selected: bool,
    pub group_by: &'a GroupBy,
    pub detail_mode: bool,
    pub spinner_tick: u64,
}

fn build_host_item<'a>(
    host: &'a crate::ssh_config::model::HostEntry,
    ctx: &HostItemContext<'_>,
) -> ListItem<'a> {
    let q = ctx.query.unwrap_or("");
    let gap = " ".repeat(ctx.cols.gap);

    // Determine which field matches for search highlighting
    let alias_matches = !q.is_empty() && app::contains_ci(&host.alias, q);
    let host_matches = !alias_matches
        && !q.is_empty()
        && (app::contains_ci(&host.hostname, q) || app::contains_ci(&host.user, q));

    let mut spans: Vec<Span> = Vec::new();

    // === NAME column (fixed width) ===
    let is_stale = host.stale.is_some();
    let alias_style = if alias_matches {
        theme::highlight_bold()
    } else if is_stale {
        theme::muted()
    } else {
        theme::bold()
    };
    let marker = if ctx.multi_selected {
        " \u{2713}"
    } else {
        "  "
    };
    spans.push(Span::styled(marker, alias_style));

    // Status indicator (2 chars wide): dual-encoded glyph (color + shape)
    let ping = ctx.ping_status.get(&host.alias);
    let glyph = app::status_glyph(ping, ctx.spinner_tick);
    let style = match ping {
        Some(PingStatus::Reachable { .. }) => theme::online_dot(),
        Some(PingStatus::Slow { .. }) => theme::warning(),
        Some(PingStatus::Unreachable) => theme::error(),
        // Skipped: style unused (glyph is empty → Span::raw), kept for exhaustive match
        Some(PingStatus::Checking) | Some(PingStatus::Skipped) | None => theme::muted(),
    };
    let status_span = if glyph.is_empty() {
        Span::raw("  ")
    } else {
        Span::styled(format!("{} ", glyph), style)
    };
    spans.push(status_span);

    let has_jump = !host.proxy_jump.is_empty();
    let has_tunnels = ctx.tunnel_active || host.tunnel_count > 0;

    // In detail mode the ADDRESS column is hidden, so append indicators
    // directly after the alias to keep them visible.
    if ctx.cols.detail_mode {
        let indicator_w = (if has_jump { 2 } else { 0 }) + (if has_tunnels { 2 } else { 0 });
        let alias_budget = ctx.cols.alias.saturating_sub(indicator_w);
        let alias_truncated = super::truncate(&host.alias, alias_budget);
        let alias_w = alias_truncated.width();
        spans.push(Span::styled(alias_truncated, alias_style));
        if has_jump {
            let jump_style = if crate::ssh_config::model::proxy_jump_contains_self(
                &host.proxy_jump,
                &host.alias,
            ) {
                theme::error()
            } else {
                theme::muted()
            };
            spans.push(Span::styled(" \u{2197}", jump_style)); // ↗
        }
        if has_tunnels {
            let tunnel_style = if ctx.tunnel_active {
                theme::version()
            } else {
                theme::muted()
            };
            spans.push(Span::styled(" \u{21C4}", tunnel_style)); // ⇄
        }
        let pad = ctx.cols.alias.saturating_sub(alias_w + indicator_w);
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
    } else {
        let alias_truncated = super::truncate(&host.alias, ctx.cols.alias);
        spans.push(Span::styled(
            format!("{:<width$}", alias_truncated, width = ctx.cols.alias),
            alias_style,
        ));
    }
    // === ADDRESS column (flex width): hostname:port with indicators ===
    // Hidden in detail_mode (cols.host == 0).
    if ctx.cols.host > 0 {
        spans.push(Span::raw(gap.clone()));
        let has_port = host.port != 22;
        let port_suffix = if has_port {
            format!(":{}", host.port)
        } else {
            String::new()
        };
        let port_suffix_w = port_suffix.width();
        let jump_w = if has_jump { 2 } else { 0 }; // " ↗"
        let tunnel_w = if has_tunnels { 2 } else { 0 }; // " ⇄"
        let suffix_w = port_suffix_w + jump_w + tunnel_w;
        let hostname_budget = ctx.cols.host.saturating_sub(suffix_w);

        let mut host_used = 0usize;
        if host_matches {
            let trunc = super::truncate(&host.hostname, hostname_budget);
            host_used += trunc.width();
            spans.push(Span::styled(trunc, theme::highlight_bold()));
        } else {
            let trunc = super::truncate(&host.hostname, hostname_budget);
            host_used += trunc.width();
            spans.push(Span::styled(trunc, theme::muted()));
        }
        if has_port {
            spans.push(Span::styled(port_suffix, theme::muted()));
            host_used += port_suffix_w;
        }
        if has_jump {
            let jump_style = if crate::ssh_config::model::proxy_jump_contains_self(
                &host.proxy_jump,
                &host.alias,
            ) {
                theme::error() // self-referencing loop
            } else {
                theme::muted()
            };
            spans.push(Span::styled(" \u{2197}", jump_style)); // ↗
            host_used += 2;
        }
        if has_tunnels {
            let tunnel_style = if ctx.tunnel_active {
                theme::version() // purple accent when active
            } else {
                theme::muted() // dim when configured but not running
            };
            spans.push(Span::styled(" \u{21C4}", tunnel_style)); // ⇄
            host_used += 2;
        }
        // padding
        let host_pad = ctx.cols.host.saturating_sub(host_used);
        if host_pad > 0 {
            spans.push(Span::raw(" ".repeat(host_pad)));
        }
    }

    // === Flex gap between left cluster (NAME+ADDRESS) and right cluster ===
    if ctx.cols.flex_gap > 0 {
        spans.push(Span::raw(" ".repeat(ctx.cols.flex_gap)));
    }

    // === TAGS column (fixed width, +N overflow) ===
    if ctx.cols.tags > 0 {
        let tag_matches = !q.is_empty() && !alias_matches && !host_matches;
        build_tag_column(
            &mut spans,
            host,
            ctx.group_by,
            ctx.detail_mode,
            tag_matches,
            q,
            ctx.cols.tags,
        );
        if ctx.cols.history > 0 {
            spans.push(Span::raw(gap.clone()));
        }
    }

    // === LAST column (right-aligned, always muted) ===
    if ctx.cols.history > 0 {
        if let Some(entry) = ctx.history.entries.get(&host.alias) {
            let ago = crate::history::ConnectionHistory::format_time_ago(entry.last_connected);
            if !ago.is_empty() {
                spans.push(Span::styled(
                    format!("{:>width$}", ago, width = ctx.cols.history),
                    theme::muted(),
                ));
            } else {
                spans.push(Span::styled(
                    format!("{:>width$}", "-", width = ctx.cols.history),
                    theme::muted(),
                ));
            }
        } else {
            spans.push(Span::styled(
                format!("{:>width$}", "-", width = ctx.cols.history),
                theme::muted(),
            ));
        }
    }

    ListItem::new(Line::from(spans))
}

fn build_pattern_item<'a>(
    pattern: &'a crate::ssh_config::model::PatternEntry,
    cols: &Columns,
) -> ListItem<'a> {
    let gap = " ".repeat(cols.gap);
    let mut spans: Vec<Span> = Vec::new();

    // NAME column: marker(2) + status area used as "* "(2) + alias at full width.
    // This matches host item layout: marker(2) + status(2) + alias(cols.alias).
    let pattern_trunc = super::truncate(&pattern.pattern, cols.alias);
    spans.push(Span::styled("  ", theme::muted())); // marker area (2 chars)
    spans.push(Span::styled("* ", theme::accent())); // status area reused for * prefix (2 chars)
    spans.push(Span::styled(
        format!("{:<width$}", pattern_trunc, width = cols.alias),
        theme::muted(),
    ));

    // ADDRESS column: hostname if present, else empty (hidden in detail_mode)
    if cols.host > 0 {
        spans.push(Span::raw(gap.clone()));
        let host_display = if !pattern.hostname.is_empty() {
            super::truncate(&pattern.hostname, cols.host)
        } else {
            String::new()
        };
        let host_used = UnicodeWidthStr::width(host_display.as_str());
        if !host_display.is_empty() {
            spans.push(Span::styled(host_display, theme::muted()));
        }
        let host_pad = cols.host.saturating_sub(host_used);
        if host_pad > 0 {
            spans.push(Span::raw(" ".repeat(host_pad)));
        }
    }

    if cols.flex_gap > 0 {
        spans.push(Span::raw(" ".repeat(cols.flex_gap)));
    }
    if cols.tags > 0 {
        build_pattern_tag_column(&mut spans, pattern, cols.tags);
        if cols.history > 0 {
            spans.push(Span::raw(gap));
        }
    }
    if cols.history > 0 {
        spans.push(Span::raw(" ".repeat(cols.history)));
    }

    ListItem::new(Line::from(spans))
}

/// Render styled tags into spans within a fixed column width, with +N overflow.
fn render_tag_spans(spans: &mut Vec<Span<'_>>, all_tags: &[(String, Style)], width: usize) {
    let mut used = 0usize;
    let mut shown = 0usize;
    for (i, (tag, style)) in all_tags.iter().enumerate() {
        let sep = if shown > 0 { 1 } else { 0 };
        let tag_w = tag.width();
        let remaining = all_tags.len() - i - 1;
        let overflow_count = all_tags.len() - i;
        let overflow_reserve = if remaining > 0 {
            format!(" +{}", overflow_count).width()
        } else {
            0
        };

        if used + sep + tag_w <= width
            && (remaining == 0 || used + sep + tag_w + overflow_reserve <= width)
        {
            if shown > 0 {
                spans.push(Span::raw(" "));
                used += 1;
            }
            spans.push(Span::styled(tag.clone(), *style));
            used += tag_w;
            shown += 1;
        } else {
            let count = all_tags.len() - i;
            let overflow = if shown > 0 {
                format!(" +{}", count)
            } else {
                format!("+{}", count)
            };
            spans.push(Span::styled(overflow.clone(), theme::muted()));
            used += overflow.width();
            break;
        }
    }

    let pad = width.saturating_sub(used);
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
}

/// Build tag spans for a pattern entry.
fn build_pattern_tag_column(
    spans: &mut Vec<Span<'_>>,
    pattern: &crate::ssh_config::model::PatternEntry,
    width: usize,
) {
    let all_tags: Vec<(String, Style)> = pattern
        .tags
        .iter()
        .map(|t| (t.clone(), theme::muted()))
        .collect();
    render_tag_spans(spans, &all_tags, width);
}

/// Build tag spans for up to 3 tags: user tags in accent, provider tags muted.
fn build_tag_column(
    spans: &mut Vec<Span<'_>>,
    host: &crate::ssh_config::model::HostEntry,
    group_by: &crate::app::GroupBy,
    detail_mode: bool,
    tag_matches: bool,
    query: &str,
    width: usize,
) {
    let tags = app::select_display_tags(host, group_by, detail_mode);
    let mut used = 0usize;

    for tag in &tags {
        let remaining = width.saturating_sub(used + if used > 0 { 1 } else { 0 });
        if remaining < 2 {
            break;
        }
        if used > 0 {
            spans.push(Span::raw(" "));
            used += 1;
        }
        let style = if tag_matches && app::contains_ci(&tag.name, query) {
            theme::highlight_bold()
        } else if tag.is_user {
            theme::version()
        } else {
            theme::muted()
        };
        let trunc = super::truncate(&tag.name, remaining);
        used += trunc.width();
        spans.push(Span::styled(trunc, style));
    }

    let pad = width.saturating_sub(used);
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
}

fn render_search_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let query = app.search.query.as_deref().unwrap_or("");
    let total = if let Some(ref scope) = app.search.scope_indices {
        scope.len()
    } else {
        app.hosts.len() + app.patterns.len()
    };
    let match_info = if query.is_empty() {
        String::new()
    } else {
        let count = app.search.filtered_indices.len() + app.search.filtered_pattern_indices.len();
        format!(" ({} of {})", count, total)
    };
    let scope_span = match &app.group_filter {
        Some(group) => Span::styled(format!(" {} ", group.to_uppercase()), theme::muted()),
        None => Span::raw(" "),
    };
    let search_line = Line::from(vec![
        Span::styled(" / ", theme::brand_badge()),
        scope_span,
        Span::raw(query),
        Span::styled("_", theme::accent()),
        Span::styled(match_info, theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(search_line), area);
}

fn footer_spans(detail_active: bool, filter_down_only: bool) -> Vec<Span<'static>> {
    let view_label = if detail_active {
        " compact "
    } else {
        " detail "
    };
    let mut spans = vec![
        Span::styled(" Enter ", theme::footer_key()),
        Span::styled(" connect ", theme::muted()),
        Span::raw("  "),
        Span::styled(" / ", theme::footer_key()),
        Span::styled(" search ", theme::muted()),
        Span::raw("  "),
        Span::styled(" # ", theme::footer_key()),
        Span::styled(" tag ", theme::muted()),
        Span::raw("  "),
        Span::styled(" v ", theme::footer_key()),
        Span::styled(view_label, theme::muted()),
        Span::raw("  "),
        Span::styled(" : ", theme::footer_key()),
        Span::styled(" cmds ", theme::muted()),
    ];
    if filter_down_only {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("DOWN ONLY", theme::warning()));
    }
    spans
}

fn pattern_footer_spans(detail_active: bool) -> Vec<Span<'static>> {
    let view_label = if detail_active {
        " compact "
    } else {
        " detail "
    };
    vec![
        Span::styled(" / ", theme::footer_key()),
        Span::styled(" search ", theme::muted()),
        Span::raw("  "),
        Span::styled(" # ", theme::footer_key()),
        Span::styled(" tag ", theme::muted()),
        Span::raw("  "),
        Span::styled(" v ", theme::footer_key()),
        Span::styled(view_label, theme::muted()),
    ]
}

fn search_footer_spans<'a>() -> Vec<Span<'a>> {
    vec![
        Span::styled(" Enter ", theme::footer_key()),
        Span::styled(" connect ", theme::muted()),
        Span::raw("  "),
        Span::styled(" Ctrl+E ", theme::footer_key()),
        Span::styled(" edit ", theme::muted()),
        Span::raw("  "),
        Span::styled(" Esc ", theme::footer_key()),
        Span::styled(" cancel ", theme::muted()),
        Span::raw("  "),
        Span::styled(" tag: ", theme::footer_key()),
        Span::styled("fuzzy ", theme::muted()),
        Span::styled(" tag= ", theme::footer_key()),
        Span::styled("exact", theme::muted()),
    ]
}

/// Build the spans for the tag input bar. Extracted for testability.
fn tag_bar_spans<'a>(input: &'a str, provider_tags: &[String]) -> Vec<Span<'a>> {
    let mut spans = vec![Span::styled(" tags: ", theme::accent_bold())];
    if !provider_tags.is_empty() {
        let ptags = provider_tags.join(", ");
        spans.push(Span::styled(format!("[{}] ", ptags), theme::muted()));
    }
    if input.is_empty() {
        spans.push(Span::styled("_", theme::accent()));
        spans.push(Span::styled(
            "  e.g. prod, staging, us-east",
            theme::muted(),
        ));
    } else {
        spans.push(Span::raw(input));
        spans.push(Span::styled("_", theme::accent()));
    }
    spans
}

fn render_tag_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let input = app.tags.input.as_deref().unwrap_or("");
    let provider_tags = app
        .selected_host()
        .map(|h| h.provider_tags.clone())
        .unwrap_or_default();
    let spans = tag_bar_spans(input, &provider_tags);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn tag_footer_spans<'a>() -> Vec<Span<'a>> {
    vec![
        Span::styled(" Enter ", theme::footer_key()),
        Span::styled(" save ", theme::muted()),
        Span::raw("  "),
        Span::styled(" Esc ", theme::footer_key()),
        Span::styled(" cancel ", theme::muted()),
        Span::raw("  "),
        Span::styled("comma-separated", theme::muted()),
    ]
}

#[cfg(test)]
#[path = "host_list_tests.rs"]
mod tests;
