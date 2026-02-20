use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::theme;

pub fn render(frame: &mut Frame) {
    let area = super::centered_rect_fixed(50, 22, frame.area());

    // Clear background
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(Span::styled(" Purple Cheat Sheet ", theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::accent());

    let help_text = vec![
        Line::from(Span::styled("  Host List", theme::section_header())),
        help_line("  j/k       ", "Move down / up"),
        help_line("  Enter     ", "Connect to host"),
        help_line("  a e d     ", "Add / edit / delete host"),
        help_line("  y         ", "Copy SSH command"),
        help_line("  /         ", "Search / filter hosts"),
        help_line("  p / P     ", "Ping host / ping all"),
        help_line("  K         ", "SSH key list"),
        help_line("  q / Esc   ", "Quit / back"),
        help_line("  Ctrl+C    ", "Quit (from anywhere)"),
        Line::from(""),
        Line::from(Span::styled("  Search", theme::section_header())),
        help_line("  Enter     ", "Connect to selected"),
        help_line("  Esc       ", "Cancel search"),
        Line::from(""),
        Line::from(Span::styled("  Form", theme::section_header())),
        help_line("  Tab/S-Tab ", "Next / previous field"),
        help_line("  Ctrl+K    ", "Pick SSH key"),
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
