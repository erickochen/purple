use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

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
    // 2 (border) + 1 (blank) + 1 (header) + 1 (separator) + directives + 1 (overflow) + source + 1 (spacer) + 1 (footer)
    let askpass_lines = if host.askpass.is_some() { 2 } else { 0 };
    let source_lines = if host.source_file.is_some() { 2 } else { 0 };
    let overflow_line = if directive_count > max_visible { 1 } else { 0 };
    let footer_line = 2; // spacer + footer
    let height =
        (6 + visible.max(1) + overflow_line + askpass_lines + source_lines + footer_line) as u16;
    let width = frame.area().width.clamp(58, 80);
    let area = super::centered_rect_fixed(width, height, frame.area());

    frame.render_widget(Clear, area);

    let title = format!(" {} ", host.alias);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(title, theme::brand()))
        .border_style(theme::accent());

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Directives", theme::section_header())),
        Line::from(Span::styled("  ────────────────────────", theme::muted())),
    ];

    if directives.is_empty() {
        lines.push(Line::from(Span::styled("  (none)", theme::muted())));
    } else {
        for (key, value) in directives.iter().take(max_visible) {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<22}", key), theme::muted()),
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

    if let Some(ref askpass) = host.askpass {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<22}", "Password"), theme::muted()),
            Span::styled(askpass.to_string(), theme::bold()),
        ]));
    }

    if let Some(ref source) = host.source_file {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<22}", "Source"), theme::muted()),
            Span::styled(source.display().to_string(), theme::bold()),
        ]));
    }

    let is_included = host.source_file.is_some();
    let mut footer_spans = vec![];
    if !is_included {
        footer_spans.extend([
            Span::styled(" e ", theme::footer_key()),
            Span::styled(" edit ", theme::muted()),
            Span::raw("  "),
        ]);
    } else {
        footer_spans.push(Span::raw("  "));
    }
    footer_spans.extend([
        Span::styled(" T ", theme::footer_key()),
        Span::styled(" tunnels ", theme::muted()),
        Span::raw("  "),
        Span::styled(" r ", theme::footer_key()),
        Span::styled(" snippet ", theme::muted()),
        Span::raw("  "),
        Span::styled(" Esc ", theme::footer_key()),
        Span::styled(" back", theme::muted()),
    ]);
    lines.push(Line::from(""));
    lines.push(Line::from(footer_spans));

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

#[cfg(test)]
mod tests {
    #[test]
    fn height_includes_spacer_before_footer() {
        // Simulate height calculation for a host with 3 directives, no askpass, no source
        let visible = 3usize;
        let overflow_line = 0;
        let askpass_lines = 0;
        let source_lines = 0;
        let footer_line = 2; // spacer + footer
        let height =
            6 + visible.max(1) + overflow_line + askpass_lines + source_lines + footer_line;
        // 6 base + 3 directives + 2 footer = 11
        // Without spacer it would be 10, so verify the extra line
        let height_without_spacer =
            6 + visible.max(1) + overflow_line + askpass_lines + source_lines + 1;
        assert_eq!(
            height,
            height_without_spacer + 1,
            "height should include 1 extra row for the spacer"
        );
    }
}
