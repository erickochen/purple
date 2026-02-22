use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App) {
    if app.tag_list.is_empty() {
        let area = super::centered_rect_fixed(44, 5, frame.area());
        frame.render_widget(Clear, area);
        let block = Block::default()
            .title(Span::styled(" Filter by Tag ", theme::brand()))
            .borders(Borders::ALL)
            .border_style(theme::accent());
        let msg = Paragraph::new(Line::from(Span::styled(
            "  No tags yet. Press t on a host to add some.",
            theme::muted(),
        )))
        .block(block);
        frame.render_widget(msg, area);
        return;
    }

    // Count hosts per tag
    let tag_counts: std::collections::HashMap<&str, usize> = {
        let mut counts = std::collections::HashMap::new();
        for host in &app.hosts {
            for tag in &host.tags {
                *counts.entry(tag.as_str()).or_insert(0) += 1;
            }
        }
        counts
    };

    let height = (app.tag_list.len() as u16 + 4).min(16);
    let area = super::centered_rect_fixed(40, height, frame.area());
    frame.render_widget(Clear, area);

    let items: Vec<ListItem> = app
        .tag_list
        .iter()
        .map(|tag| {
            let count = tag_counts.get(tag.as_str()).copied().unwrap_or(0);
            let line = Line::from(vec![
                Span::styled(format!(" #{}", tag), theme::bold()),
                Span::styled(format!(" ({})", count), theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::default()
        .title(Span::styled(" Filter by Tag ", theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::accent());

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, area, &mut app.tag_picker_state);
}
