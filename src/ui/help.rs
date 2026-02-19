use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::theme;

pub fn render(frame: &mut Frame) {
    let area = super::centered_rect_fixed(50, 20, frame.area());

    // Clear background
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(Span::styled(" Purple Cheat Sheet ", theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::accent());

    let help_text = vec![
        Line::from(""),
        Line::from(Span::styled("  Host List", theme::section_header())),
        Line::from(""),
        help_line("  j / Down  ", "Move down"),
        help_line("  k / Up    ", "Move up"),
        help_line("  Enter     ", "Connect to host"),
        help_line("  a         ", "Add new host"),
        help_line("  e         ", "Edit selected host"),
        help_line("  d         ", "Delete selected host"),
        help_line("  Ctrl+C    ", "Quit (from anywhere)"),
        help_line("  q / Esc   ", "Quit / back"),
        Line::from(""),
        Line::from(Span::styled("  Form", theme::section_header())),
        Line::from(""),
        help_line("  Tab       ", "Next field"),
        help_line("  Shift+Tab ", "Previous field"),
        help_line("  Enter     ", "Save"),
        help_line("  Esc       ", "Cancel"),
    ];

    let paragraph = Paragraph::new(help_text).block(block);
    frame.render_widget(paragraph, area);
}

fn help_line<'a>(key: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(key, theme::accent_bold()),
        Span::raw(desc),
    ])
}
