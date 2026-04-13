use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, Screen, SnippetFormField};

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let title = match &app.screen {
        Screen::SnippetForm {
            editing: Some(_), ..
        } => " Snippets > Edit ",
        _ => " Snippets > Add ",
    };

    let fields = SnippetFormField::ALL;

    // Block: top(1) + fields * 2 (divider + content) + bottom(1)
    let block_height = 2 + fields.len() as u16 * 2;
    let total_height = block_height + 1; // + footer

    let base = super::centered_rect(70, 80, area);
    let form_area = super::centered_rect_fixed(base.width, total_height, area);
    frame.render_widget(Clear, form_area);

    let block_area = Rect::new(form_area.x, form_area.y, form_area.width, block_height);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(title, theme::brand()))
        .border_style(theme::accent());

    let inner = block.inner(block_area);
    frame.render_widget(block, block_area);

    for (i, &field) in fields.iter().enumerate() {
        let divider_y = inner.y + (2 * i) as u16;
        let content_y = divider_y + 1;

        let is_focused = app.snippet_form.focused_field == field;
        let label_style = if is_focused {
            theme::accent_bold()
        } else {
            theme::muted()
        };
        let required = matches!(field, SnippetFormField::Name | SnippetFormField::Command);
        let label = if required {
            format!(" {}* ", field.label())
        } else {
            format!(" {} ", field.label())
        };
        render_divider(
            frame,
            block_area,
            divider_y,
            &label,
            label_style,
            theme::accent(),
        );

        let content_area = Rect::new(inner.x + 1, content_y, inner.width.saturating_sub(1), 1);
        render_field_content(frame, content_area, field, &app.snippet_form);
    }

    // Footer below the block
    let footer_area = Rect::new(form_area.x, form_area.y + block_height, form_area.width, 1);
    let footer_spans = if app.pending_discard_confirm {
        vec![
            Span::styled(" Discard changes? ", theme::error()),
            Span::styled(" y ", theme::footer_key()),
            Span::styled(" yes ", theme::muted()),
            Span::raw("  "),
            Span::styled(" Esc ", theme::footer_key()),
            Span::styled(" no", theme::muted()),
        ]
    } else {
        vec![
            Span::styled(" Enter ", theme::footer_key()),
            Span::styled(" save ", theme::muted()),
            Span::raw("  "),
            Span::styled(" Tab ", theme::footer_key()),
            Span::styled(" next ", theme::muted()),
            Span::raw("  "),
            Span::styled(" Esc ", theme::footer_key()),
            Span::styled(" cancel", theme::muted()),
        ]
    };
    super::render_footer_with_status(frame, footer_area, footer_spans, app);
}

fn render_divider(
    frame: &mut Frame,
    block_area: Rect,
    y: u16,
    label: &str,
    label_style: Style,
    border_style: Style,
) {
    super::render_divider(frame, block_area, y, label, label_style, border_style);
}

fn render_field_content(
    frame: &mut Frame,
    area: Rect,
    field: SnippetFormField,
    form: &crate::app::SnippetForm,
) {
    let is_focused = form.focused_field == field;

    let placeholder = match field {
        SnippetFormField::Name => "check-disk",
        SnippetFormField::Command => "df -h",
        SnippetFormField::Description => "",
    };

    let field_value = match field {
        SnippetFormField::Name => &form.name,
        SnippetFormField::Command => &form.command,
        SnippetFormField::Description => &form.description,
    };

    let content = if field_value.is_empty() && is_focused {
        if placeholder.is_empty() {
            Line::from(Span::styled("(optional)", theme::muted()))
        } else {
            Line::from(Span::styled(placeholder, theme::muted()))
        }
    } else if field_value.is_empty() {
        Line::from(Span::raw(""))
    } else {
        Line::from(Span::styled(field_value.to_string(), theme::bold()))
    };

    frame.render_widget(Paragraph::new(content), area);

    if is_focused {
        let prefix: String = field_value.chars().take(form.cursor_pos).collect();
        let cursor_x = area
            .x
            .saturating_add(prefix.width().min(u16::MAX as usize) as u16);
        let cursor_y = area.y;
        if area.width > 0 && cursor_x < area.x.saturating_add(area.width) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}
