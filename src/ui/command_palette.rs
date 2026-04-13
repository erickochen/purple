use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App) {
    let palette = match app.palette.as_ref() {
        Some(p) => p,
        None => return,
    };

    let filtered = palette.filtered_commands();
    let max_visible: u16 = 16;
    let list_height = (filtered.len() as u16).min(max_visible).max(1);
    // border(2) + input(1) + separator(1) + list + spacer(1) + footer(1)
    let total_height = 2 + 1 + 1 + list_height + 1 + 1;

    // Dynamic width: max(48, 60% of terminal), capped at terminal - 4
    let dynamic_width = 48u16.max(frame.area().width * 60 / 100);
    let overlay_width = dynamic_width.min(frame.area().width.saturating_sub(4));
    let height = total_height.min(frame.area().height.saturating_sub(2));
    let area = super::centered_rect_fixed(overlay_width, height, frame.area());

    frame.render_widget(Clear, area);

    let title = Span::styled(" Commands ", theme::brand());
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(theme::accent());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1), // input line
        Constraint::Length(1), // separator
        Constraint::Min(1),    // command list
        Constraint::Length(1), // spacer
        Constraint::Length(1), // footer
    ])
    .split(inner);

    // Input line with cursor
    let input_line = if palette.query.is_empty() {
        Line::from(Span::styled(
            "  type to filter, Enter to run...",
            theme::muted(),
        ))
    } else {
        Line::from(vec![
            Span::styled("  /", theme::accent_bold()),
            Span::styled(palette.query.clone(), theme::brand()),
            Span::styled("\u{2588}", theme::accent_bold()), // block cursor
        ])
    };
    frame.render_widget(Paragraph::new(input_line), rows[0]);

    // Separator using box-drawing char
    let sep_width = (inner.width as usize).saturating_sub(1);
    let sep = Line::from(Span::styled(
        format!(" {}", "\u{2500}".repeat(sep_width)),
        theme::muted(),
    ));
    frame.render_widget(Paragraph::new(sep), rows[1]);

    // Command list or empty state
    if filtered.is_empty() {
        let msg = Paragraph::new(Line::from(Span::styled(
            "  no matching commands",
            theme::muted(),
        )));
        frame.render_widget(msg, rows[2]);
    } else {
        let items: Vec<ListItem> = filtered
            .iter()
            .map(|cmd| {
                let line = Line::from(vec![
                    Span::styled(format!("  {:>1}  ", cmd.key), theme::accent_bold()),
                    Span::styled(cmd.label, theme::muted()),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).highlight_style(theme::selected_row());

        let mut list_state = ListState::default();
        let clamped = palette.selected.min(filtered.len().saturating_sub(1));
        list_state.select(Some(clamped));
        frame.render_stateful_widget(list, rows[2], &mut list_state);
    }

    // Footer
    let mut spans: Vec<Span<'_>> = Vec::new();
    spans.push(Span::raw(" "));
    let [k, l] = super::footer_action("Enter", " run ");
    spans.extend([k, l]);
    spans.push(Span::raw("  "));
    let [k, l] = super::footer_action("\u{2191}\u{2193}", " select ");
    spans.extend([k, l]);
    spans.push(Span::raw("  "));
    let [k, l] = super::footer_action("Esc", " close");
    spans.extend([k, l]);
    super::render_footer_with_status(frame, rows[4], spans, app);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        let config = crate::ssh_config::model::SshConfigFile {
            elements: Vec::new(),
            path: std::path::PathBuf::from("/tmp/purple_palette_test"),
            crlf: false,
            bom: false,
        };
        let mut app = App::new(config);
        app.palette = Some(crate::app::CommandPaletteState::new());
        app
    }

    #[test]
    fn palette_renders_without_panic() {
        let mut app = test_app();
        let backend = ratatui::backend::TestBackend::new(80, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
    }

    #[test]
    fn palette_renders_all_commands_when_no_filter() {
        let mut app = test_app();
        let backend = ratatui::backend::TestBackend::new(80, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(text.contains("file explorer"), "should show file explorer");
        assert!(text.contains("tunnels"), "should show tunnels");
    }

    #[test]
    fn palette_renders_filtered_commands() {
        let mut app = test_app();
        app.palette.as_mut().unwrap().push_query('t');
        let backend = ratatui::backend::TestBackend::new(80, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(text.contains("tunnels"), "tunnels contains 't'");
    }

    #[test]
    fn palette_renders_empty_state() {
        let mut app = test_app();
        app.palette.as_mut().unwrap().push_query('z');
        app.palette.as_mut().unwrap().push_query('z');
        app.palette.as_mut().unwrap().push_query('z');
        let backend = ratatui::backend::TestBackend::new(80, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(text.contains("no matching"), "should show empty state");
    }

    #[test]
    fn palette_renders_cursor_when_filtering() {
        let mut app = test_app();
        app.palette.as_mut().unwrap().push_query('t');
        let backend = ratatui::backend::TestBackend::new(80, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(text.contains("\u{2588}"), "should show block cursor");
    }

    #[test]
    fn palette_on_narrow_terminal() {
        let mut app = test_app();
        let backend = ratatui::backend::TestBackend::new(50, 15);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, &mut app)).unwrap();
    }
}
