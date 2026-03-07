use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::theme;
use crate::app::{App, PingStatus};
use crate::history::ConnectionHistory;
use crate::ssh_config::model::ConfigElement;

const LABEL_WIDTH: usize = 14;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let host = match app.selected_host() {
        Some(h) => h,
        None => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border());
            let empty = Paragraph::new("  Select a host to see details.")
                .style(theme::muted())
                .block(block);
            frame.render_widget(empty, area);
            return;
        }
    };

    let title = format!(" {} ", host.alias);
    let block = Block::default()
        .title(Span::styled(title, theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::border());

    let inner_width = (area.width as usize).saturating_sub(2); // minus borders
    let max_value_width = inner_width.saturating_sub(2 + LABEL_WIDTH); // minus indent + label
    let separator = "─".repeat(inner_width.saturating_sub(4).min(26));

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Connection section
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  Connection", theme::section_header())));
    lines.push(Line::from(Span::styled(
        format!("  {}", separator),
        theme::muted(),
    )));

    push_field(&mut lines, "Host", &host.hostname, max_value_width);

    if !host.user.is_empty() {
        push_field(&mut lines, "User", &host.user, max_value_width);
    }

    if host.port != 22 {
        push_field(&mut lines, "Port", &host.port.to_string(), max_value_width);
    }

    if !host.proxy_jump.is_empty() {
        push_field(&mut lines, "ProxyJump", &host.proxy_jump, max_value_width);
    }

    if !host.identity_file.is_empty() {
        let key_display = host
            .identity_file
            .rsplit('/')
            .next()
            .unwrap_or(&host.identity_file);
        push_field(&mut lines, "Key", key_display, max_value_width);
    }

    if let Some(ref askpass) = host.askpass {
        push_field(&mut lines, "Password", askpass, max_value_width);
    }

    // Activity section
    let history_entry = app.history.entries.get(&host.alias);
    let ping = app.ping_status.get(&host.alias);

    if history_entry.is_some() || ping.is_some() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Activity",
            theme::section_header(),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {}", separator),
            theme::muted(),
        )));

        if let Some(entry) = history_entry {
            let ago = ConnectionHistory::format_time_ago(entry.last_connected);
            if !ago.is_empty() {
                push_field(&mut lines, "Last SSH", &ago, max_value_width);
            }
            push_field(&mut lines, "Connections", &entry.count.to_string(), max_value_width);
        }

        if let Some(status) = ping {
            let (text, style) = match status {
                PingStatus::Checking => ("checking...", theme::muted()),
                PingStatus::Reachable => ("reachable", theme::success()),
                PingStatus::Unreachable => ("unreachable", theme::error()),
                PingStatus::Skipped => ("skipped", theme::muted()),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<width$}", "Status", width = LABEL_WIDTH),
                    theme::muted(),
                ),
                Span::styled(text, style),
            ]));
        }
    }

    // Tags section
    if !host.tags.is_empty() || host.provider.is_some() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  Tags", theme::section_header())));
        lines.push(Line::from(Span::styled(
            format!("  {}", separator),
            theme::muted(),
        )));

        let mut tag_spans = vec![Span::raw("  ")];
        for tag in &host.tags {
            tag_spans.push(Span::styled(format!("#{}", tag), theme::accent()));
            tag_spans.push(Span::raw("  "));
        }
        if let Some(ref provider) = host.provider {
            tag_spans.push(Span::styled(format!("#{}", provider), theme::accent()));
        }
        lines.push(Line::from(tag_spans));
    }

    // Tunnels section
    let tunnel_active = app.active_tunnels.contains_key(&host.alias);
    if host.tunnel_count > 0 {
        lines.push(Line::from(""));
        let tunnel_header = if tunnel_active {
            "  Tunnels (active)"
        } else {
            "  Tunnels"
        };
        lines.push(Line::from(Span::styled(
            tunnel_header,
            theme::section_header(),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {}", separator),
            theme::muted(),
        )));

        let rules = find_tunnel_rules(&app.config.elements, &host.alias);
        let style = if tunnel_active {
            theme::bold()
        } else {
            theme::muted()
        };
        for rule in rules.iter().take(5) {
            lines.push(Line::from(Span::styled(format!("  {}", rule), style)));
        }
        if rules.len() > 5 {
            lines.push(Line::from(Span::styled(
                format!("  (and {} more...)", rules.len() - 5),
                theme::muted(),
            )));
        }
    }

    // Source section (for included hosts)
    if let Some(ref source) = host.source_file {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<width$}", "Source", width = LABEL_WIDTH),
                theme::muted(),
            ),
            Span::styled(source.display().to_string(), theme::muted()),
        ]));
    }

    lines.push(Line::from(""));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn push_field(lines: &mut Vec<Line<'static>>, label: &'static str, value: &str, max_value_width: usize) {
    let display = if max_value_width > 0 {
        super::truncate(value, max_value_width)
    } else {
        value.to_string()
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<width$}", label, width = LABEL_WIDTH),
            theme::muted(),
        ),
        Span::styled(display, theme::bold()),
    ]));
}

fn find_tunnel_rules(elements: &[ConfigElement], alias: &str) -> Vec<String> {
    for element in elements {
        match element {
            ConfigElement::HostBlock(block) if block.host_pattern == alias => {
                return block
                    .directives
                    .iter()
                    .filter(|d| !d.is_non_directive)
                    .filter_map(|d| {
                        let prefix = match d.key.to_lowercase().as_str() {
                            "localforward" => "L",
                            "remoteforward" => "R",
                            "dynamicforward" => "D",
                            _ => return None,
                        };
                        Some(format!("{} {}", prefix, d.value))
                    })
                    .collect();
            }
            ConfigElement::Include(include) => {
                for file in &include.resolved_files {
                    let result = find_tunnel_rules(&file.elements, alias);
                    if !result.is_empty() {
                        return result;
                    }
                }
            }
            _ => {}
        }
    }
    Vec::new()
}
