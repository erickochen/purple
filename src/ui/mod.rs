mod confirm_dialog;
mod help;
mod host_detail;
mod host_form;
mod host_list;
mod key_detail;
mod key_list;
pub mod theme;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Screen};

const MIN_WIDTH: u16 = 50;
const MIN_HEIGHT: u16 = 10;

/// Top-level render dispatcher.
pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Terminal too small guard
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = Paragraph::new("Terminal too small. Need at least 50x10.")
            .style(theme::error());
        frame.render_widget(msg, area);
        return;
    }

    match &app.screen {
        Screen::HostList => host_list::render(frame, app),
        Screen::AddHost | Screen::EditHost { .. } => host_form::render(frame, app),
        Screen::ConfirmDelete { index } => {
            let index = *index;
            host_list::render(frame, app);
            confirm_dialog::render(frame, app, index);
        }
        Screen::Help => {
            host_list::render(frame, app);
            help::render(frame);
        }
        Screen::KeyList => key_list::render(frame, app),
        Screen::KeyDetail { index } => {
            let index = *index;
            key_list::render(frame, app);
            key_detail::render(frame, app, index);
        }
        Screen::HostDetail { index } => {
            let index = *index;
            host_list::render(frame, app);
            host_detail::render(frame, app, index);
        }
    }
}

/// Render the status bar at the bottom.
pub fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    if let Some(ref status) = app.status {
        let line = if status.is_error {
            Line::from(vec![
                Span::styled("! ", theme::error()),
                Span::styled(status.text.as_str(), theme::error()),
            ])
        } else {
            Line::from(Span::styled(status.text.as_str(), theme::success()))
        };
        frame.render_widget(Paragraph::new(line), area);
    }
}

/// Create a centered rect of given percentage within the parent rect.
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

/// Create a centered rect with fixed dimensions.
pub fn centered_rect_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
