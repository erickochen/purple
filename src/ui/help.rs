use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::theme;

pub fn render(frame: &mut Frame) {
    let area = super::centered_rect_fixed(50, 25, frame.area());

    // Clear background
    frame.render_widget(Clear, area);

    let title = Line::from(vec![
        Span::styled(" purple. ", theme::brand_badge()),
        Span::raw(" Cheat Sheet "),
    ]);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(theme::accent());

    let help_text = vec![
        Line::from(Span::styled("  Navigate", theme::section_header())),
        help_line("  j/k       ", "Move down / up"),
        help_line("  /         ", "Search / filter hosts"),
        help_line("  #         ", "Filter by tag"),
        help_line("  s         ", "Cycle sort mode"),
        Line::from(""),
        Line::from(Span::styled("  Manage", theme::section_header())),
        help_line("  Enter     ", "Connect to host"),
        help_line("  a e d c   ", "Add / edit / delete / clone"),
        help_line("  t         ", "Tag host"),
        help_line("  u         ", "Undo last delete"),
        help_line("  S         ", "Cloud provider sync"),
        Line::from(""),
        Line::from(Span::styled("  Tools", theme::section_header())),
        help_line("  i         ", "Inspect host details"),
        help_line("  p / P     ", "Ping host / ping all"),
        help_line("  y / x     ", "Copy command / config block"),
        help_line("  K         ", "SSH key list"),
        Line::from(""),
        help_line("  q / Esc   ", "Quit"),
        help_line("  Ctrl+C    ", "Quit (from anywhere)"),
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
