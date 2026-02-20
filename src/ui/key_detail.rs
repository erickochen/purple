use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &App, index: usize) {
    let Some(key) = app.keys.get(index) else {
        return;
    };

    // Calculate height based on content, capped to prevent overflow
    let linked_count = key.linked_hosts.len();
    let max_visible_hosts = 10;
    let visible_hosts = linked_count.min(max_visible_hosts);
    // 2 (border) + 1 (blank) + 4 (metadata) + 1 (blank) + 2 (header+sep) + hosts + 1 (blank)
    let height = (11 + visible_hosts.max(1)) as u16;
    let area = super::centered_rect_fixed(58, height, frame.area());

    frame.render_widget(Clear, area);

    let title = format!(" {} ", key.name);
    let block = Block::default()
        .title(Span::styled(title, theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::accent());

    let type_display = key.type_display();
    let mut lines = vec![
        Line::from(""),
        detail_line("  Type           ", &type_display),
        detail_line("  Fingerprint    ", &key.fingerprint),
        detail_line("  Comment        ", if key.comment.is_empty() { "(none)" } else { &key.comment }),
        detail_line("  Path           ", &key.display_path),
        Line::from(""),
        Line::from(Span::styled("  Linked Hosts", theme::section_header())),
        Line::from(Span::styled("  ────────────────────────", theme::muted())),
    ];

    if key.linked_hosts.is_empty() {
        lines.push(Line::from(Span::styled("  (none)", theme::muted())));
    } else {
        for alias in key.linked_hosts.iter().take(max_visible_hosts) {
            let hostname = app
                .hosts
                .iter()
                .find(|h| h.alias == *alias)
                .map(|h| h.hostname.as_str())
                .unwrap_or("");
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<14}", alias), theme::bold()),
                Span::styled(" -> ", theme::muted()),
                Span::styled(hostname.to_string(), theme::muted()),
            ]));
        }
        if linked_count > max_visible_hosts {
            lines.push(Line::from(Span::styled(
                format!("  (and {} more...)", linked_count - max_visible_hosts),
                theme::muted(),
            )));
        }
    }

    lines.push(Line::from(""));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn detail_line<'a>(label: &'a str, value: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(label, theme::muted()),
        Span::styled(value, theme::bold()),
    ])
}
