use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::design;
use super::theme;
use crate::app::{App, Screen};

pub fn render(frame: &mut Frame, app: &mut App) {
    if !matches!(app.screen, Screen::SnippetParamForm) {
        return;
    }
    let snippet = match app.snippets.param_snippet() {
        Some(s) => s.clone(),
        None => return,
    };

    let form = match app.snippets.param_form() {
        Some(f) => f,
        None => return,
    };

    let area = frame.area();
    let title = format!("Parameters for '{}'", super::truncate(&snippet.name, 30));

    let scroll = form.scroll_offset;
    let max_visible = form.params.len().min(8);
    let end = (scroll + max_visible).min(form.params.len());
    let rendered_count = end - scroll;

    // Block: top(1) + rendered_params * 2 (divider + content) + divider(1) + preview(1) + bottom(1)
    let block_height = 2 + rendered_count as u16 * 2 + 2;
    let total_height = block_height + 1; // + footer

    // Clamp total_height to available terminal space
    let clamped_height = total_height.min(area.height.saturating_sub(2));
    let form_area =
        design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, clamped_height);
    frame.render_widget(Clear, form_area);

    let block_height = block_height.min(form_area.height.saturating_sub(1));
    let block_area = Rect::new(form_area.x, form_area.y, form_area.width, block_height);

    let block = design::overlay_block(&title);
    let inner = block.inner(block_area);
    frame.render_widget(block, block_area);

    // Compute actual visible capacity (accounting for preview divider + preview line)
    let actual_visible = (inner.height.saturating_sub(2) / 2) as usize;
    let actual_visible = actual_visible.min(max_visible);
    let param_count = form.params.len();

    // Update form state so the handler uses the correct window size
    if let Some(f) = app.snippets.param_form_mut() {
        f.visible_count = actual_visible.max(1);
    }
    // Re-borrow form immutably after mutation
    let form = match app.snippets.param_form() {
        Some(f) => f,
        None => return,
    };

    // Render visible parameter fields
    let end = (scroll + actual_visible).min(param_count);
    for (vi, pi) in (scroll..end).enumerate() {
        let divider_y = design::form_divider_y(inner, vi);
        let content_y = divider_y + 1;

        // Bounds check: skip if we'd render outside the inner area
        if content_y >= inner.y + inner.height {
            break;
        }

        let param = &form.params[pi];
        let value = &form.values[pi];
        let is_focused = form.focused_index == pi;

        let label_style = if is_focused {
            theme::accent_bold()
        } else {
            theme::muted()
        };
        let label = format!(" {} ", param.name);
        super::render_divider(
            frame,
            block_area,
            divider_y,
            &label,
            label_style,
            theme::border_dim(),
        );

        let content_area = Rect::new(inner.x + 1, content_y, inner.width.saturating_sub(1), 1);

        let content = if value.is_empty() {
            match &param.default {
                Some(d) => Line::from(Span::styled(d.to_string(), theme::muted())),
                None => Line::from(Span::styled("(required)", theme::muted())),
            }
        } else {
            Line::from(Span::styled(value.to_string(), theme::bold()))
        };

        frame.render_widget(Paragraph::new(content), content_area);

        if is_focused {
            let prefix: String = value.chars().take(form.cursor_pos).collect();
            let cursor_x = content_area
                .x
                .saturating_add(prefix.width().min(u16::MAX as usize) as u16);
            if content_area.width > 0
                && cursor_x < content_area.x.saturating_add(content_area.width)
            {
                frame.set_cursor_position((cursor_x, content_y));
            }
        }
    }

    // Preview divider + resolved command (based on actual rendered count)
    let actual_rendered = end - scroll;
    let preview_divider_y = inner.y + (2 * actual_rendered) as u16;
    if preview_divider_y < inner.y + inner.height {
        super::render_divider(
            frame,
            block_area,
            preview_divider_y,
            crate::i18n::text(" Preview "),
            theme::muted(),
            theme::border_dim(),
        );

        let preview_y = preview_divider_y + 1;
        if preview_y < inner.y + inner.height {
            let preview_area = Rect::new(inner.x + 1, preview_y, inner.width.saturating_sub(1), 1);

            let resolved = crate::snippet::substitute_params(&snippet.command, &form.values_map());
            let preview_text =
                super::truncate(&resolved, preview_area.width.saturating_sub(1) as usize);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(preview_text, theme::muted()))),
                preview_area,
            );
        }
    }

    // Footer below the block. Snippet param form runs the snippet on submit
    // (primary verb is "run", not "save"), so we cannot use the generic
    // form_save_footer helper. Build manually but follow the same shape.
    let footer_area = design::render_overlay_footer(frame, block_area);
    if footer_area.y < form_area.y + form_area.height {
        if app.forms.is_discard_pending() {
            design::render_discard_prompt(frame, footer_area, app);
        } else {
            use crate::messages::footer as fl;
            design::Footer::new()
                .primary("Enter", crate::i18n::text(fl::ENTER_RUN))
                .action("Tab", crate::i18n::text(fl::TAB_NEXT))
                .action("Esc", crate::i18n::text(fl::ESC_CANCEL))
                .render_with_status(frame, footer_area, app);
        }
    }
}
