use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::design;
use super::theme;
use crate::app::{App, Screen, SnippetFormField};

pub fn render(frame: &mut Frame, app: &mut App) {
    let title = match (&app.screen, app.snippets.form_editing()) {
        (Screen::SnippetForm, Some(_)) => crate::i18n::text("Snippets > Edit"),
        _ => crate::i18n::text("Snippets > Add"),
    };

    let fields = SnippetFormField::ALL;

    // Block: top(1) + fields * 2 (divider + content) + bottom(1)
    let block_height = 2 + fields.len() as u16 * 2;
    let total_height = block_height + 1; // + footer

    let form_area = design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, total_height);
    frame.render_widget(Clear, form_area);

    let block_area = Rect::new(form_area.x, form_area.y, form_area.width, block_height);

    let block = design::overlay_block(title);
    let inner = block.inner(block_area);
    frame.render_widget(block, block_area);

    for (i, &field) in fields.iter().enumerate() {
        let divider_y = design::form_divider_y(inner, i);
        let content_y = divider_y + 1;

        let is_focused = app.snippets.form().focused_field == field;
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
        super::render_divider(
            frame,
            block_area,
            divider_y,
            &label,
            label_style,
            theme::border_dim(),
        );

        let content_area = Rect::new(inner.x + 1, content_y, inner.width.saturating_sub(1), 1);
        render_field_content(frame, content_area, field, app.snippets.form());
    }

    // Footer below the block. The Interactive shell toggle and the Default
    // hosts picker carry their Space hint; the text fields map to FieldKind::Text.
    let footer_area = design::render_overlay_footer(frame, block_area);
    if app.forms.is_discard_pending() {
        design::render_discard_prompt(frame, footer_area, app);
    } else {
        let kind = app.snippets.form().focused_field.kind();
        design::form_save_footer(design::FormFooterMode::Expanded(kind)).render_with_status(
            frame,
            footer_area,
            app,
        );
    }
}

fn render_field_content(
    frame: &mut Frame,
    area: Rect,
    field: SnippetFormField,
    form: &crate::app::SnippetForm,
) {
    let is_focused = form.focused_field == field;

    if field.is_toggle() {
        let value_text = crate::messages::hints::snippet_interactive_value(form.interactive);
        let content = if is_focused {
            let gap = (area.width as usize).saturating_sub(value_text.width() + 3);
            Line::from(vec![
                Span::styled(value_text, theme::bold()),
                Span::raw(" ".repeat(gap)),
                Span::styled(design::TOGGLE_HINT, theme::muted()),
            ])
        } else {
            Line::from(Span::styled(value_text, theme::bold()))
        };
        frame.render_widget(Paragraph::new(content), area);
        return;
    }

    let placeholder = match field {
        SnippetFormField::Name => crate::i18n::text(crate::messages::hints::SNIPPET_NAME),
        SnippetFormField::Command => crate::i18n::text(crate::messages::hints::SNIPPET_COMMAND),
        SnippetFormField::Description | SnippetFormField::Interactive => "",
        SnippetFormField::DefaultHosts => {
            crate::i18n::text(crate::messages::hints::SNIPPET_DEFAULT_HOSTS)
        }
    };

    let field_value: String = match field {
        SnippetFormField::Name => form.name.clone(),
        SnippetFormField::Command => form.command.clone(),
        SnippetFormField::Description => form.description.clone(),
        SnippetFormField::Interactive => String::new(),
        SnippetFormField::DefaultHosts => {
            crate::messages::snippet::snippet_default_hosts_summary(&form.default_hosts)
        }
    };

    let is_picker = field.is_picker();
    let content = if is_picker && is_focused {
        // Picker field: value (or placeholder) plus a right-aligned arrow,
        // mirroring how host_form renders its picker fields.
        let arrow_pos = (area.width as usize).saturating_sub(1);
        let (display, display_style) = if field_value.is_empty() {
            (placeholder.to_string(), theme::muted())
        } else {
            (field_value.clone(), theme::bold())
        };
        let gap = arrow_pos.saturating_sub(display.width());
        Line::from(vec![
            Span::styled(display, display_style),
            Span::raw(" ".repeat(gap)),
            Span::styled(design::PICKER_ARROW, theme::muted()),
        ])
    } else if field_value.is_empty() && is_focused {
        if placeholder.is_empty() {
            Line::from(Span::styled(
                crate::i18n::text(crate::messages::hints::SNIPPET_OPTIONAL),
                theme::muted(),
            ))
        } else {
            Line::from(Span::styled(placeholder, theme::muted()))
        }
    } else if field_value.is_empty() {
        Line::from(Span::raw(""))
    } else {
        Line::from(Span::styled(field_value.clone(), theme::bold()))
    };

    frame.render_widget(Paragraph::new(content), area);

    // The Default hosts field is picker-driven, not text, so it shows no cursor.
    if is_focused && !is_picker {
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
