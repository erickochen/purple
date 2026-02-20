use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::theme;
use crate::app::App;
use crate::ssh_config::model::ConfigElement;

pub fn render(frame: &mut Frame, app: &App, index: usize) {
    let Some(host) = app.hosts.get(index) else {
        return;
    };

    let directives = find_host_directives(&app.config.elements, &host.alias);

    let directive_count = directives.len();
    let max_visible = 15;
    let visible = directive_count.min(max_visible);
    // 2 (border) + 1 (blank) + 1 (header) + 1 (separator) + directives + 1 (overflow) + source + 1 (blank)
    let source_lines = if host.source_file.is_some() { 2 } else { 0 };
    let overflow_line = if directive_count > max_visible { 1 } else { 0 };
    let height = (6 + visible.max(1) + overflow_line + source_lines) as u16;
    let area = super::centered_rect_fixed(58, height, frame.area());

    frame.render_widget(Clear, area);

    let title = format!(" {} ", host.alias);
    let block = Block::default()
        .title(Span::styled(title, theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::accent());

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Directives", theme::section_header())),
        Line::from(Span::styled(
            "  ────────────────────────",
            theme::muted(),
        )),
    ];

    if directives.is_empty() {
        lines.push(Line::from(Span::styled("  (none)", theme::muted())));
    } else {
        for (key, value) in directives.iter().take(max_visible) {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<16}", key), theme::muted()),
                Span::styled(value.to_string(), theme::bold()),
            ]));
        }
        if directive_count > max_visible {
            lines.push(Line::from(Span::styled(
                format!("  (and {} more...)", directive_count - max_visible),
                theme::muted(),
            )));
        }
    }

    if let Some(ref source) = host.source_file {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Source          ", theme::muted()),
            Span::styled(source.display().to_string(), theme::bold()),
        ]));
    }

    lines.push(Line::from(""));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

/// Find all real directives for a host by searching config elements.
fn find_host_directives(elements: &[ConfigElement], alias: &str) -> Vec<(String, String)> {
    for element in elements {
        match element {
            ConfigElement::HostBlock(block) if block.host_pattern == alias => {
                return block
                    .directives
                    .iter()
                    .filter(|d| !d.is_non_directive)
                    .map(|d| (d.key.clone(), d.value.clone()))
                    .collect();
            }
            ConfigElement::Include(include) => {
                for file in &include.resolved_files {
                    let result = find_host_directives(&file.elements, alias);
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
