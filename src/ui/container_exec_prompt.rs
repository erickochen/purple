//! Single-line prompt overlay for `Screen::ContainerExecPrompt`.
//! Reads a one-off command string; Enter submits via the existing
//! `pending_container_exec` flow.

use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::design;
use super::theme;
use crate::app::{App, Screen};
use crate::messages::footer as fl;

pub fn render(frame: &mut Frame, app: &mut App) {
    let Screen::ContainerExecPrompt {
        alias,
        container_name,
        query,
        ..
    } = &app.screen
    else {
        return;
    };

    let area = super::centered_rect_fixed(60, 7, frame.area());
    frame.render_widget(Clear, area);
    let block = design::overlay_block(crate::i18n::text(" Run command on container "));

    let identity_line = Line::from(vec![
        Span::raw("  "),
        Span::styled(container_name.clone(), theme::bold()),
        Span::raw("  "),
        Span::styled(format!("on  {}", alias), theme::muted()),
    ]);

    let label_span = Span::styled("  command  ", theme::muted());
    let value_span = Span::styled(query.clone(), theme::bold());
    let cursor_span = Span::styled("█", theme::accent_bold());
    let prompt_line = Line::from(vec![label_span, value_span, cursor_span]);

    let text = vec![Line::from(""), identity_line, Line::from(""), prompt_line];

    design::render_body(frame, area, block, text);

    let footer_area = design::render_overlay_footer(frame, area);
    let footer = design::Footer::new()
        .primary("Enter", crate::i18n::text(fl::ENTER_RUN))
        .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
        .into_spans();
    frame.render_widget(Paragraph::new(Line::from(footer)), footer_area);
}
