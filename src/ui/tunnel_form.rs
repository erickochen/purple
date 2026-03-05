use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, Screen, TunnelFormField};
use crate::tunnel::TunnelType;

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let title = match &app.screen {
        Screen::TunnelForm { editing: Some(_), .. } => " Edit Tunnel ",
        _ => " Add Tunnel ",
    };

    let is_dynamic = app.tunnel_form.tunnel_type == TunnelType::Dynamic;

    // Overlay: percentage-based width, fixed height
    let height: u16 = if is_dynamic { 10 } else { 15 };
    let form_area = super::centered_rect(70, 80, area);
    let form_area = super::centered_rect_fixed(form_area.width, height, area);

    frame.render_widget(Clear, form_area);

    let outer_block = Block::default()
        .title(Span::styled(title, theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::border());

    let inner = outer_block.inner(form_area);
    frame.render_widget(outer_block, form_area);

    let mut constraints = vec![
        Constraint::Length(3), // Type
        Constraint::Length(3), // Bind Port
    ];
    if !is_dynamic {
        constraints.push(Constraint::Length(3)); // Remote Host
        constraints.push(Constraint::Length(3)); // Remote Port
    }
    constraints.push(Constraint::Min(0));   // Spacer
    constraints.push(Constraint::Length(1)); // Footer

    let chunks = Layout::vertical(constraints).split(inner);

    // Type field (special: Left/Right cycle, not text input)
    render_type_field(frame, chunks[0], &app.tunnel_form);

    // Bind Port
    render_text_field(
        frame,
        chunks[1],
        TunnelFormField::BindPort,
        &app.tunnel_form,
        "8080",
        true,
    );

    if !is_dynamic {
        // Remote Host
        render_text_field(
            frame,
            chunks[2],
            TunnelFormField::RemoteHost,
            &app.tunnel_form,
            "localhost",
            true,
        );

        // Remote Port
        render_text_field(
            frame,
            chunks[3],
            TunnelFormField::RemotePort,
            &app.tunnel_form,
            "80",
            true,
        );
    }

    // Footer with status
    let footer_idx = chunks.len() - 1;
    super::render_footer_with_status(frame, chunks[footer_idx], vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" save ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Tab", theme::accent_bold()),
        Span::styled(" next ", theme::muted()),
        Span::styled("L/R", theme::accent_bold()),
        Span::styled(" type ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" cancel", theme::muted()),
    ], app);
}

fn render_type_field(frame: &mut Frame, area: Rect, form: &crate::app::TunnelForm) {
    let is_focused = form.focused_field == TunnelFormField::Type;

    let (border_style, label_style) = if is_focused {
        (theme::border_focused(), theme::accent_bold())
    } else {
        (theme::border(), theme::muted())
    };

    let block = Block::default()
        .title(Span::styled(" Type* ", label_style))
        .borders(Borders::ALL)
        .border_style(border_style);

    let type_label = form.tunnel_type.label();
    let content = if is_focused {
        let inner_width = area.width.saturating_sub(2) as usize;
        let val_width = type_label.len();
        let gap = inner_width.saturating_sub(val_width + 3);
        Line::from(vec![
            Span::styled(type_label, theme::bold()),
            Span::raw(" ".repeat(gap)),
            Span::styled("\u{25C2} \u{25B8}", theme::muted()),
        ])
    } else {
        Line::from(Span::raw(type_label))
    };
    let paragraph = Paragraph::new(content).block(block);
    frame.render_widget(paragraph, area);
}

fn render_text_field(
    frame: &mut Frame,
    area: Rect,
    field: TunnelFormField,
    form: &crate::app::TunnelForm,
    placeholder: &str,
    required: bool,
) {
    let value = match field {
        TunnelFormField::BindPort => &form.bind_port,
        TunnelFormField::RemoteHost => &form.remote_host,
        TunnelFormField::RemotePort => &form.remote_port,
        TunnelFormField::Type => unreachable!("Type uses render_type_field"),
    };
    let is_focused = form.focused_field == field;

    let (border_style, label_style) = if is_focused {
        (theme::border_focused(), theme::accent_bold())
    } else {
        (theme::border(), theme::muted())
    };

    let label = if required {
        format!(" {}* ", field.label())
    } else {
        format!(" {} ", field.label())
    };

    let block = Block::default()
        .title(Span::styled(label, label_style))
        .borders(Borders::ALL)
        .border_style(border_style);

    let display: Span = if value.is_empty() && !is_focused {
        Span::styled(placeholder, theme::muted())
    } else {
        Span::raw(value)
    };

    let paragraph = Paragraph::new(display).block(block);
    frame.render_widget(paragraph, area);

    // Cursor
    if is_focused {
        let prefix: String = value.chars().take(form.cursor_pos).collect();
        let cursor_x = area
            .x
            .saturating_add(1)
            .saturating_add(UnicodeWidthStr::width(prefix.as_str()).min(u16::MAX as usize) as u16);
        let cursor_y = area.y + 1;
        if area.width > 1 && cursor_x < area.x.saturating_add(area.width).saturating_sub(1) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}
