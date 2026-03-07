use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::theme;

pub fn render(frame: &mut Frame) {
    let width: u16 = 44;
    let help_lines = help_text();
    let height = (help_lines.len() as u16 + 4).min(frame.area().height.saturating_sub(2));
    let area = super::centered_rect_fixed(width, height, frame.area());

    frame.render_widget(Clear, area);

    let title = Span::styled(" Cheat Sheet ", theme::brand());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);

    frame.render_widget(Paragraph::new(help_lines), chunks[0]);

    let footer = Line::from(vec![
        Span::styled(" Esc", theme::accent_bold()),
        Span::styled(" close", theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(footer), chunks[1]);
}

fn help_text() -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(" Navigate", theme::section_header())),
        help_line(" j/k      ", "up / down"),
        help_line(" /        ", "search"),
        help_line(" #        ", "filter by tag"),
        help_line(" s        ", "cycle sort"),
        help_line(" g        ", "group by provider"),
        Line::from(""),
        Line::from(Span::styled(" Manage", theme::section_header())),
        help_line(" Enter    ", "connect"),
        help_line(" a e d c  ", "add / edit / delete / clone"),
        help_line(" t        ", "tag host"),
        help_line(" u        ", "undo delete"),
        Line::from(""),
        Line::from(Span::styled(" Tools", theme::section_header())),
        help_line(" i        ", "inspect directives"),
        help_line(" T        ", "tunnels"),
        help_line(" S        ", "cloud providers"),
        help_line(" K        ", "SSH keys"),
        help_line(" p / P    ", "ping / ping all"),
        help_line(" y / x    ", "copy cmd / config"),
        help_line(" v        ", "toggle detail panel"),
        Line::from(""),
        help_line(" q / Esc  ", "quit / close"),
    ]
}

fn help_line<'a>(key: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(key, theme::accent_bold()),
        Span::raw(desc),
    ])
}
