use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &App, index: usize) {
    let alias = app
        .hosts
        .get(index)
        .map(|h| h.alias.as_str())
        .unwrap_or("???");

    let area = super::centered_rect_fixed(44, 7, frame.area());

    // Clear background
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(Span::styled(" Confirm Delete ", theme::danger()))
        .borders(Borders::ALL)
        .border_style(theme::border_danger());

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  Delete \"{}\"?", alias),
            theme::bold(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("    Enter", theme::danger()),
            Span::styled(" yes   ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" no", theme::muted()),
        ]),
    ];

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}
